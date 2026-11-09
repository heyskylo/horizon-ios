#!/usr/bin/env python3
"""Compare 82 full mob-tick wet-fire observations against the pinned vanilla server.

Reads prepared resources from HORIZON_TEST_RESOURCES, or .local's when unset. Does not start a server,
open a window, or modify the resource root. The Java fixture fixes movement and
world boundaries while retaining real Mob.tick, damage, fluids and block effects.
"""
import os
from pathlib import Path
import re
import subprocess

try:
    from . import lab, oracle_harness, test_rust_parity as base
except ImportError:
    import lab, oracle_harness, test_rust_parity as base


def vanilla_rows(text):
    rows = []
    for line in text.splitlines():
        match = re.search(r"(?:RESULT|TRANSITION|IMMUNE) (.*)", line)
        if not match:
            continue
        fields = match[1].split()
        no_ai, environment = fields[:2]
        if "RESULT " in line:
            label = f"NoAI={no_ai} {environment} fire={fields[2]}"
        elif "IMMUNE " in line:
            label = f"NoAI={no_ai} immune {environment}"
        else:
            phase = environment if fields[2] == "wet" else "dry"
            label = f"NoAI={no_ai} transition={environment}/{phase}"
        values = dict(field.split("=", 1) for field in fields if "=" in field)
        rows.append((label, (float(values["health"]), int(values["fire"]), int(values["sounds"]))))
    return rows


def native_rows(text):
    rows = []
    for match in re.finditer(r"WET_FIRE (.*?) actual=\(([^)]*)\)", text):
        health, fire, sounds = match[2].split(", ")
        rows.append((match[1], (float(health), int(fire), int(sounds))))
    return rows


def main():
    # parity.py sets no HORIZON_TEST_RESOURCES: default to the prepared resources, as the other harnesses do.
    resources = Path(os.environ.get("HORIZON_TEST_RESOURCES", lab.RESOURCES))
    if not (resources / "movement-properties.nbt").is_file():
        raise RuntimeError("HORIZON_TEST_RESOURCES must contain prepared 26.3 resources")
    build = lab.LOCAL / "wet-fire-parity"
    build.mkdir(parents=True, exist_ok=True)
    lock, jars, libraries = base.verify_artifacts()
    classes = build / "classes"
    classes.mkdir(exist_ok=True)
    dependencies = os.pathsep.join(map(str, [jars["server"], *libraries]))
    source = lab.ROOT / "mods/horizon/src/test/java/dev/horizonlab/parity/VanillaWetFireOracle.java"
    sources = [Path(__file__), source, lab.ROOT / "crates/horizon-client/src/mobs/ai.rs",
               lab.ROOT / "crates/horizon-client/src/mobs/wet_fire_tests.rs"]
    hashes = base.source_hashes(sources)
    oracle_harness.run_logged(["javac", "--release", lock["minecraft"]["java_major"],
        "-proc:none", "-sourcepath", lab.ROOT / "mods/horizon/src/test/java", "-cp",
        dependencies, "-d", classes, source], build, "compile-java")
    oracle_harness.java_run(build, classes, dependencies, "VanillaWetFireOracle", [], "vanilla")
    oracle_harness.run_logged(["cargo", "test", "--locked", "-p", "horizon-client-lib", "--lib",
        "wet_fire_damage_and_extinguishing_follow_the_full_mob_tick", "--", "--ignored", "--nocapture"],
        build, "rust", env=dict(lab.game_env(), HORIZON_TEST_RESOURCES=str(resources)))
    expected = vanilla_rows((build / "vanilla.log").read_text())
    actual = native_rows((build / "rust.log").read_text())
    # Sort by labels, retaining repeated transition labels and their tick order.
    expected.sort(key=lambda row: row[0])
    actual.sort(key=lambda row: row[0])
    assert len(expected) == len(actual) == 82, (len(expected), len(actual))
    differences = [(want, got) for want, got in zip(expected, actual) if want != got]
    assert not differences, f"Full mob-tick wet-fire observations differ: {differences}; see .local/wet-fire-parity"
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(build / "report.json", dict(
        minecraft=lock["minecraft"]["version"], sources_sha256=hashes,
        server_jar_sha256=lab.digest(jars["server"], "sha256"),
        matched_observations=len(actual),
        scope="Full stationary Mob.tick: health, fire timer and extinguish sounds; native contact-boundary RNG assertions. No full movement or rendering."))
    print("82 full mob-tick wet-fire observations match vanilla 26.3")


if __name__ == "__main__":
    main()
