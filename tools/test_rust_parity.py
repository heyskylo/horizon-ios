#!/usr/bin/env python3
"""Compare Horizon core operations with both checksum-pinned vanilla game JARs."""

import argparse
from collections import Counter
import itertools
import json
import os
from pathlib import Path
import platform
import random
import subprocess
import zipfile

try:
    from . import lab
except ImportError:
    import lab


def corpus():
    """Stable inputs; Java supplies expected results instead of a second port."""
    rng = random.Random(0x484F52495A4F4E)
    edges = [-2147483648, -33554433, -33554432, -2097152, -2049, -2048,
             -1025, -1024, -513, -512, -33, -32, -17, -16, -1, 0, 1,
             15, 16, 31, 32, 511, 512, 2047, 2048, 2097151, 33554431, 33554432, 2147483647]
    for axis in range(3):
        for edge in edges:
            p = [0, 0, 0]
            p[axis] = edge
            yield f"position {' '.join(map(str, p))} 1 -1 2147483647"
    for x, y, z in itertools.product(range(16), repeat=3):
        yield f"position {x - 16} {y - 16} {z - 16} 0 0 0"
    for _ in range(8192):
        values = [rng.randrange(-2147483648, 2147483648) for _ in range(6)]
        yield "position " + " ".join(map(str, values))
    seeds = [-9223372036854775808, -281474976710656, -1, 0, 1, 281474976710656,
             9223372036854775807, *[rng.randrange(-2**63, 2**63) for _ in range(16)]]
    for kind in ["legacy", "xoroshiro"]:
        for seed in seeds:
            yield f"{kind} {seed} 2200"
    for lo, hi in [(0, 0), (0, 1), (1, 0), (-1, -1), (-9223372036854775808, 9223372036854775807)]:
        yield f"xoroshiro_state {lo} {hi} 2200"
    for bits in range(1, 33):
        per_word = 64 // bits
        for length in sorted({0, 1, per_word - 1, per_word, per_word + 1, 63, 64, 65, 4096}):
            yield f"storage {bits} {length} {rng.randrange(-2**63, 2**63)}"
    names = [[], [0], [0xffff], [0xd800], [0xdc00], [0xd800, 0x61, 0xdc00], [0xd83d, 0xde00], [0xdbff, 0xdfff],
             *[[ord(c) for c in text] for text in ["minecraft:temperature", "octave_-3", "Aa", "BB", "Horizon", "àé中"]],
             *[[0x61] * n for n in [55, 56, 63, 64, 65, 119, 120, 127, 128, 129, 1024]]]
    for scenario in range(2200):
        name = names[scenario % len(names)] if scenario % 3 else [rng.randrange(65536) for _ in range(rng.randrange(160))]
        encoded = "".join(f"{unit:04x}" for unit in name) or "-"
        lo = seeds[scenario % len(seeds)] if scenario % 3 else rng.randrange(-2**63, 2**63)
        hi = lo if scenario % 11 == 0 else rng.randrange(-2**63, 2**63)
        position = [rng.choice(edges) if scenario % 4 else rng.randrange(-2**31, 2**31) for _ in range(3)]
        seed = lo if scenario % 7 == 0 else rng.randrange(-2**63, 2**63)
        for kind in ["legacy", "xoroshiro"]:
            yield f"factory {kind} {scenario % 2} {lo} {hi} {' '.join(map(str, position))} {seed} {encoded} 24"
    double_edges = [0, 1, 0x8000000000000000, 0x3ff0000000000000, 0xbff0000000000000,
                    0x7fefffffffffffff, 0x7ff0000000000000, 0xfff0000000000000, 0x7ff8000000000000]
    float_edges = [0, 1, 0x80000000, 0x3f800000, 0xbf800000, 0x7f7fffff, 0x7f800000, 0xff800000, 0x7fc00000]
    interval_edges = [-(2**31), -(2**31)+1, -1, 0, 1, 2, 16, 2**30+1, 2**31-2, 2**31-1]
    intervals = [*itertools.product(interval_edges, repeat=2),
                 *[(rng.randrange(-2**31,2**31),rng.randrange(-2**31,2**31)) for _ in range(2400)]]
    for scenario, (minimum, maximum) in enumerate(intervals):
        d = [rng.choice(double_edges) if scenario % 3 else rng.getrandbits(64) for _ in range(2)]
        f = [rng.choice(float_edges) if scenario % 3 else rng.getrandbits(32) for _ in range(2)]
        for kind in ["legacy", "xoroshiro"]:
            yield f"helpers {kind} {seeds[scenario % len(seeds)]} {minimum} {maximum} {d[0]:016x} {d[1]:016x} {f[0]:08x} {f[1]:08x}"


