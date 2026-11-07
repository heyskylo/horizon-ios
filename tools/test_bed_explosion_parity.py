#!/usr/bin/env python3
"""A bed used in the Nether blows up as vanilla's does, for all four facings, seen by a vanilla client.

A vanilla dedicated server and Horizon's each serve a copy of one flat world. Alice (survival, Normal, 200 health,
full netherite) stands in the Nether on an obsidian floor at y=200 and right-clicks a bed's head (the pillow). For
each facing (north, south, west, east) she stands in three places: behind the bed's foot, beside the head, and beside
the foot with an obsidian block hiding her from the head. Her health after the blast and the push her client is sent
are compared exactly (AbstractBedBlock.useWithoutItem, BedBlock.destroyOnUse, ServerExplosion.hurtEntities).
What the blast does to something that is not a player is compared too: an armour stand, an invisible one, an item and
a boat stand in front of the bed's head, and each one is asked whether it survived and, if it did, where it is and how
it moves (`Entity.ignoreExplosion` and `ArmorStand.ignoreExplosion`'s `isInvisible`).
"""
import os
from pathlib import Path
import platform
import re
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


# The things that are not the player standing in the blast, by the tag their
# facts are read back under: an armour stand, one made invisible
# (`ArmorStand.ignoreExplosion` asks `isInvisible`), an item and a boat close
# to the bed, and an armour stand and an item at the edge of the blast, past
# the damage but inside the push, which are the two that survive it.
VICTIMS = ('stand', 'invisible', 'item', 'boat', 'edge')


def victim_facts(tag):
    """What the blast did to one victim: whether it is still there, and its
    position and motion if it is."""
    at = f'@e[tag={tag},limit=1]'
    facts = [f'cmd Alice execute if entity {at}',
             f'cmd Alice data get entity {at} Pos',
             f'cmd Alice data get entity {at} Motion']
    if tag == 'edge-item':
        # The edge item's motion decays with its own ticks, so its age says
        # which tick the value belongs to.
        facts.append(f'cmd Alice data get entity {at} Age')
    return facts


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
    # The victims stand in front of the head, where no placement puts Alice or
    # the wall, and none of them carries gravity, so the only motion any of
    # them can get is the blast's own.
    def spot(along, side):
        return f'{hx + dx * along + lx * side}.5 200 {hz + dz * along + lz * side}.5'
    # The edge pair sits past the blast's damage but inside its reach: the
    # damage falls to nothing around 9.3 blocks of a power-5 blast, and what is
    # left there is the push, so both survive and their Motion is that push.
    def ring(along, side):
        x = hx + dx * along + lx * side
        z = hz + dz * along + lz * side
        return f'{x + .5:.1f} 200 {z + .5:.1f}'
    cmds += [f'cmd Alice summon minecraft:armor_stand {spot(2, 0)} {{Tags:["stand"],NoGravity:1b}}',
             f'cmd Alice summon minecraft:armor_stand {spot(3, 0)} {{Tags:["invisible"],Invisible:1b,NoGravity:1b}}',
             f'cmd Alice summon minecraft:item {spot(4, 0)} '
             f'{{Item:{{id:"minecraft:stone",count:1}},Tags:["item"],NoGravity:1b,Age:1s}}',
             f'cmd Alice summon minecraft:oak_boat {spot(2, -2)} {{Tags:["boat"],NoGravity:1b}}',
             f'cmd Alice summon minecraft:armor_stand {ring(9.5, 0)} {{Tags:["edge"],NoGravity:1b}}',
             f'cmd Alice summon minecraft:item {ring(8.5, -4)} '
             f'{{Item:{{id:"minecraft:stone",count:1}},Tags:["edge-item"],NoGravity:1b,Age:1s}}',
]
    pos = f'{at[0]} 200.0 {at[1]}'
    return [*cmds, f'cmd Alice tp Alice {pos} 0 0', f'pos Alice {pos} 0 0 1', 'ticks 20',
            'cmd Alice effect give @s minecraft:instant_health 1 10 true', 'ticks 5',
            # The two servers need not advance the same number of game ticks
            # between the same commands, and a push on an item decays every
            # tick, so the survivor is read as whole compounds (`Age`, `Pos` and
            # `Motion` from one snapshot) at two pinned ages, with the age it had
            # when the blast landed read first. That is enough to compare both
            # the push itself and the rate it decays at, whatever the pacing.
            'cmd Alice data get entity @e[tag=edge-item,limit=1] Age',
            f'useblock Alice {hx} 200 {hz}',
            'waitage @e[tag=edge-item,limit=1] 81',
            'cmd Alice data get entity @e[tag=edge-item,limit=1]',
            'waitage @e[tag=edge-item,limit=1] 91',
            'cmd Alice data get entity @e[tag=edge-item,limit=1]',
            'cmd Alice execute if entity @e[tag=edge-item,limit=1]',
            'cmd Alice data get entity @s Health',
            *[line for tag in VICTIMS for line in victim_facts(tag)],
            'cmd Alice kill @e[type=!player]']


