#!/usr/bin/env python3
"""Moving on wool makes no vibration on a dedicated server, against vanilla's, seen by a vanilla client.

A vanilla dedicated server and Horizon's each serve a copy of one flat world. KnockbackClient, a player made of
vanilla's protocol code, sends the positions a real client sends for a jump, a drop and a walk, sneaking or not, on
stone (the control), wool, a wool carpet and a wool slab, and beside a hole at a block's edge. A cow, an item and an
armor stand fall on the same floors. A sculk sensor stands three blocks away, and a shrieker beside it in two cases.

After each case the sensor's `last_vibration_frequency` and phase (and the shrieker's state) are read with commands.
A player hurt on each floor is the positive control: the same sensor hears it, so a silent sensor on wool is the
dampening (VibrationSystem.User.isValidVibration) and not an occluded signal.

Each case is compared on its own. KNOWN lists the cases Horizon still gets wrong and why; one that matches again is
reported, and should leave the list.
"""
import difflib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time

try:
    from . import lab, test_rust_parity as base
    from .test_dedicated_server_parity import Server, wait_ticks
    from .test_locator_bar_parity import generate_world
    from .test_server_knockback_parity import server_dir
except ImportError:
    import lab
    import test_rust_parity as base
    from test_dedicated_server_parity import Server, wait_ticks
    from test_locator_bar_parity import generate_world
    from test_server_knockback_parity import server_dir

NAME = 'server-wool-vibration-parity'
KNOWN = {}

SENSOR = (2, -60, 3)
SHRIEKER = (4, -60, 3)
START = (0.5, 0.5)
FAR = '0.5 -60 30.5'
FULL = -60.0
FLOORS = {
    'stone': (FULL, []),
    'wool': (FULL, ['fill -2 -61 -2 7 -61 1 minecraft:white_wool']),
    'carpet': (FULL + 0.0625, ['fill -2 -60 -2 7 -60 1 minecraft:white_carpet']),
    'slab': (FULL + 0.5, ['fill -2 -60 -2 7 -60 1 minecraft:white_wool_slab']),
    # The player's middle is over the hole: their supporting block is the one at x = 0.
    'stone-edge': (FULL, ['fill 1 -61 -2 7 -61 1 minecraft:air']),
    'wool-edge': (FULL, ['fill -2 -61 -2 0 -61 1 minecraft:white_wool', 'fill 1 -61 -2 7 -61 1 minecraft:air']),
}


def at(x, y, ground):
    return [f'pos Alice {x} {y} {START[1]} 0 0 {1 if ground else 0}', 'sleep 0.05']


def jump(x, ground):
    """LivingEntity.jumpFromGround's rise and the fall back, one client position a tick."""
    lines, y, v = [], ground, 0.41999998688697815
    while True:
        y += v
        v = (v - 0.08) * 0.98
        if y <= ground:
            return lines + at(x, ground, True) * 2
        lines += at(x, y, False)


def drop(x, ground, height=1.25):
    lines, y, v = [], ground + height, 0.0
    while True:
        v = (v - 0.08) * 0.98
        y += v
        if y <= ground:
            return lines + at(x, ground, True) * 2
        lines += at(x, y, False)


def walk(x, ground):
    return [line for i in range(1, 21) for line in at(round(x + 0.2 * i, 3), ground, True)]


def arena(floor):
    # The previous case's cow drops its loot as /kill runs, after the selector picked the cow, and its corpse stays
    # for 20 ticks (LivingEntity.tickDeath), still moving: the items, or the corpse once the fill below removes the
    # carpet or slab under it, hit the ground while the next case measures, at a wall-clock moment (item-stone,
    # item-carpet and item-slab flaked on vanilla's side, 2026-11-07). The second /kill removes the items, and 25
    # server ticks let the corpse go before the arena changes.
    return ['step setup', 'cmd Alice time set 1000', 'cmd Alice kill @e[type=!player]', 'cmd Alice kill @e[type=item]',
            'ticks 25',
            'cmd Alice fill -3 -61 -3 8 -61 5 minecraft:stone', 'cmd Alice fill -3 -60 -3 8 -57 5 minecraft:air',
            *[f'cmd Alice {command}' for command in FLOORS[floor][1]]]


def sensor(shrieker=False):
    lines = [f'cmd Alice setblock {SENSOR[0]} {SENSOR[1]} {SENSOR[2]} minecraft:sculk_sensor']
    if shrieker:
        lines.append(f'cmd Alice setblock {SHRIEKER[0]} {SHRIEKER[1]} {SHRIEKER[2]} minecraft:sculk_shrieker')
    return lines


