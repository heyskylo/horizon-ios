#!/usr/bin/env python3
"""Compare native comparator/hopper saves and resumed transfers with a headless Game."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--local', type=Path, default=lab.LOCAL)
    parser.add_argument('--resources', type=Path)
    parser.add_argument('--java-only', action='store_true')
    args = parser.parse_args()
    lab.LOCAL = args.local.resolve()
    lock, jars, libraries = base.verify_artifacts()
    build = lab.ROOT / '.local/redstone-persistence-parity'
    build.mkdir(parents=True, exist_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaRedstonePersistenceOracle.java'
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-cp', dependencies, '-d', str(build), str(oracle),
                    str(oracle.with_name('ParityData.java'))], check=True, timeout=120)
    reference = build / 'vanilla.txt'
    subprocess.run(['java', '-Xmx512M', '-cp', str(build) + os.pathsep + dependencies,
                    'dev.horizonlab.parity.VanillaRedstonePersistenceOracle', str(reference)],
                   cwd=build, check=True, timeout=120)
    if args.java_only:
        print(reference.read_text())
        return
    resources = (args.resources or lab.LOCAL / 'horizon-client' / lock['minecraft']['version']).resolve()
    env = lab.game_env(HORIZON_TEST_RESOURCES=resources, HORIZON_PERSISTENCE_ORACLE=reference)
    result = subprocess.run(['cargo', 'test', '-p', 'horizon-client', '--lib', '--no-run',
                             '--message-format=json'], cwd=lab.ROOT, env=env,
                            capture_output=True, text=True, timeout=3600)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    executables = [row['executable'] for line in result.stdout.splitlines()
                   if (row := json.loads(line)).get('reason') == 'compiler-artifact'
                   and row.get('executable') and row.get('profile', {}).get('test')]
    assert len(executables) == 1, executables
    with tempfile.TemporaryDirectory(prefix='horizon-redstone-persistence-') as scratch:
        result = subprocess.run([executables[0], 'game::persist::tests::comparator_and_hopper_live_state_survives_chunk_reload',
                                 '--ignored', '--exact', '--nocapture'], cwd=scratch, env=env,
                                capture_output=True, text=True, timeout=180)
        print(result.stdout + result.stderr)
        result.check_returncode()
        assert 'test result: ok. 1 passed; 0 failed;' in result.stdout, 'regression test did not run'
    print('Comparator output, live hopper cooldown and six resumed transfer ticks match vanilla.')


if __name__ == '__main__':
    main()
