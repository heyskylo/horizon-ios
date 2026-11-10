#!/usr/bin/env python3
"""End-to-end check of client captures: an offscreen game writes a .hzperf into its home's reports/."""
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


def profiler(ctl, action):
    ctl('chat', f'/profiler {action}')


def commands(bins, work, env):
    socket = work / 'c.sock'
    ctl = lambda *words: run_json([bins / 'horizon-ctl', '--socket', socket, *words], work, env)
    perf = lambda *words: run_json([bins / 'horizon-perf', *words], work, env)
    return ctl, perf, socket


def check_ctl(bins, resources, official=False):
    """profile start, mark and stop in single player, and the file they leave: both sides, the window and the server thread.

    `official`: horizon-client is the horizon-official build, which has the captures part (open_at, [Open] and [Share]).
    """
    work, env = ac.scratch('profile', resources)
    ctl, perf, socket = commands(bins, work, env)
    try:
        ctl('launch', '--world', 'Prof', '--creative', '--offscreen', '--wait')
        assert ctl('profile', 'status')['capturing'] is False
        before = ctl('status')['frames']
        started = ctl('profile', 'start', '--hz', '250')
        assert started['sides'] == ['server', 'client'] and started['path'].startswith('reports/profile-'), started
        running = ctl('profile', 'status')
        assert running['capturing'] is True and running['sides'] == ['server', 'client'], running
        time.sleep(RUN_S)
        client_mark = ctl('profile', 'mark', 'checkpoint', 'one')
        server_mark = ctl('profile', 'server', 'mark', 'server', 'only')
        assert RUN_S <= client_mark['t'] <= server_mark['t'] <= client_mark['t'] + 1.5, (client_mark, server_mark)
        time.sleep(RUN_S)
        running = ctl('profile', 'status')
        assert running['elapsed_s'] >= 2 * RUN_S and running['bytes'] > 0, running
        stopped = ctl('profile', 'stop')
        after = ctl('status')['frames']
        assert stopped['path'] == started['path'], (started, stopped)
        assert stopped['sides'] == ['server', 'client'], stopped
        if official:
            assert stopped['open_at'].startswith('https://'), stopped
        else:
            assert 'open_at' not in stopped, stopped
        file = work / stopped['path']
        assert file.exists(), file
        assert ctl('profile', 'status')['capturing'] is False

        sides = {row['side']: row for row in perf('query', 'meta', file)['rows']}
        assert set(sides) == {'server', 'client'} and all('stop' in row for row in sides.values()), sides
        for side, thread in (('client', 'render'), ('server', 'tick')):
            picked = perf('query', 'hot', file, '--side', side)['selection']
            assert picked['thread'] == thread and picked['samples'] > 0, (side, picked)
        marks = [m for m in perf('query', 'marks', file, '--side', 'client')['rows']]
        assert [m['name'] for m in marks] == ['checkpoint one'], marks
        assert RUN_S <= marks[0]['t'] <= RUN_S + 1.5, marks
        server_marks = [m['name'] for m in perf('query', 'marks', file, '--side', 'server')['rows']]
        assert server_marks == ['checkpoint one', 'server only'], server_marks
        series = perf('query', 'series', 'h:fps', file, '--side', 'client', '--every', '1', '--from', '0')['rows'][0]['values']
        counted = sum(v for v in series if v is not None)
        drawn = after - before
        # The first and last second of a capture are partial, so they are not counted.
        busiest = max(v for v in series if v is not None)
        assert counted <= drawn <= counted + 3 * busiest, (counted, drawn, series)
        assert len(series) >= 2 * RUN_S - 1, series
        assert whole(bins, file, work, env)
        print(f'PASS: {counted:.0f} of {drawn} frames in {len(series)} whole seconds, mark at {marks[0]["t"]:.2f} s')

        for side in ('server', 'client'):
            alone = ctl('profile', side, 'start', '--hz', '250')
            assert alone['sides'] == [side], alone
            time.sleep(RUN_S)
            stopped = ctl('profile', side, 'stop')
            assert stopped['sides'] == [side] and (work / stopped['path']).exists(), stopped
            file = work / stopped['path']
            deadline = time.time() + 120
            while not whole(bins, file, work, env):
                assert time.time() < deadline, f'profile {side} saved no whole capture'
                time.sleep(0.5)
            rows = perf('query', 'meta', file)['rows']
            assert [row['side'] for row in rows] == [side], rows
            print(f'PASS: profile {side} alone wrote {file.name}')
            ended = ctl('profile', 'status')
            assert ended['capturing'] is False and ended['path'] is None and ended['elapsed_s'] is None, ended
            try:
                ctl('profile', side, 'mark', 'late')
                raise AssertionError(f'profile {side} mark worked with no capture')
            except ac.Failed:
                pass

        for side in ('client', 'server'):
            ctl('profile', 'start', '--hz', '250')
            time.sleep(1)
            stopped = ctl('profile', side, 'stop')
            assert stopped['sides'] == ['server', 'client'] and (work / stopped['path']).exists(), (side, stopped)
            deadline = time.time() + 120
            while not whole(bins, work / stopped['path'], work, env):
                assert time.time() < deadline, f'profile {side} stop saved no whole capture'
                time.sleep(0.5)
            print(f'PASS: profile {side} stop ended the joined capture')

        kept = set((work / 'reports').glob(PATTERN))
        profiler(ctl, 'start')
        assert ctl('profile', 'status')['capturing'] is True
        time.sleep(RUN_S)
        profiler(ctl, 'stop')
        deadline = time.time() + 120
        while not (new := set((work / 'reports').glob(PATTERN)) - kept) or not whole(bins, next(iter(new)), work, env):
            assert time.time() < deadline, '/profiler saved no whole capture'
            time.sleep(0.5)
        assert ctl('profile', 'status')['capturing'] is False
        print(f'PASS: /profiler saved {next(iter(new)).name}')
        lines = ' '.join(ctl('chat-log', '10')['chat'])
        assert 'Saved' in lines and 'reports' in lines, lines
        assert ('[Open]' in lines and '[Share]' in lines) == official, (official, lines)
        if not official:
            ctl('chat', '/profiler open')
            assert 'not available in this build' in ' '.join(ctl('chat-log', '3')['chat'])
        print(f'PASS: /profiler stop {"offers [Open] and [Share]" if official else "offers neither, and open says it is not available"}')
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


