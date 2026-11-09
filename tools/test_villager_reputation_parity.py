#!/usr/bin/env python3
"""Compare accepted villager hurt dispatch and cached witnesses without starting a server."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import re
try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, default=lab.GAME)
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    parser.add_argument('--java-only', action='store_true')
    args = parser.parse_args(argv)
    lab.GAME = args.game
    lab.config = lambda: (json.loads(lab.LOCK.read_text(encoding='utf-8')), args.game)
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'villager-reputation-parity'
    classes = build / 'classes'
    classes.mkdir(parents=True, exist_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaVillagerReputationOracle.java'
    sources = [Path(__file__), oracle,
               lab.ROOT / 'crates/horizon-client/src/mobs.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/damage.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/villager_brain/reputation.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/villager_brain/reputation_witness_tests.rs',
               lab.ROOT / 'crates/horizon-client/src/game.rs',
               lab.ROOT / 'crates/horizon-client/src/game/melee.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/villager.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/villager_brain.rs',
               lab.ROOT / 'crates/horizon-client/src/game/villagers.rs',
               lab.ROOT / 'crates/horizon-client/src/game/villager_reputation_tests.rs',
               lab.ROOT / 'crates/horizon-client/src/game/villager_reputation_fixture.rs',
               lab.ROOT / 'crates/horizon-client/src/game/villager_reputation_baseline_tests.rs',
               lab.ROOT / 'crates/horizon-client/src/game/lan_actions.rs',
               lab.ROOT / 'crates/horizon-client/src/game/trading.rs',
               lab.ROOT / 'crates/horizon-client/src/game/lan_trading.rs',
               lab.ROOT / 'crates/horizon-client/src/game/commands/world_cmds.rs',
               lab.ROOT / 'crates/horizon-client/src/game/potions.rs',
               lab.ROOT / 'crates/horizon-client/src/game/mace.rs']
    hashes = base.source_hashes(sources)
    clean = dict(os.environ)
    for key in ('HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'):
        clean.pop(key, None)
    env = lab.game_env(clean, HORIZON_TEST_RESOURCES=args.resources.resolve(), PYTHONNOUSERSITE=1)
    scratch = Path(lab._scratch_dir('villager-reputation-cwd-'))
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-cp', cp, '-d', str(classes), str(oracle)], cwd=scratch, env=env, check=True)
    expected = build / 'vanilla.txt'
    subprocess.run(['java', '-Xmx1G', '-cp', str(classes) + os.pathsep + cp,
                    'dev.horizonlab.parity.VanillaVillagerReputationOracle', str(expected)],
                   cwd=scratch, env=env, check=True)
    rows = expected.read_text(encoding='utf-8').splitlines()
    if [line.split()[0] for line in rows] != ['hurt', 'witnesses', 'cached', 'fresh', 'prices', 'cap', 'nested']:
        raise AssertionError('Missing/extra actual Java records')
    if args.java_only:
        base.ensure_sources_unchanged(sources, hashes)
        print('\n'.join(rows))
        return
    env['HORIZON_REPUTATION_ORACLE'] = str(expected.resolve())
    artifacts = {}
    executed = {}
    for package, group, count in [
        ('horizon-client-lib', 'mobs::villager_reputation_tests', 4),
        ('horizon-client-lib', 'mobs::villager_brain::reputation_witness_tests', 1),
        ('horizon-client', 'game::villagers::reputation_tests', 1),
        ('horizon-client', 'game::lan::reputation_baseline_tests', 1),
    ]:
        if package not in artifacts:
            source = (lab.ROOT / 'crates/horizon-client/src' / ('client_lib.rs' if package == 'horizon-client-lib' else 'lib.rs')).resolve()
            built = subprocess.run(['cargo', 'test', '--locked', '-p', package, '--lib', '--no-run', '--message-format=json'],
                                   cwd=lab.ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, encoding='utf-8')
            (build / (package + '-build.jsonl')).write_text(built.stdout, encoding='utf-8')
            found = []
            for line in built.stdout.splitlines():
                try: value = json.loads(line)
                except json.JSONDecodeError: continue
                if value.get('reason') == 'compiler-artifact' and value.get('executable') and value.get('profile', {}).get('test') and value.get('target', {}).get('kind') == ['lib'] and Path(value['target']['src_path']).resolve() == source:
                    found.append(value['executable'])
            if built.returncode or len(set(found)) != 1:
                raise RuntimeError(f'{package} --lib artifact failure; see build log')
            artifacts[package] = found[0]
        artifact = artifacts[package]
        listed = subprocess.run([artifact, group, '--list'], cwd=scratch, env=env, capture_output=True, text=True, check=True).stdout
        if len([line for line in listed.splitlines() if line.startswith(group + '::') and line.endswith(': test')]) != count:
            raise AssertionError(f'{group}: wrong listed test count')
        ran = subprocess.run([artifact, group, '--include-ignored', '--nocapture'], cwd=scratch,
                             env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, encoding='utf-8')
        (build / (group.replace('::', '-') + '.log')).write_text(ran.stdout, encoding='utf-8')
        print(ran.stdout)
        summary = re.search(r'test result: ok\. (\d+) passed; 0 failed;', ran.stdout)
        if ran.returncode or not summary or int(summary[1]) != count:
            raise AssertionError(f'{group}: execution did not pass all {count} tests')
        executed[group] = count
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(build / 'report.json', {'minecraft': lock['minecraft']['version'],
        'sources_sha256': hashes, 'executed_tests': executed, 'java_records': len(rows), 'scope': 'Actual accepted-hurt reputation dispatch and lazy cached witness visibility; isolated entities, no server/world loop.'})
    print('Accepted hurt UUIDs and cached witnesses match vanilla; rejected/fatal-hit regressions pass')


if __name__ == '__main__':
    main()
