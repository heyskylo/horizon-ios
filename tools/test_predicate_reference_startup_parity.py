#!/usr/bin/env python3
"""Startup of a world whose data pack holds a `minecraft:reference` predicate graph.

Cases (each a fresh dedicated server, a fresh flat world and one enabled world pack `prg`):
- `valid`: `prg:target` is a constant true check; `prg:ref` (`all_of`) references it by id. A control: the server must
  reach Ready and serve commands that evaluate the reference.
- `missing`: the same pack, but `prg:ref` references `prg:missing`, which no pack defines. Nothing uses
  `prg:ref`.
- `cycle`: `prg:ref` references `prg:loop`, which references `prg:ref`.

Each run keeps the complete server log, an archive of the exact pack bytes and independent facts:
Ready (the `Done (` line), service (an RCON-free console probe), timeout, how the probe ended the
process, the exit code and the reaping. A case that never reaches Ready is never sent a command.
"""
import argparse
import datetime
import hashlib
import json
import shutil
import subprocess
import sys
import tarfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import lab  # noqa: E402

META = '{"pack":{"description":"predicate reference graph","min_format":[121,0],"max_format":[121,0]}}\n'
# 26.3 dispatches a predicate on `type`; a named reference is a holder: an id where a predicate goes.
TARGET = {'type': 'minecraft:all_of', 'terms': []}
CASES = {
    'valid': {'target': TARGET, 'ref': {'type': 'minecraft:all_of', 'terms': ['prg:target']}},
    'missing': {'target': TARGET, 'ref': {'type': 'minecraft:all_of', 'terms': ['prg:missing']}},
    # Two predicates that reference each other; nothing uses either.
    'cycle': {'target': TARGET, 'ref': {'type': 'minecraft:all_of', 'terms': ['prg:loop']},
              'loop': {'type': 'minecraft:inverted', 'term': 'prg:ref'}},
}
# Commands for a Ready server; the last one says the marker only when the reference evaluates true.
SERVICE = ['datapack list enabled', 'execute if predicate prg:target run say PRG_TARGET_TRUE',
           'execute if predicate prg:ref run say PRG_REF_TRUE', 'say PRG_SERVICE_END']


def sha(b):
    return hashlib.sha256(b).hexdigest()


def encode(value):
    return (json.dumps(value, separators=(',', ':')) + '\n').encode()


def run(kind, case, parent, binary, timeout):
    out = parent / f'{kind}-{case}'
    out.mkdir(parents=True, exist_ok=False)
    game = out / 'game'
    pack = game / 'world/datapacks/prg'
    files = {f'data/prg/predicate/{name}.json': encode(value) for name, value in CASES[case].items()}
    files['pack.mcmeta'] = META.encode()
    for rel, data in files.items():
        (pack / rel).parent.mkdir(parents=True, exist_ok=True)
        (pack / rel).write_bytes(data)
    with tarfile.open(out / 'inputs.tar', 'w') as archive:
        archive.add(pack, arcname='prg')
    port = lab.free_port()
    props = {'server-ip': '127.0.0.1', 'server-port': port, 'online-mode': 'false', 'level-name': 'world',
             'level-seed': 121, 'level-type': 'minecraft:flat',
             'generator-settings': json.dumps({'layers': [{'block': 'minecraft:bedrock', 'height': 1}],
                                               'biome': 'minecraft:plains', 'features': False, 'lakes': False,
                                               'structure_overrides': []}, separators=(',', ':')),
             'generate-structures': 'false', 'initial-enabled-packs': 'vanilla,file/prg',
             'view-distance': 2, 'simulation-distance': 2, 'max-tick-time': -1,
             'pause-when-empty-seconds': -1, 'enable-rcon': 'false'}
    (game / 'server.properties').write_text(''.join(f'{k}={v}\n' for k, v in props.items()))
    (game / 'eula.txt').write_text('eula=true\n')
    java = shutil.which('java')
    command = ([java, '-Xms256M', '-Xmx2G', '-jar', str(ROOT / '.local/minecraft/26.3/jars/server.jar'), '--nogui']
               if kind == 'java' else
               [str(binary), '--resources', str(ROOT / '.local/horizon-client/26.3'), '--dedicated', '--nogui'])
    env = lab.game_env(HORIZON_HOME=out / 'homes/horizon', HORIZON_LAUNCHER_HOME=out / 'homes/launcher',
                       XDG_DATA_HOME=out / 'homes/xdg-data', XDG_CONFIG_HOME=out / 'homes/xdg-config')
    start = time.monotonic()
    facts = dict(start_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), kind=kind, case=case,
                 command=command, timeout_seconds=timeout,
                 inputs={rel: sha(data) for rel, data in files.items()},
                 inputs_tar_sha256=sha((out / 'inputs.tar').read_bytes()),
                 server_properties_sha256=sha((game / 'server.properties').read_bytes()),
                 ready=False, service=False, timed_out=False, probe_termination=[], return_code=None,
                 reaped=False, ready_seconds=None, exit_seconds=None, console_requests=[],
                 console_reply_lines=[])
    if kind == 'java':
        facts['java_version'] = subprocess.run([java, '-version'], capture_output=True, text=True).stderr
    lines = []
    log = (out / 'server.log').open('w')
    p = subprocess.Popen(command, cwd=game, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True, encoding='utf-8', errors='replace',
                         start_new_session=True)

    def pump():
        for line in p.stdout:
            lines.append(line)
            log.write(line)
            log.flush()
    thread = threading.Thread(target=pump)
    thread.start()
    try:
        while time.monotonic() - start < timeout and p.poll() is None:
            if any('Done (' in line and 'For help' in line for line in lines):
                facts['ready'] = True
                facts['ready_seconds'] = time.monotonic() - start
                break
            time.sleep(.1)
        if not facts['ready'] and p.poll() is None:
            facts['timed_out'] = True
        if facts['ready']:
            offset = len(lines)
            facts['console_requests'] = SERVICE
            for text in SERVICE:
                p.stdin.write(text + '\n')
            p.stdin.flush()
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline and p.poll() is None:
                if any(line.rstrip().endswith('[Server] PRG_SERVICE_END') for line in lines[offset:]):
                    facts['service'] = True
                    break
                time.sleep(.1)
            time.sleep(.5)
            facts['console_reply_lines'] = lines[offset:].copy()
    except Exception as e:
        facts['probe_error'] = repr(e)
    finally:
        if p.poll() is None:
            facts['probe_termination'].append(dict(action='console stop', seconds=time.monotonic() - start))
            try:
                p.stdin.write('stop\n')
                p.stdin.flush()
                p.wait(timeout=60)
            except (BrokenPipeError, subprocess.TimeoutExpired):
                facts['probe_termination'].append(dict(action='kill', seconds=time.monotonic() - start))
                p.kill()
                p.wait(timeout=10)
        else:
            p.wait()
        facts['return_code'] = p.returncode
        facts['reaped'] = True
        facts['exit_seconds'] = time.monotonic() - start
        thread.join(timeout=10)
        assert not thread.is_alive()
        log.close()
        facts['log_sha256'] = sha((out / 'server.log').read_bytes())
        for rel, data in files.items():
            assert (pack / rel).read_bytes() == data, rel
        (out / 'outcome.json').write_text(json.dumps(facts, indent=2) + '\n')
    print(kind, case, json.dumps({k: facts[k] for k in ['ready', 'service', 'timed_out', 'return_code', 'reaped',
                                                       'ready_seconds', 'exit_seconds']}), flush=True)
    return facts


