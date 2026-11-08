#!/usr/bin/env python3
"""`/test pos` under `execute as` a real connected player (H274).

`TestCommand.showPos` takes `source.getPlayerOrException()` and that
player's own `pick(10, 1, false)`: the source player's eyes and view, not
the `execute at` position. Its message goes to the command's source
(`sendSuccess`), here the issuing player. The highlight packet the player
gets (`ClientboundGameTestHighlightPosPacket`) is not recorded here.

Two real network players (LocatorBarClient) join a dedicated server. A
issues every command. A and B stand on stone above two different test
instance boxes, each with its own view, set by `tp` and sent back by the
client (`still`). Each step records every chat line each player gets, and
the result and success of a fresh `execute store`.

Label classes, declared before any native run: `target-` steps run
`test pos` with B as the source player; `diag-` keys are recorded
diagnostics; every other key is a setup admission or a control.
"""
import argparse
import json
from pathlib import Path

import anvil_dump as nbt
import test_locator_bar_parity as net
from test_disconnect_crafting_parity import generate, uuid_words, wait_new

A, B = 'H274Host', 'H274Guest'
PIG = '00000274-0000-0000-0000-000000000001'
# name, position, yaw, pitch, the test block of the box below
# Both stand on the boxes' bedrock floor (y 100) and look straight down.
VIEWS = {A: ((4.5, 101, 6.5), 0, 90), B: ((22.5, 101, 9.5), 0, 90)}
# Where the host stands for each /test create (TestCommand.createTestPositionAround: the
# player's x, the surface, the player's z + 3).
BOXES = [(0.5, 100, 0.5), (16.5, 100, 0.5)]


def program():
    ops = []

    def cmd(text, label):
        ops.append({'op': 'cmd', 'cmd': text, 'label': label})

    def store(label, command):
        # The command as it is, with its full feedback, then again with fresh stores.
        cmd(command, label + '-plain')
        cmd('scoreboard players set r h274 -7', label + '-r-reset')
        cmd('scoreboard players set s h274 -7', label + '-s-reset')
        cmd(f'execute store result score r h274 store success score s h274 run {command}', label)
        cmd(f'execute store result storage h274:proof {label.replace("-", "_")}.r int 1 run '
            'scoreboard players get r h274', label + '-r')
        cmd(f'execute store result storage h274:proof {label.replace("-", "_")}.s int 1 run '
            'scoreboard players get s h274', label + '-s')

    # Admin feedback to the other op is not this task's (diagnostic).
    cmd('gamerule log_admin_commands false', 'diag-admin-log')
    for text in ['gamerule spawn_mobs false', 'gamerule advance_time false',
                 'gamerule send_command_feedback true', 'scoreboard objectives add h274 dummy']:
        cmd(text, 'setup-' + text.split()[1])
    # The two test boxes, made by /test create where the host stands.
    for i, (x, y, z) in enumerate(BOXES):
        cmd(f'tp {A} {x} {y} {z} 0 0', f'box{i}-tp')
        ops.append({'op': 'still', 'name': A})
        cmd('test create minecraft:always_pass 10 10 10', f'box{i}')
    cmd('test locate minecraft:always_pass', 'boxes-locate')
    cmd(f'summon pig 12.5 100 12.5 {{UUID:[I;628,0,0,1],NoAI:1b,NoGravity:1b}}', 'pig-summon')
    for name, ((x, y, z), yaw, pitch) in VIEWS.items():
        cmd(f'gamemode creative {name}', f'mode-{name}')
        cmd(f'tp {name} {x} {y} {z} {yaw} {pitch}', f'tp-{name}')
        ops.append({'op': 'still', 'name': name})
    # The server takes each player's new place and view, then the world stops.
    ops.append({'op': 'wait', 'ticks': 10})
    cmd('tick freeze', 'freeze')
    for name in VIEWS:
        cmd(f'data modify storage h274:proof players.{name} set from entity {name}', f'player-{name}')
    cmd('execute store result storage h274:proof clock.start long 1 run time query gametime', 'diag-clock-start')
    for name in VIEWS:
        cmd(f'execute as {name} run data get entity @s Rotation', f'diag-view-{name}')
    # The host itself: its own view (the positive).
    store('host-pos', 'test pos')
    # B as the source player, from A's position: B's view.
    store('target-as-guest', f'execute as {B} run test pos')
    store('target-as-guest-at-host', f'execute as {B} at {A} rotated 0 -90 run test pos')
    # A as the source at B's position: still A's view.
    store('host-at-guest', f'execute at {B} rotated 0 -90 run test pos')
    # A non-player source.
    store('pig', f'execute as {PIG} run test pos')
    # B looking up (a diagnostic: vanilla answers with the block below it).
    x, y, z = VIEWS[B][0]
    cmd('tick unfreeze', 'unfreeze-up')
    cmd(f'tp {B} {x} {y} {z} 0 -90', 'tp-guest-up')
    ops.append({'op': 'still', 'name': B})
    ops.append({'op': 'wait', 'ticks': 10})
    cmd('tick freeze', 'freeze-up')
    cmd('execute store result storage h274:proof clock.up long 1 run time query gametime', 'diag-clock-up')
    cmd(f'data modify storage h274:proof players.up set from entity {B}', 'player-up')
    store('diag-as-guest-up', f'execute as {B} run test pos')
    for name in VIEWS:
        cmd(f'data modify storage h274:proof after.{name} set from entity {name}', f'after-{name}')
    cmd('execute store result storage h274:proof clock.end long 1 run time query gametime', 'diag-clock-end')
    return ops


