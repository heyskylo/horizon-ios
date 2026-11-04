#!/usr/bin/env python3
"""Compare MapData's saved data compound with Java 26.3's real map CODEC.

All six default/nondefault combinations, scales 0..4, required fields, full
color bytes and nonempty banner/frame contents are compared as typed NBT.
This does not exercise menus, marker behavior, rendering or save/reopen.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def corpus():
    for mask, scale in [*((mask, int(bool(mask & 1))) for mask in range(64)),
                        *((63, scale) for scale in (2, 3, 4))]:
        yield dict(name=f"flags-{mask:02d}-scale-{scale}", scale=scale,
                   locked=bool(mask & 2), tracking=not bool(mask & 4),
                   unlimited=bool(mask & 8), banner=bool(mask & 16),
                   frame=bool(mask & 32), x=-64 if mask & 1 else 128,
                   z=192 if mask & 1 else 0,
                   dimension="minecraft:the_nether" if mask & 1 else "minecraft:overworld")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=lab.LOCAL / "map-saved-data-parity")
    parser.add_argument("--java-only", action="store_true")
    parser.add_argument("--allow-gaps", action="store_true")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    (out / "report.json").unlink(missing_ok=True)
    cases = list(corpus())
    inputs = out / "cases.json"
    inputs.write_text(json.dumps(cases, sort_keys=True) + "\n", encoding="utf-8")
    lock, jars, libraries = base.verify_artifacts()
    source = lab.ROOT / "mods/horizon/src/test/java/dev/horizonlab/parity/VanillaMapSavedDataOracle.java"
    tested_sources = base.source_hashes([
        Path(__file__), source,
        lab.ROOT / "mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java",
        lab.ROOT / "crates/horizon-client/src/game/maps.rs",
        lab.ROOT / "crates/horizon-client/src/game/maps/save_tests.rs",
        lab.ROOT / "crates/horizon-client/Cargo.toml",
        lab.ROOT / "Cargo.toml", lab.ROOT / "Cargo.lock"])
    classes = out / "classes"
    classes.mkdir(exist_ok=True)
    classpath = os.pathsep.join(map(str, [jars["server"], *libraries]))

    def run(command, name, env=None):
        with (out / (name + ".log")).open("w", encoding="utf-8") as log:
            base.run(list(map(str, command)), stdout=log, stderr=subprocess.STDOUT, env=env)

    run(["javac", "--release", lock["minecraft"]["java_major"], "-proc:none",
         "-sourcepath", lab.ROOT / "mods/horizon/src/test/java", "-cp", classpath,
         "-d", classes, source], "compile-java")
    java = out / "java.txt"
    run(["java", "-Xmx1G", "-cp", str(classes) + os.pathsep + classpath,
         "dev.horizonlab.parity.VanillaMapSavedDataOracle", inputs, java], "java")
    reference = java.read_text(encoding="utf-8").splitlines()
    assert len(reference) == len(cases)
    assert [line.split(" ", 1)[0] for line in reference] == [case["name"] for case in cases]
    if args.java_only:
        print(f"Fresh Java map CODEC: {len(reference)} records; Rust not run.")
        return
    rust = out / "rust.txt"
    rust.unlink(missing_ok=True)
    env = dict(os.environ, HORIZON_MAP_SAVE_CASES=str(inputs), HORIZON_MAP_SAVE_OUTPUT=str(rust))
    run(["cargo", "test", "-p", "horizon-client", "--lib",
         "game::maps::save_tests::saved_map_codec_fixture", "--", "--ignored", "--exact"],
        "rust", env)
    actual = rust.read_text(encoding="utf-8").splitlines()
    assert len(actual) == len(reference), (len(actual), len(reference))
    mismatches = [case["name"] for case, expected, value in zip(cases, reference, actual)
                  if expected != value]
    base.ensure_sources_unchanged([
        lab.ROOT / path for path in tested_sources], tested_sources)
    report = dict(schema=1, minecraft=lock["minecraft"]["version"],
                  tested_sources=tested_sources,
                  cases=len(cases), matched=len(cases) - len(mismatches),
                  mismatches=mismatches, scope=__doc__,
                  input_sha256=hashlib.sha256(inputs.read_bytes()).hexdigest(),
                  java_sha256=hashlib.sha256(java.read_bytes()).hexdigest())
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"{report['matched']}/{len(cases)} saved-map records match")
    if mismatches and not args.allow_gaps:
        raise SystemExit(f"{len(mismatches)} mismatches; see {out / 'report.json'}")


if __name__ == "__main__":
    main()
