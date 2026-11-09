#!/usr/bin/env python3
"""Tame wolves left behind by their owner on a dedicated server, against vanilla's, seen by a vanilla client.

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each serve a copy of one flat world
vanilla generated first, with Alice an operator. Three standing wolves she owns wait near her: one free, one on a lead
held by a pig that never moves (NoAI), one sitting in a boat. She then teleports 40 blocks away.
`TamableAnimal.shouldTryTeleportToOwner` (12 blocks or more) sends a pet to its owner unless `unableToMoveToOwner`:
ordered to sit, `isPassenger`, `mayBeLeashed` or an owner who is a spectator. So only the free wolf comes to her.
Whether each wolf is within 6 blocks of Alice afterwards is compared (where exactly a teleported wolf lands is a random
draw, not under test).
"""
import re
import sys

try:
    from . import test_locator_bar_parity as locator
except ImportError:
    import test_locator_bar_parity as locator

NAME = 'pet-owner-teleport-parity'
# Alice's offline UUID: UUID.nameUUIDFromBytes("OfflinePlayer:Alice"), as int words.
ALICE = '[I;278005000,-707248403,-1814916367,-1817188393]'
HOLDER = '[I;7193,0,0,1]'
WOLF = f'Owner:{ALICE},Sitting:0b,PersistenceRequired:1b'
PETS = ('free', 'leashed', 'boat')
SCRIPT = [
    ('setup', ['chat on',
               f'cmd Alice summon minecraft:wolf -3.5 -60 6.5 {{{WOLF},Tags:["free"]}}',
               f'cmd Alice summon minecraft:pig 4.5 -60 6.5 {{UUID:{HOLDER},NoAI:1b,PersistenceRequired:1b}}',
               f'cmd Alice summon minecraft:wolf 2.5 -60 6.5 {{{WOLF},Tags:["leashed"],leash:{{UUID:{HOLDER}}}}}',
               f'cmd Alice summon minecraft:oak_boat 0.5 -60 10.5 '
               f'{{Passengers:[{{id:"minecraft:wolf",{WOLF},Tags:["boat"]}}]}}']),
    ('away', ['cmd Alice tp @s 0.5 -60 40.5']),
    *[(f'{pet}-near', [f'cmd Alice data get entity @e[type=minecraft:wolf,tag={pet},limit=1] Pos']) for pet in PETS],
]
# Where `away` puts Alice.
AWAY = (0.5, -60.0, 40.5)


def shown(lines):
    """Whether each wolf ended within 6 blocks of Alice, from its `Pos`."""
    out = []
    for line in lines:
        parts = line.split(' ', 3)
        if len(parts) < 4 or parts[2] != 'chat' or not parts[1].endswith('-near'):
            continue
        if 'commands.data.entity.query' not in line:
            continue
        position = [float(v) for v in re.findall(r'"text":"(-?[0-9.]+)","color":"gold"', line)]
        if len(position) != 3:
            continue
        near = sum((a - b) ** 2 for a, b in zip(position, AWAY)) <= 36.0
        out.append(f'{parts[0]} {parts[1]} {"near" if near else "behind"}')
    return out


def check(vanilla, vanilla_all):
    expected = ['Alice step=free-near near', 'Alice step=leashed-near behind', 'Alice step=boat-near behind']
    if vanilla != expected:
        return f'vanilla did not bring only the free wolf: {vanilla}'
    return None


def main():
    lines = locator.one_player(NAME, SCRIPT, shown, check, 'Tame wolves (free, leashed, in a boat) left 40 blocks '
                               'behind by their owner on a dedicated server: which ones teleport to her.',
                               step_seconds=5.0)
    print(f'pet owner teleport: {len(lines)} lines identical')


if __name__ == '__main__':
    sys.exit(main())
