"""A dedicated server's capture end to end and what it must not hold (docs/perf-platform.md section 4.3)."""
import argparse
import gzip
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402
from server_bench import DONE, Server  # noqa: E402

EXE = '.exe' if os.name == 'nt' else ''
WORLD, SEED, MOTD, PLAYER, MOB, SIGN, ADDRESS = (
    'SecretWorldName', '8675309', 'SecretMotd', 'SteveSecret', 'Zorblax', 'SecretSignText', '127.0.0.1')
PROPERTIES = {
    'online-mode': 'false',
    'white-list': 'false',
    'level-name': WORLD,
    'level-seed': SEED,
    'motd': MOTD,
    'level-type': 'minecraft\\:flat',
    'generate-structures': 'false',
    'gamemode': 'creative',
    'force-gamemode': 'true',
    'view-distance': '3',
    'simulation-distance': '3',
}


class Failed(Exception):
    pass


def perf(bins, *args):
    done = subprocess.run([bins / f'horizon-perf{EXE}', *map(str, args)], capture_output=True, text=True, timeout=60)
    if done.returncode:
        raise Failed(f'horizon-perf {" ".join(map(str, args))}: {done.stderr or done.stdout}')
    return done.stdout


def rows(bins, op, capture, *flags):
    reply = json.loads(perf(bins, 'query', op, capture, *flags))
    if not reply.get('ok'):
        raise Failed(f'query {op}: {reply.get("error")}')
    return reply['rows']


def capture_file(folder):
    return next(iter(sorted((folder / 'reports').glob('profile-*.hzperf'))), None)


def check(bins, scratch, out):
    folder = scratch / 'server'
    folder.mkdir()
    port = lab.free_port()
    props = dict(PROPERTIES, **{'server-port': str(port)})
    (folder / 'server.properties').write_text(''.join(f'{k}={v}\n' for k, v in props.items()))
    (folder / 'eula.txt').write_text('eula=true\n')
    command = [bins / f'horizon-client{EXE}', '--resources', lab.RESOURCES, '--dedicated', '--nogui']
    server = Server(command, folder, out / 'server.log')
    bots = None
    try:
        server.wait_for(DONE, 300)
        server.console(f'summon minecraft:cow 0 -59 0 {{CustomName:"{MOB}"}}')
        server.console(f'setblock 3 -60 3 minecraft:oak_sign{{front_text:{{messages:["{SIGN}","","",""]}}}}')
        bots = subprocess.Popen(
            [bins / f'horizon-bots{EXE}', '--server', f'127.0.0.1:{port}', '--bots', '2', '--behaviour', 'walk',
             '--seconds', '14', '--name', PLAYER, '--report', out / 'bots.json'],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        server.wait_for(rf'{PLAYER}1 joined the game', 60)
        time.sleep(2)
        server.console('horizon-report 4')
        deadline = time.time() + 60
        path = None
        while time.time() < deadline and path is None:
            time.sleep(0.5)
            path = capture_file(folder)
            if path and not any('Open it at' in line for line in server.since(0)):
                path = None
        if path is None:
            raise Failed('no capture was saved:\n' + '\n'.join(server.since(0)[-15:]))
        lines = server.since(0)
        if not any('Summoned new' in line for line in lines):
            raise Failed('the fixture cow was not summoned')
    finally:
        if bots:
            bots.wait(timeout=120)
        server.stop()
    bytes_ = gzip.decompress(path.read_bytes())
    for secret in (WORLD, SEED, MOTD, MOB, SIGN, ADDRESS):
        if secret.encode() in bytes_:
            raise Failed(f'the capture holds {secret!r}')
    print(f'{path.name}: none of the fixture strings is in the {len(bytes_)} inflated bytes')
    shutil.copy(path, out / path.name)

    spans = {row['name']: row for row in rows(bins, 'spans', path)}
    if not spans or spans.get('tick') is None and not any(name.startswith('tick') for name in spans):
        raise Failed(f'no tick spans: {sorted(spans)}')
    net = rows(bins, 'net', path)
    total = net[0]
    if total['players'] != 2 or total['out_bytes'] <= 0:
        raise Failed(f'the network rows do not show the two players: {net}')
    names = sorted(row.get('name', '') for row in net[1:])
    if names != [f'{PLAYER}0', f'{PLAYER}1']:
        raise Failed(f'the network rows do not name the players: {names}')
    world = rows(bins, 'world', path)
    if sum(row['entities'] for row in world) < 1:
        raise Failed(f'the world rows count no entity: {world}')
    live = rows(bins, 'hot', path, '--select', 'live')
    if not live or sum(row['self'] for row in live) <= 0:
        raise Failed(f'the capture holds no live heap samples: {live}')
    perf(bins, 'summary', path)
    print(f'read back: {len(spans)} spans, {total["players"]} players, {total["out_bytes"]} bytes out, '
          f'{sum(row["chunks"] for row in world)} chunks, {len(live)} functions holding sampled heap')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin-dir', default=str(lab.ROOT / 'target/fast'))
    parser.add_argument('--out', default=str(lab.LOCAL / 'server-perf-e2e'))
    args = parser.parse_args()
    bins, out = Path(args.bin_dir).resolve(), Path(args.out)
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    os.environ.update(lab.game_env())
    with tempfile.TemporaryDirectory(prefix='server-perf-e2e-') as scratch:
        try:
            check(bins, Path(scratch), out)
        except (Failed, RuntimeError) as e:
            sys.exit(f'server_perf_e2e failed (logs in {out}): {e}')


if __name__ == '__main__':
    main()
