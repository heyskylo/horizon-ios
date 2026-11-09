#!/usr/bin/env python3
"""Actual vanilla Entity counter methods versus in-memory production Game portal handlers.

No world/server bootstrap. Destination chunk resolution and the full entity tick
loop remain outside this method-level fixture; grounded arrows are not covered.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base

TEST = 'game::nether_portal::cooldown_tests::portal_contact_and_waiting_handoff_match_vanilla_counters'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work-dir', type=Path)
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    parser.add_argument('--reference-only', action='store_true')
    args = parser.parse_args()
    build = (args.work_dir or lab.LOCAL / 'rust-portal-cooldown-parity').resolve()
    build.mkdir(parents=True, exist_ok=True)
    lock, jars, libraries = base.verify_artifacts()
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    source = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaPortalCooldownOracle.java'
    reference = build / 'reference.json'
    dependency_path = os.pathsep.join(map(str, [jars['server'], *libraries]))
    def run(command, name, env=None):
        with (build / (name + '.log')).open('w', encoding='utf-8') as log:
            result = subprocess.run(list(map(str, command)), cwd=lab.ROOT, env=env,
                                    stdout=log, stderr=subprocess.STDOUT)
        if result.returncode:
            raise SystemExit(f'{name} failed ({result.returncode}); see {build / (name + ".log")}')
    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none',
         '-cp', dependency_path, '-d', classes, source], 'compile-java')
    run(['java', '-Xmx512M', '-cp', str(classes) + os.pathsep + dependency_path,
         'dev.horizonlab.parity.VanillaPortalCooldownOracle', reference], 'java')
    observed = json.loads(reference.read_text())
    if len(observed['contact']) != 5 or any(len(row) != 4 for row in observed['contact']):
        raise SystemExit('actual vanilla reference shape failed')
    print('Actual vanilla methods: 4 classes, 20 guarded observations', flush=True)
    if args.reference_only:
        return
    inherited = dict(os.environ)
    for key in ('HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'):
        inherited.pop(key, None)
    env = lab.game_env(inherited, HORIZON_TELEMETRY_URL="off", HORIZON_TEST_RESOURCES=args.resources.resolve(strict=True),
                       HORIZON_PORTAL_REFERENCE=reference.resolve(),
                       HORIZON_PORTAL_REPORT=(build / 'native.json').resolve())
    run(['cargo', 'test', '-p', 'horizon-client', '--lib', '--no-run',
         '--message-format=json'], 'build-native', env)
    executables = [row['executable']
                   for line in (build / 'build-native.log').read_text(encoding='utf-8').splitlines()
                   if line.startswith('{')
                   and (row := json.loads(line)).get('reason') == 'compiler-artifact'
                   and row.get('executable') and row.get('profile', {}).get('test')]
    if len(executables) != 1:
        raise SystemExit(f'expected one production test binary, got {executables}')
    with tempfile.TemporaryDirectory(prefix='horizon-portal-cooldown-') as scratch:
        with (build / 'native.log').open('w', encoding='utf-8') as log:
            result = subprocess.run([executables[0], TEST, '--ignored', '--exact', '--nocapture'],
                                    cwd=scratch, env=env, stdout=log, stderr=subprocess.STDOUT)
        if result.returncode:
            raise SystemExit(f'native failed ({result.returncode}); see {build / "native.log"}')
    output = (build / 'native.log').read_text(encoding='utf-8', errors='replace')
    if not re.search(r'test result: ok\. 1 passed; 0 failed;', output):
        raise SystemExit('native test did not execute exactly one passing production regression')
    native = json.loads((build / 'native.json').read_text())
    for field in ('runtime', 'nbt', 'saved'):
        if native[field] != observed['contact']:
            raise SystemExit(f'{field} differs from actual vanilla methods')
    if native['handoff'] != observed['delays']:
        raise SystemExit('arrival handoff differs from actual vanilla delays')
    print('Production contact, live NBT, save and arrival handoff match actual vanilla counters', flush=True)


if __name__ == '__main__':
    main()