FREQUENCY = f'cmd Alice data get block {SENSOR[0]} {SENSOR[1]} {SENSOR[2]} last_vibration_frequency'
PHASE = (f'cmd Alice execute if block {SENSOR[0]} {SENSOR[1]} {SENSOR[2]} '
         'minecraft:sculk_sensor[sculk_sensor_phase=active]')
SHRIEKING = (f'cmd Alice execute if block {SHRIEKER[0]} {SHRIEKER[1]} {SHRIEKER[2]} '
             'minecraft:sculk_shrieker[shrieking=true]')


def player_case(case, floor, action, sneak=False, shrieker=False):
    ground = FLOORS[floor][0]
    x = 1.05 if floor.endswith('-edge') else START[0]
    height = 1.25 if action is drop else 0
    lines = [*arena(floor), f'cmd Alice tp @s {x} {ground + height} {START[1]} 0 0',
             *at(x, ground + height, height == 0) * 3]
    if sneak:
        lines += ['sneak Alice on', 'sleep 0.1']
    lines += [*sensor(shrieker), 'ticks 5', f'step {case}', FREQUENCY]
    if action == 'hurt':
        lines.append('cmd Alice damage @s 1 minecraft:generic')
    else:
        lines += action(x, ground)
    lines += ['ticks 12', FREQUENCY, PHASE]
    if shrieker:
        lines.append(SHRIEKING)
    lines += ['step setup', 'cmd Alice effect give @s minecraft:instant_health 1 5 true']
    if sneak:
        lines += ['sneak Alice off', 'sleep 0.1']
    return lines


def entity_case(case, floor, summon):
    ground = FLOORS[floor][0]
    x = 1.05 if floor.endswith('-edge') else START[0]
    return [*arena(floor), f'cmd Alice tp @s {FAR} 0 0', *['sleep 0.05'] * 3, *sensor(), 'ticks 5', f'step {case}', FREQUENCY,
            f'cmd Alice summon {summon.format(pos=f"{x} {ground + 3} {START[1]}")}', 'ticks 40', FREQUENCY,
            'step setup']


COW = 'minecraft:cow {pos} {{active_effects:[{{id:"minecraft:slowness",amplifier:127b,duration:1000}}]}}'
CASES = {}
for floor_name in ('stone', 'wool', 'carpet', 'slab'):
    CASES[f'hurt-{floor_name}'] = player_case(f'hurt-{floor_name}', floor_name, 'hurt')
    for action_name, action_fn in (('jump', jump), ('drop', drop), ('walk', walk)):
        CASES[f'{action_name}-{floor_name}'] = player_case(f'{action_name}-{floor_name}', floor_name, action_fn)
    for entity, summon in (('cow', COW),
                           ('item', 'minecraft:item {pos} {{Item:{{id:"minecraft:stone",count:1}},PickupDelay:32767s}}'),
                           ('stand', 'minecraft:armor_stand {pos}')):
        CASES[f'{entity}-{floor_name}'] = entity_case(f'{entity}-{floor_name}', floor_name, summon)
for floor_name in ('stone', 'wool'):
    CASES[f'sneak-jump-{floor_name}'] = player_case(f'sneak-jump-{floor_name}', floor_name, jump, sneak=True)
    CASES[f'sneak-walk-{floor_name}'] = player_case(f'sneak-walk-{floor_name}', floor_name, walk, sneak=True)
    CASES[f'shrieker-jump-{floor_name}'] = player_case(f'shrieker-jump-{floor_name}', floor_name, jump, shrieker=True)
for floor_name in ('stone-edge', 'wool-edge'):
    CASES[f'jump-{floor_name}'] = player_case(f'jump-{floor_name}', floor_name, jump)
    CASES[f'cow-{floor_name}'] = entity_case(f'cow-{floor_name}', floor_name, COW)


