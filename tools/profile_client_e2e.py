#!/usr/bin/env python3
"""End-to-end check of client captures: an offscreen game writes a .hzperf into its home's reports/.

It launches a game with a scratch HORIZON_HOME, runs `horizon-ctl profile start`, `mark` and `stop`, then reads the
file back with `horizon-perf`: the seconds' frame counts must add up to the frames the game drew while the capture ran,
and the mark must sit at the time it was typed. F3+O starts and saves a capture too, and HORIZON_PROFILE=DIR writes
one into DIR when the game quits.

    python3 tools/profile_client_e2e.py                    # builds with the fast profile first
    python3 tools/profile_client_e2e.py --bin-dir target/fast

Needs the prepared resources in .local/horizon-client and a build of horizon-client, horizon-ctl and horizon-perf.
"""
import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

try:
    from . import agent_control_e2e as ac
    from . import lab
except ImportError:
    import agent_control_e2e as ac
    import lab

ROOT = Path(__file__).resolve().parents[1]
RUN_S = 3.0
PATTERN = 'profile-*.hzperf'


def run_json(words, cwd, env):
    result = subprocess.run([str(w) for w in words], cwd=cwd, env=env, capture_output=True, text=True, timeout=600)
    lines = result.stdout.strip().splitlines()
    if not lines:
        raise ac.Failed(f'{words}: no output: {result.stderr.strip()}')
    reply = json.loads(lines[-1])
    if result.returncode or (isinstance(reply, dict) and 'error' in reply):
        raise ac.Failed(f'{words}: exit {result.returncode}: {reply}')
    return reply


def whole(bins, file, work, env):
    """Whether horizon-perf reads the file, which it does only once the writer has closed the stream."""
    result = subprocess.run([str(bins / 'horizon-perf'), 'summary', str(file)], cwd=work, env=env, capture_output=True)
    return result.returncode == 0


def f3_o(ctl):
    ctl('key', 'f3', 'down')
    ctl('key', 'o', 'tap')
    ctl('key', 'f3', 'up')


def commands(bins, work, env):
    socket = work / 'c.sock'
    ctl = lambda *words: run_json([bins / 'horizon-ctl', '--socket', socket, *words], work, env)
    perf = lambda *words: run_json([bins / 'horizon-perf', *words], work, env)
    return ctl, perf, socket


def check_ctl(bins, resources):
    """profile start, mark and stop, and the file they leave."""
    work, env = ac.scratch('profile', resources)
    ctl, perf, socket = commands(bins, work, env)
    try:
        ctl('launch', '--world', 'Prof', '--creative', '--offscreen', '--wait')
        assert ctl('profile', 'status')['capturing'] is False
        before = ctl('status')['frames']
        started = ctl('profile', 'start', '--hz', '250')
        assert started['side'] == 'client' and started['path'].startswith('reports/profile-'), started
        assert ctl('profile', 'status')['capturing'] is True
        time.sleep(RUN_S)
        ctl('profile', 'mark', 'checkpoint', 'one')
        time.sleep(RUN_S)
        running = ctl('profile', 'status')
        assert running['elapsed_s'] >= 2 * RUN_S and running['bytes'] > 0, running
        stopped = ctl('profile', 'stop')
        after = ctl('status')['frames']
        assert stopped['path'] == started['path'], (started, stopped)
        assert stopped['open_at'].startswith('https://'), stopped
        file = work / stopped['path']
        assert file.exists(), file
        assert ctl('profile', 'status')['capturing'] is False

        marks = perf('query', 'marks', file)['rows']
        assert [m['name'] for m in marks] == ['checkpoint one'], marks
        assert RUN_S <= marks[0]['t'] <= RUN_S + 1.5, marks
        series = perf('query', 'series', 'h:fps', file, '--every', '1', '--from', '0')['rows'][0]['values']
        counted = sum(v for v in series if v is not None)
        drawn = after - before
        # The first and last second of a capture are partial, so they are not counted.
        busiest = max(v for v in series if v is not None)
        assert counted <= drawn <= counted + 3 * busiest, (counted, drawn, series)
        assert len(series) >= 2 * RUN_S - 1, series
        assert whole(bins, file, work, env)
        print(f'PASS: {counted:.0f} of {drawn} frames in {len(series)} whole seconds, mark at {marks[0]["t"]:.2f} s')

        kept = set((work / 'reports').glob(PATTERN))
        f3_o(ctl)
        assert ctl('profile', 'status')['capturing'] is True
        time.sleep(RUN_S)
        f3_o(ctl)
        deadline = time.time() + 120
        while not (new := set((work / 'reports').glob(PATTERN)) - kept) or not whole(bins, next(iter(new)), work, env):
            assert time.time() < deadline, 'F3+O saved no whole capture'
            time.sleep(0.5)
        assert ctl('profile', 'status')['capturing'] is False
        print(f'PASS: F3+O saved {next(iter(new)).name}')
        return True
    except (ac.Failed, AssertionError) as e:
        print(f'FAIL: {e!r}')
        log = socket.with_suffix('.log')
        if log.exists():
            print('game log tail:\n' + '\n'.join(log.read_text(errors='replace').splitlines()[-15:]))
        return False
    finally:
        try:
            ctl('quit')
        except Exception:
            pass
        time.sleep(2)


def check_env(bins, resources):
    """HORIZON_PROFILE=DIR: a capture of the run is in DIR once the game has quit."""
    work, env = ac.scratch('profile-env', resources)
    target = work / 'captures'
    env['HORIZON_PROFILE'] = str(target)
    ctl, _, _ = commands(bins, work, env)
    try:
        ctl('launch', '--world', 'ProfEnv', '--creative', '--offscreen', '--wait')
        time.sleep(RUN_S)
        assert ctl('profile', 'status')['capturing'] is True
        ctl('quit')
        deadline = time.time() + 120
        files = []
        while not files or not whole(bins, files[0], work, env):
            assert time.time() < deadline, 'HORIZON_PROFILE wrote no whole capture'
            time.sleep(0.5)
            files = sorted(target.glob(PATTERN))
        assert not list((work / 'reports').glob(PATTERN)), 'the capture went to reports/ too'
        print(f'PASS: HORIZON_PROFILE wrote {files[0].name}')
        return True
    except (ac.Failed, AssertionError) as e:
        print(f'FAIL: {e!r}')
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin-dir', help='prebuilt horizon-client, horizon-ctl and horizon-perf (default: build with the fast profile)')
    args = parser.parse_args()
    if args.bin_dir:
        bins = Path(args.bin_dir).resolve()
    else:
        subprocess.run(['cargo', 'build', '--locked', '--profile', 'fast', '-p', 'horizon-client', '-p', 'horizon-client-lib',
                        '-p', 'horizon-perf', '--bins'], cwd=ROOT, check=True)
        meta = json.loads(subprocess.run(['cargo', 'metadata', '--format-version', '1', '--no-deps'], cwd=ROOT,
                                         capture_output=True, text=True, check=True).stdout)
        bins = Path(meta['target_directory']) / 'fast'
    resources = lab.RESOURCES.resolve()
    if not (resources / 'horizon-resources.json').exists():
        sys.exit(f'no prepared resources in {resources}')
    passed = [check_ctl(bins, resources), check_env(bins, resources)]
    sys.exit(0 if all(passed) else 1)


if __name__ == '__main__':
    main()
