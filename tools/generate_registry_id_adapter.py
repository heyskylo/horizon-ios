#!/usr/bin/env python3
"""Generate crates/horizon-protocol/src/adapter/v<OLD>_ids.rs: how an older release numbers the registry entries
the protocol sends by id (block states, items, entity types, ...) against the pinned one, from both versions'
data generator reports (registries.json, blocks.json).

    generate_registry_id_adapter.py --old ~/code/horizon-parity/versions/26.2

`--old` (and `--new`, the pinned version in .local by default) is a directory `tools/version_port.py prepare
VERSION --download` made. Entries are matched by name; block states by block name and properties. Each
registry gets two tables of runs `(from, to, length)`: `<NAME>_UP` from the old server's ids to the pinned ones,
`<NAME>_DOWN` back. An id in no run has no entry in the other version. Only registries whose ids differ are written; the rest map to themselves.

`--inherit PREPARED_VERSION` reuses that version's generated module where its reports have identical
registry or block-state ids. Both modules must target the same pinned version. Changed registries
are generated normally, so a patch release need not duplicate unchanged tables.

`--state-alias BLOCK[old=value]=BLOCK[new=value]` matches changed state properties explicitly.
Repeat an old state for several new states to map all back; its first alias is used going forward.

`--registry-names REGISTRY` emits an old static registry's names in wire-id order, for registries
that newer versions synchronize. `--item-names` additionally applies canonical item renames.

`--pinned-constants` instead emits the named pinned ids used by the translators, checked against
the pinned registry report. Regenerate these together with the tables before a pinned-version bump.

With `--substitutes` (a version a Horizon server lets join, docs/hosting.md) it also writes `<NAME>_SUBSTITUTES`: for
each pinned entry the older version lacks, the pinned entry shown in its place, picked by the rules in SUBSTITUTES
(the first whose name the older version has) and, for a block state, the substitute block's state that keeps the
most of its properties.
"""

import argparse
import json
import re
from pathlib import Path

try:
    from . import lab
except ImportError:
    import lab

ROOT = Path(__file__).resolve().parents[1]
PER_LINE = 16
NONE = 65535
# Registries the protocol sends by numeric id (ByteBufCodecs.registry / holderRegistry / idMapper over a
# registry), as the adapter reads them.
REGISTRIES = [
    "minecraft:item",
    "minecraft:entity_type",
    "minecraft:particle_type",
    "minecraft:sound_event",
    "minecraft:data_component_type",
    "minecraft:command_argument_type",
    "minecraft:custom_stat",
    "minecraft:map_decoration_type",
    "minecraft:block",
    "minecraft:block_entity_type",
    "minecraft:menu",
    "minecraft:mob_effect",
    "minecraft:attribute",
    "minecraft:stat_type",
    "minecraft:fluid",
    "minecraft:game_event",
    "minecraft:recipe_serializer",
    "minecraft:slot_display",
    "minecraft:recipe_display",
    "minecraft:recipe_book_category",
    "minecraft:villager_type",
    "minecraft:villager_profession",
    "minecraft:position_source_type",
    "minecraft:consume_effect_type",
    "minecraft:enchantment_effect_component_type",
]

