# OpenJDK regex quirks that this implementation faithfully reproduces

The `java.util.regex.Pattern` Javadoc is the official specification, but it leaves a lot under-specified — and where it is precise, OpenJDK's parser and engine sometimes diverge from a naive reading. Because OpenJDK is by far the dominant Java regex implementation (Oracle JDK, GraalVM, Amazon Corretto, Azul Zulu, IBM Semeru / OpenJ9 all ship its `java.base/java.util.regex.Pattern` verbatim; only Android uses something else, via ICU), the working definition of "Java regex" is "what OpenJDK does." This crate replicates OpenJDK's behavior, including the surprises listed below.

If you're porting a regex from Java, **nothing here will catch you off-guard.** If you're coming from a different regex engine or reading the Javadoc, this is the list of "huh, why does it do that?" answers.

For *intentional* divergences (where we do NOT match OpenJDK), see [DIFFERENCES.md](https://github.com/MathieuDutSik/java_regex/blob/main/DIFFERENCES.md).

---

## 1. Multiline `^` never matches at end of input

```text
Pattern:    ^             (with MULTILINE / `m` flag)
Input:      "\n"
matches():  false
find():     1 match at position 0 only
```

In multiline mode, `^` would naively be expected to match both at position 0 (start of input) and at position 1 (after the trailing `\n`). OpenJDK's `Caret.match` has an explicit `if (i == endIndex) return false;` early-out — the same quirk Perl has. So `^` never matches at the very end of input, even after a final line terminator. For empty input, position 0 *is* the end, so even unanchored `^` doesn't match in multiline mode.

OpenJDK source: `Pattern.java` `Caret.match` / `UnixCaret.match`.

---

## 2. Quantified atom is atomic when its body is "deterministic"

```text
Pattern:    \R{2}                 (also (?:\R){2}, (\R){2}, (?>\R){2}, (?i:\R){2})
Input:      "\r\n"
matches():  false
```

The naive reading: `\R` is `\r\n | [linebreak chars]`, an alternation that can match 2 chars or 1 char. So `\R{2}` on `"\r\n"` should match — iter 1 takes `\r`, iter 2 takes `\n`, total 2 chars.

OpenJDK's actual behavior: the quantifier picks one of two implementations at parse time. If the body is "deterministic" (no top-level alternation, no variable-count nested quantifier), OpenJDK uses `Curly` / `GroupCurly` whose `cmin` loop calls `atom.match` directly — and the atom's `next` field is the `accept` sink, so `\R`'s `\r\n` branch always succeeds and the `\r`-only fallback is never tried. The whole atom is effectively atomic.

When the body has alternation (e.g. `(?:a|aa){2}`), OpenJDK uses `Loop` instead, which threads the continuation through and *does* backtrack. So `(?:a|aa){2}` on `"aaa"` matches (iter 1 = `a`, iter 2 = `aa` after backtrack).

This implementation mirrors the split via `is_deterministic_body` in `src/engine.rs`.

---

## 3. `\R` backtracks in sequence but not in a quantifier

A direct consequence of #2.

```text
Pattern:    \R\n
Input:      "\r\n"
matches():  true        (sequential `\R\R`-style backtracking)

Pattern:    \R{2}
Input:      "\r\n"
matches():  false       (quantified atomic)
```

For `\R\n`, the first `\R`'s `next` field points to the literal `\n` node. When `\R`'s 2-char branch produces a position where `\n` fails, the engine falls back to `\R`'s 1-char branch, which then lets `\n` match the second char.

For `\R{2}`, the atom's `next` is the empty sink (it's just the atom in isolation), so the 2-char branch unconditionally succeeds and no backtrack happens.

OpenJDK source: `Pattern.java` `LineEnding.match`.

---

## 4. Chained `[A && B && C]` drops trailing clauses when middle has a literal after a nested class

```text
Pattern:    [abc && [\w]a && z]
Input:      "abcxz"
find():     "a", "b", "c"          (the `&& z` clause is silently dropped)
```

Compare:
```text
Pattern:    [abc && [\w] && z]   →   no match (proper 3-way intersection)
Pattern:    [abc && [x] && z]    →   no match
Pattern:    [abc && [\w]a && z]  →   matches a, b, c   ← QUIRK
```

OpenJDK's `clazz` method parses the RHS of `&&` with a loop that, on encountering a literal, calls `clazz(false)` recursively. The recursive call consumes everything up to the closing `]`, *including* any further `&&` clauses, but their effect collapses into the recursive scope's local result rather than chaining at the outer level. The trailing `&& z` becomes part of an inner intersection (`a ∩ z = ∅`), which is then *unioned* into the right operand of the outer `&&` — and unioning with the empty set is a no-op, so the outer right operand is just the original nested class.

If you write chained intersections, use explicit grouping: `[[abc && [\w]a] && z]` evaluates as you'd expect.

OpenJDK source: `Pattern.java` `clazz`, specifically the `else { unread(); rightOperand = clazz(false); }` branch.

---

## 5. Inline `(?s)` propagates across alternation, but only at the top level

```text
Pattern:    (?s)|.            on "\n"   matches: true   ← top-level (?s) leaks
Pattern:    (?s)xx|.          on "\n"   matches: true   ← still leaks even from failed branch
Pattern:    (?:(?s))|.        on "\n"   matches: false  ← wrapped in any group → scoped
Pattern:    ((?s))|.          on "\n"   matches: false  ← scoped to the capture
Pattern:    (?>(?s))|.        on "\n"   matches: false  ← scoped to the atomic group
```

