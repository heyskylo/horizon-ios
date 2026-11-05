#!/usr/bin/env python3
"""Real guest menu clicks, live close, disconnect drops, saves and same-UUID reconnect.

The raw transcript retains complete decoded packets and their bytes. Canonical menu
checkpoints replay authoritative content/slot/cursor packets, never a client prediction.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import time
import uuid

import anvil_dump as nbt
import test_locator_bar_parity as net

OBSERVER, GUEST = 'H319Observer', 'H319Guest'


def offline(name):
    raw = bytearray(hashlib.md5(f'OfflinePlayer:{name}'.encode()).digest())
    raw[6], raw[8] = (raw[6] & 15) | 48, (raw[8] & 63) | 128
    return str(uuid.UUID(bytes=bytes(raw)))


def uuid_words(name):
    import struct
    return list(struct.unpack('>4i', uuid.UUID(offline(name)).bytes))


def stack(value):
    return None if value is None else nbt.read_named(bytes.fromhex(value['nbt']))


def menus(path, label):
    windows, carried, opened = {}, None, None
    for line in path.read_text().splitlines():
        prefix = GUEST + ' step='
        if not line.startswith(prefix) or ' menu-wire ' not in line:
            continue
        step, raw = line[len(prefix):].split(' menu-wire ', 1)
        p = json.loads(raw)
        kind = p['kind']
        if kind == 'login':
            windows, carried, opened = {}, None, None
        elif kind == 'open':
            opened = p['window']
        elif kind == 'content':
            windows[p['window']] = [stack(s) for s in p['items']]
            carried = stack(p['carried'])
        elif kind == 'slot':
            if p['window'] in windows:
                windows[p['window']][p['slot']] = stack(p['item'])
        elif kind == 'cursor':
            carried = stack(p['item'])
        elif kind == 'inventory':
            i = p['slot']
            if 0 in windows:
                slot = 36 + i if i < 9 else i if i < 36 else 44 - i if i < 40 else 45
                windows[0][slot] = stack(p['item'])
            if opened in windows and i < 36:
                windows[opened][37 + i if i < 9 else 10 + i - 9] = stack(p['item'])
    return {'windows': windows, 'carried': carried, 'opened': opened}


def wait_new(server, start, pattern):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if any(re.search(pattern, line) for line in server.lines[start:]):
            return
        if server.process.poll() is not None:
            break
        time.sleep(.02)
    raise AssertionError(f'no new {pattern}: {server.lines[start:][-10:]}')


def generate(command, build):
    directory = build / 'generate'
    net.server_dir(directory)
    server = net.Server(command, directory, 'generate')
    try:
        server.wait_for(r'Done \(')
        for cmd, ack in [('tick freeze', 'frozen'), ('gamerule spawn_mobs false', 'spawn_mobs'),
                         ('gamerule advance_time false', 'advance_time'), ('gamerule respawn_radius 0', 'respawn_radius'),
                         ('forceload add 0 0 31 31', 'force loaded'), ('fill 0 99 0 31 99 31 stone', '1024'),
                         ('fill 0 100 0 31 104 31 air', 'blocks'), ('setworldspawn 4 100 4', 'world spawn'),
                         ('save-all flush', 'Saved the game')]:
            start = len(server.lines)
            server.console(cmd)
            wait_new(server, start, ack)
    finally:
        server.stop()
    return directory / 'world'


def run(label, command, client_command, build, world):
    directory = build / label
    port = net.server_dir(directory, world)
    server = net.Server(command, directory, label)
    out = build / f'{label}-wire.txt'
    checkpoints = {}
    log = (build / f'{label}-client.log').open('w')
    client = None
    try:
        server.wait_for(r'Done \(')
        client = net.Client(client_command, port, out, log)
        client.tell('menus on')
        client.tell('state on')
        client.tell('chat on')

        def sync():
            client.sync([OBSERVER, GUEST])

        def cmd(text, who=GUEST):
            action = 'cmd' if 'run time query gametime' in text else 'send-cmd'
            client.tell(f'{action} {who} {text}')
            client.sync([who])

        def phase(name):
            client.tell(f'step {name}')

        def checkpoint(name):
            sync()
            client.tell(f'dump {name}')
            while (line := client.process.stdout.readline()) != f'dumped {name}\n':
                assert line, 'client exited before menu dump'
                log.write(line)
            checkpoints[name] = menus(out, name)
            (build / f'{label}-menus.json').write_text(json.dumps(checkpoints, indent=2))
            return checkpoints[name]

        def await_menu(name, expected):
            deadline = time.monotonic() + 3
            while True:
                actual = checkpoint(name)
                if expected(actual):
                    return actual
                assert time.monotonic() < deadline, f'authoritative menu prerequisite {name}: {actual}'
                time.sleep(.02)

        def await_return(name, item):
            def returned(_):
                for line in out.read_text().splitlines():
                    if not line.startswith(f'{GUEST} step={name} menu-wire '):
                        continue
                    packet = json.loads(line.split(' menu-wire ', 1)[1])
                    player_slot = packet.get('kind') == 'inventory' and packet.get('slot') == 0
                    menu_slot = packet.get('kind') == 'slot' and packet.get('window') == 0 and packet.get('slot') == 36
                    if (player_slot or menu_slot) and stack(packet['item']) == item:
                        return True
                return False
            return await_menu(name, returned)

        def snapshot(name, player=True):
            cmd(f'data modify storage h319:proof {name}.clock set value 0L', OBSERVER)
            cmd(f'execute store result storage h319:proof {name}.clock long 1 run time query gametime', OBSERVER)
            if player:
                cmd(f'data modify storage h319:proof {name}.player set from entity {GUEST}', OBSERVER)
            cmd(f'data modify storage h319:proof {name}.drops set value []', OBSERVER)
            cmd(f'execute as @e[type=minecraft:item] run data modify storage h319:proof {name}.drops append from entity @s', OBSERVER)

        def disconnect(name):
            phase(name)
            start = len(server.lines)
            client.tell(f'quit {GUEST}')
            wait_new(server, start, GUEST + r'.*(left the game|lost connection)')
            client.sync([OBSERVER])
            cmd(f'execute store success storage h319:proof {name}.absent byte 1 run execute unless entity {offline(GUEST)}', OBSERVER)
            cmd(f'data modify storage h319:proof {name}.observer set from entity {OBSERVER}', OBSERVER)
            snapshot(name, False)
            start = len(server.lines)
            server.console('save-all flush')
            wait_new(server, start, 'Saved the game')
            files = list((directory / 'world').rglob(offline(GUEST) + '.dat'))
            assert len(files) == 1, files
            shutil.copy2(files[0], build / f'{label}-{name}-player.dat')

        def join(name):
            phase(name)
            client.tell(f'join {GUEST}')
            sync()
            checkpoint(name)
            snapshot(name)

        client.tell(f'join {OBSERVER}')
        client.sync([OBSERVER])
        cmd('tick freeze', OBSERVER)
        cmd('gamerule spawn_mobs false', OBSERVER)
        cmd('gamerule advance_time false', OBSERVER)
        cmd('tp @s 20.5 100 20.5', OBSERVER)
        join('first-join')
        cmd('gamemode survival')
        cmd('tp @s 4.5 100 4.5 0 0')
        cmd('clear @s')
        cmd('kill @e[type=minecraft:item]', OBSERVER)
        for i, (x, z) in enumerate([(0, 0), (0, 16), (16, 0), (16, 16)]):
            cmd(f'execute store success storage h319:proof guards.loaded{i} byte 1 run execute if loaded {x} 100 {z}', OBSERVER)
        for key, condition in [('floor', 'if block 4 99 4 minecraft:stone'), ('air', 'if block 4 100 4 minecraft:air')]:
            cmd(f'execute store success storage h319:proof guards.{key} byte 1 run execute {condition}', OBSERVER)
        cmd(f'execute store success storage h319:proof guards.guest_type byte 1 run execute if entity @a[nbt={{UUID:[I;{",".join(map(str, uuid_words(GUEST))) }]}}]', OBSERVER)
        snapshot('inventory_ready')
        cmd('item replace entity @s hotbar.0 with minecraft:dirt 10')
        cmd('item replace entity @s hotbar.1 with minecraft:stone 5')
        phase('inventory-inputs')
        checkpoint('inventory-inputs')
        for slot in (36, 1, 37):
            phase(f'inventory-click-{slot}')
            client.tell(f'click {GUEST} 0 {slot}')
            expected = None if slot == 1 else {'count': 10 if slot == 36 else 5, 'id': 'minecraft:dirt' if slot == 36 else 'minecraft:stone'}
            await_menu(f'inventory-click-{slot}', lambda m: m['carried'] == expected)
        admitted = checkpoint('inventory-admitted')
        assert admitted['windows'][0][1] == {'id': 'minecraft:dirt', 'count': 10}
        assert admitted['carried'] == {'id': 'minecraft:stone', 'count': 5}
        snapshot('inventory_admitted')
        disconnect('inventory_disconnect')
        join('inventory-reconnect')
        cmd('kill @e[type=minecraft:item]', OBSERVER)
        cmd('clear @s')
        cmd('item replace entity @s hotbar.0 with minecraft:oak_log 3')
        cmd('setblock 6 100 4 minecraft:crafting_table', OBSERVER)
        cmd('execute store success storage h319:proof guards.table byte 1 run execute if block 6 100 4 minecraft:crafting_table', OBSERVER)
        phase('table-open')
        client.tell(f'use-on {GUEST} 6 100 4')
        table = await_menu('table-open', lambda m: m['opened'] in m['windows'])
        window = table['opened']
        assert window is not None, 'no actual crafting-table window'
        for slot in (37, 5):
            phase(f'table-click-{slot}')
            client.tell(f'click {GUEST} {window} {slot}')
            expected = {'id': 'minecraft:oak_log', 'count': 3} if slot == 37 else None
            await_menu(f'table-click-{slot}', lambda m: m['carried'] == expected)
        admitted = checkpoint('table-admitted')
        assert admitted['windows'][window][5] == {'id': 'minecraft:oak_log', 'count': 3}
        assert admitted['windows'][window][0] == {'id': 'minecraft:oak_planks', 'count': 4}
        snapshot('table_admitted')
        disconnect('table_disconnect')
        join('table-reconnect')
        cmd('kill @e[type=minecraft:item]', OBSERVER)
        cmd('clear @s')
        cmd('item replace entity @s hotbar.0 with minecraft:stone[minecraft:custom_data={h319:7}] 5')
        phase('live-cursor-pickup')
        client.tell(f'click {GUEST} 0 36')
        await_menu('live-cursor-admitted', lambda m: m['carried'] is not None)
        phase('live-cursor-close')
        client.tell(f'close-menu {GUEST} 0')
        await_menu('live-cursor-returned', lambda m: m['carried'] is None)
        snapshot('live_cursor_returned')
        cmd('clear @s')
        cmd('item replace entity @s hotbar.0 with minecraft:oak_log 3')
        phase('live-table-open')
        client.tell(f'use-on {GUEST} 6 100 4')
        window = await_menu('live-table-open', lambda m: m['opened'] in m['windows'])['opened']
        for slot in (37, 5):
            client.tell(f'click {GUEST} {window} {slot}')
            expected = {'id': 'minecraft:oak_log', 'count': 3} if slot == 37 else None
            await_menu(f'live-table-click-{slot}', lambda m: m['carried'] == expected)
        checkpoint('live-table-admitted')
        phase('live-table-close')
        client.tell(f'close-menu {GUEST} {window}')
        await_return('live-table-close', {'id': 'minecraft:oak_log', 'count': 3})
        checkpoint('live-table-returned')
        snapshot('live_table_returned')
        client.end()
    finally:
        if client:
            client.kill()
        server.stop()
        log.close()
    storage = list((directory / 'world').rglob('*command_storage*.dat'))
    assert storage, 'no saved command-storage proof'
    raw = {str(p.relative_to(directory)): nbt.read_dat(p) for p in storage}
    (build / f'{label}-storage.json').write_text(json.dumps(raw, indent=2))
    return checkpoints, raw


def facts(build, label):
    """Declared exact fields plus typed finite/local facts for random drop entities.

    Complete packets, entity compounds and saves remain unmodified diagnostics.
    State IDs are protocol revision counters; drop UUIDs/motion are random.
    These are not compared as deterministic values or replaced in raw captures.
    """
    raw = json.loads((build / f'{label}-storage.json').read_text())
    proof = next(iter(raw.values()))['data']['contents']['proof']
    checkpoints = json.loads((build / f'{label}-menus.json').read_text())
    result = {}
    for name, value in checkpoints.items():
        active = value['opened'] if ('table' in name and name not in ('table-reconnect', 'live-table-returned', 'live-table-close')) else 0
        result[f'menu/{name}/window'] = active
        result[f'menu/{name}/slots'] = value['windows'][str(active)]
        result[f'menu/{name}/cursor'] = value['carried']
    start = proof['inventory_ready']['clock']
    for name, value in sorted(proof.items()):
        if name == 'guards':
            for key, actual in value.items():
                result[f'guard/{key}'] = actual
            continue
        result[f'{name}/clock-type'] = value['clock'][0]
        result[f'{name}/clock-delta'] = value['clock'][1] - start[1]
        if 'player' in value:
            for key in ('UUID', 'Health', 'Pos', 'Rotation', 'playerGameType', 'Inventory'):
                result[f'{name}/player/{key}'] = value['player'].get(key)
        if 'absent' in value:
            result[f'{name}/guest-absent'] = value['absent']
            for key in ('UUID', 'Health', 'Pos', 'playerGameType'):
                result[f'{name}/observer/{key}'] = value['observer'].get(key)
        drops = value['drops']
        result[f'{name}/drop-stacks'] = sorted((d['Item'] for d in drops), key=lambda s: s['id'])
        result[f'{name}/drop-count'] = len(drops)
        ids = [d.get('UUID') for d in drops]
        result[f'{name}/drop-identity'] = all(
            isinstance(v, list) and len(v) == 2 and v[0] == 'I' and len(v[1]) == 4
            and any(v[1]) and all(type(i) is int for i in v[1]) for v in ids
        ) and len({json.dumps(v) for v in ids}) == len(ids)
        result[f'{name}/drop-typed-state'] = all(
            d.get('Health') == ['s', 5] and d.get('Age') == ['s', 0]
            and d.get('PickupDelay') == ['s', 40]
            and d.get('Pos') == [['d', 4.5], ['d', 101.31999999284744], ['d', 4.5]] for d in drops)
        import math
        result[f'{name}/drop-motion-local-finite'] = all(
            len(d.get('Motion', [])) == 3 and all(
                isinstance(v, list) and v[0] == 'd' and math.isfinite(v[1]) and abs(v[1]) <= 1
                for v in d['Motion']) for d in drops)
    for name in ('inventory_disconnect', 'table_disconnect'):
        saved = nbt.read_dat(build / f'{label}-{name}-player.dat')
        for key in ('UUID', 'Health', 'Pos', 'playerGameType', 'Inventory'):
            result[f'save/{name}/{key}'] = saved.get(key)
    # The complete relevant component JSON is retained alongside the raw packet bytes.
    wires = [json.loads(line.split(' menu-wire ', 1)[1]) for line in (build / f'{label}-wire.txt').read_text().splitlines()
             if line.startswith(GUEST + ' ') and ' menu-wire ' in line]
    result['network/login-modes'] = [p['mode'] for p in wires if p['kind'] == 'login']
    result['network/open-menu-types'] = [p['type'] for p in wires if p['kind'] == 'open']
    return json.loads(json.dumps(result))


def validate_java(result):
    """Independent fixture expectations, checked before using Java as the paired reference."""
    def item(name, count, **extra):
        return {'id': 'minecraft:' + name, 'count': count, **extra}
    def slots(entries):
        return [entries.get(i) for i in range(46)]
    custom = item('stone', 5, components={'minecraft:custom_data': {'h319': 7}})
    expected = {
        'first-join': ({}, None),
        'inventory-inputs': ({36: item('dirt', 10), 37: item('stone', 5)}, None),
        'inventory-click-36': ({37: item('stone', 5)}, item('dirt', 10)),
        'inventory-click-1': ({1: item('dirt', 10), 37: item('stone', 5)}, None),
        'inventory-click-37': ({1: item('dirt', 10)}, item('stone', 5)),
        'inventory-admitted': ({1: item('dirt', 10)}, item('stone', 5)),
        'inventory-reconnect': ({}, None),
        'table-open': ({37: item('oak_log', 3)}, None),
        'table-click-37': ({}, item('oak_log', 3)),
        'table-click-5': ({0: item('oak_planks', 4), 5: item('oak_log', 3)}, None),
        'table-admitted': ({0: item('oak_planks', 4), 5: item('oak_log', 3)}, None),
        'table-reconnect': ({}, None),
        'live-cursor-admitted': ({}, custom),
        'live-cursor-returned': ({36: custom}, None),
        'live-table-open': ({37: item('oak_log', 3)}, None),
        'live-table-click-37': ({}, item('oak_log', 3)),
        'live-table-click-5': ({0: item('oak_planks', 4), 5: item('oak_log', 3)}, None),
        'live-table-admitted': ({0: item('oak_planks', 4), 5: item('oak_log', 3)}, None),
        'live-table-close': ({36: item('oak_log', 3)}, None),
        'live-table-returned': ({36: item('oak_log', 3)}, None),
    }
    # Closed table packets need not refresh the abandoned preview/grid; its active
    # inventory is checked by full saved command state below, not stale client slots.
    for name, (entries, cursor) in expected.items():
        assert result[f'menu/{name}/slots'] == slots(entries), name
        assert result[f'menu/{name}/cursor'] == cursor, name
    for key, actual in result.items():
        if key.endswith('/clock-delta'):
            assert actual == 0, (key, actual)
        if key.startswith('guard/') or key.endswith('/guest-absent'):
            assert actual == ['b', 1], (key, actual)
        if '/drop-' in key and key.endswith(('identity', 'typed-state', 'local-finite')):
            assert actual is True, (key, actual)
    for phase in ('inventory_disconnect', 'inventory-reconnect'):
        assert result[f'{phase}/drop-stacks'] == [item('dirt', 10), item('stone', 5)]
    for phase in ('table_disconnect', 'table-reconnect'):
        assert result[f'{phase}/drop-stacks'] == [item('oak_log', 3)]
    assert result['live_cursor_returned/player/Inventory'] == [dict(custom, Slot=['b', 0])]
    assert result['live_table_returned/player/Inventory'] == [dict(item('oak_log', 3), Slot=['b', 0])]
    for name in ('inventory_disconnect', 'table_disconnect'):
        assert result[f'save/{name}/Inventory'] == []
    for key, actual in result.items():
        if key.endswith('/UUID'):
            who = OBSERVER if '/observer/' in key else GUEST
            assert actual == ['I', uuid_words(who)], (key, actual)
        elif key.endswith('/Health'):
            assert actual == ['f', 20.0], (key, actual)
        elif key.endswith('/Pos'):
            expected_pos = [20.5, 100., 20.5] if '/observer/' in key else [4.5, 100., 4.5]
            assert actual == [['d', value] for value in expected_pos], (key, actual)
        elif key.endswith('/playerGameType'):
            expected_mode = 1 if '/observer/' in key or key.startswith('first-join/') else 0
            assert actual == expected_mode, (key, actual)
        elif key.endswith('/Rotation'):
            assert actual == [['f', 0.], ['f', 0.]], (key, actual)
    assert result['network/login-modes'] == [1, 0, 0]
    assert result['network/open-menu-types'] == ['minecraft:crafting', 'minecraft:crafting']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--side', choices=['java', 'native', 'both'], default='both')
    parser.add_argument('--label')
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--build-name', default='disconnect-crafting-parity')
    parser.add_argument('--reference', type=Path, help='An unchanged fresh Java facts file for an old/fixed pair')
    args = parser.parse_args()
    net.PLAYERS = [OBSERVER, GUEST]
    net.PROPERTIES['force-gamemode'] = 'false'
    _, build, java, native, client = net.sides(args.build_name, args.binary.resolve() if args.binary else None)
    world = build / 'generate/world'
    if not world.exists():
        world = generate(java, build)
    reference = json.loads(args.reference.read_text()) if args.reference else None
    for side in (['java', 'native'] if args.side == 'both' else [args.side]):
        label = args.label or side
        if args.side == 'both':
            label = f'{label}-{side}' if args.label else side
        run(label, java if side == 'java' else native, client, build, world)
        actual = facts(build, label)
        (build / f'{label}-facts.json').write_text(json.dumps(actual, indent=2, sort_keys=True))
        if side == 'java':
            validate_java(actual)
            reference = actual
        if reference is not None:
            differences = [{'label': key, 'expected': reference[key], 'actual': actual.get(key)}
                           for key in sorted(reference) if reference[key] != actual.get(key)]
            assert set(actual) == set(reference), 'the declared fact labels changed'
            report = {'checks': len(reference), 'passed': len(reference) - len(differences),
                      'differences': differences, 'scope': __doc__}
            (build / f'{label}-report.json').write_text(json.dumps(report, indent=2))
            print(f'{label}: {report["passed"]}/{report["checks"]} exact facts')
            if differences:
                raise SystemExit(1)
        else:
            print(f'{label}: {len(actual)} declared facts captured')


if __name__ == '__main__':
    main()