# Only ids interpreted directly by translators; the remaining entries use generated remap tables.
PINNED_NAMES = {
    "entity_type": """
        acacia_boat acacia_chest_boat allay area_effect_cloud armadillo armor_stand axolotl
        bamboo_chest_raft bamboo_raft bat bee birch_boat birch_chest_boat blaze breeze camel camel_husk cat
        cave_spider cherry_boat cherry_chest_boat chest_minecart chicken cod command_block_minecart cow
        creeper dark_oak_boat dark_oak_chest_boat dolphin donkey drowned elder_guardian ender_dragon
        enderman endermite evoker experience_orb falling_block fox frog furnace_minecart ghast giant
        glow_item_frame glow_squid goat guardian happy_ghast hoglin hopper_minecart horse husk illusioner
        iron_golem item_frame jungle_boat jungle_chest_boat llama magma_cube mangrove_boat
        mangrove_chest_boat mannequin minecart mooshroom mule nautilus oak_boat oak_chest_boat ocelot
        painting panda parrot phantom pig piglin piglin_brute pillager player polar_bear pufferfish rabbit
        ravager salmon sheep shulker silverfish skeleton skeleton_horse slime sniffer snow_golem
        spawner_minecart spider spruce_boat spruce_chest_boat squid stray strider tadpole tnt_minecart
        trader_llama tropical_fish turtle vex villager vindicator wandering_trader warden witch wither
        wither_skeleton wolf zoglin zombie zombie_horse zombie_nautilus zombie_villager zombified_piglin
    """.split(),
    "item": """
        blast_furnace bolt_armor_trim_smithing_template campfire coast_armor_trim_smithing_template
        crafting_table dune_armor_trim_smithing_template eye_armor_trim_smithing_template
        flow_armor_trim_smithing_template furnace host_armor_trim_smithing_template
        raiser_armor_trim_smithing_template rib_armor_trim_smithing_template saddle
        sentry_armor_trim_smithing_template shaper_armor_trim_smithing_template
        silence_armor_trim_smithing_template smithing_table smoker snout_armor_trim_smithing_template
        spire_armor_trim_smithing_template stonecutter tide_armor_trim_smithing_template
        vex_armor_trim_smithing_template ward_armor_trim_smithing_template
        wayfinder_armor_trim_smithing_template wild_armor_trim_smithing_template
    """.split(),
    "particle_type": """
        entity_effect explosion explosion_emitter
    """.split(),
}

# What stands in for a pinned entry an older client lacks: (registries, pattern, replacement), the pattern matched
# against the whole name without its namespace and tried in order; the first replacement the older version has wins.
# A block keeps its shape (a slab for a slab), so a client's movement agrees with the server's; where no block of the
# same shape exists, the stand-in's collision holds the original's (a client walking into what the server calls solid
# is pulled back; one stopped short of it is not).
BLOCKS = ("minecraft:block", "minecraft:item")
SUBSTITUTES = [
    (BLOCKS, r"\w+_poplar_leaves", "birch_leaves"),
    (BLOCKS + ("minecraft:entity_type",), r"(.*)poplar(.*)", r"\1birch\2"),
    (BLOCKS, r"\w+_(wool|concrete)_slab", "smooth_stone_slab"),
    (BLOCKS, r"\w+_(wool|concrete)_stairs", "stone_stairs"),
    (BLOCKS, r"straw_bed", "yellow_bed"),
    (BLOCKS, r"red_shrub", "dead_bush"),
    # The shelf mushroom is a solid shelf players stand and bounce on; the brown mushroom has no collision.
    (("minecraft:block",), r"shelf_mushroom", "brown_mushroom_block"),
    (("minecraft:item",), r"shelf_mushroom", "brown_mushroom"),
    (("minecraft:item",), r"(\w+)_cushion", r"\1_carpet"),
    # The explorer maps are filled maps (MapItem): an empty map would not draw their map_id.
    (("minecraft:item",), r"\w+_map", "filled_map"),
    (("minecraft:particle_type",), r"\w+_poplar_leaves", "cherry_leaves"),
    (("minecraft:sound_event",), r"block\.poplar_leaves\.(\w+)", r"block.cherry_leaves.\1"),
    (("minecraft:sound_event",), r"entity\.cushion\.(break|place)", r"block.wool.\1"),
    (("minecraft:sound_event",), r"entity\.cushion\.\w+", "block.wool.step"),
    (("minecraft:sound_event",), r"block\.shelf_mushroom\.(\w+)", r"block.fungus.\1"),
    (("minecraft:sound_event",), r"block\.\w+\.(break|fall|hit|place|step)", r"block.grass.\1"),
]
# Registries that get substitutes, as the Substitutes fields name them.
SUBSTITUTED = ["minecraft:block", "minecraft:item", "minecraft:entity_type", "minecraft:particle_type",
               "minecraft:sound_event"]


def substitute(registry, name, has):
    """The name standing in for `name` in `registry`, one `has` accepts, or None."""
    path = name.split(":", 1)[1]
    for registries, pattern, replacement in SUBSTITUTES:
        if registry in registries and re.fullmatch(pattern, path):
            found = "minecraft:" + re.sub(pattern, replacement, path)
            if has(found):
                return found
    return None


