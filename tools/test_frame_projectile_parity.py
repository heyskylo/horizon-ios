#!/usr/bin/env python3
"""Arrows against item frames, through the command execution harness (tools/test_command_exec_parity.py).

Three frames hang on the west face of a stone wall: one holding a diamond, one empty, one fixed and holding a diamond.
An arrow with no owner flies east into each. Item frames are pickable (`BlockAttachedEntity.isPickable`), so
`AbstractArrow.onHitEntity` hurts them: `ItemFrame.hurtServer` gives the diamond up (`shouldDamageDropItem`), breaks
the empty frame (`BlockAttachedEntity.hurtServer`), and a fixed frame refuses the hit (`canHurtWhenFixed`), which
turns the arrow back at a fifth of its speed. Read after the arrows land: whether each frame is there, what it holds,
the items dropped beside it, and whether an arrow's box reaches into the wall behind it. That last read tells a frame
that let the arrow through (filled, empty) from one that took it; a fixed frame's deflected arrow drops right in front
of it, so its box still reaches the wall either way.

    test_frame_projectile_parity.py [--binary PATH] [--allow-gaps] [--reference-only]
"""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import test_command_exec_parity as driver  # noqa: E402
from command_exec_corpus import cmd  # noqa: E402

# name, z, frame data
FRAMES = [
    ('filled', 2, 'Item:{id:"minecraft:diamond",count:1}'),
    ('empty', 6, ''),
    ('fixed', 10, 'Fixed:1b,Item:{id:"minecraft:diamond",count:1}'),
]


def ops():
    out = []

    def run(text, expected=None, record=True):
        out.append(cmd(text, record=record, expect_result=expected))

    for rule in ['spawn_mobs', 'advance_time', 'advance_weather', 'mob_griefing']:
        run(f'/gamerule {rule} false', record=False)
    run('/fill 0 209 0 15 209 15 minecraft:stone', 256)
    run('/fill 8 210 0 8 212 15 minecraft:stone', 48)
    run('/execute if loaded 7 210 6', 1)
    for name, z, data in FRAMES:
        comma = ',' if data else ''
        run(f'/summon minecraft:item_frame 7.5 210.5 {z}.5 {{Facing:4b,Tags:["f_{name}"]{comma}{data}}}', 1)
        run(f'/data get entity @e[tag=f_{name},limit=1] Item.id')
    for name, z, _ in FRAMES:
        run(f'/summon minecraft:arrow 4.5 210.5 {z}.5 {{Tags:["a_{name}"],Motion:[1.0d,0.0d,0.0d]}}', 1)
    out.append({'op': 'tick', 'n': 20})
    for name, z, _ in FRAMES:
        run(f'/execute if entity @e[type=minecraft:item_frame,tag=f_{name}]')
        run(f'/data get entity @e[tag=f_{name},limit=1] Item.id')
        near = f'x=5,y=209,z={z},dx=4,dy=4,dz=1'
        run(f'/execute if entity @e[type=minecraft:item,{near},nbt={{Item:{{id:"minecraft:diamond"}}}}]')
        run(f'/execute if entity @e[type=minecraft:item,{near},nbt={{Item:{{id:"minecraft:item_frame"}}}}]')
        # Stuck in the wall behind the frame (the frame is 7.9375 to 8): the frame let it through.
        run(f'/execute if entity @e[type=minecraft:arrow,tag=a_{name},x=7.9,y=209,z={z},dx=0.2,dy=4,dz=1]')
    return out


if __name__ == '__main__':
    driver.corpus_module.groups = lambda: [('setup', driver.corpus_module.group_setup()),
                                           ('arrows against item frames', ops())]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(Path(__file__).resolve().parents[1] / '.local/frame-projectile-parity')]
    driver.main()
