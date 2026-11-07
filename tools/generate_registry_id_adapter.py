#!/usr/bin/env python3
"""Generate crates/horizon-protocol/src/adapter/v<OLD>_ids.rs: how an older release numbers the registry entries
the protocol sends by id (block states, items, entity types, ...) against the pinned one, from both versions'
data generator reports (registries.json, blocks.json).

    generate_registry_id_adapter.py --old ~/code/horizon-parity/versions/26.2

`--old` (and `--new`, the pinned version in .local by default) is a directory `tools/version_port.py prepare
VERSION --download` made. Entries are matched by name; block states by block name and properties. Each
registry gets two tables of runs `(from, to, length)`: `<NAME>_UP` from the old server's ids to the pinned ones,
`<NAME>_DOWN` back. An id in no run has no entry in the other version. Only registries whose ids differ are written; the rest map to themselves.

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


def states(blocks):
    """{(block, properties): state id} over every state of every block."""
    found = {}
    for name, block in blocks.items():
        for state in block["states"]:
            key = (name, tuple(sorted(state.get("properties", {}).items())))
            found[key] = state["id"]
    assert sorted(found.values()) == list(range(len(found)))
    return found


def table(source, target, renames=None):
    """For each id of `source` (in order), the id `target` gives the same key (or the key it was renamed
    to), or NONE."""
    renames = renames or {}
    by_id = sorted(source.items(), key=lambda kv: kv[1])
    return [target.get(renames.get(key, key), NONE) for key, _ in by_id]


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old", type=Path, required=True, help="the older version's prepared directory")
    parser.add_argument("--new", type=Path, help="the pinned version's prepared directory (default: .local)")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--substitutes", action="store_true",
                        help="also write the stand-ins for pinned entries the older version lacks")
    parser.add_argument("--rename", action="append", default=[], metavar="REGISTRY:OLD=NEW",
                        help="an entry the pinned version knows by another name, e.g. "
                             "minecraft:command_argument_type:minecraft:color=minecraft:team_color")
    args = parser.parse_args()
    renames = {}
    for spec in args.rename:
        head, new = spec.split("=", 1)
        registry, old_path = head.rsplit(":", 1)
        registry, old_ns = registry.rsplit(":", 1)
        renames.setdefault(registry, {})[f"{old_ns}:{old_path}"] = new
    old_reports = args.old / "datagen/reports"
    new_reports = args.new / "datagen/reports" if args.new else ROOT / ".local/datagen/out/reports"
    old_info = lab.version_info(args.old / "jars/client.jar")
    info = lab.version_info(args.new / "jars/client.jar") if args.new else lab.version_info()
    old_reg = json.loads((old_reports / "registries.json").read_text())
    new_reg = json.loads((new_reports / "registries.json").read_text())
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
    old_states = states(old_blocks)
    new_states = states(new_blocks)
    lines.append(f"/// Block states: {len(old_states)} in {old_info['id']}, {len(new_states)} pinned.")
    lines.append(f"pub const OLD_STATES: u32 = {len(old_states)};")
    lines.append(f"pub const PINNED_STATES: u32 = {len(new_states)};")
    lines.extend(rust_table("BLOCK_STATE_UP", table(old_states, new_states)))
    lines.extend(rust_table("BLOCK_STATE_DOWN", table(new_states, old_states)))
    same = []
    for registry in REGISTRIES:
        o, n = entries(old_reg, registry), entries(new_reg, registry)
        if o == n:
            same.append(registry)
            continue
        name = const_name(registry)
        lines.append(f"/// `{registry}`: {len(o)} in {old_info['id']}, {len(n)} pinned.")
        forward = renames.get(registry, {})
        backward = {new: old for old, new in forward.items()}
        lines.extend(rust_table(f"{name}_UP", table(o, n, forward)))
        lines.extend(rust_table(f"{name}_DOWN", table(n, o, backward)))
    # Data components the pinned version no longer has: the item readers read their values past and drop them.
    old_components = entries(old_reg, "minecraft:data_component_type")
    new_components = entries(new_reg, "minecraft:data_component_type")
    known = renames.get("minecraft:data_component_type", {})
    dropped = sorted((i, name) for name, i in old_components.items()
                     if name not in new_components and name not in known)
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
    lines.append("    quirks: super::ids::Quirks::NONE,")
    lines.append("    states: (OLD_STATES, PINNED_STATES),")
    lines.append("    block_state: super::ids::Remap::Runs { up: BLOCK_STATE_UP, down: BLOCK_STATE_DOWN },")
    for registry in REGISTRIES:
        field = registry.split(":", 1)[1]
        if registry in same:
            lines.append(f"    {field}: super::ids::Remap::Same,")
        else:
            name = const_name(registry)
            lines.append(f"    {field}: super::ids::Remap::Runs {{ up: {name}_UP, down: {name}_DOWN }},")
    lines.append("    dropped_components: DROPPED_COMPONENTS,")
    lines.append("    // Not a registry: set by hand where a version numbers EntityDataSerializers differently.")
    lines.append("    entity_data_serializer: super::ids::Remap::Same,")
    if args.substitutes:
        lines.append("    substitutes: super::ids::Substitutes {")
        lines.append("        block_state: BLOCK_STATE_SUBSTITUTES,")
        lines.extend(f"        {r.split(':', 1)[1]}: {const_name(r)}_SUBSTITUTES," for r in SUBSTITUTED)
        lines.append("    },")
    else:
        lines.append("    substitutes: super::ids::Substitutes::NONE,")
    lines.append("};")
    out.write_text("\n".join(lines) + "\n")
    print(out, "same:", ", ".join(same))


if __name__ == "__main__":
    main()