def check_gpu(bins, resources):
    """profile start --gpu: the capture holds the passes' GPU times, or says the device gave none."""
    work, env = ac.scratch('profile-gpu', resources)
    ctl, perf, socket = commands(bins, work, env)
    try:
        ctl('launch', '--world', 'ProfGpu', '--creative', '--offscreen', '--wait')
        started = ctl('profile', 'start', '--gpu', '--no-samples')
        assert started['gpu'] == 'passes', started
        time.sleep(RUN_S)
        stopped = ctl('profile', 'stop')
        file = work / stopped['path']
        options = perf('query', 'meta', file, '--side', 'client')['rows'][0]['options']
        assert options['gpu'] == 'passes', options
        warnings = [w['code'] for w in perf('query', 'warnings', file)['rows']]
        if 'WARNING_CODE_GPU_UNAVAILABLE' in warnings:
            print('PASS: --gpu on a device that gives no pass times writes the warning')
            return True
        rows = perf('query', 'gpu', file)['rows']
        passes = [r for r in rows if r['id'] not in ('g:device', 'g:frame')]
        assert passes and all(r['ms'] > 0 for r in passes if r['ms'] is not None), rows
        summary = next(r for r in rows if r['id'] == 'g:frame')
        assert summary['gpu_frame']['samples'] > 0 and summary['bound'] in ('gpu', 'cpu', 'mixed'), summary
        frames = perf('query', 'gpu_frames', file)['rows']
        assert len(frames) == summary['gpu_frame']['samples'] and all(f['total_ms'] > 0 for f in frames), frames[:3]
        print(f'PASS: --gpu recorded {len(passes)} passes over {len(frames)} frames on {rows[0]["adapter"]} ({summary["bound"]}-bound), the biggest {passes[0]["name"]} at {passes[0]["ms"]} ms')
        return True
    except (ac.Failed, AssertionError) as e:
        print(f'FAIL: {e!r}')
        return False
    finally:
        try:
            ctl('quit')
        except Exception:
            pass
        time.sleep(2)


def check_gpu_env(bins, resources):
    """HORIZON_SHADER_PACK_TIMING starts a capture with the passes timed, written to reports/."""
    work, env = ac.scratch('profile-gpu-env', resources)
    env['HORIZON_SHADER_PACK_TIMING'] = '1'
    ctl, perf, _ = commands(bins, work, env)
    try:
        ctl('launch', '--world', 'ProfGpuEnv', '--creative', '--offscreen', '--wait')
        time.sleep(RUN_S)
        assert ctl('profile', 'status')['capturing'] is True
        stopped = ctl('profile', 'stop')
        options = perf('query', 'meta', work / stopped['path'], '--side', 'client')['rows'][0]['options']
        assert options['gpu'] == 'passes' and options['window_s'] is None, options
        print('PASS: HORIZON_SHADER_PACK_TIMING started a --gpu capture')
        return True
    except (ac.Failed, AssertionError) as e:
        print(f'FAIL: {e!r}')
        return False
    finally:
        try:
            ctl('quit')
        except Exception:
            pass
        time.sleep(2)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin-dir', help='prebuilt horizon-client, horizon-ctl and horizon-perf (default: build with the fast profile)')
    parser.add_argument('--official', action='store_true', help='the horizon-client in --bin-dir is the horizon-official build: expect open_at and the [Open] and [Share] buttons')
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
    passed = [check_ctl(bins, resources, args.official), check_env(bins, resources), check_gpu(bins, resources), check_gpu_env(bins, resources)]
    sys.exit(0 if all(passed) else 1)


if __name__ == '__main__':
    main()