def verify_artifacts():
    """Authenticate against the lock and the bundled checksums, not generated reports."""
    lock, directory = lab.config()
    manifest_path = directory / "manifest.json"
    if not lab.manifest_accepted(lock, lab.digest(manifest_path, "sha1")):
        raise ValueError("Manifest checksum changed; run tools/lab.py prepare")
    manifest = json.loads(manifest_path.read_text())
    if manifest["id"] != lock["minecraft"]["version"]:
        raise ValueError("Manifest version differs from the lock")
    jars = {}
    for side in ["client", "server"]:
        jar = directory / "jars" / f"{side}.jar"
        if lab.digest(jar, "sha1") != manifest["downloads"][side]["sha1"]:
            raise ValueError(f"{side} artifact checksum changed; run tools/lab.py prepare")
        jars[side] = jar
    libraries = []
    with zipfile.ZipFile(jars["server"]) as archive:
        versions = list(lab.bundle_entries(archive, "META-INF/versions.list"))
        if len(versions) != 1 or versions[0][1] != manifest["id"]:
            raise ValueError("Unexpected server bundle")
        checksum, _, _ = versions[0]
        jars["server"] = directory / "jars/server-game.jar"
        if lab.digest(jars["server"], "sha256") != checksum:
            raise ValueError("Inner server artifact checksum changed")
        for checksum, _, relative in lab.bundle_entries(archive, "META-INF/libraries.list"):
            library = directory / "libraries/server" / relative
            if lab.digest(library, "sha256") != checksum:
                raise ValueError(f"Dependency checksum changed: {relative}")
            libraries.append(library)
    return lock, jars, libraries


def compare(reference, candidate):
    # Text mode retains cross-platform universal-newline behavior. Equal chunks
    # avoid a Python loop for every line in multi-gigabyte differential traces.
    count = 0
    last = ''
    with reference.open(encoding='utf-8') as expected, candidate.open(encoding='utf-8') as actual:
        while True:
            left, right = expected.read(1024 * 1024), actual.read(1024 * 1024)
            if left != right:
                break
            if not left:
                if not last:
                    raise AssertionError('Empty output cannot establish parity')
                return count + (last != '\n')
            count += left.count('\n')
            last = left[-1]
    # Preserve the existing exact first-line mismatch diagnostic. Mismatches are
    # exceptional; successful runs use only the bounded chunk buffers above.
    with reference.open(encoding='utf-8') as expected, candidate.open(encoding='utf-8') as actual:
        for line, (left, right) in enumerate(itertools.zip_longest(expected, actual), 1):
            if left != right:
                raise AssertionError(f'Parity mismatch at output line {line}: vanilla={left!r}, Rust={right!r}')
    raise AssertionError('Parity outputs changed during comparison')

def run(command, **kwargs):
    # A hang guard only: a loaded machine (14 harnesses at once on the Mac mini) can take minutes for one oracle.
    subprocess.run(command, cwd=lab.ROOT, check=True, timeout=900, **kwargs)


def repo_path(path):
    """`path` relative to the repository. Its absolute spelling first: a worktree's `.local` may be a
    link (a junction on Windows) to another checkout's, which resolving would leave; then its
    resolved one, for a root reached through a link (macOS's /var is /private/var)."""
    for candidate in (Path(os.path.abspath(path)), Path(path).resolve()):
        try:
            return str(candidate.relative_to(lab.ROOT))
        except ValueError:
            pass
    raise ValueError(f"{path} is outside {lab.ROOT}")