def run_side(label, server_command, client_command, build, world):
    directory = build / label
    port = server_dir(directory, world)
    out = build / f'{label}.txt'
    out.unlink(missing_ok=True)
    server = Server(server_command, directory, label)
    try:
        server.wait_for(r'Done \(')
        with (build / f'{label}-client.log').open('w') as log:
            client = subprocess.Popen([*map(str, client_command), '127.0.0.1', str(port), str(out)], cwd=lab.ROOT,
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)

            def sync():
                client.stdin.write('sync Alice\n')
                client.stdin.flush()
                while (reply := client.stdout.readline()) != 'synced Alice\n':
                    if not reply:
                        raise AssertionError(f'{label}: the client exited (see {build}/{label}-client.log)')
                    log.write(reply)

            def send(line):
                word = line.split()[0]
                if word == 'sleep':
                    client.stdin.flush()
                    time.sleep(float(line.split()[1]))
                elif word == 'ticks':
                    sync()
                    wait_ticks(server, int(line.split()[1]))
                    sync()
                else:
                    client.stdin.write(line + '\n')
                    client.stdin.flush()
            try:
                send('join Alice')
                server.wait_for(r'Alice joined the game', timeout=120)
                wait_ticks(server, 40)
                for lines in CASES.values():
                    for line in lines:
                        send(line)
                send('end')
                log.write(client.communicate(timeout=120)[0])
            finally:
                if client.poll() is None:
                    client.kill()
    finally:
        server.stop()
    if not out.exists():
        raise AssertionError(f'{label}: the client wrote no transcript (see {build}/{label}-client.log)')
    return out.read_text(encoding='utf-8').splitlines()


def compared(lines):
    cases = {}
    for line in lines:
        _, step, text = line.split(' ', 2)
        step = step.removeprefix('step=')
        if step in CASES and text.startswith(('chat ', 'teleport ', 'error', 'disconnected')):
            cases.setdefault(step, []).append(text)
    return cases


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / NAME
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/KnockbackClient.java'
    classes = build / 'classes'
    shutil.rmtree(classes, ignore_errors=True)
    classes.mkdir()
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none', '-nowarn', '-sourcepath',
                    lab.ROOT / 'mods/horizon/src/test/java', '-cp', dependencies, '-d', classes, oracle],
                   cwd=lab.ROOT, check=True, capture_output=True)
    subprocess.run(['cargo', 'build', '--locked', '--profile', 'fast', '-p', 'horizon-client', '--bin', 'horizon-client'],
                   cwd=lab.ROOT, check=True, capture_output=True)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
                                                  cwd=lab.ROOT, text=True))
    binary = Path(metadata['target_directory']) / 'fast' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    vanilla_server = ['java', '-Xmx2g', '-Duser.timezone=UTC', '-cp', dependencies, 'net.minecraft.server.Main', 'nogui']
    horizon_server = [binary, '--resources', lab.LOCAL / 'horizon-client/26.3', '--dedicated', '--nogui']
    client = ['java', '-Xmx1g', '-cp', str(classes) + os.pathsep + dependencies, 'dev.horizonlab.parity.KnockbackClient']

    world = generate_world(vanilla_server, build)
    vanilla = compared(run_side('vanilla', vanilla_server, client, build, world))
    horizon = compared(run_side('horizon', horizon_server, client, build, world))
    differ, fixed = [], []
    for case in CASES:
        want, got = vanilla.get(case, []), horizon.get(case, [])
        print(f'{"same  " if want == got else "DIFFER"} {case}: vanilla {want}' + ('' if want == got else f' horizon {got}'))
        if not want or any(line.startswith(('error', 'disconnected')) for line in want):
            raise AssertionError(f'{case}: vanilla recorded nothing or failed: {want}')
        if want == got:
            if case in KNOWN:
                fixed.append(case)
        elif case not in KNOWN:
            differ.append(f'{case}:\n' + '\n'.join(difflib.unified_diff(want, got, 'vanilla', 'horizon', n=1, lineterm='')))
    if fixed:
        print('matching vanilla now, take them out of KNOWN: ' + ', '.join(fixed))
    if differ:
        raise AssertionError(f'{len(differ)} cases differ (see {build}/vanilla.txt and horizon.txt):\n'
                             + '\n'.join(differ)[:8000])
    lab.write_json(report, dict(
        schema=1, minecraft=lock['minecraft']['version'], platform=platform.platform(),
        java=subprocess.check_output(['java', '--version'], text=True).splitlines()[0],
        cases=len(CASES), known=sorted(KNOWN), output_sha256=lab.digest(build / 'horizon.txt', 'sha256'),
        scope='A sculk sensor (and a shrieker) on a dedicated server, as a vanilla-codec player jumps, drops, walks '
              'and sneaks on stone, wool, a wool carpet and a wool slab or at a block\'s edge, and as a cow, an item '
              'and an armor stand fall on the same floors.'))
    print(f'server wool vibration: {len(CASES) - len(KNOWN)} of {len(CASES)} cases identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
