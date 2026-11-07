#!/usr/bin/env python3
"""Compare per-stack brewing fuel acceptance and two production refuel ticks."""
import argparse
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
    patches = [{}, {"!minecraft:brewing_fuel": {}},
               *[{"minecraft:brewing_fuel": {"uses": uses, "speed_multiplier": speed}}
                 for uses, speed in [(7, 2.5), (1, 0.5), (0, 1.0), (3, 0.0)]]]
    for item in ["minecraft:blaze_powder", "minecraft:stone"]:
        for count in [1, 2]:
            for patch in patches:
                yield {"item": item, "count": count, "components": patch}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--resources", type=Path, default=lab.RESOURCES)
    parser.add_argument("--profile", choices=["dev", "release"], default="release")
    args = parser.parse_args()
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / "rust-brewing-fuel-parity"
    build.mkdir(parents=True, exist_ok=True)
    fixtures = list(corpus())
    inputs = build / "inputs.txt"
    inputs.write_text("".join(json.dumps(case) + "\n" for case in fixtures), encoding="utf-8")
    classes = build / "classes"
    classes.mkdir(exist_ok=True)
    sources = lab.ROOT / "mods/horizon/src/test/java/dev/horizonlab/parity"
    tested_paths = [Path(__file__), sources / "ParityData.java", sources / "VanillaBrewingFuelOracle.java",
                    lab.ROOT / "crates/horizon-client-render/examples/brewing_fuel_probe.rs",
                    lab.ROOT / "crates/horizon-client-render/src/brewing.rs",
                    lab.ROOT / "crates/horizon-client-render/src/providers.rs",
                    lab.ROOT / "crates/horizon-items/src/items.rs",
                    lab.ROOT / "crates/horizon-client/src/menu.rs",
                    lab.ROOT / "crates/horizon-client/src/game/machines.rs",
                    lab.ROOT / "crates/horizon-items/src/item_data.rs", lab.ROOT / "Cargo.lock"]
    tested_sources = base.source_hashes(tested_paths)
    tested_fixtures = base.source_hashes([inputs])
    dependency_path = os.pathsep.join(map(str, libraries))

    def run(command, name):
        with (build / (name + ".log")).open("w", encoding="utf-8") as log:
            base.run(list(map(str, command)), stdout=log, stderr=subprocess.STDOUT)

    run(["javac", "--release", lock["minecraft"]["java_major"], "-proc:none", "-cp",
         str(jars["server"]) + os.pathsep + dependency_path, "-d", classes,
         sources / "ParityData.java", sources / "VanillaBrewingFuelOracle.java"], "compile-java")
    run(["cargo", "build", "--profile", args.profile, "-p", "horizon-client-render",
         "--example", "brewing_fuel_probe"], "compile-rust")
    target = lab.cargo_target() / ("debug" if args.profile == "dev" else "release")
    binary = target / "examples" / ("brewing_fuel_probe.exe" if os.name == "nt" else "brewing_fuel_probe")
    run([binary, args.resources, inputs, build / "rust.txt"], "rust")
    references = []
    for side, jar in jars.items():
        reference = build / (side + ".txt")
        run(["java", "-Xmx512M", "-cp", os.pathsep.join([str(classes), str(jar), dependency_path]),
             "dev.horizonlab.parity.VanillaBrewingFuelOracle", inputs, reference], side)
        matched = base.compare(reference, build / "rust.txt")
        references.append({"side": side, "jar_sha256": lab.digest(jar, "sha256"), "matched_lines": matched})
        print(f"{side}: {matched} brewing-fuel output lines match Rust", flush=True)
    base.ensure_sources_unchanged(tested_paths, tested_sources)
    base.ensure_sources_unchanged([inputs], tested_fixtures)
    lab.write_json(build / "report.json", {"schema": 1, "fixtures": len(fixtures), "references": references,
                   "sources_sha256": tested_sources, "fixtures_sha256": tested_fixtures,
                   "scope": "Native stack components, native FuelSlot/hopper predicates versus the Rust shared resolver, and two production serverTick refuels; native provider records evaluated with the vanilla registry resolver. Empty reagent avoids recipes/world simulation. Actual Rust menu routing, brew completion and hopper transfer are separate regression tests."})


if __name__ == "__main__":
    main()
