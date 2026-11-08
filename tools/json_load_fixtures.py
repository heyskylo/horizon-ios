#!/usr/bin/env python3
"""Generate schema-shaped synthetic JSON for json_load_bench (not vanilla exports).

Usage: python3 tools/json_load_fixtures.py OUTPUT_DIRECTORY
Targets match the reported production file sizes in decimal MB. Fields mirror
VanillaGameplayFactsExporter and VanillaHeightmapOracle; values/shapes/state
distribution are synthetic. These fixtures measure parsing, not registry validity.
"""
import json
from pathlib import Path
import sys


def gameplay(i):
    solid = i % 4 != 0
    boxes = [[0.0, 0.0, 0.0, 1.0, (j + 1) / 4, 1.0] for j in range(i % 4)]
    edges = []
    for axis in range(3):
        for u, v in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]:
            start = [0.0] * 3
            start[(axis + 1) % 3], start[(axis + 2) % 3] = u, v
            end = start.copy()
            end[axis] = 1.0
            edges.append(start + end)
    return dict(
        id=i, emission=i % 16, dampening=15 if solid else 0,
        skylight_down=not solid, shape_occlusion=not solid, can_occlude=solid,
        solid_render=solid, hardness=(i % 20) / 4, requires_tool=solid,
        replaceable=not solid, random_ticks=i % 3 == 0, block_entity=i % 5 == 0,
        render_shape="MODEL", liquid=False, ignited_by_lava=not solid,
        push="NORMAL", offset=not solid, full_collision=solid, map_color=8368696,
        outline_offset=not solid, outline=boxes, outline_edges=edges,
        friction=0.6, speed_factor=1.0, jump_factor=1.0, explosion_resistance=6.0,
        item=f"minecraft:fixture_{i % 128}",
        sound=dict(volume=1.0, pitch=1.0, **{
            event: {"id": f"minecraft:block.stone.{event}"}
            for event in ["break", "step", "place", "hit", "fall"]
        }),
    )


def facts(i):
    name = f"minecraft:fixture_{i // 16}"
    properties = dict(facing=["north", "south", "east", "west"][i % 4],
                      level=str(i % 16), waterlogged=str(i % 2 == 0).lower())
    description = name + "[" + ",".join(f"{k}={v}" for k, v in properties.items()) + "]"
    return [name, False, i % 2 == 0, description,
            dict(Name=name, Properties=properties)]


def write(path, target, entry, defaults=False):
    with path.open("wb") as output:
        output.write(b'{"states":[')
        count = 0
        while output.tell() < target:
            if count:
                output.write(b",")
            output.write(json.dumps(entry(count), separators=(",", ":")).encode())
            count += 1
        output.write(b"]")
        if defaults:
            output.write(b',"defaults":')
            output.write(json.dumps({f"minecraft:fixture_{i // 16}": i
                                     for i in range(0, count, 16)},
                                    separators=(",", ":")).encode())
        output.write(b"}")
    print(f"{path.name}: {path.stat().st_size} bytes, {count} states")


# A flag is not a directory: `--help` must print this usage instead of writing 66 MB into `./--help`.
if len(sys.argv) != 2 or sys.argv[1].startswith("-"):
    sys.exit(__doc__)

root = Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=True)
write(root / "gameplay-facts.json", 55_700_000, gameplay)
# Leave room for the defaults object (~95 KB at this state count).
write(root / "facts.json", 10_020_000, facts, defaults=True)
