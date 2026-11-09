#!/usr/bin/env python3
"""An allay dancing to the jukebox it heard first, against vanilla's dedicated server.

`Allay.JukeboxListener` hears `jukebox_play` and `jukebox_stop_play`. `setJukeboxPlaying` keeps the position
of the jukebox that started a dance (`jukeboxPos`) while the allay dances, and a stop ends the dance only
from that position (or with none kept); `aiStep` also ends it every 20 ticks when that jukebox is out of
reach or gone. A playing jukebox posts `jukebox_play` again every 20 ticks of its song.

A vanilla dedicated server and Horizon's each serve a fresh flat world; a vanilla-codec client
(VanillaProtocolClient, allay-dance mode: AllayDanceTranscript) joins and records, in arrival order and per
console phase, every entity data value sent for the scene's allays and every level event. With the ticks
frozen the console walls two allays (AI on, fixed UUIDs) into glass cells:

- allay 1 between jukebox A (3 blocks) and jukebox B (4 blocks);
- allay 2, 25 blocks away, beside jukebox C alone (the single-source control).

A real record goes into each jukebox through its container slot (`/item replace block`). A starts, then
B; B stops (not the source) and starts again; then A (the source) stops, and C stops. Between these the
console steps one tick per phase. The transcripts, every console answer and the game time of each phase
are kept; the derived dance timelines and the raw transcripts are compared.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

try:
    from . import lab, test_rust_parity as base
    from .test_dedicated_server_parity import Server, free_port, game_time
    from .test_server_world_gen_parity import escape
except ImportError:
    import lab
    import test_rust_parity as base
    from test_dedicated_server_parity import Server, free_port, game_time
    from test_server_world_gen_parity import escape

PLAYER = 'Tester'
MESSAGE = re.compile(r'\[Server thread/INFO\]: (?:System chat: )?(.*)$')
Y = 221
ALLAYS = {'allay1': ((8, Y, 8), 1), 'allay2': ((40, Y, 8), 2)}
JUKEBOXES = {'a': (5, Y, 8), 'b': (12, Y, 8), 'c': (37, Y, 8)}
DISC = 'minecraft:music_disc_cat'


def uuid(n):
    return f'00000a11-0000-0000-0000-00000000000{n}'


def at(p):
    return ' '.join(map(str, p))


# The scene: (phase, console commands) for the events, ('ticks', n) for n one-tick phases.
TIMELINE = [
    ('a_start', [f'item replace block {at(JUKEBOXES["a"])} container.0 with {DISC}',
                 f'item replace block {at(JUKEBOXES["c"])} container.0 with {DISC}']),
    ('ticks', 5),
    ('b_start', [f'item replace block {at(JUKEBOXES["b"])} container.0 with {DISC}']),
    ('ticks', 25),
    ('b_stop', [f'item replace block {at(JUKEBOXES["b"])} container.0 with minecraft:air']),
    ('ticks', 10),
    ('b_restart', [f'item replace block {at(JUKEBOXES["b"])} container.0 with {DISC}']),
    ('ticks', 10),
    ('a_stop', [f'item replace block {at(JUKEBOXES["a"])} container.0 with minecraft:air',
                f'item replace block {at(JUKEBOXES["c"])} container.0 with minecraft:air']),
    ('ticks', 45),
]


def run_side(label, command, client_command, out):
    directory = out / label
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    port = free_port()
    props = {'server-ip': '127.0.0.1', 'server-port': str(port), 'online-mode': 'false', 'white-list': 'false',
             'level-type': 'minecraft:flat', 'generate-structures': 'false', 'level-seed': '42',
             'gamemode': 'creative', 'force-gamemode': 'true', 'difficulty': 'normal',
             'view-distance': '4', 'simulation-distance': '4', 'spawn-protection': '0',
             'pause-when-empty-seconds': '-1', 'max-tick-time': '-1'}
    (directory / 'server.properties').write_text(
        ''.join(f'{k}={escape(v)}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    transcript = out / f'{label}-packets.txt'
    server = Server(command, directory, label)
    client = None
    serial = 0
    console = []
    clocks = []
    phase = 'setup'
    base = None

    def reply(text):
        nonlocal serial
        serial += 1
        marker = f'ajs_barrier_{serial}'
        start = len(server.lines)
        server.console(text)
        server.console(f'say {marker}')
        server.wait_for(re.escape(f'[Server] {marker}') + '$', timeout=180)
        said = [m.group(1) for line in server.lines[start:] if (m := MESSAGE.search(line))
                and not m.group(1).endswith(f'[Server] {marker}')]
        console.append({'phase': phase, 'command': text, 'said': said})
        return said

    def passed(text):
        return any('Test passed' in line for line in reply(text))

    def announce(name):
        nonlocal phase
        phase = name
        server.console(f'say allay_dance_{name}')
        server.wait_for(rf'<{PLAYER}> allay dance phase {name}$', timeout=120)
        # Each answer adds 20 to the client's chat spam count, which falls by 1 a server tick (200 kicks it).
        time.sleep(1.05)

    def read_state(tag):
        for name, pos in JUKEBOXES.items():
            reply(f'data get block {at(pos)} RecordItem')
            reply(f'data get block {at(pos)} ticks_since_song_started')
            reply(f'execute if block {at(pos)} minecraft:jukebox[has_record=true]')
        for name, (pos, n) in ALLAYS.items():
            reply(f'execute if entity {uuid(n)}')
            reply(f'data get entity {uuid(n)} Pos')

    try:
        server.wait_for(r'Done \(')
        with (out / f'{label}-client.log').open('w') as log:
            client = subprocess.Popen([*map(str, client_command), '127.0.0.1', str(port), PLAYER,
                                       str(transcript), 'allay-dance'], cwd=lab.ROOT,
                                      stdout=log, stderr=subprocess.STDOUT)
            server.wait_for(rf'<{PLAYER}> summon the scene', timeout=180)
            reply('tick freeze')
            for rule in ('spawn_mobs false', 'random_tick_speed 0', 'advance_time false', 'advance_weather false'):
                reply(f'gamerule minecraft:{rule}')
            reply('kill @e[type=!minecraft:player]')
            reply('forceload add 0 0 47 15')
            deadline = time.monotonic() + 180
            while not (passed(f'execute if loaded 8 {Y} 8') and passed(f'execute if loaded 40 {Y} 8')):
                assert time.monotonic() < deadline, f'{label}: scene chunks did not load'
            reply(f'fill 0 {Y - 1} 0 47 {Y - 1} 15 minecraft:stone')
            reply(f'fill 0 {Y} 0 47 {Y + 4} 15 minecraft:air')
            reply(f'tp {PLAYER} 24.5 {Y} 14.5')
            for name, ((x, y, z), n) in ALLAYS.items():
                reply(f'fill {x - 1} {y - 1} {z - 1} {x + 1} {y + 2} {z + 1} minecraft:glass')
                reply(f'fill {x} {y} {z} {x} {y + 1} {z} minecraft:air')
            for name, pos in JUKEBOXES.items():
                reply(f'setblock {at(pos)} minecraft:jukebox')
            reply('kill @e[type=minecraft:item]')
            announce('summon')
            for name, ((x, y, z), n) in ALLAYS.items():
                reply(f'summon minecraft:allay {x + 0.5} {y} {z + 0.5} '
                      f'{{PersistenceRequired:1b,UUID:[I;2577,0,0,{n}]}}')
            for name, ((x, y, z), n) in ALLAYS.items():
                assert passed(f'execute as {uuid(n)} if entity @s[type=minecraft:allay]'), (label, name)
                reply(f'data get entity {uuid(n)} NoAI')
                reply(f'data get entity {uuid(n)} Brain.memories')
                reply(f'data get entity {uuid(n)} HurtTime')
            # A new allay's listeners join within its first ticks.
            announce('settle')
            for _ in range(12):
                reply('tick step 1')
            read_state('before')
            for step in TIMELINE:
                if step[0] == 'ticks':
                    for _ in range(step[1]):
                        start = game_time(server)
                        # Named by the tick after A started: absolute times differ between runs and servers.
                        announce(f'r{start + 1 - base:03d}')
                        reply('tick step 1')
                        deadline = time.monotonic() + 300
                        while game_time(server) < start + 1:
                            assert time.monotonic() < deadline, f'{label}: tick step stalled'
                        now = game_time(server)
                        assert now == start + 1, (label, start, now)
                        clocks.append({'phase': phase, 'game_time': now, 'relative': now - base})
                else:
                    name, commands = step
                    announce(name)
                    now = game_time(server)
                    if name == 'a_start':
                        base = now
                    clocks.append({'phase': name, 'game_time': now, 'relative': now - base})
                    for c in commands:
                        reply(c)
                    read_state(name)
            server.console('say allay_dance_done')
            client.wait(timeout=180)
    finally:
        if client is not None and client.poll() is None:
            client.kill()
            client.wait()
        server.stop()
    lines = transcript.read_text().splitlines()
    assert not any(line.startswith('error ') for line in lines), lines[-10:]
    records = [line[len('play-allay-dance '):] for line in lines if line.startswith('play-allay-dance ')]
    return {'transcript': records, 'console': console, 'clocks': clocks}


# Allay.DATA_DANCING: after Mob's flags (15).
DANCING = 16


def timelines(result):
    """The level events (`diag-level`: a recorded diagnostic, its starts and stops admitted apart) and each allay's `DATA_DANCING` values, in order, with their phase (`dance ...`), and
    its other boolean entity data (`diag-booleans ...`: movement sets `NoGravity`, recorded apart)."""
    out = {'diag-level': []}
    for line in result['transcript']:
        parts = line.split(' | ')
        if parts[1].startswith('level '):
            out['diag-level'].append([parts[0], parts[1]])
            continue
        if len(parts) < 3:
            continue
        values = out.setdefault('dance ' + parts[1], [])
        other = out.setdefault('diag-booleans ' + parts[1], [])
        if parts[2] == 'data':
            continue
        if parts[2].startswith('data '):
            for item in parts[2][5:].split():
                index, value = item.split('=', 1)
                if value.split(':', 1)[0] == '8':
                    (values if int(index) == DANCING else other).append(
                        [parts[0], int(index), value.split(':', 1)[1]])
        else:
            values.append([parts[0], parts[2]])
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon game (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'allay-jukebox-source-parity')
    parser.add_argument('--vanilla-only', action='store_true', help='run only the vanilla server')
    args = parser.parse_args()
    lock, jars, libraries = base.verify_artifacts()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    classes = out / 'classes'
    classes.mkdir(exist_ok=True)
    source = lab.ROOT / 'mods/horizon/src/test/java'
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-sourcepath', source, '-cp', cp, '-d', classes,
                    source / 'dev/horizonlab/parity/VanillaProtocolClient.java'], check=True)
    client = ['java', '-Xmx1g', '-cp', str(classes) + os.pathsep + cp,
              'dev.horizonlab.parity.VanillaProtocolClient']
    commands = {'vanilla': ['java', '-Xmx2g', '-cp', cp, 'net.minecraft.server.Main', 'nogui']}
    if not args.vanilla_only:
        if args.binary:
            binary = args.binary.resolve(strict=True)
        else:
            subprocess.run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client',
                            '--bin', 'horizon-client'], cwd=lab.ROOT, check=True)
            metadata = json.loads(subprocess.check_output(
                ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT, text=True))
            binary = Path(metadata['target_directory']) / 'fast' / (
                'horizon-client.exe' if os.name == 'nt' else 'horizon-client')
        commands['horizon'] = [binary, '--resources', lab.RESOURCES.resolve(), '--dedicated', '--nogui']
    results = {label: run_side(label, command, client, out) for label, command in commands.items()}
    derived = {label: timelines(result) for label, result in results.items()}
    for label, result in results.items():
        lab.write_json(out / f'{label}-records.json', result)
    lab.write_json(out / 'timelines.json', derived)
    if 'horizon' not in derived:
        print(json.dumps(derived['vanilla'], indent=1))
        return
    # Admission: each server sent every start (1010) and stop (1011) vanilla sent after the setup, in its
    # phase; a server whose slot writes or songs did not happen measured nothing.
    wanted = [e for e in derived['vanilla']['diag-level'] if e[0] != 'setup']
    for label in derived:
        missing = [e for e in wanted if e not in derived[label]['diag-level']]
        assert not missing, (label, 'jukebox events not sent', missing)
    keys = sorted(set(derived['vanilla']) | set(derived['horizon']))
    differences = {key: {'vanilla': derived['vanilla'].get(key), 'horizon': derived['horizon'].get(key)}
                   for key in keys if derived['vanilla'].get(key) != derived['horizon'].get(key)}
    # Diagnostics are reported, never failed on.
    for key in [k for k in differences if k.startswith('diag-')]:
        print(f'diagnostic difference: {key}', flush=True)
        differences.pop(key)
    lab.write_json(out / 'report.json', {'schema': 1, 'minecraft': lock['minecraft']['version'],
                                         'differences': differences})
    for key in differences:
        print(f'difference: {key}', flush=True)
    compared = [k for k in keys if not k.startswith('diag-')]
    print(f"{len(compared) - len(differences)}/{len(compared)} timelines match", flush=True)
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
