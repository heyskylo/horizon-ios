#!/usr/bin/env python3
"""An iron golem defends a villager from the real player it selects (golem-defense-selected-player-reputation).

`DefendVillageTargetGoal.canUse` asks each villager in the golem's box (10 across, 8 up and down) for the
reputation of each player in that box (`Villager.getPlayerReputation`, the villager's gossips about that
player's UUID); a player at -100 or below who is not creative or a spectator becomes the target.

Two real network players (LocatorBarClient) join a dedicated server. The host A stands 30 blocks away (out of
the box), the guest B 2 blocks from an iron golem with AI (`PlayerCreated` false); a NoAI villager stands in
the box. The villager's gossips name the players' real UUIDs (copied from their entities). B has Resistance IV
so it survives. First the control: B at -95 (major_negative 19) and A at -100 (major_negative 20, out of the
box); then the target: B at -100 and A at 0. Each phase runs 100 ticks; B's Health and its damage_taken
statistic are read before and after, with the golem's and villager's state and both players' places.

Label classes, declared before any native run: `target-` keys are the B at -100 phase's outcome; `diag-` keys
are recorded diagnostics (raw random damage, clocks); every other key is a setup admission or a control.
"""
import argparse
import json
from pathlib import Path

import anvil_dump as nbt
import test_locator_bar_parity as net
from test_disconnect_crafting_parity import generate, wait_new

A, B = 'GolemHost', 'GolemGuest'
GOLEM = '00000901-0000-0000-0000-000000000001'
VILLAGER = '00000901-0000-0000-0000-000000000002'
Y = 100
VIEWS = {A: (40.5, Y, 10.5), B: (12.5, Y, 10.5)}
PHASES = [('control', 19, 20), ('target', 20, None)]


def program():
    ops = []

    def cmd(text, label, who=A):
        ops.append({'op': 'cmd', 'cmd': text, 'label': label, 'who': who})

    cmd('gamerule log_admin_commands false', 'diag-admin-log')
    for text in ['gamerule spawn_mobs false', 'gamerule advance_time false', 'gamerule mob_griefing true',
                 'gamerule natural_regeneration false', 'difficulty normal']:
        cmd(text, 'setup-' + text.split()[-2 if text.startswith('gamerule') else 0])
    cmd('forceload add 0 0 47 31', 'forceload')
    cmd(f'fill 0 {Y - 1} 0 47 {Y - 1} 31 stone', 'floor')
    cmd(f'fill 0 {Y} 0 47 {Y + 4} 31 air', 'clear')
    cmd(f'summon iron_golem 10.5 {Y} 10.5 {{PersistenceRequired:1b,PlayerCreated:0b,Rotation:[-90f,0f],'
        f'UUID:[I;2305,0,0,1]}}', 'golem-summon')
    cmd(f'summon villager 14.5 {Y} 12.5 {{NoAI:1b,PersistenceRequired:1b,Silent:1b,UUID:[I;2305,0,0,2]}}',
        'villager-summon')
    for name, (px, py, pz) in VIEWS.items():
        cmd(f'gamemode survival {name}', f'mode-{name}')
        cmd(f'clear {name}', f'clear-{name}')
        cmd(f'tp {name} {px} {py} {pz} 90 0', f'tp-{name}')
        ops.append({'op': 'still', 'name': name})
    cmd(f'effect give {B} resistance infinite 3 true', 'resistance')
    cmd('scoreboard objectives add hurt minecraft.custom:minecraft.damage_taken', 'objective')
    for phase, b_value, a_value in PHASES:
        gossip = [f'{{Type:"major_negative",Value:{b_value}}}']
        if a_value is not None:
            gossip.append(f'{{Type:"major_negative",Value:{a_value}}}')
        # A gossip without a Target does not load: the whole list is made in storage first.
        cmd(f'data modify storage golem:proof {phase}.gossip set value [{",".join(gossip)}]', f'{phase}-gossip')
        cmd(f'data modify storage golem:proof {phase}.gossip[0].Target set from entity {B} UUID', f'{phase}-gossip-b')
        if a_value is not None:
            cmd(f'data modify storage golem:proof {phase}.gossip[1].Target set from entity {A} UUID',
                f'{phase}-gossip-a')
        cmd(f'data modify entity {VILLAGER} Gossips set from storage golem:proof {phase}.gossip',
            f'{phase}-gossip-set')
        cmd(f'effect give {B} instant_health 1 10 true', f'{phase}-heal')
        cmd(f'scoreboard players set {B} hurt 0', f'{phase}-hurt-reset')
        for name, (px, py, pz) in VIEWS.items():
            cmd(f'tp {name} {px} {py} {pz} 90 0', f'{phase}-tp-{name}')
            ops.append({'op': 'still', 'name': name})
        cmd(f'data modify storage golem:proof {phase}.villager set from entity {VILLAGER}', f'{phase}-villager')
        cmd(f'data modify storage golem:proof {phase}.golem set from entity {GOLEM}', f'{phase}-golem')
        for name in VIEWS:
            cmd(f'data modify storage golem:proof {phase}.before.{name} set from entity {name}',
                f'{phase}-before-{name}')
        cmd(f'execute store result storage golem:proof {phase}.clock.start long 1 run time query gametime',
            f'diag-{phase}-clock-start')
        ops.append({'op': 'wait', 'ticks': 100})
        cmd(f'execute store result storage golem:proof {phase}.clock.end long 1 run time query gametime',
            f'diag-{phase}-clock-end')
        for name in VIEWS:
            cmd(f'data modify storage golem:proof {phase}.after.{name} set from entity {name}',
                f'{phase}-after-{name}')
        cmd(f'execute store result storage golem:proof {phase}.hurt int 1 run scoreboard players get {B} hurt',
            f'{phase}-hurt')
        cmd(f'data modify storage golem:proof {phase}.golem_after set from entity {GOLEM}', f'{phase}-golem-after')
    return ops