Inline `(?flags)` is a compile-time directive in OpenJDK: it mutates the parser's flag state. But the change is scoped to the enclosing group — once parsing exits a group of any kind (capturing, non-capturing, atomic, lookaround), the flag state is restored. Only at the *top* level (no surrounding group) does the flag change persist past the surrounding `|` alternation.

Because Rust's engine evaluates flags at match time (the parser emits `SetFlags` nodes), this implementation prepends a `SetFlags(branch_start_flags)` node to each parsed branch *and* save/restores `self.flags` around `parse_pattern` calls inside every group kind. The combined effect reproduces both rules: top-level alternation propagates, group-internal flag changes do not escape.

OpenJDK source: `Pattern.java` parser `self.flags` mutation in inline-flag handling, plus per-group bracketing in `group0()`.

---

## 6. Case-folded range membership uses the input char, not the endpoints

```text
Pattern:    [1-c]         (with CASE_INSENSITIVE / `i` flag)
Input:      "g"
matches():  true
```

The naive interpretation is "fold the range endpoints into a single-case form and test against that." Under that rule `[1-c]` with `/i` would become `[1-C]` (or `[1-c]`), and `g` would be outside. OpenJDK instead checks whether the input char *or its case variant* lies inside the original (unmodified) range. `G` (0x47) is inside the range `[1-c]` (0x31..0x63), so `g`'s uppercase satisfies membership.

OpenJDK source: `Pattern.java` `CIRange` predicate construction in `range` method.

---

## 7. `split()` suppresses a leading empty for zero-width matches at position 0

```text
Pattern:    \Q\E          (zero-width)
Input:      "\t"
split():    ["\t"]        (no leading empty)
```

OpenJDK's `Pattern.split` has the explicit check `if (index == 0 && index == m.start() && m.start() == m.end()) continue;` — when the first match is zero-width at position 0, the would-be leading empty string is suppressed. This is documented behavior in the Javadoc (under `split(CharSequence input)`), but it's still surprising the first time you see it next to a non-zero-width match where empty strings *are* produced.

OpenJDK source: `Pattern.java` `split(CharSequence input, int limit)`.

---

## 8. Group captures leak across `find()` start positions and from failed lookaround inner attempts

```text
Pattern:    (?=(\w))*\s
Input:      "a "
find()[0]:  start=1, end=2, text=" ", g1="a"     ← g1 leaks from failed pos 0 attempt
```

```text
Pattern:    (?<!(a|bb))c?
Input:      "ac"
find()[1]:  start=2, end=2, text="",  g1="a"     ← g1 leaks from failed pos 1 attempt
```

OpenJDK's `Matcher` represents capture state as a single `int[] groups` shared across the entire `find()` call. The position-iterating `Start.match` loop never resets it between starting-position attempts, `Branch.match` doesn't save/restore around alternatives, and lookarounds don't save/restore around their inner match. The only construct that does save/restore is `GroupTail` (per-group), and only when the *rest* of the pattern fails after the group successfully completed. So:

- Captures set during a failed `find()` attempt persist into subsequent attempts.
- Captures set inside a negative lookaround that's then inverted to failure persist.
- Captures set inside an inner branch that fails are typically restored only at the boundary of the immediately enclosing group's `GroupTail`.

This implementation reproduces all of this: `try_match_at_persistent` carries State across iterations, `check_lookbehind` shares State directly, `Lookahead` shares State directly, and `match_greedy`/`match_reluctant` no longer save/restore around their atom attempts.

OpenJDK source: `Pattern.java` `Start.match`, `Branch.match`, `NotBehind.match`, `Pos`/`Neg` lookahead; absence of save/restore in those, presence of save/restore in `GroupTail.match`.

---

## Summary

| # | Quirk | OpenJDK source class |
|---|---|---|
| 1 | Multiline `^` never matches at end of input | `Caret` / `UnixCaret` |
| 2 | Quantified deterministic atom is atomic | `Curly` / `GroupCurly` |
| 3 | `\R` backtracks in sequence, atomic in quantifier | `LineEnding` |
| 4 | Chained `&&` drops trailing clause when middle has literal after nested class | `clazz` parser |
| 5 | Inline `(?s)` propagates across alternation branches | parser `self.flags` |
| 6 | `[1-c]/i` matches `g` (input case-folded against unmodified range) | `range` |
| 7 | `split()` suppresses leading empty for zero-width match at pos 0 | `Pattern.split` |
| 8 | Capture state leaks across find positions and from failed lookarounds | `Start.match`, `Branch.match`, `Lookahead`, `NotBehind` |

All eight behaviors are tested as regressions in `src/lib.rs` (`test_multiline_caret_not_at_end_of_input`, `test_quantified_deterministic_atom_is_atomic`, `test_linebreak_is_not_atomic`, `test_chained_intersection_mirrors_openjdk_quirk`, `test_inline_flags_propagate_across_alternation_branches`, `test_case_insensitive_range_outside_ascii_bounds`, `test_split_zero_width_at_start_no_leading_empty`, `test_capture_leak_across_find_positions`, `test_capture_leak_from_negative_lookbehind`).
