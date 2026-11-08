#!/usr/bin/env python3
"""`/loot … kill` under `execute as` a real connected player (H038).

`LootCommand.dropKillLoot` takes the source's entity as the killer: a player
is `LAST_DAMAGE_PLAYER` (so `killed_by_player` holds), and the killer is the
attacking entity whose weapon counts for looting. An elder guardian's
`wet_sponge` drops only when a player kills it; its shards gain the
killer's looting.

Two real network players (LocatorBarClient) join a dedicated server. The
host A holds a plain diamond sword, the guest B one with Looting III. Each
case empties a chest, puts the elder guardian's kill loot in it, and reads
the whole chest, the sponge count, every chat line each player gets, and
the result and success of a fresh `execute store`. The guardian is not
killed by the command, and its state is read before and after.

Label classes, declared before any native run: `target-` keys are the
loot of `execute as <guest>` (the H038 path); `diag-` keys are recorded
diagnostics; every other key is a setup admission or a control.
"""
import argparse
import json
from pathlib import Path

import anvil_dump as nbt
import test_locator_bar_parity as net
from test_disconnect_crafting_parity import generate, wait_new

A, B = 'H038Host', 'H038Guest'
ELDER = '00000038-0000-0000-0000-000000000001'
PIG = '00000038-0000-0000-0000-000000000002'
CHEST = (8, 100, 8)
VIEWS = {A: (4.5, 100, 4.5), B: (6.5, 100, 4.5)}


def program():
    ops = []

    def cmd(text, label, who=A):
        ops.append({'op': 'cmd', 'cmd': text, 'label': label, 'who': who})

    x, y, z = CHEST
    loot = f'loot insert {x} {y} {z} kill {ELDER}'

    def case(label, command, who=A):
        key = label.replace('-', '_')
        cmd(f'data merge block {x} {y} {z} {{Items:[]}}', label + '-empty')
        cmd(command, label + '-plain', who)
        cmd(f'data modify storage h038:proof {key}.chest set from block {x} {y} {z} Items', label + '-chest')
        cmd(f'execute store result storage h038:proof {key}.sponge int 1 if items block {x} {y} {z} '
            'container.* minecraft:wet_sponge', label + '-sponge')
        # The same again from an empty chest, with fresh stores.
        cmd(f'data merge block {x} {y} {z} {{Items:[]}}', label + '-empty-again')
        cmd('scoreboard players set r h038 -7', label + '-r-reset')
        cmd('scoreboard players set s h038 -7', label + '-s-reset')
        stored = f'execute store result score r h038 store success score s h038 run {command}'
        cmd(stored, label + '-stored', who)
        cmd(f'execute store result storage h038:proof {key}.r int 1 run scoreboard players get r h038',
            label + '-r')
        cmd(f'execute store result storage h038:proof {key}.s int 1 run scoreboard players get s h038',
            label + '-s')
        cmd(f'data modify storage h038:proof {key}.chest_again set from block {x} {y} {z} Items',
            label + '-chest-again')

    # Admin feedback to the other op is not this task's (diagnostic).
    cmd('gamerule log_admin_commands false', 'diag-admin-log')
    for text in ['gamerule spawn_mobs false', 'gamerule advance_time false', 'gamerule mob_griefing false',
                 'scoreboard objectives add h038 dummy']:
        cmd(text, 'setup-' + text.split()[1])
    cmd(f'setblock {x} {y} {z} chest', 'chest')
    still = 'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b,Motion:[0d,0d,0d]'
    cmd(f'summon elder_guardian 12.5 100 12.5 {{{still},UUID:[I;56,0,0,1]}}', 'elder-summon')
    cmd(f'summon pig 12.5 100 4.5 {{{still},UUID:[I;56,0,0,2]}}', 'pig-summon')
    for name, (px, py, pz) in VIEWS.items():
        cmd(f'gamemode survival {name}', f'mode-{name}')
        cmd(f'clear {name}', f'clear-{name}')
        cmd(f'tp {name} {px} {py} {pz} 0 0', f'tp-{name}')
        ops.append({'op': 'still', 'name': name})
    cmd(f'item replace entity {A} weapon.mainhand with diamond_sword', 'held-host')
    cmd(f'item replace entity {B} weapon.mainhand with diamond_sword[enchantments={{"minecraft:looting":3}}]',
        'held-guest')
    ops.append({'op': 'wait', 'ticks': 10})
    cmd('tick freeze', 'freeze')
    for name in VIEWS:
        cmd(f'data modify storage h038:proof players.{name} set from entity {name}', f'player-{name}')
    cmd(f'data modify storage h038:proof elder.before set from entity {ELDER}', 'elder-before')
    cmd('execute store result storage h038:proof clock.start long 1 run time query gametime', 'diag-clock-start')
    case('host', loot)
    case('target-as-guest', f'execute as {B} run {loot}')
    case('guest-direct', loot, B)
    case('pig', f'execute as {PIG} run {loot}')
    case('host-again', loot)
    # Last: every case shares the table's random sequence.
    case('console', loot, 'console')
    cmd(f'data modify storage h038:proof elder.after set from entity {ELDER}', 'elder-after')
    for name in VIEWS:
        cmd(f'data modify storage h038:proof after.{name} set from entity {name}', f'after-{name}')
    cmd('execute store result storage h038:proof clock.end long 1 run time query gametime', 'diag-clock-end')
    return ops