def run(label, command, client_command, build, world, ops):
    directory = build / label
    port = net.server_dir(directory, world)
    server = net.Server(command, directory, label)
    wire = build / f'{label}-wire.txt'
    log = (build / f'{label}-client.log').open('w')
    client = None
    try:
        server.wait_for(r'Done \(')
        client = net.Client(client_command, port, wire, log)
        client.tell('chat on')
        client.tell('flat-chat on')
        client.tell('state on')
        for name in [A, B]:
            client.tell(f'join {name}')
            client.sync([name])
        for op in ops:
            if op['op'] == 'cmd':
                client.tell('step ' + op['label'])
                client.tell(f'send-cmd {A} ' + op['cmd'])
            elif op['op'] == 'wait':
                client.tell(f'step wait-{op["ticks"]}')
                client.wait(server, [A, B], op['ticks'])
                continue
            else:
                client.tell('step still-' + op['name'])
                client.tell('still ' + op['name'])
            client.sync([A, B])
        client.tell('step save')
        client.sync([A, B])
        start = len(server.lines)
        server.console('save-all flush')
        wait_new(server, start, 'Saved the game')
        client.end()
    finally:
        if client:
            client.kill()
        server.stop()
        log.close()
    storage = [p for p in (directory / 'world').rglob('*.dat')
               if 'command_storage' in p.name and ('h274' in p.parts or 'h274' in p.name)]
    assert len(storage) == 1, storage
    proof = json.loads(json.dumps(nbt.read_dat(storage[0])))['data']['contents']['proof']
    chat = {op['label']: {A: [], B: []} for op in ops if op['op'] == 'cmd'}
    raw_wire = []
    for line in wire.read_text().splitlines():
        raw_wire.append(line)
        if ' chat-flat ' not in line:
            continue
        who, rest = line.split(' step=', 1)
        step, text = rest.split(' chat-flat ', 1)
        if step in chat:
            chat[step][who].append(json.loads(text))
    return proof, chat


def facts(ops, proof, chat):
    out = {}
    for op in ops:
        if op['op'] == 'cmd' and not op['label'].endswith(('-r-reset', '-s-reset')):
            out[op['label'] + '/chat'] = chat[op['label']]
    for name in ['players', 'after']:
        for who, p in proof[name].items():
            for key in ['UUID', 'Health', 'Pos', 'Rotation', 'playerGameType', 'Dimension']:
                out[f'{name}/{who}/{key}'] = p.get(key)
    for key, value in proof.items():
        if isinstance(value, dict) and set(value) == {'r', 's'}:
            out[key.replace('_', '-') + '/result'] = value
    out['diag-clock'] = proof['clock']
    # Frozen while the views are read: from the first freeze to the unfreeze, and after the second.
    out['clock-stationary'] = proof['clock']['up'] == proof['clock']['end']
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--side', choices=['java', 'native', 'both'], default='both')
    parser.add_argument('--label')
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--reference', type=Path, help='a Java facts file to compare with')
    args = parser.parse_args()
    net.PLAYERS = [A, B]
    net.PROPERTIES['force-gamemode'] = 'false'
    _, build, java, native, client = net.sides('test-pos-guest-parity',
                                              args.binary.resolve() if args.binary else None)
    world = build / 'generate/world'
    if not world.exists():
        world = generate(java, build)
    ops = program()
    sides = ['java', 'native'] if args.side == 'both' else [args.side]
    reference = json.loads(args.reference.read_text()) if args.reference else None
    for side in sides:
        label = args.label or side
        proof, chat = run(label, java if side == 'java' else native, client, build, world, ops)
        actual = facts(ops, proof, chat)
        (build / f'{label}-facts.json').write_text(json.dumps(actual, indent=2, sort_keys=True) + '\n')
        if side == 'java' and reference is None:
            reference = actual
            continue
        if reference is not None and side != 'java' or args.reference:
            assert actual.keys() == reference.keys(), 'canonical keys changed'
            all_differences = [{'label': k, 'expected': reference[k], 'actual': actual[k]}
                               for k in reference if reference[k] != actual[k]]
            # Absolute clocks are engine-specific: recorded, compared apart.
            differences = [d for d in all_differences if not d['label'].startswith('diag-')]
            strict = [k for k in reference if not k.startswith('diag-')]
            report = {'records': len(strict), 'passed': len(strict) - len(differences),
                      'differences': differences,
                      'diagnostic_differences': [d for d in all_differences if d['label'].startswith('diag-')]}
            (build / f'{label}-report.json').write_text(json.dumps(report, indent=2) + '\n')
            print(label, report['passed'], '/', report['records'])
        else:
            print(label, len(actual), 'facts captured')


if __name__ == '__main__':
    main()
