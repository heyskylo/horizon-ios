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
"""

import argparse
import json
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


def table(source, target, renames=None):
    """For each id of `source` (in order), the id `target` gives the same key (or the key it was renamed
    to), or NONE."""
    renames = renames or {}
    by_id = sorted(source.items(), key=lambda kv: kv[1])
    # A removed entry may alias an entry that still exists (creaking_transient -> creaking).
    # Prefer that exact name in reverse, rather than sending the removed variant back.
    return [target.get(key, target.get(renames.get(key, key), NONE)) for key, _ in by_id]


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old", type=Path, required=True, help="the older version's prepared directory")
    parser.add_argument("--new", type=Path, help="the pinned version's prepared directory (default: .local)")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--inherit", type=Path,
                        help="another prepared version: reuse its generated tables when registry ids match")
    parser.add_argument("--rename", action="append", default=[], metavar="REGISTRY:OLD=NEW",
                        help="an entry the pinned version knows by another name, e.g. "
                             "minecraft:command_argument_type:minecraft:color=minecraft:team_color")
    parser.add_argument("--state-alias", action="append", default=[], metavar="OLD_STATE=NEW_STATE",
                        help="explicit state properties, e.g. minecraft:block[old=a]=minecraft:block[new=b]")
    parser.add_argument("--item-names", action="store_true",
                        help="also emit canonical item identifiers indexed by the old item id")
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
        "//! registries and blocks reports; do not edit by hand.",
        "",
    ]
    old_states = states(json.loads((old_reports / "blocks.json").read_text()),
                        renames.get("minecraft:block"))
    new_states = states(json.loads((new_reports / "blocks.json").read_text()))
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
        if inherited_reg is not None and o == entries(inherited_reg, registry):
            inherited.append(registry)
            continue
        if o == n:
            same.append(registry)
            continue
        name = const_name(registry)
        lines.append(f"/// `{registry}`: {len(o)} in {old_info['id']}, {len(n)} pinned.")
        forward = renames.get(registry, {})
        backward = {new: old for old, new in forward.items()}
        lines.extend(rust_table(f"{name}_UP", table(o, n, forward)))
        lines.extend(rust_table(f"{name}_DOWN", table(n, o, backward)))
    if args.item_names:
        names = item_names(entries(old_reg, "minecraft:item"), entries(new_reg, "minecraft:item"),
                           renames.get("minecraft:item"))
        lines.extend(["/// Canonical item identifiers for item-based cooldowns, indexed by old id.",
                      "#[rustfmt::skip]", "pub const ITEM_NAMES: &[&str] = &["])
        for i in range(0, len(names), 6):
            lines.append("    " + ", ".join(json.dumps(name) for name in names[i:i + 6]) + ",")
        lines.append("];")
    lines.append(f"/// Every table above, as the adapter reads them.")
    lines.append("pub const IDS: super::ids::Ids = super::ids::Ids {")
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
