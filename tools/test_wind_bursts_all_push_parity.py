#!/usr/bin/env python3
"""Every wind burst of a tick pushes an entity that does not tick. Dedicated servers.

A chunk next to a forced (entity-ticking) one ticks its blocks but not its entities. A wind charge flying in the
ticking chunk bursts (`WindCharge.explode`) against a block by the border, and `ServerExplosion.hurtEntities`
adds its knockback to the motion of a primed TNT just over the border. The TNT never ticks, so its motion only
grows: every burst of every tick adds to it, two bursts in one tick twice.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen; only chunk (10, 0) is forced, so chunk (11, 0) holds the TNTs (x 176 and on) and does not tick them. Each
lane has a stone block one over the floor at x 175, a primed TNT on the floor at x 176.5 beside it, and one wind
charge (control lane) or two (one on each side, along z) flying into the stone, which they reach on the same tick.
An arrow without gravity or motion joins before the TNTs. The servers are stepped; after every step each TNT's exact Pos and Motion are read with `data get`. The transcripts
must match vanilla's (the oracle) line for line, and vanilla's two-charge TNT must move other than the control's.
"""
import re
import sys
import time

try:
    from . import test_locator_bar_parity as locator
    from .test_dedicated_server_parity import Server
except ImportError:
    import test_locator_bar_parity as locator
    from test_dedicated_server_parity import Server

NAME = 'wind-bursts-all-push-parity'
TICKS = 3
# tag, its lane's z, the charges (z offset, motion z)
LANES = [('one_charge', 4, [(-0.6, 0.5)]), ('two_charges', 10, [(-0.6, 0.5), (1.6, -0.5)])]
FORCELOAD = 'forceload add 160 0 175 15'
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)[df]')
BLOCKS = ['time set 6000', *[f'setblock 175 -59 {z} minecraft:stone' for _tag, z, _charges in LANES]]
# An arrow that waits in the air joins first: Horizon ticks its TNT after the projectiles then, and holds the
# bursts' pushes for after that tick.
ENTITIES = ['summon minecraft:arrow 170.5 -55.0 0.5 {NoGravity:1b,Motion:[0.0d,0.0d,0.0d]}',
            *[f'summon minecraft:tnt 176.5 -60.0 {z}.5 {{fuse:200s,Motion:[0.0d,0.0d,0.0d],Tags:["{tag}"]}}'
              for tag, z, _charges in LANES],
            *[f'summon minecraft:wind_charge 175.5 -58.5 {z + dz} {{Motion:[0.0d,0.0d,{motion}d]}}'
              for _tag, z, charges in LANES for dz, motion in charges]]


def run(label, command, world, build):
    directory = build / label
    locator.server_dir(directory, world)
    server = Server(command, directory, label)
    lines = []
    try:
        server.wait_for(r'Done \(', timeout=600)
        server.console('tick freeze')
        server.console(FORCELOAD)
        server.console('say READY')
        server.wait_for(r'READY', timeout=120)
        time.sleep(3)
        for command_line in BLOCKS:
            server.console(command_line)
        time.sleep(1)
        for command_line in ENTITIES:
            server.console(command_line)
        time.sleep(1)
        for tick in range(TICKS + 1):
            if tick:
                server.console('tick step 1')
                time.sleep(1.0)
            start = len(server.lines)
            for tag, *_rest in LANES:
                for path in ('Pos', 'Motion'):
                    server.console(f'data get entity @e[tag={tag},limit=1] {path}')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            answers = [l.split('INFO]: ', 1)[-1] for l in server.lines[start:]
                       if 'entity data:' in l or 'No entity was found' in l or 'Found no' in l]
            for i, (tag, *_rest) in enumerate(LANES):
                pos, motion = answers[2 * i:2 * i + 2] if len(answers) >= 2 * i + 2 else ('', '')
                if 'entity data:' not in pos:
                    lines.append(f'tick {tick} {tag} gone')
                else:
                    lines.append(f'tick {tick} {tag} pos {" ".join(NUMBER.findall(pos))}'
                                 f' motion {" ".join(NUMBER.findall(motion))}')
    finally:
        server.stop()
    return lines



def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    one, two = (next(l for l in vanilla if l.startswith(f'tick 2 {tag} ')).split(' ', 3)[3] for tag, *_ in LANES)
    if 'motion' not in one or one.endswith('motion 0.0 0.0 0.0'):
        return "the control lane's burst did not push its TNT"
    if one == two:
        return 'the second burst did not push the TNT'
    return None

def main():
    lock, build, vanilla_server, horizon_server, _client = locator.sides(NAME)
    world = locator.generate_world(vanilla_server, build)
    vanilla = run('vanilla', vanilla_server, world, build)
    (build / 'vanilla.txt').write_text('\n'.join(vanilla) + '\n', encoding='utf-8')
    why = proves(vanilla)
    if why:
        raise AssertionError(f'vanilla is no proof: {why}:\n' + '\n'.join(vanilla))
    horizon = run('horizon', horizon_server, world, build)
    (build / 'horizon.txt').write_text('\n'.join(horizon) + '\n', encoding='utf-8')
    if vanilla != horizon:
        differ = [f'- {v}\n+ {h}' for v, h in zip(vanilla, horizon) if v != h]
        raise AssertionError(f'{len(differ)} lines differ from vanilla:\n' + '\n'.join(differ[:24]))
    print(f'wind bursts all push: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
