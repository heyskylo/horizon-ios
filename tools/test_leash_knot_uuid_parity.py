#!/usr/bin/env python3
"""A wolf and a boat leashed to a leash knot by its UUID on a dedicated server, against vanilla's, seen by a vanilla
client.

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each serve a copy of one flat world
vanilla generated first, with Alice an operator. A leash knot with a fixed UUID is summoned on a fence, then a wolf
(NoAI) and a boat whose `leash` names that UUID. `Leashable.restoreLeashFromSave` finds the holder with
`ServerLevel.getEntity(uuid)`, any entity, the knot included, and ties them to it; a holder still missing after 100
ticks would drop the lead. Seven seconds later each one's saved `leash` is read: a knot holder saves as its fence's
block position (`LeashData.CODEC`). The numbers in each reply are compared exactly.
"""
import re
import sys

try:
    from . import test_locator_bar_parity as locator
except ImportError:
    import test_locator_bar_parity as locator

NAME = 'leash-knot-uuid-parity'
KNOT = '[I;7193,0,0,1]'
SCRIPT = [
    ('setup', ['chat on',
               'cmd Alice setblock 4 -60 6 minecraft:oak_fence',
               f'cmd Alice summon minecraft:leash_knot 4.5 -60 6.5 {{UUID:{KNOT}}}',
               f'cmd Alice summon minecraft:wolf 2.5 -60 6.5 {{NoAI:1b,Tags:["tied"],leash:{{UUID:{KNOT}}}}}',
               f'cmd Alice summon minecraft:oak_boat 2.5 -60 9.5 {{Tags:["tied_boat"],leash:{{UUID:{KNOT}}}}}']),
    ('settle', []),
    ('wolf-leash', ['cmd Alice data get entity @e[type=minecraft:wolf,tag=tied,limit=1] leash']),
    ('boat-leash', ['cmd Alice data get entity @e[type=minecraft:oak_boat,tag=tied_boat,limit=1] leash']),
]


def shown(lines):
    """The numbers of each `leash` reply: the knot's block."""
    out = []
    for line in lines:
        parts = line.split(' ', 3)
        if len(parts) < 4 or parts[2] != 'chat' or not parts[1].endswith('-leash'):
            continue
        if 'commands.data.entity.query' in line:
            numbers = re.findall(r'"text":"(-?[0-9]+)","color":"gold"', line)
            out.append(f'{parts[0]} {parts[1]} leash ' + ' '.join(numbers))
    return out


def check(vanilla, vanilla_all):
    expected = ['Alice step=wolf-leash leash 4 -60 6', 'Alice step=boat-leash leash 4 -60 6']
    if vanilla != expected:
        return f'vanilla did not keep both on the knot: {vanilla}'
    return None


def main():
    lines = locator.one_player(NAME, SCRIPT, shown, check, 'A wolf and a boat whose leash names a leash knot by '
                               'UUID on a dedicated server: the holder they end up tied to.', step_seconds=3.5)
    print(f'leash knot uuid: {len(lines)} lines identical')


if __name__ == '__main__':
    sys.exit(main())