def run(label, command, client_command, build, world, ops):
    directory = build / label
    port = net.server_dir(directory, world)
    server = net.Server(command, directory, label)
    wire = build / f'{label}-wire.txt'
    log = (build / f'{label}-client.log').open('w')
    console = {}
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
                if op['who'] == 'console':
                    start = len(server.lines)
                    server.console(op['cmd'])
                    wait_new(server, start, r'.')
                    client.sync([A, B])
                    console[op['label']] = server.lines[start:]
                    continue
                client.tell(f'send-cmd {op["who"]} ' + op['cmd'])
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
    (build / f'{label}-console.json').write_text(json.dumps(console, indent=2) + '\n')
    storage = [p for p in (directory / 'world').rglob('*.dat')
               if 'command_storage' in p.name and ('h038' in p.parts or 'h038' in p.name)]
    assert len(storage) == 1, storage
    proof = json.loads(json.dumps(nbt.read_dat(storage[0])))['data']['contents']['proof']
    chat = {op['label']: {A: [], B: []} for op in ops if op['op'] == 'cmd'}
    for line in wire.read_text().splitlines():
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
            # What the issuing player is told, and apart, what the other player gets.
            issuer = op['who'] if op['who'] in (A, B) else A
            other = B if issuer == A else A
            out[op['label'] + '/chat'] = chat[op['label']][issuer]
            out[op['label'] + '/other-chat'] = chat[op['label']][other]
    for name in ['players', 'after']:
        for who, p in proof[name].items():
            for key in ['UUID', 'Health', 'Pos', 'playerGameType', 'SelectedItem', 'Inventory']:
                out[f'{name}/{who}/{key}'] = p.get(key)
    for stage in ['before', 'after']:
        e = proof['elder'][stage]
        for key in ['UUID', 'Health', 'Pos', 'NoAI', 'DeathTime']:
            out[f'elder/{stage}/{key}'] = e.get(key)
    for key, value in proof.items():
        if isinstance(value, dict) and 'chest' in value:
            name = key.replace('_', '-')
            for field in ['chest', 'sponge', 'r', 's', 'chest_again']:
                out[f'{name}/{field.replace("_", "-")}'] = value.get(field)
    out['diag-clock'] = proof['clock']
    out['clock-stationary'] = proof['clock']['start'] == proof['clock']['end']
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
    _, build, java, native, client = net.sides('loot-kill-guest-parity',
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
        if reference is None:
            reference = actual
            print(label, len(actual), 'facts captured')
            continue
        assert actual.keys() == reference.keys(), 'canonical keys changed'
        all_differences = [{'label': k, 'expected': reference[k], 'actual': actual[k]}
                           for k in reference if reference[k] != actual[k]]
        # Absolute clocks are engine-specific: recorded, compared apart.
        differences = [d for d in all_differences if not d['label'].startswith('diag-')]
        strict = [k for k in reference if not k.startswith('diag-')]
        report = {'records': len(strict), 'passed': len(strict) - len(differences), 'differences': differences,
                  'diagnostic_differences': [d for d in all_differences if d['label'].startswith('diag-')]}
        (build / f'{label}-report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(label, report['passed'], '/', report['records'])


if __name__ == '__main__':
    main()
