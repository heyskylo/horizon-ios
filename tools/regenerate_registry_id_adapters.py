#!/usr/bin/env python3
"""Regenerate every checked-in registry adapter from official prepared reports.

Each VERSION directory needs jars/client.jar and datagen/reports/{registries,
blocks}.json from the same official release (docs/native-client.md). --versions
defaults to version_port.py's HORIZON_VERSIONS store; --new accepts a separately
prepared pin. Without --new the pin comes from the repository's .local/.
Nothing is downloaded. --check compares rustfmt output without changing the tree.
"""

import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

try:
    from . import version_port
except ImportError:
    import version_port

ROOT = Path(__file__).resolve().parents[1]
ADAPTER = Path("crates/horizon-protocol/src/adapter")
RENAMES = [
    "block:minecraft:chain=minecraft:iron_chain",
    "item:minecraft:chain=minecraft:iron_chain",
    "command_argument_type:minecraft:color=minecraft:team_color",
    "entity_type:minecraft:potion=minecraft:splash_potion",
]
ATTRIBUTES = {
    "generic": "armor armor_toughness attack_damage attack_knockback attack_speed burning_time "
               "explosion_knockback_resistance fall_damage_multiplier flying_speed follow_range gravity "
               "jump_strength knockback_resistance luck max_absorption max_health movement_efficiency "
               "movement_speed oxygen_bonus safe_fall_distance scale step_height water_movement_efficiency",
    "player": "block_break_speed block_interaction_range entity_interaction_range mining_efficiency "
              "sneaking_speed submerged_mining_speed sweeping_damage_ratio",
    "zombie": "spawn_reinforcements",
}
COMPONENT_RENAMES = [
    "entity_type:minecraft:boat=minecraft:oak_boat",
    "entity_type:minecraft:chest_boat=minecraft:oak_chest_boat",
    "data_component_type:minecraft:fire_resistant=minecraft:damage_resistant",
] + [f"attribute:minecraft:{prefix}.{name}=minecraft:{name}"
     for prefix, names in ATTRIBUTES.items() for name in names.split()]
LEGACY_RENAMES = [
    "item:minecraft:scute=minecraft:turtle_scute",
    "attribute:minecraft:horse.jump_strength=minecraft:jump_strength",
    "particle_type:minecraft:ambient_entity_effect=minecraft:entity_effect",
    "sound_event:minecraft:entity.generic.wind_burst=minecraft:entity.breeze.wind_burst",
]
LEGACY_NAMES = "item=OLD_ITEM block entity_type block_entity_type attribute sound_event mob_effect " \
               "potion enchantment painting_variant banner_pattern instrument recipe_serializer"
# (inherit, substitutes, item names, extra renames, static registry names, legacy aliases)
RELEASES = {
    "1.20.2": ("1.20.3", False, True, COMPONENT_RENAMES + LEGACY_RENAMES + [
        "block:minecraft:grass=minecraft:short_grass", "item:minecraft:grass=minecraft:short_grass"], LEGACY_NAMES, True),
    "1.20.3": ("1.20.6", False, True, COMPONENT_RENAMES + LEGACY_RENAMES, LEGACY_NAMES, True),
    "1.20.6": ("1.21.1", True, True, COMPONENT_RENAMES, "enchantment painting_variant", False),
    "1.21.1": ("1.21.3", True, True, COMPONENT_RENAMES, "", False),
    "1.21.3": ("1.21.4", True, False, ["entity_type:minecraft:creaking_transient=minecraft:creaking"], "", False),
    "1.21.4": ("1.21.5", True, False, [], "", False),
    "1.21.5": ("1.21.6", True, False, [], "", False),
    "1.21.6": ("1.21.8", True, False, [], "", False),
    "1.21.8": (None, True, False, [], "", False),
    "1.21.10": (None, False, False, [], "", False),
    "1.21.11": (None, False, False, [], "", False),
    "26.1.2": (None, False, False, [], "", False),
    "26.2": (None, True, False, [], "", False),
}


def release_args(version, versions):
    inherit, substitutes, item_names, renames, names, aliases = RELEASES[version]
    args = ["--old", str(versions / version)]
    if inherit:
        args += ["--inherit", str(versions / inherit)]
    for enabled, option in ((substitutes, "--substitutes"), (item_names, "--item-names"),
                            (aliases, "--name-aliases")):
        if enabled:
            args.append(option)
    for rename in RENAMES + renames:
        args += ["--rename", "minecraft:" + rename]
    for name in names.split():
        args += ["--registry-names", "minecraft:" + name]
    if version == "1.20.3":
        args += ["--reassign", "minecraft:entity_type:minecraft:wind_charge=minecraft:breeze_wind_charge"]
    if version == "1.21.3":
        for axis in "xyz":
            for old, new in (("disabled", "uprooted"), ("dormant", "dormant"), ("active", "awake")):
                for natural in ("false", "true"):
                    args += ["--state-alias", f"minecraft:creaking_heart[axis={axis},creaking={old}]="
                             f"minecraft:creaking_heart[axis={axis},creaking_heart_state={new},natural={natural}]"]
    return args


def sync_generated(generated, check):
    stale = []
    for source in generated:
        target = ROOT / ADAPTER / source.name
        if not target.exists() or target.read_text(encoding="utf-8") != source.read_text(encoding="utf-8"):
            stale.append(source.name)
            if not check:
                target.write_bytes(source.read_bytes())
    if check and stale:
        print("Registry adapters need regeneration: " + ", ".join(stale), file=sys.stderr)
        return 1
    print(f"Registry adapters {'checked' if check else 'regenerated'}: {len(generated)} files")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--versions", type=Path, default=version_port.STORE, help="prepared version store")
    parser.add_argument("--new", type=Path, help="prepared pinned version (default: repository .local)")
    parser.add_argument("--only", nargs="+", choices=RELEASES, help="regenerate selected releases only")
    parser.add_argument("--check", action="store_true", help="fail on stale generated files; write nothing")
    args = parser.parse_args(argv)
    versions = args.only or list(RELEASES)
    required = set(versions) | {RELEASES[v][0] for v in versions if RELEASES[v][0]}
    for version in sorted(required):
        for relative in ("jars/client.jar", "datagen/reports/registries.json", "datagen/reports/blocks.json"):
            if not (args.versions / version / relative).is_file():
                parser.error(f"missing prepared input: {args.versions / version / relative}; "
                             "see registry mapping inputs in docs/native-client.md")
    common = ["--new", str(args.new)] if args.new else []
    generator = [sys.executable, str(ROOT / "tools/generate_registry_id_adapter.py")]
    with tempfile.TemporaryDirectory(prefix="horizon-registry-adapters-") as temporary:
        output = Path(temporary)
        generated = []
        for version in versions:
            destination = output / ("v" + version.replace(".", "_") + "_ids.rs")
            subprocess.run(generator + release_args(version, args.versions) + common + ["--out", str(destination)],
                           check=True, stdout=subprocess.DEVNULL)
            generated.append(destination)
        pinned = output / "pinned_ids.rs"
        subprocess.run(generator + ["--pinned-constants"] + common + ["--out", str(pinned)],
                       check=True, stdout=subprocess.DEVNULL)
        generated.append(pinned)
        subprocess.run(["rustfmt", "--edition", "2024", *(str(p) for p in generated)], check=True)
        return sync_generated(generated, args.check)


if __name__ == "__main__":
    sys.exit(main())
