#!/usr/bin/env python3
"""A bed used in the Nether blows up as vanilla's does, for all four facings, seen by a vanilla client.

A vanilla dedicated server and Horizon's each serve a copy of one flat world. Alice (survival, Normal, 200 health,
full netherite) stands in the Nether on an obsidian floor at y=200 and right-clicks a bed's head (the pillow). For
each facing (north, south, west, east) she stands in three places: behind the bed's foot, beside the head, and beside
the foot with an obsidian block hiding her from the head. Her health after the blast and the push her client is sent
are compared exactly (AbstractBedBlock.useWithoutItem, BedBlock.destroyOnUse, ServerExplosion.hurtEntities).
"""
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import difflib

try:
    from . import lab, test_rust_parity as base, test_server_knockback_parity as kb
    from .test_locator_bar_parity import generate_world
except ImportError:
    import lab
    import test_rust_parity as base
    import test_server_knockback_parity as kb
    from test_locator_bar_parity import generate_world

NAME = 'bed-explosion-parity'
FACINGS = {'north': (0, -1), 'south': (0, 1), 'west': (-1, 0), 'east': (1, 0)}
NETHERITE = [f'cmd Alice item replace entity @s armor.{s} with minecraft:netherite_{p}'
             for s, p in (('head', 'helmet'), ('chest', 'chestplate'), ('legs', 'leggings'), ('feet', 'boots'))]


def scene(index, facing, place):
    dx, dz = FACINGS[facing]
    lx, lz = -dz, dx
    hx, hz = 100 + 16 * index, 0
    fx, fz = hx - dx, hz - dz
    cf = (fx + .5, fz + .5)
    ch = (hx + .5, hz + .5)
    wall = None
    if place == 'behind-foot':
        at = (cf[0] - dx, cf[1] - dz)
    elif place == 'beside-head':
        at = (ch[0] + lx, ch[1] + lz)
    else:
        at = (cf[0] + lx, cf[1] + lz)
        wall = (hx + lx, hz + lz)
    cmds = [f'cmd Alice fill {hx - 4} 199 {hz - 4} {hx + 4} 199 {hz + 4} minecraft:obsidian',
            f'cmd Alice fill {hx - 4} 200 {hz - 4} {hx + 4} 203 {hz + 4} minecraft:air',
            f'cmd Alice setblock {fx} 200 {fz} minecraft:red_bed[facing={facing},part=foot]',
            f'cmd Alice setblock {hx} 200 {hz} minecraft:red_bed[facing={facing},part=head]']
    if wall:
        cmds.append(f'cmd Alice setblock {wall[0]} 200 {wall[1]} minecraft:obsidian')
    pos = f'{at[0]} 200.0 {at[1]}'
    return [*cmds, f'cmd Alice tp Alice {pos} 0 0', f'pos Alice {pos} 0 0 1', 'ticks 20',
            'cmd Alice effect give @s minecraft:instant_health 1 10 true', 'ticks 5',
            f'useblock Alice {hx} 200 {hz}', 'ticks 10', 'cmd Alice data get entity @s Health',
            'cmd Alice kill @e[type=!player]']


CASES = {f'{f}-{p}': scene(i, f, p) for i, f in enumerate(FACINGS) for p in ('behind-foot', 'beside-head', 'beside-foot-hidden')}
SETUP = ['cmd Alice gamerule spawn_mobs false', 'cmd Alice gamerule advance_time false', 'cmd Alice gamerule natural_health_regeneration false', 'cmd Alice time set midnight',
         'cmd Alice execute in minecraft:the_nether run tp Alice 100.5 200 0.5', 'ticks 100',
         'cmd Alice attribute @s minecraft:max_health base set 200', *NETHERITE, 'cmd Alice effect give @s minecraft:fire_resistance infinite 0 true', 'cmd Alice gamemode survival',
         'ticks 20']


def compared(lines):
    cases = {}
    for line in lines:
        who, step, text = line.split(' ', 2)
        step = step.removeprefix('step=')
        if step not in CASES:
            continue
        kept = cases.setdefault(step, [])
        if 'has the following entity data' in text or text.startswith(('explode push', 'error', 'disconnected')):
            kept.append(text)
    return cases


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / NAME
    build.mkdir(parents=True, exist_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/KnockbackClient.java'
    classes = build / 'classes'
    shutil.rmtree(classes, ignore_errors=True)
    classes.mkdir()
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none', '-nowarn', '-sourcepath',
                    lab.ROOT / 'mods/horizon/src/test/java', '-cp', dependencies, '-d', classes, oracle],
                   cwd=lab.ROOT, check=True, capture_output=True)
    subprocess.run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client', '--bin', 'horizon-client'], cwd=lab.ROOT,
                   check=True, capture_output=True)
    import json
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT, text=True))
    binary = Path(metadata['target_directory']) / 'fast' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    vanilla_server = ['java', '-Xmx2g', '-Duser.timezone=UTC', '-cp', dependencies, 'net.minecraft.server.Main', 'nogui']
    horizon_server = [binary, '--resources', lab.LOCAL / 'horizon-client/26.3', '--dedicated', '--nogui']
    client = ['java', '-Xmx1g', '-cp', str(classes) + os.pathsep + dependencies, 'dev.horizonlab.parity.KnockbackClient']
    kb.CASES, kb.SETUP, kb.PLAYERS = CASES, SETUP, ('Alice',)
    world = generate_world(vanilla_server, build)
    vanilla = compared(kb.run_side('vanilla', vanilla_server, client, build, world))
    horizon = compared(kb.run_side('horizon', horizon_server, client, build, world))
    differ = []
    for case in CASES:
        want, got = vanilla.get(case, []), horizon.get(case, [])
        if not any('entity data' in l for l in want) or any(l.startswith(('error', 'disconnected')) for l in want):
            raise AssertionError(f'{case}: vanilla recorded no health or failed: {want}')
        if want != got:
            differ.append(f'{case}:\n' + '\n'.join(difflib.unified_diff(want, got, 'vanilla', 'horizon', n=1, lineterm='')))
    if differ:
        raise AssertionError(f'{len(differ)} cases differ:\n' + '\n'.join(differ)[:8000])
    print(f'bed explosion: {len(CASES)} of {len(CASES)} cases identical')
    for case in CASES:
        print(case, vanilla[case])


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