CASES = {f'{f}-{p}': scene(i, f, p) for i, f in enumerate(FACINGS) for p in ('behind-foot', 'beside-head', 'beside-foot-hidden')}
SETUP = ['cmd Alice gamerule spawn_mobs false', 'cmd Alice gamerule advance_time false', 'cmd Alice gamerule natural_health_regeneration false', 'cmd Alice time set midnight',
         'cmd Alice execute in minecraft:the_nether run tp Alice 100.5 200 0.5', 'ticks 100',
         'cmd Alice attribute @s minecraft:max_health base set 200', *NETHERITE, 'cmd Alice effect give @s minecraft:fire_resistance infinite 0 true', 'cmd Alice gamemode survival',
         'ticks 20']


DATA = re.compile(r'has the following entity data: (\[.*\])$')


def nine(text):
    """A list of doubles or floats to 9 decimal places. The blast writes
    arbitrary bits into a victim's Pos and Motion, and NBT prints the shortest
    form that round-trips, so the raw text of those two is not a stable
    comparison (`test_server_knockback_parity` reads its mob the same way).
    A scalar (`@s Health`) stays as it was printed."""
    found = DATA.search(text)
    if not found:
        return text
    return 'data ' + ' '.join(f'{float(v.rstrip("df")):.9f}' for v in found.group(1)[1:-1].split(', '))


# The survivor's whole NBT in one reply: `Age`, `Pos` and `Motion` from one
# snapshot, so the motion can be rescaled to a fixed age rather than compared at
# whatever tick each server's read landed on.
COMPOUND = re.compile(r'has the following entity data: \{(.*)\}$')
AGE_REF = 71
AIR_DRAG = 0.98


def snapshot(text):
    """One compound reply as `(age, motion, pos)`: the survivor's own tick
    counter, how it moves and where it is."""
    found = COMPOUND.search(text)
    if not found:
        return None
    body = found.group(1)
    age = re.search(r'\bAge:\s*(-?\d+)', body)
    motion = re.search(r'\bMotion:\s*\[([^\]]+)\]', body)
    pos = re.search(r'\bPos:\s*\[([^\]]+)\]', body)
    health = re.search(r'\bHealth:\s*(-?\d+)', body)
    if not (age and motion and pos and health):
        return None
    number = lambda v: float(v.strip().rstrip('dfb'))  # NBT prints doubles with a `d` suffix
    return (int(age.group(1)), [number(v) for v in motion.group(1).split(',')],
            [number(v) for v in pos.group(1).split(',')], int(health.group(1)))


def age_pinned(blast_age, shots):
    """Two facts about the survivor that no longer depend on how many ticks
    either server spent between commands:

    - the blast's push: the first snapshot's motion carried back over the ticks
      since `blast_age` with vanilla's `Entity.getAirDrag`. A tick of slack
      between the blast and the read is worth 2%, so it is compared to two
      decimals; the divergences this looks for are tens of percent.
    - the decay itself, `(|motion| later / |motion| earlier) ** (1 / ticks)`,
      which is `getAirDrag`'s 0.98 per tick whatever ages the reads landed on.
    """
    if blast_age is None or not shots:
        return None
    age1, motion1, _, health1 = shots[0]
    scale = AIR_DRAG ** -(age1 - blast_age)
    line = ('data push ' + ' '.join(f'{v * scale:.2f}' for v in motion1)
            + f' health {health1}')
    if len(shots) > 1 and shots[1][0] != age1:
        age2, motion2, _, _ = shots[1]
        first = sum(v * v for v in motion1) ** 0.5
        second = sum(v * v for v in motion2) ** 0.5
        if first > 0.0:
            line += f' keep {(second / first) ** (1.0 / (age2 - age1)):.4f}'
    return line


def compared(lines):
    cases = {}
    last_age = {}
    shots_of = {}
    for line in lines:
        who, step, text = line.split(' ', 2)
        step = step.removeprefix('step=')
        if step not in CASES:
            continue
        kept = cases.setdefault(step, [])
        plain = re.search(r'has the following entity data:\s*(-?\d+)s?$', text)
        if plain is not None:
            last_age[step] = int(plain.group(1))
            continue
        shot = snapshot(text)
        if shot is not None:
            shots = shots_of.setdefault(step, [])
            shots.append(shot)
            if len(shots) == 2:
                kept.append(age_pinned(last_age.get(step), shots))
        elif 'has the following entity data' in text:
            kept.append(nine(text))
        elif text.startswith(('explode push', 'error', 'disconnected')):
            kept.append(text)
        elif text.removeprefix('chat ').startswith(('Test passed', 'Test failed')):
            # `/execute if entity`: whether the victim survived the blast.
            kept.append(text.removeprefix('chat '))
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
