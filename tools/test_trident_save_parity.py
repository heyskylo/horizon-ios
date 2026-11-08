#!/usr/bin/env python3
"""Compare trident pickup and projectile save/load flags through both real games."""
import argparse
import json
from pathlib import Path
import subprocess
import sys


def command(text, record=True):
    return {'op': 'cmd', 'cmd': text, 'record': record}


def fields(entity, keys):
    return [command(f'/data get entity {entity} {key}') for key in keys]


def groups():
    trident = '@e[type=trident,limit=1]'
    keys = ['pickup', 'HasBeenShot', 'LeftOwner']
    saves = []
    for saved in ['', 'pickup:0b', 'pickup:1b', 'pickup:2b',
                  'HasBeenShot:1b,LeftOwner:1b', 'HasBeenShot:0b,LeftOwner:0b']:
        saves.append(command('/kill @e[type=trident]', False))
        suffix = ',' + saved if saved else ''
        saves.append(command('/summon trident 12.5 234 8.5 {NoGravity:1b' + suffix + '}'))
        saves.extend(fields(trident, keys))
    saves.extend([
        {'op': 'tick', 'n': 1},
        *fields(trident, keys),
        command(f'/data merge entity {trident} {{pickup:0b,HasBeenShot:0b,LeftOwner:0b}}'),
        *fields(trident, keys),
        command('/kill @e[type=trident]', False),
    ])

    owner = '@e[type=pig,limit=1]'
    lifecycle = [
        command('/fill 11 229 7 13 229 9 stone', False),
        command('/summon trident 12.5 230 8.5 {NoGravity:1b,inGround:1b}'),
        {'op': 'tick', 'n': 2},
        *fields(trident, keys),
        command('/kill @e[type=trident]', False),
        command('/summon pig 12.5 234 8.5 {UUID:[I;12525,0,0,1],NoAI:1b,NoGravity:1b}', False),
        command('/summon trident 12.5 234 8.5 {Owner:[I;12525,0,0,1],NoGravity:1b}'),
        {'op': 'tick', 'n': 1},
        *fields(trident, ['Owner', 'HasBeenShot', 'LeftOwner']),
        command('/tp @e[type=trident] 20.5 234 8.5', False),
        {'op': 'tick', 'n': 1},
        *fields(trident, ['Owner', 'HasBeenShot', 'LeftOwner']),
        command('/kill @e[type=trident]', False),
        command(f'/kill {owner}', False),
        command('/summon fireball 12.5 236 8.5 {NoGravity:1b}'),
        *fields('@e[type=fireball,limit=1]', ['HasBeenShot', 'LeftOwner']),
        {'op': 'tick', 'n': 1},
        *fields('@e[type=fireball,limit=1]', ['HasBeenShot', 'LeftOwner']),
        command('/kill @e[type=fireball]', False),
    ]

    pickup = [command('/fill 6 229 6 10 229 10 stone', False)]
    for mode in ['survival', 'creative']:
        for saved in ['', ',pickup:0b', ',pickup:1b', ',pickup:2b']:
            pickup.extend([
                command('/kill @e[type=trident]', False),
                command('/clear @s', False),
                command(f'/gamemode {mode}', False),
                command('/tp @s 8.5 230 8.5', False),
                command('/summon trident 8.5 230 8.5 {NoGravity:1b,inGround:1b' + saved + '}'),
                {'op': 'player_loop', 'n': 2, 'label': f'{mode}{saved}'},
                command('/clear @s minecraft:trident 0'),
                command('/execute if entity @e[type=trident]'),
            ])
    pickup.append(command('/kill @e[type=trident]', False))

    reloads = []
    for index, (kind, shot) in enumerate([
        ('trident', 0), ('trident', 1), ('fireball', 0), ('fireball', 1),
    ]):
        x = 4096 + index * 48
        entity = f'@e[type={kind},limit=1]'
        saved_keys = keys if kind == 'trident' else keys[1:]
        reloads.extend([
            command(f'/forceload add {x} 4096', False),
            {'op': 'wait_chunks', 'chunks': [[x >> 4, 256]]},
            command(f'/summon {kind} {x + 8}.5 300 4104.5 '
                    f'{{NoGravity:1b,HasBeenShot:{shot}b,LeftOwner:{shot}b}}'),
            *fields(entity, saved_keys),
            command(f'/forceload remove {x} 4096', False),
            {'op': 'tick', 'n': 60},
            {'op': 'cmd', 'cmd': f'/execute unless loaded {x} 300 4096', 'expect_result': 1},
            command(f'/forceload add {x} 4096', False),
            {'op': 'wait_chunks', 'chunks': [[x >> 4, 256]]},
            *fields(entity, saved_keys),
            command(f'/kill {entity}', False),
            command(f'/forceload remove {x} 4096', False),
        ])
    return [('trident saved flags', saves), ('trident flight and owner', lifecycle),
            ('trident pickup modes', pickup),
            ('projectile chunk reload', reloads)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--work-dir', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    work = (args.work_dir or root / '.local' / 'trident-save-parity').resolve()
    work.mkdir(parents=True, exist_ok=True)
    cases = work / 'groups.json'
    cases.write_text(json.dumps(groups()), encoding='utf-8')
    run = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
           '--groups-file', str(cases), '--work-dir', str(work)]
    if args.binary:
        run.extend(['--binary', str(args.binary.resolve())])
    if args.prepare:
        run.append('--prepare')
    if args.reference_only:
        run.append('--reference-only')
    return subprocess.run(run, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