# The Java 26.3 diagnostics each invalid case must log (as substrings; their order and log prefixes are
# not part of the contract).
REFUSAL = ("Failed to load datapacks, can't proceed with server load. "
           'You can either fix your datapacks or reset to vanilla with --safeMode')
DIAGNOSTICS = {
    'missing': ['Unbound values in registry ResourceKey[minecraft:root / minecraft:predicate]: [prg:missing]'],
    'cycle': ['Found loot table element validation problem in {prg:loop@minecraft:predicate}.term'
              '->{prg:ref@minecraft:predicate}.terms[0]: prg:loop of type minecraft:predicate is recursively called',
              'Found loot table element validation problem in {prg:ref@minecraft:predicate}.terms[0]'
              '->{prg:loop@minecraft:predicate}.term: prg:ref of type minecraft:predicate is recursively called'],
}


def check(folder, facts):
    """The contract: a valid world serves the reference; an invalid one ends by itself before Ready."""
    log = (folder / 'server.log').read_bytes()
    assert sha(log) == facts['log_sha256'], 'sealed log changed'
    assert facts['reaped'] and not facts['timed_out'] and not facts.get('probe_error'), facts
    replies = ''.join(facts['console_reply_lines'])
    if facts['case'] == 'valid':
        assert facts['ready'] and facts['service'], facts
        for marker in ('[Server] PRG_TARGET_TRUE', '[Server] PRG_REF_TRUE', 'file/prg (world)'):
            assert marker in replies, (marker, replies)
        return
    text = log.decode('utf-8', 'replace')
    assert not facts['ready'] and not facts['service'], 'an invalid world reached Ready'
    assert not facts['probe_termination'] and not facts['console_requests'], 'the probe ended or drove it'
    assert facts['return_code'] == 0, facts['return_code']
    assert 'Done (' not in text
    for line in [*DIAGNOSTICS[facts['case']], REFUSAL]:
        assert line in text, line


def main():
    a = argparse.ArgumentParser()
    a.add_argument('--kind', choices=['java', 'native'], required=True)
    a.add_argument('--output', type=Path, required=True)
    a.add_argument('--binary', type=Path)
    a.add_argument('--timeout', type=float, default=240)
    a.add_argument('--cases', default='valid,missing,cycle')
    a.add_argument('--verify-only', action='store_true', help='check sealed outcomes without running')
    o = a.parse_args()
    failures = []
    for case in o.cases.split(','):
        folder = o.output.resolve() / f'{o.kind}-{case}'
        facts = (json.loads((folder / 'outcome.json').read_text()) if o.verify_only
                 else run(o.kind, case, o.output.resolve(), o.binary, o.timeout))
        try:
            check(folder, facts)
            print(o.kind, case, 'contract: ok')
        except AssertionError as e:
            failures.append(case)
            print(o.kind, case, 'contract: FAILED', str(e)[:300])
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
