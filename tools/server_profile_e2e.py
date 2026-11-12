"""A client profiles the server it joined over horizon:perf (docs/perf-platform.md section 4.2): a dedicated Horizon server and a vanilla one."""
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
import agent_control_e2e as ac  # noqa: E402
import lab  # noqa: E402
from server_bench import DONE, Server  # noqa: E402

EXE = '.exe' if os.name == 'nt' else ''
PLAYER = 'Profiler'
PROPERTIES = {
    'online-mode': 'false',
    'white-list': 'false',
    'level-type': 'minecraft\\:flat',
    'generate-structures': 'false',
    'view-distance': '3',
    'simulation-distance': '3',
}


class Failed(Exception):
    pass


def ctl_for(bins, work, env):
    socket = work / 'c.sock'

    def ctl(*words, expect_error=False):
        result = subprocess.run([str(bins / f'horizon-ctl{EXE}'), '--socket', str(socket), *map(str, words)],
                                cwd=work, env=env, capture_output=True, text=True, timeout=600)
        lines = result.stdout.strip().splitlines()
        reply = json.loads(lines[-1]) if lines else {'error': result.stderr.strip()}
        failed = result.returncode != 0 or (isinstance(reply, dict) and 'error' in reply)
        if expect_error is not None and failed != expect_error:
            raise Failed(f'{" ".join(map(str, words))}: {reply}')
        return reply

    return ctl


def client_capture(bins, ctl, work, seconds):
    """A bare capture of `seconds` on the joined server: the warning codes and the mark names in the client's file."""
    started = ctl('profile', 'start', '--seconds', seconds)
    if started['sides'] != ['client']:
        raise Failed(f'a bare start on a server: {started}')
    deadline = time.time() + 60
    while ctl('profile', 'status')['capturing']:
        if time.time() > deadline:
            raise Failed('a timed capture never finished')
        time.sleep(0.3)
    time.sleep(1)
    file = work / started['path']
    if not file.exists():
        file = next(iter(sorted(work.rglob(Path(started['path']).name))), file)
    perf = lambda op: json.loads(subprocess.run([str(bins / f'horizon-perf{EXE}'), 'query', op, str(file)], capture_output=True,
                                                text=True, timeout=60).stdout)['rows']
    return [row['code'] for row in perf('warnings') if row['side'] == 'client'], [row['name'] for row in perf('marks')]


def perf_rows(bins, file, op):
    out = subprocess.run([str(bins / f'horizon-perf{EXE}'), 'query', op, str(file)], capture_output=True, text=True, timeout=60)
    return json.loads(out.stdout)['rows']


def start_server(command, folder, props, log):
    folder.mkdir()
    (folder / 'server.properties').write_text(''.join(f'{k}={v}\n' for k, v in props.items()))
    (folder / 'eula.txt').write_text('eula=true\n')
    server = Server(command, folder, log)
    server.wait_for(DONE, 400)
    return server


def join(bins, resources, port):
    work, env = ac.scratch('serverprofile', resources)
    ctl = ctl_for(bins, work, env)
    ctl('launch', '--offscreen', '--no-pause')
    ctl('connect', f'127.0.0.1:{port}', PLAYER)
    ctl('wait-for', 'loaded', '--timeout', 300)
    return work, ctl