def substitutes(registry, old, new):
    """For each pinned id (in order), the pinned id of its stand-in when `old` lacks it, or NONE."""
    by_id = sorted(new.items(), key=lambda kv: kv[1])
    out = []
    for name, _ in by_id:
        found = None if name in old else substitute(registry, name, lambda n: n in old and n in new)
        out.append(new[found] if found else NONE)
    return out


def state_substitutes(old_blocks, new_blocks):
    """For each pinned block state, the pinned state of the substitute block that keeps the most of its
    properties (its default state on a tie), when the older version lacks the block."""
    old_states, new_states = states(old_blocks), states(new_blocks)
    out = [NONE] * len(new_states)
    for name, block in new_blocks.items():
        if name in old_blocks:
            continue
        found = substitute("minecraft:block", name, lambda n: n in old_blocks and n in new_blocks)
        if not found:
            continue
        candidates = [s for s in new_blocks[found]["states"]
                      if (found, tuple(sorted(s.get("properties", {}).items()))) in old_states]
        for state in block["states"]:
            props = state.get("properties", {})
            best = max(candidates, key=lambda c: (sum(props.get(k) == v for k, v in c.get("properties", {}).items()),
                                                  c.get("default", False)))
            out[state["id"]] = best["id"]
    return out


def entries(report, registry):
    """{name: id} of one registry, checked to number its entries 0..n."""
    found = {name: e["protocol_id"] for name, e in report.get(registry, {}).get("entries", {}).items()}
    assert sorted(found.values()) == list(range(len(found))), registry
    return found


def states(blocks, renames=None):
    """{(block, properties): state id} over every state of every block."""
    renames = renames or {}
    found = {}
    for name, block in blocks.items():
        for state in block["states"]:
            key = (renames.get(name, name), tuple(sorted(state.get("properties", {}).items())))
            found[key] = state["id"]
    assert sorted(found.values()) == list(range(len(found)))
    return found


def table(source, target, renames=None, reassign=None):
    """For each id of `source` (in order), the id `target` gives the same key (or the key it was renamed
    to), or NONE."""
    renames = renames or {}
    reassign = reassign or {}
    by_id = sorted(source.items(), key=lambda kv: kv[1])
    # A removed entry may alias an entry that still exists (creaking_transient -> creaking).
    # Prefer that exact name in reverse, rather than sending the removed variant back.
    return [target.get(reassign[key], NONE) if key in reassign else
            target.get(key, target.get(renames.get(key, key), NONE)) for key, _ in by_id]


def reverse_table(source, target, renames=None, reassign=None):
    """Invert semantic assignments without mapping a reused newer name to its old meaning."""
    forward = table(source, target, renames, reassign)
    names = {index: name for name, index in source.items()}
    out = []
    for name, index in sorted(target.items(), key=lambda entry: entry[1]):
        candidates = [old for old, new in enumerate(forward) if new == index]
        exact = next((old for old in candidates if names[old] == name), None)
        out.append(exact if exact is not None else min(candidates, default=NONE))
    return out


def runs(values):
    """The mapping as runs (from, to, length): ids from..from+length map to to..to+length. Ids in no run
    have no counterpart."""
    found = []
    for i, v in enumerate(values):
        if v == NONE:
            continue
        if found and found[-1][0] + found[-1][2] == i and found[-1][1] + found[-1][2] == v:
            found[-1][2] += 1
        else:
            found.append([i, v, 1])
    return found


def item_names(source, target, renames=None):
    """Canonical item identifiers by old id, for pre-1.21.2 item-based cooldown packets."""
    renames = renames or {}
    names = []
    for name, _ in sorted(source.items(), key=lambda entry: entry[1]):
        canonical = name if name in target else renames.get(name, name)
        names.append(canonical if canonical in target else "")
    return names


def registry_names(source):
    """Static registry names in wire-id order, including any unused slots."""
    names = [""] * (max(source.values(), default=-1) + 1)
    for name, index in source.items():
        names[index] = name
    return names


def state_alias(spec):
    """Two block-state keys, keeping property order irrelevant as it is in the reports."""
    left, right = spec.split("]=", 1)

    def key(value):
        block, properties = value.rstrip("]").split("[", 1)
        return block, tuple(sorted(tuple(pair.split("=", 1)) for pair in properties.split(",") if pair))

    return key(left), key(right)