def source_hashes(sources):
    return {repo_path(path): lab.digest(Path(path), "sha256") for path in sources}


def ensure_sources_unchanged(sources, expected):
    actual = source_hashes(sources)
    changed = sorted(key for key in expected.keys() | actual.keys() if expected.get(key) != actual.get(key))
    if changed:
        raise RuntimeError("Sources changed during validation: " + ", ".join(changed))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare", action="store_true", help="Download and verify the pinned reference artifacts first")
    args = parser.parse_args()
    build = lab.LOCAL / "rust-parity"
    build.mkdir(parents=True, exist_ok=True)
    # Remove a previous success marker before any build or comparison can fail.
    report_path = build / "report.json"
    report_path.unlink(missing_ok=True)
    if args.prepare:
        lab.prepare()
    lock, jars, libraries = verify_artifacts()
    commands = list(corpus())
    inputs = build / "inputs.txt"
    inputs.write_text("\n".join(commands) + "\n", encoding="utf-8")
    source = lab.ROOT / "mods/horizon/src/test/java/dev/horizonlab/parity/VanillaCoreOracle.java"
    sources = [source, Path(__file__), *sorted((lab.ROOT / "crates/horizon-core").rglob("*.rs")),
               lab.ROOT / "crates/horizon-core/Cargo.toml", lab.ROOT / "Cargo.toml", lab.ROOT / "Cargo.lock"]
    tested_sources = source_hashes(sources)
    classes = build / "classes"
    classes.mkdir(exist_ok=True)
    dependency_path = os.pathsep.join(map(str, libraries))
    run(["javac", "--release", str(lock["minecraft"]["java_major"]), "-proc:none",
         "-cp", os.pathsep.join([str(jars["server"]), dependency_path]), "-d", str(classes), str(source)])
    run(["cargo", "build", "--release", "--bin", "horizon-parity"])
    # Ask Cargo for its artifact path so custom target directories work too.
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=lab.ROOT, text=True))
    binary = Path(metadata["target_directory"]) / "release" / ("horizon-parity.exe" if os.name == "nt" else "horizon-parity")
    candidate = build / "rust.txt"
    with inputs.open("rb") as stdin, candidate.open("wb") as stdout:
        run([str(binary)], stdin=stdin, stdout=stdout)
    references = []
    for side, jar in jars.items():
        reference = build / f"vanilla-{side}.txt"
        run(["java", "-Xmx512M", "-cp", os.pathsep.join([str(classes), str(jar), dependency_path]),
             "dev.horizonlab.parity.VanillaCoreOracle", str(inputs), str(reference)])
        matched = compare(reference, candidate)
        references.append({"side": side, "jar_sha256": lab.digest(jar, "sha256"),
                           "output_sha256": lab.digest(reference, "sha256"), "matched_output_lines": matched})
        print(f"{side}: {matched:,} output lines match the Rust implementation", flush=True)
    ensure_sources_unchanged(sources, tested_sources)
    report = {"schema": 1, "minecraft": lock["minecraft"]["version"], "platform": platform.platform(),
              "java": subprocess.check_output(["java", "--version"], text=True).splitlines()[0],
              "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
              "manifest_sha1": lock["minecraft"]["manifest_sha1"], "commands": dict(Counter(line.split()[0] for line in commands)),
              "inputs_sha256": lab.digest(inputs, "sha256"), "rust_output_sha256": lab.digest(candidate, "sha256"),
              "sources_sha256": tested_sources,
              "references": references,
              "scope": "Selected integer positions, seeded RNG operations including inclusive/exclusive integer intervals and float/double triangular draws, positional factories, UTF-16 Java string hashing, MD5-derived seeds and packed-storage operations. Floating-point helper NaNs canonicalized; finite results and signed zeros compared by bits. No gameplay, rendering or performance claim."}
    lab.write_json(report_path, report)
    print(f"Scoped parity checks passed: {report_path}")


if __name__ == "__main__":
    main()