def check_horizon(bins, resources, scratch, out):
    """Operator level 4 gets a capture in the server's reports/, a deopped player is refused, the log names the player."""
    port = lab.free_port()
    folder = scratch / 'horizon'
    command = [bins / f'horizon-client{EXE}', '--resources', lab.RESOURCES, '--dedicated', '--nogui']
    server = start_server(command, folder, dict(PROPERTIES, **{'server-port': str(port)}), out / 'horizon-server.log')
    work = ctl = None
    try:
        work, ctl = join(bins, resources, port)
        reports = folder / 'reports'
        server.console(f'deop {PLAYER}')
        time.sleep(1)
        why = ctl('profile', 'server', 'start', '--seconds', 30, expect_error=True)['error']
        if 'operator level 4' not in why:
            raise Failed(f'a player who is not an operator was not refused: {why}')
        if reports.exists() and list(reports.glob('profile-*.hzperf')):
            raise Failed('a refused request left a file')
        if not any(f'{PLAYER} asked for a server profile and was refused' in l for l in server.since(0)):
            raise Failed('the server log does not name the refused player')

        server.console(f'op {PLAYER}')
        time.sleep(1)
        started = ctl('profile', 'server', 'start', '--seconds', 30, '--no-samples')
        if started['sides'] != ['server'] or not started['path'].startswith('reports/profile-'):
            raise Failed(f'start: {started}')
        server.console('horizon-report 5')
        if not any('already being recorded' in l for l in server.since(0)):
            time.sleep(1)
        if not any('already being recorded' in l for l in server.since(0)):
            raise Failed('the console started a report over the player\'s')
        stopped = ctl('profile', 'server', 'stop')
        made = folder / started['path']
        if not made.exists() or not gzip.decompress(made.read_bytes()):
            raise Failed(f'the server has no copy at {made}')
        received = next(iter(work.rglob(Path(stopped['path']).name)), None)
        if received is None or received.read_bytes() != made.read_bytes():
            raise Failed(f'the client did not receive the server file: {stopped}')
        lines = server.since(0)
        if not any(f'{PLAYER} started a server profile (30 s, samples off)' in l for l in lines):
            raise Failed('the server log does not name the player at the start')
        if not any(f"{PLAYER}'s server profile saved to {started['path']}" in l for l in lines):
            raise Failed('the server log does not name the player at the end')

        timed = ctl('profile', 'server', 'start', '--seconds', 2)
        deadline = time.time() + 60
        while ctl('profile', 'status')['capturing']:
            if time.time() > deadline:
                raise Failed('a timed capture never finished')
            time.sleep(0.5)
        if not (folder / timed['path']).exists():
            raise Failed('the timed capture left no file')

        deadline = time.time() + 30
        while 'error' in ctl('profile', 'server', 'start', expect_error=None):
            if time.time() > deadline:
                raise Failed('the timed capture\'s file never finished arriving')
            time.sleep(0.5)
        ctl('disconnect')
        deadline = time.time() + 30
        while not any(f'{PLAYER} left; their server profile stops' in l for l in server.since(0)):
            if time.time() > deadline:
                raise Failed('the server kept the profile of a player who left')
            time.sleep(0.5)
        ctl('connect', f'127.0.0.1:{port}', PLAYER)
        ctl('wait-for', 'loaded', '--timeout', 300)
        again = ctl('profile', 'server', 'start', '--seconds', 2)
        deadline = time.time() + 60
        while ctl('profile', 'status')['capturing']:
            if time.time() > deadline:
                raise Failed('the capture after a reconnect never finished')
            time.sleep(0.5)
        if not (folder / again['path']).exists():
            raise Failed('the capture after a reconnect left no file')

        codes, marks = client_capture(bins, ctl, work, 3)
        if codes or marks != ['server_started']:
            raise Failed(f'a bare capture at level 4 holds warnings {codes} and marks {marks}')
        merged = sorted(work.rglob('profile-*.hzperf'), key=lambda p: p.stat().st_mtime)[-1]
        sides = {row['side'] for row in perf_rows(bins, merged, 'meta')}
        if sides != {'client', 'server'}:
            raise Failed(f'a bare capture at level 4 holds the sides {sides}')
        server.console(f'deop {PLAYER}')
        time.sleep(1)
        codes, marks = client_capture(bins, ctl, work, 2)
        if codes != ['WARNING_CODE_SERVER_DENIED'] or marks:
            raise Failed(f'a bare capture refused by the server holds warnings {codes} and marks {marks}')
        files = sorted(p.name for p in reports.glob('profile-*.hzperf'))
        if len(files) != 5:
            raise Failed(f'the server holds {files}')
        print(f'horizon: the server kept {files}')
    finally:
        if ctl:
            try:
                ctl('quit')
            except Failed:
                pass
        server.stop()
        if work:
            shutil.rmtree(work, ignore_errors=True)


def check_vanilla(bins, resources, scratch, out):
    """A server that is not Horizon: the request is refused before it is sent, and a bare capture holds the client side."""
    jar = lab.LOCAL / 'minecraft' / lab.VERSION / 'jars' / 'server.jar'
    if not jar.exists():
        print('vanilla: skipped, no server jar')
        return
    port = lab.free_port()
    folder = scratch / 'vanilla'
    server = start_server(['java', '-jar', jar, 'nogui'], folder, dict(PROPERTIES, **{'server-port': str(port)}),
                          out / 'vanilla-server.log')
    work = ctl = None
    try:
        work, ctl = join(bins, resources, port)
        why = ctl('profile', 'server', 'start', expect_error=True)['error']
        if "isn't running Horizon" not in why:
            raise Failed(f'a vanilla server: {why}')
        codes, marks = client_capture(bins, ctl, work, 2)
        if codes != ['WARNING_CODE_SERVER_NOT_HORIZON'] or marks:
            raise Failed(f'a bare capture on a vanilla server holds warnings {codes} and marks {marks}')
        print(f'vanilla: refused with {why!r}, the client file warns {codes}')
    finally:
        if ctl:
            try:
                ctl('quit')
            except Failed:
                pass
        server.stop()
        if work:
            shutil.rmtree(work, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin-dir', default=str(lab.ROOT / 'target/fast'))
    parser.add_argument('--out', default=str(lab.LOCAL / 'server-profile-e2e'))
    args = parser.parse_args()
    bins, out = Path(args.bin_dir).resolve(), Path(args.out)
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    os.environ.update(lab.game_env())
    resources = lab.RESOURCES
    with tempfile.TemporaryDirectory(prefix='server-profile-e2e-') as scratch:
        try:
            check_horizon(bins, resources, Path(scratch), out)
            check_vanilla(bins, resources, Path(scratch), out)
        except (Failed, RuntimeError) as e:
            sys.exit(f'server_profile_e2e failed (logs in {out}): {e}')


if __name__ == '__main__':
    main()
