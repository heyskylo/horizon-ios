# Fuzzing and benchmarking

This crate's correctness claim — *byte-for-byte compatible with OpenJDK 25* — is enforced by a small fleet of complementary test strategies. This document explains how each one works and how to run it.

| Strategy | Hunts for | Toolchain |
|---|---|---|
| **proptest** invariants (`tests/proptest_invariants.rs`) | engine self-consistency: find-positions monotonic, replace-with-identity is no-op, etc. | stable Rust, runs in `cargo test` |
| **cargo-fuzz** targets (`fuzz/`) | parser/engine panics, infinite loops, OOM | nightly + `cargo install cargo-fuzz` |
| **Differential fuzzer** (`examples/diff_fuzz.rs` + `DiffOracle.java`) | semantic divergence from OpenJDK | stable Rust + JDK |
| **Benchmark vs regex/fancy-regex/onig** (`examples/bench_engines.rs`) | performance regressions | stable Rust |

The differential fuzzer is the one with teeth: it runs both engines on the same randomly-generated inputs and compares match positions, captures, replacements, and splits. The proptest and cargo-fuzz layers catch panics and self-inconsistencies that wouldn't surface as a divergence (because both engines could agree on a panic, technically).

---

## proptest invariants

Eight property-based tests verify engine self-consistency without referencing an oracle. They run as part of the regular `cargo test`:

```sh
cargo test --test proptest_invariants

# Heavier run (5000 cases per invariant — ~40k total)
PROPTEST_CASES=5000 cargo test --release --test proptest_invariants
```

The invariants cover:
- `literal_flag_matches_self` — a pattern compiled with the LITERAL flag matches its source string exactly.
- `quote_roundtrip` — `Regex::quote(s)` produces a pattern whose only match is `s`.
- `parser_never_panics` — feeding any UTF-8 string to `Regex::new` does not panic (errors out gracefully).
- `engine_never_panics` — running any compiled regex over any UTF-8 input does not panic.
- `find_positions_are_monotonic` — every match in `find_iter` starts at or after the previous match's end.
- `matches_iff_anchored_find` — `re.matches(s)` is equivalent to `re.find` returning a match at position 0 covering the whole string.
- `replace_with_identity_is_noop` — `replace_all(s, "$0")` returns `s` unchanged.
- `split_find_cover_input` — `split(s)` plus the match texts in between covers `s` exactly.

These run fast and form the first line of defense against regressions.

---

## cargo-fuzz targets

Three `libFuzzer`-based targets exercise different APIs with raw-byte coverage feedback. They require nightly Rust:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz

cd fuzz
cargo +nightly fuzz run fuzz_parser    # raw-byte parser robustness
cargo +nightly fuzz run fuzz_engine    # structured AST + input + flags
cargo +nightly fuzz run fuzz_replace   # replacement-string mini-syntax
```

`fuzz_parser` feeds random byte sequences to `Regex::new` and asserts that any non-panic outcome is either `Ok(_)` or `Err(PatternSyntaxError)` — no silent wrong return values.

`fuzz_engine` uses the `Arbitrary` derives on `gen::RegexNode` to build structured patterns, plus a random input string and flag set, and runs `matches` / `find` / `replace_all` / `split` to catch panics and infinite loops.

`fuzz_replace` exercises the `appendReplacement` DSL: `$N`, `${name}`, `\$`, `\\`, literal text, with multi-digit greedy consumption.

Findings: 0 panics or hangs across cumulative ~10 CPU-hours as of the latest run.

---

## Differential fuzzer against OpenJDK

This is the tool that backs the crate's "byte-for-byte compatible with OpenJDK 25" claim. It compiles a Java oracle (`DiffOracle.java`) once and spawns it as a long-lived child process; the Rust fuzzer feeds randomly-generated patterns, inputs, and operations to both engines through JSON requests and compares responses against the local engine's output. Throughput is roughly **20 000 cases per second** on a modern desktop CPU.

```sh
javac DiffOracle.java                                              # one-time
cargo run --release --example diff_fuzz --features fuzz-gen -- 20000
```

The numeric argument is the number of test cases; 20 000 is a 1-second smoke test. Production runs use 100 000–10 000 000.

**Environment knobs.**

| Variable | Effect |
|---|---|
| `DIFF_SEED` | Deterministic seed for reproducibility (default: time-derived) |
| `DIFF_JAVA` | Path to the `java` binary to use (default: `java` from PATH) |
| `DIFF_CLASSPATH` | Where `DiffOracle.class` lives (default: current directory) |
| `DIFF_LOG` | JSONL file to append every mismatch — invaluable for analysis |

**Current status.** The latest 700 000-case batch (8 seeds: 20, 32, 42, 100, 200, 300, 400, 500) produced **zero** semantic divergences on BMP inputs. Inputs containing supplementary characters produce position-offset differences only (matched text always agrees) — this is the documented, intentional [UTF-16 / UTF-8 gap](https://github.com/MathieuDutSik/java_regex/blob/main/DIFFERENCES.md).

**Reading a mismatch log.** Each line in `DIFF_LOG` is one JSON object: `{"kind": "find" | "matches" | "split" | ..., "case": {"pattern", "input", "flags", ...}}`. Reduce a failure with `cargo run --release --example diff_one < case.json`.

---

## Benchmark vs other engines

```sh
sudo apt-get install -y libonig-dev          # one-time, for the onig crate
cargo run --release --example bench_engines
```

Compares `java_regex`, [`regex`](https://crates.io/crates/regex), [`fancy-regex`](https://crates.io/crates/fancy-regex), and [`onig`](https://crates.io/crates/onig) on a standard mix of literal-find, URL extraction, email matching, and an adversarial `(a+)+` "catastrophic backtracking" pattern. Output is a single table with median timings.

The benchmark is honest about where we win (Java-exact semantics, lookarounds + backrefs that `regex` won't compile) and where we lose (raw throughput on linear-search-friendly patterns where `regex`'s DFA shines).

CI runs this monthly via `.github/workflows/ci_03_benchmark.yml` — failure means a 10× regression vs the previous baseline.

---

## Reduction harness

When the differential fuzzer finds a mismatch, the case in `DIFF_LOG` is usually large. Reduce it with the test programs in the repo:

```sh
# Run a single saved case
cargo run --release --example diff_one < case.json

# Run a small fixed corpus, useful for re-verifying after a fix
cargo run --release --example batch_test
```

The `DiffTest1.java` through `DiffTest9.java` tools at the repo root drive specific reduction strategies (random shrinking, alternation pruning, etc.) against the live JVM.

---

## CI schedule

See `.github/workflows/` for the monthly schedule. The fuzz-relevant entries:

- `ci_01_tests.yml` (day 1) — `cargo test --all-targets` including the `proptest` invariants.
- `ci_03_benchmark.yml` (day 3) — runs the benchmark and prints the comparison table.

The full differential fuzzer is not in CI (it requires a long-lived JVM and several minutes per run); it is run locally before tagging a release.
