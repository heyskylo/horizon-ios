"""Summon entities, save the world, reload it, and read fields back with `/data get`, on vanilla's
dedicated server and on Horizon's, and compare what the two print.

Shared by the `test_*_save_reload_parity.py` harnesses (one file of cases each). A case set is

    SETUP  commands run once on the fresh flat world, after its chunks are loaded (a summon is
           `{tag}` in its NBT so the probes find it)
    PROBES (label, command) pairs run twice: right after the summons (`live`), and after
           `save-all flush`, `stop` and a restart of the same world directory (`reloaded`).
           The restart makes the server read every entity back from the region files, so
           `reloaded` is the save and load path of a real world, not a command's in-memory copy.

Each server's probe results are printed, and the run fails when the two servers' texts differ, or
when a probe finds no entity or reports an error. Run files stay under `.local/save-reload/<name>`.

    test_<x>_save_reload_parity.py [--binary PATH] [--keep]

`--binary` is a built `horizon-client` (default: `cargo build --profile fast -p horizon-client`).
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

try:
    from . import lab
    from .test_dedicated_server_parity import Server, free_port
except ImportError:
    import lab
    from test_dedicated_server_parity import Server, free_port

CONSOLE_LINE = re.compile(r'^\[\d\d:\d\d:\d\d\] \[([^\]]+)\]: (.*)$')
# Lines a loaded server logs by itself, between a command and its marker.
SERVER_NOISE = re.compile(r"Horizon: slow tick|Can't keep up")
FAILED = re.compile(r'No entity was found|Unknown|Expected|Incorrect|Invalid|Could not|Unable|Found no elements|'
                    r'Failed|error', re.IGNORECASE)
PROPERTIES = {
    'server-ip': '127.0.0.1', 'online-mode': 'false', 'white-list': 'false', 'level-type': 'minecraft:flat',
    'generate-structures': 'false', 'level-seed': '4242', 'view-distance': '4', 'simulation-distance': '4',
    'spawn-protection': '0', 'spawn-monsters': 'false', 'spawn-npcs': 'false', 'spawn-animals': 'false',
    'pause-when-empty-seconds': '-1', 'sync-chunk-writes': 'true', 'max-tick-time': '-1',
    'enable-rcon': 'false', 'enable-query': 'false', 'management-server-enabled': 'false',
}


def write_server_dir(directory):
    shutil.rmtree(directory, ignore_errors=True)
    directory.mkdir(parents=True)
    props = dict(PROPERTIES, **{'server-port': str(free_port())})
    (directory / 'server.properties').write_text(''.join(f'{k}={v}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')


class Console:
    """A server's console: a command, then a `say` marker, so the lines between are the command's."""

    def __init__(self, server):
        self.server = server
        self.counter = 0

    def run(self, command, timeout=120):
        self.counter += 1
        marker = f'@@{self.counter}@@'
        start = len(self.server.lines)
        self.server.console(command)
        self.server.console(f'say {marker}')
        self.server.wait_for(re.escape(marker) + '$', timeout=timeout)
        out = []
        for line in self.server.lines[start:]:
            m = CONSOLE_LINE.match(line)
            text = m.group(2) if m else line
            if marker in text:
                break
            if not SERVER_NOISE.match(text):
                out.append(text)
        return out

    def wait_loaded(self, command, timeout=300):
        """Run `command` until it stops reporting an unloaded chunk or a missing entity."""
        import time
        end = time.time() + timeout
        while time.time() < end:
            out = self.run(command)
            if out and not any('No entity was found' in l or 'Test failed' in l for l in out):
                return out
            time.sleep(1)
        raise AssertionError(f'never loaded: {command}')


def start(command, directory, name, boot_timeout):
    server = Server(command, directory, name)
    server.wait_for(r'Done \(', timeout=boot_timeout)
    return server


def run_server(name, command, directory, setup, probes, boot_timeout=600):
    """Returns {(phase, label): text} and the setup commands' replies."""
    write_server_dir(directory)
    results, replies = {}, []
    server = start(command, directory, f'{name}-1', boot_timeout)
    try:
        console = Console(server)
        console.run('forceload add 0 0 15 15')
        console.wait_loaded('execute if loaded 8 -60 8')
        for line in setup:
            replies.append((line, console.run(line)))
        for label, probe in probes:
            results[('live', label)] = ' '.join(console.run(probe))
        console.run('save-all flush')
    finally:
        server.stop()
    server = start(command, directory, f'{name}-2', boot_timeout)
    try:
        console = Console(server)
        console.wait_loaded(probes[0][1])
        for label, probe in probes:
            results[('reloaded', label)] = ' '.join(console.run(probe))
    finally:
        server.stop()
    return results, replies


def horizon_binary(args, build):
    if args.binary:
        return Path(args.binary)
    with (build / 'cargo-build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client', '--bin', 'horizon-client'],
                       cwd=lab.ROOT, check=True, stdout=log, stderr=subprocess.STDOUT)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
                                                  cwd=lab.ROOT, text=True))
    return Path(metadata['target_directory']) / 'fast' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')


def main(name, setup, probes, description):
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument('--binary', help='a built horizon-client (default: cargo build --profile fast)')
    parser.add_argument('--keep', action='store_true', help='keep the run directory')
    args = parser.parse_args()
    build = lab.LOCAL / 'save-reload' / name
    shutil.rmtree(build, ignore_errors=True)
    build.mkdir(parents=True)
    binary = horizon_binary(args, build)
    vanilla = ['java', '-Xmx2g', '-jar', str(lab.GAME / 'jars' / 'server.jar'), '--nogui']
    horizon = [binary, '--resources', lab.RESOURCES, '--dedicated', '--nogui']
    runs = {}
    for who, command in (('vanilla', vanilla), ('horizon', horizon)):
        runs[who] = run_server(f'{who}', command, build / who, setup, probes)
        for line, reply in runs[who][1]:
            if any(FAILED.search(l) for l in reply):
                print(f'{who} setup failed: {line}: {reply}')
                return 1
    failures = []
    print(f'{"phase":9} {"probe":28} {"vanilla":34} horizon')
    for key in runs['vanilla'][0]:
        v, h = runs['vanilla'][0][key], runs['horizon'][0].get(key)
        verdict = 'ok' if v == h else 'DIFFERENT'
        print(f'{key[0]:9} {key[1]:28} {v.split("data: ")[-1]:34} {h.split("data: ")[-1] if h else h}  {verdict}')
        if v != h or FAILED.search(v or '') or FAILED.search(h or ''):
            failures.append(key)
            print(f'    vanilla: {v!r}')
            print(f'    horizon: {h!r}')
    if not args.keep:
        shutil.rmtree(build, ignore_errors=True)
    print('FAIL' if failures else 'PASS', f'{len(runs["vanilla"][0])} probes')
    return 1 if failures else 0
