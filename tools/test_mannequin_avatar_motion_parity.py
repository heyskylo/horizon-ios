#!/usr/bin/env python3
"""Compare actual native avatar-state motion with the real remote tick helper.

Mannequin and player call ordering, cloak/history, wing angles/counter, swim
interpolation and raw bob/walk bits are checked across movement, crouching,
gliding/cancel, tiny velocity, swimming, death and frozen controls. The oracle
executes LivingEntity.tick with isolated environment/AI endpoints plus actual
ElytraAnimationState and ClientAvatarState; it does not simulate a native world,
network interpolation, skin completion, or compare GPU pixels.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

try:
    from . import lab, test_rust_parity as base, test_model_baking_parity as baking
except ImportError:
    import lab
    import test_rust_parity as base
    import test_model_baking_parity as baking


def corpus():
    motions = [
        {"position": [0, 64, 0], "velocity": [0, 0, 0.25]},
        {"position": [0, 64, 0.25], "velocity": [0, 0, 0.25]},
        {"position": [0, 64, 0.5], "velocity": [0, 0, 0.25]},
        {"position": [0, 64, 0.5], "velocity": [0, 0, 0]},
        {"position": [20, 80, 20], "velocity": [0, 0, 0]},
        {"position": [20.25, 80, 20], "velocity": [0.25, 0, 0]},
        {"position": [20.25, 80, 20], "velocity": [0, 0, 0], "pose": 5, "sneaking": True},
        {"position": [20.25, 80, 20.25], "velocity": [0, -0.1, 0.3], "pose": 1, "flying": True},
        {"position": [20.25, 80, 20.25], "velocity": [0, -0.0000001, 0], "pose": 1, "flying": True},
        {"position": [20.25, 80, 20.25], "velocity": [0, 0, 0], "pose": 1},
        {"position": [20.25, 80, 20.25], "velocity": [0.1, 0, 0], "pose": 3, "in_water": True},
        {"position": [20.25, 80, 20.25], "velocity": [0.1, 0, 0], "pose": 3, "in_water": True},
        {"position": [20.25, 80, 20.25], "velocity": [0, 0, 0]},
        {"position": [20.25, 80, 20.25], "velocity": [0, 0, 0]},
        {"position": [20.25, 80, 20.25], "velocity": [0, 0, 0], "pose": 5},
        {"position": [20.25, 80, 20.25], "velocity": [0, 0, 0], "sneaking": True},
    ]
    for player in (False, True):
        for state in ("alive", "dead", "frozen"):
            name = ("player" if player else "mannequin") + "_" + state
            yield {
                "name": name,
                "entity": "minecraft:player" if player else "minecraft:mannequin",
                "player": player,
                "running": state != "frozen",
                "alive": state != "dead",
                "on_ground": True,
                # Deliberately separate the mannequin's visual crouching pose
                # from its sneak/name-tag flag. Keep the existing player policy
                # as a control with consistent player flags and pose.
                "motions": [dict(motion, sneaking=motion.get("pose", 0) == 5)
                            if player else motion for motion in motions],
            }


def verify(native, rust):
    assert len(native) == len(rust) == 96, (len(native), len(rust))
    for expected, actual in zip(native, rust):
        assert expected == actual, f"native={expected}, Horizon={actual}"
    # Mannequins never call the player's updateBob/addWalkDistance paths.
    for row in rust:
        if row["case"].startswith("mannequin_"):
            assert row["bob_bits"] == row["bob_old_bits"] == row["walk_bits"] == row["walk_old_bits"] == 0, row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=("fast", "dev"), default="fast")
    parser.add_argument("--output", type=Path, default=lab.LOCAL / "mannequin-avatar-motion-parity")
    parser.add_argument("--rust-results", type=Path, help="compare an existing Rust capture instead of rebuilding/running the probe")
    args = parser.parse_args()
    lock, jars, _ = base.verify_artifacts()
    dependencies = os.pathsep.join(map(str, [jars["client"], *baking.client_libraries()]))
    build = args.output.resolve()
    classes = build / "classes"
    classes.mkdir(parents=True, exist_ok=True)
    (build / "input.json").write_text(json.dumps(list(corpus())))
    oracle = lab.ROOT / "mods/horizon/src/test/java/dev/horizonlab/parity/VanillaMannequinAvatarOracle.java"
    commands = [
        ("compile-java", ["javac", "--release", str(lock["minecraft"]["java_major"]), "-proc:none", "-nowarn",
                          "-cp", dependencies, "-d", str(classes), str(oracle)]),
        ("vanilla", ["java", "--sun-misc-unsafe-memory-access=allow", "-cp", str(classes) + os.pathsep + dependencies,
                     "dev.horizonlab.parity.VanillaMannequinAvatarOracle", str(build / "input.json"), str(build / "vanilla.jsonl")]),
    ]
    if args.rust_results is None:
        commands.append(("rust", ["cargo", "run", "--quiet", "--locked", "--profile", args.profile,
                                  "-p", "horizon-client-lib", "--example", "client_probe", "--",
                                  "mannequin_avatar_fixture", str(build / "input.json"), str(lab.RESOURCES), str(build / "rust.jsonl")]))
    for name, command in commands:
        with (build / (name + ".log")).open("w") as log:
            result = subprocess.run(command, cwd=build if name == "vanilla" else lab.ROOT, env=lab.game_env(), stdout=log,
                                    stderr=subprocess.STDOUT, timeout=1800)
        if result.returncode:
            raise RuntimeError(f"{name} failed: {(build / (name + '.log')).read_text()[-4000:]}")
    native = [json.loads(line) for line in (build / "vanilla.jsonl").read_text().splitlines()]
    rust_path = args.rust_results or build / "rust.jsonl"
    rust = [json.loads(line) for line in rust_path.read_text().splitlines()]
    verify(native, rust)
    print("96/96 mannequin/player avatar-state/wing/swim motion cases match actual vanilla 26.3; frozen policy controls preserved")


if __name__ == "__main__":
    main()
