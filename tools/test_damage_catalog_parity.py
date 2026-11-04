#!/usr/bin/env python3
"""Compare damage resource decoding with the pinned Java DamageType codec."""
import argparse
import hashlib
import itertools
from decimal import Decimal, localcontext
import struct
import json
import os
from pathlib import Path
import subprocess
import lab


def corpus():
    defaults = dict(message_id="test", scaling="never", exhaustion=0.1)
    yield defaults
    for key in defaults:
        yield {k: v for k, v in defaults.items() if k != key}
    for value in [None, True, 0, "", [], {}, {"message_id": "test"}]:
        yield value
    for message, scaling, exhaustion in itertools.product(
        ["test", "", "with spaces", None, False, 3],
        ["never", "always", "when_caused_by_living_non_player", "ALWAYS", None],
        [-0.0, 0, 0.1, -1, 1e-30, 1e30, 1e100, 1.0000000596046448, "0.1", True, None],
    ):
        yield dict(message_id=message, scaling=scaling, exhaustion=exhaustion)
    for effects, death in itertools.product(
        [None, "hurt", "thorns", "drowning", "burning", "poking", "freezing", "invalid", 1],
        [None, "default", "fall_variants", "intentional_game_design", "invalid", False],
    ):
        yield dict(defaults, effects=effects, death_message_type=death)
    yield dict(defaults, ignored_extension={"value": [1, 2, 3]})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game-dir", type=Path, default=lab.GAME)
    args = parser.parse_args()
    build = lab.LOCAL / "rust-damage-catalog-parity"
    build.mkdir(parents=True, exist_ok=True)
    source = lab.ROOT / "mods/horizon/src/test/java/dev/horizonlab/parity/VanillaDamageCatalogOracle.java"
    sources = [source, Path(__file__).resolve(),
               lab.ROOT / "crates/horizon-world/examples/damage_catalog_probe.rs",
               lab.ROOT / "crates/horizon-world/src/damage_source.rs",
               lab.ROOT / "crates/horizon-world/src/damage_source/resources.rs"]
    hashes = {str(p.relative_to(lab.ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    libraries = sorted((args.game_dir / "libraries/server").rglob("*.jar"))
    jar = args.game_dir / "jars/server-game.jar"
    cp = os.pathsep.join(map(str, [jar, *libraries]))
    subprocess.run(["javac", "-proc:none", "-cp", cp, "-d", str(build), str(source)], check=True)
    cases = list(corpus())
    inputs = "".join(json.dumps(v, separators=(",", ":")) + "\n" for v in cases)
    raw = [
        '{"message_id":"test","scaling":"never","exhaustion":1e400}',
        '{"message_id":"test","scaling":"never","exhaustion":-1e400}',
        '{"message_id":"test","scaling":"never","exhaustion":-0}',
        '{"message_id":"test","message_id":"last","scaling":"never","exhaustion":0.1}',
        '{"message_id":"test","scaling":"never","exhaustion":0.1,"effects":"invalid","effects":null}',
    ]
    # Decimal literals on either side of a float halfway point must be parsed
    # directly to float (an intermediate double rounds these to the midpoint).
    with localcontext() as context:
        context.prec = 200
        for bits in [0x00800000, 0x3f800000, 0x40000000, 0x4b000000, 0x7e000000]:
            lo = struct.unpack(">f", struct.pack(">I", bits))[0]
            hi = struct.unpack(">f", struct.pack(">I", bits + 1))[0]
            mid = (Decimal.from_float(lo) + Decimal.from_float(hi)) / 2
            for value in [mid, mid + (Decimal.from_float(hi) - Decimal.from_float(lo)) / Decimal(10**20)]:
                raw.append('{"message_id":"test","scaling":"never","exhaustion":' + str(value) + '}')
    cases.extend(json.loads(value) for value in raw)
    inputs += "".join(value + "\n" for value in raw)
    (build / "inputs.jsonl").write_text(inputs)
    java = subprocess.run(["java", "-cp", str(build) + os.pathsep + cp,
                           "dev.horizonlab.parity.VanillaDamageCatalogOracle"], input=inputs, text=True,
                          capture_output=True, check=True)
    (build / "java.log").write_text(java.stdout + java.stderr)
    subprocess.run(["cargo", "build", "--offline", "-p", "horizon-world", "--example", "damage_catalog_probe"], cwd=lab.ROOT, check=True)
    probe = lab.cargo_target() / "debug/examples/damage_catalog_probe"
    rust = subprocess.run([str(probe)], input=inputs, text=True, capture_output=True, check=True)
    (build / "rust.jsonl").write_text(rust.stdout)
    expected = [json.loads(line) for line in java.stdout.splitlines() if line.startswith(("[\"", "false"))]
    actual = [json.loads(line) for line in rust.stdout.splitlines()]
    assert len(expected) == len(actual) == len(cases), (len(expected), len(actual), len(cases))
    for i, (want, got) in enumerate(zip(expected, actual)):
        assert want == got, (i, cases[i], want, got)
    assert hashes == {str(p.relative_to(lab.ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}, "source changed during comparison"
    report = {"schema": 1, "sources_sha256": hashes, "server_game_sha256": hashlib.sha256(jar.read_bytes()).hexdigest(), "minecraft": lab.VERSION, "cases": len(cases), "pass": True,
              "scope": "Actual DamageType.DIRECT_CODEC acceptance, defaults, enum values, exhaustion float bits and NbtOps float/default-field encoding; registry/tag layering is covered by Rust snapshot tests."}
    (build / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Damage resource codec parity passed: {len(cases)} cases")


if __name__ == "__main__":
    main()