def rust_table(name, values):
    assert all(0 <= v <= NONE for v in values)
    cells = [f"({a}, {b}, {n})" for a, b, n in runs(values)]
    lines = ["#[rustfmt::skip]", f"pub const {name}: &[(u16, u16, u16)] = &["]
    for i in range(0, len(cells), PER_LINE // 2):
        lines.append("    " + ", ".join(cells[i:i + PER_LINE // 2]) + ",")
    lines.append("];")
    return lines


def const_name(registry):
    return registry.split(":", 1)[1].upper()


def pinned_constants(report, names):
    """Named ids for the translators; a removed or renamed entry must stop regeneration."""
    lines = []
    for registry, selected in names.items():
        ids = entries(report, f"minecraft:{registry}")
        lines.append(f"pub mod {registry} {{")
        for name in selected:
            key = f"minecraft:{name}"
            if key not in ids:
                raise ValueError(f"pinned registry entry is absent: minecraft:{registry} / {key}")
            lines.append(f"    pub const {name.upper()}: i32 = {ids[key]};")
        lines.append("}")
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old", type=Path, help="the older version's prepared directory")
    parser.add_argument("--new", type=Path, help="the pinned version's prepared directory (default: .local)")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--pinned-constants", action="store_true",
                        help="emit named pinned registry ids instead of an older version's tables")
    parser.add_argument("--inherit", type=Path,
                        help="another prepared version: reuse its generated tables when registry ids match")
    parser.add_argument("--substitutes", action="store_true",
                        help="also write the stand-ins for pinned entries the older version lacks")
    parser.add_argument("--rename", action="append", default=[], metavar="REGISTRY:OLD=NEW",
                        help="an entry the pinned version knows by another name, e.g. "
                             "minecraft:command_argument_type:minecraft:color=minecraft:team_color")
    parser.add_argument("--reassign", action="append", default=[], metavar="REGISTRY:OLD=NEW",
                        help="an old registry meaning moved to a new name while its old name was reused")
    parser.add_argument("--state-alias", action="append", default=[], metavar="OLD_STATE=NEW_STATE",
                        help="explicit state properties, e.g. minecraft:block[old=a]=minecraft:block[new=b]")
    parser.add_argument("--item-names", action="store_true",
                        help="also emit canonical item identifiers indexed by the old item id")
    parser.add_argument("--registry-names", action="append", default=[], metavar="REGISTRY[=CONSTANT]",
                        help="also emit old registry identifiers in wire-id order, optionally naming the constant")
    parser.add_argument("--name-aliases", action="store_true",
                        help="also emit the explicit old/current name aliases used by legacy NBT")
    args = parser.parse_args()
    if not args.pinned_constants and not args.old:
        parser.error("--old is required unless --pinned-constants is used")
    new_reports = args.new / "datagen/reports" if args.new else ROOT / ".local/datagen/out/reports"
    info = lab.version_info(args.new / "jars/client.jar") if args.new else lab.version_info()
    new_reg = json.loads((new_reports / "registries.json").read_text())
    if args.pinned_constants:
        out = args.out or ROOT / "crates/horizon-protocol/src/adapter/pinned_ids.rs"
        lines = [f"//! Named registry ids of the pinned {info['name']}.",
                 "//! Generated by tools/generate_registry_id_adapter.py --pinned-constants; do not edit.", ""]
        lines.extend(pinned_constants(new_reg, PINNED_NAMES))
        out.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return
    renames = {}
    reassign = {}
    for specs, destination in ((args.rename, renames), (args.reassign, reassign)):
        for spec in specs:
            head, new = spec.split("=", 1)
            registry, old_path = head.rsplit(":", 1)
            registry, old_ns = registry.rsplit(":", 1)
            destination.setdefault(registry, {})[f"{old_ns}:{old_path}"] = new
    old_reports = args.old / "datagen/reports"
    old_info = lab.version_info(args.old / "jars/client.jar")
    old_reg = json.loads((old_reports / "registries.json").read_text())
    inherited_reg = None
    inherited_states = None
    inherited_module = None
    if args.inherit:
        inherited_info = lab.version_info(args.inherit / "jars/client.jar")
        inherited_module = "v" + inherited_info["id"].replace(".", "_") + "_ids"
        inherited_reports = args.inherit / "datagen/reports"
        inherited_reg = json.loads((inherited_reports / "registries.json").read_text())
        inherited_states = states(json.loads((inherited_reports / "blocks.json").read_text()),
                                  renames.get("minecraft:block"))
    module = "v" + old_info["id"].replace(".", "_") + "_ids"
    out = args.out or ROOT / f"crates/horizon-protocol/src/adapter/{module}.rs"
    lines = [
        f"//! Registry ids of {old_info['id']} against the pinned {info['name']}: block states and the registries",
        "//! the protocol sends by id. Generated by tools/generate_registry_id_adapter.py from both versions'",
        "//! registries and blocks reports" + (" (--substitutes)" if args.substitutes else "") + "; do not edit by hand.",
        "",
    ]
    old_blocks = json.loads((old_reports / "blocks.json").read_text())
    new_blocks = json.loads((new_reports / "blocks.json").read_text())
    old_states = states(old_blocks, renames.get("minecraft:block"))
    new_states = states(new_blocks)
    state_up, state_down = {}, {}
    for spec in args.state_alias:
        old, new = state_alias(spec)
        if old not in old_states or new not in new_states:
            parser.error(f"state alias is absent from the reports: {spec}")
        state_up.setdefault(old, new)
        state_down.setdefault(new, old)
    inherit_states = old_states == inherited_states and not args.state_alias
    if not inherit_states:
        lines.append(f"/// Block states: {len(old_states)} in {old_info['id']}, {len(new_states)} pinned.")
        lines.append(f"pub const OLD_STATES: u32 = {len(old_states)};")
        lines.append(f"pub const PINNED_STATES: u32 = {len(new_states)};")
        lines.extend(rust_table("BLOCK_STATE_UP", table(old_states, new_states, state_up)))
        lines.extend(rust_table("BLOCK_STATE_DOWN", table(new_states, old_states, state_down)))
    same = []
    inherited = []
    for registry in REGISTRIES:
        o, n = entries(old_reg, registry), entries(new_reg, registry)
        if inherited_reg is not None and o == entries(inherited_reg, registry) and not reassign.get(registry):
            inherited.append(registry)
            continue
        if o == n and not reassign.get(registry):
            same.append(registry)
            continue
        name = const_name(registry)
        lines.append(f"/// `{registry}`: {len(o)} in {old_info['id']}, {len(n)} pinned.")
        forward = renames.get(registry, {})
        backward = {new: old for old, new in forward.items()}
        forced = reassign.get(registry)
        lines.extend(rust_table(f"{name}_UP", table(o, n, forward, forced)))
        reverse = reverse_table(o, n, forward, forced) if forced else table(n, o, backward)
        lines.extend(rust_table(f"{name}_DOWN", reverse))
    if args.item_names:
        names = item_names(entries(old_reg, "minecraft:item"), entries(new_reg, "minecraft:item"),
                           renames.get("minecraft:item"))
        lines.extend(["/// Canonical item identifiers for item-based cooldowns, indexed by old id.",
                      "#[rustfmt::skip]", "pub const ITEM_NAMES: &[&str] = &["])
        for i in range(0, len(names), 6):
            lines.append("    " + ", ".join(json.dumps(name) for name in names[i:i + 6]) + ",")
        lines.append("];")
    for spec in dict.fromkeys(args.registry_names):
        registry, _, override = spec.partition("=")
        name = override or const_name(registry)
        if registry not in old_reg or not name.isascii() or not name.isidentifier():
            parser.error("--registry-names requires a reported registry with a simple identifier")
        if name == "ITEM" and args.item_names:
            parser.error("--item-names already emits ITEM_NAMES")
        names = registry_names(entries(old_reg, registry))
        lines.extend([f"/// `{registry}` identifiers, indexed by old id (before registry sync).",
                      "#[rustfmt::skip]", f"pub const {name}_NAMES: &[&str] = &["])
        for i in range(0, len(names), 6):
            lines.append("    " + ", ".join(json.dumps(name) for name in names[i:i + 6]) + ",")
        lines.append("];")
    if args.name_aliases:
        lines.append("/// Explicit old/current registry spellings for legacy NBT name lookups.")
        lines.append("pub const NAME_ALIASES: &[(&str, &str, &str)] = &[")
        for registry in sorted(renames.keys() | reassign.keys()):
            aliases = renames.get(registry, {}) | reassign.get(registry, {})
            source, target = entries(old_reg, registry), entries(new_reg, registry)
            for old, new in sorted(aliases.items()):
                if old in source and new in target:
                    lines.append("    (" + ", ".join(json.dumps(v) for v in (registry, old, new)) + "),")
        lines.append("];")
    # Data components the pinned version no longer has: the item readers read their values past and drop them.
    old_components = entries(old_reg, "minecraft:data_component_type")
    new_components = entries(new_reg, "minecraft:data_component_type")
    known = renames.get("minecraft:data_component_type", {})
    dropped = sorted((i, name) for name, i in old_components.items()
                     if name not in new_components and name not in known)
    # Legacy NBT stacks have no component registry; their successor handles component write-back.
    inherit_components = inherited_module and not old_components
    if not inherit_components:
        lines.append(f"/// Data components {old_info['id']} has and the pinned version lacks, by their {old_info['id']} id.")
        lines.append("pub const DROPPED_COMPONENTS: &[(u16, &str)] = &[")
        lines.extend(f'    ({i}, "{name}"),' for i, name in dropped)
        lines.append("];")
    if args.substitutes:
        lines.append(f"/// Stand-ins for pinned block states {old_info['id']} lacks, in the pinned numbering.")
        lines.extend(rust_table("BLOCK_STATE_SUBSTITUTES", state_substitutes(old_blocks, new_blocks)))
        for registry in SUBSTITUTED:
            o, n = entries(old_reg, registry), entries(new_reg, registry)
            lines.append(f"/// Stand-ins for pinned `{registry}` entries {old_info['id']} lacks, in the pinned numbering.")
            table_ = substitutes(registry, o, n)
            lines.extend(rust_table(f"{const_name(registry)}_SUBSTITUTES", table_))
            by_id = sorted(n, key=n.get)
            lacking = [by_id[i] for i, v in enumerate(table_) if v == NONE and by_id[i] not in o]
            if lacking:
                print(f"{registry}: no stand-in for", ", ".join(lacking))
    lines.append(f"/// Every table above, as the adapter reads them.")
    lines.append("pub const IDS: super::ids::Ids = super::ids::Ids {")
    if not inherit_components:
        lines.append("    dropped_components: DROPPED_COMPONENTS,")
    if args.substitutes:
        lines.append("    substitutes: super::ids::Substitutes {")
        lines.append("        block_state: BLOCK_STATE_SUBSTITUTES,")
        lines.extend(f"        {r.split(':', 1)[1]}: {const_name(r)}_SUBSTITUTES," for r in SUBSTITUTED)
        lines.append("    },")
    elif not inherited_module:
        lines.append("    substitutes: super::ids::Substitutes::NONE,")
    if not inherited_module:
        lines.append("    quirks: super::ids::Quirks::NONE,")
    if not inherit_states:
        lines.append("    states: (OLD_STATES, PINNED_STATES),")
        lines.append("    block_state: super::ids::Remap::Runs { up: BLOCK_STATE_UP, down: BLOCK_STATE_DOWN },")
    for registry in REGISTRIES:
        if registry in inherited:
            continue
        field = registry.split(":", 1)[1]
        if registry in same:
            lines.append(f"    {field}: super::ids::Remap::Same,")
        else:
            name = const_name(registry)
            lines.append(f"    {field}: super::ids::Remap::Runs {{ up: {name}_UP, down: {name}_DOWN }},")
    if inherited_module:
        lines.append(f"    // Unchanged registries and block states were checked against {inherited_module}'s reports.")
        lines.append(f"    ..super::{inherited_module}::IDS")
    else:
        lines.append("    // Not a registry: set by hand where a version numbers EntityDataSerializers differently.")
        lines.append("    entity_data_serializer: super::ids::Remap::Same,")
    lines.append("};")
    out.write_text("\n".join(lines) + "\n")
    print(out, "same:", ", ".join(same))


if __name__ == "__main__":
    main()