def value(tag):
    """A typed NBT value's number (`['f', 20.0]` is 20.0)."""
    return tag[1] if isinstance(tag, list) and len(tag) == 2 and isinstance(tag[0], str) else tag


def facts(proof):
    out = {}
    for phase, b_value, a_value in PHASES:
        p = proof[phase]
        prefix = 'target-' if phase == 'target' else ''
        v = p['villager']
        out[f'{phase}/villager/UUID'] = v.get('UUID')
        out[f'{phase}/villager/NoAI'] = v.get('NoAI')
        out[f'{phase}/villager/Health'] = v.get('Health')
        out[f'{phase}/villager/Gossips'] = v.get('Gossips')
        g = p['golem']
        for key in ['UUID', 'Health', 'PlayerCreated', 'NoAI']:
            out[f'{phase}/golem/{key}'] = g.get(key)
        out[f'diag-{phase}/golem/Pos'] = g.get('Pos')
        for name in VIEWS:
            before = p['before'][name]
            for key in ['UUID', 'Health', 'playerGameType', 'Pos']:
                out[f'{phase}/before/{name}/{key}'] = before.get(key)
        hurt = value(p.get('hurt', 0))
        after = value(p['after'][B]['Health'])
        out[f'{phase}/gossip-built'] = p.get('gossip')
        out[f'{prefix}{phase}/guest-hurt'] = after < value(p['before'][B]['Health'])
        out[f'{prefix}{phase}/guest-damage-taken-positive'] = hurt > 0
        out[f'{phase}/host-health-kept'] = p['after'][A]['Health'] == p['before'][A]['Health']
        out[f'diag-{phase}/guest-health-after'] = after
        out[f'diag-{phase}/guest-damage-taken'] = hurt
        out[f'diag-{phase}/clock'] = p['clock']
        out[f'{phase}/ticks'] = value(p['clock']['end']) - value(p['clock']['start']) >= 100
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
    _, build, java, native, client = net.sides('golem-defense-guest-parity',
                                              args.binary.resolve() if args.binary else None)
    world = build / 'generate/world'
    if not world.exists():
        world = generate(java, build)
    ops = program()
    sides = ['java', 'native'] if args.side == 'both' else [args.side]
    reference = json.loads(args.reference.read_text()) if args.reference else None
    for side in sides:
        label = args.label or side
        proof = proof_of(label, java if side == 'java' else native, client, build, world, ops)
        actual = facts(proof)
        (build / f'{label}-facts.json').write_text(json.dumps(actual, indent=2, sort_keys=True) + '\n')
        if reference is None:
            reference = actual
            print(label, len(actual), 'facts captured')
            continue
        assert actual.keys() == reference.keys(), 'canonical keys changed'
        all_differences = [{'label': k, 'expected': reference[k], 'actual': actual[k]}
                           for k in reference if reference[k] != actual[k]]
        differences = [d for d in all_differences if not d['label'].startswith('diag-')]
        strict = [k for k in reference if not k.startswith('diag-')]
        report = {'records': len(strict), 'passed': len(strict) - len(differences), 'differences': differences,
                  'diagnostic_differences': [d for d in all_differences if d['label'].startswith('diag-')]}
        (build / f'{label}-report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(label, report['passed'], '/', report['records'])


def proof_of(label, command, client_command, build, world, ops):
    """Both players join, the program runs (commands as their sender, `still`, `wait`), the world is saved and
    the `golem:proof` storage is read (the run loop of test_loot_kill_guest_parity)."""
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
    storage = [p for p in (directory / 'world').rglob('*.dat')
               if 'command_storage' in p.name and ('golem' in p.parts or 'golem' in p.name)]
    assert len(storage) == 1, storage
    return json.loads(json.dumps(nbt.read_dat(storage[0])))['data']['contents']['proof']


if __name__ == '__main__':
    main()
