#!/usr/bin/env python3
"""Compare accepted villager hurt dispatch and cached witnesses without starting a server."""
import argparse
import json
import os
from pathlib import Path
import subprocess
try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, default=lab.GAME)
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    parser.add_argument('--java-only', action='store_true')
    args = parser.parse_args()
    lab.GAME = args.game
    lab.config = lambda: (json.loads(lab.LOCK.read_text(encoding='utf-8')), args.game)
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'villager-reputation-parity'
    classes = build / 'classes'
    classes.mkdir(parents=True, exist_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaVillagerReputationOracle.java'
    sources = [Path(__file__), oracle,
               lab.ROOT / 'crates/horizon-client/src/mobs.rs',
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
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-cp', cp, '-d', str(classes), str(oracle)], check=True)
    expected = build / 'vanilla.txt'
    subprocess.run(['java', '-Xmx512m', '-cp', str(classes) + os.pathsep + cp,
                    'dev.horizonlab.parity.VanillaVillagerReputationOracle', str(expected)], check=True)
    if args.java_only:
        print(expected.read_text(encoding='utf-8'), end='')
        return
    home = (build / 'home').resolve()
    home.mkdir(exist_ok=True)
    env = dict(os.environ, HORIZON_TEST_RESOURCES=str(args.resources.resolve()),
               HORIZON_REPUTATION_ORACLE=str(expected.resolve()), HORIZON_HOME=str(home),
               HORIZON_LAUNCHER_HOME=str(home / 'launcher'), XDG_DATA_HOME=str(home / 'data'),
               XDG_CONFIG_HOME=str(home / 'config'), HORIZON_NO_AUDIO='1',
               HORIZON_LAUNCHER_NO_SELF_UPDATE='1')
    for group in ('mobs::villager_reputation_tests', 'mobs::villager_brain::reputation_witness_tests'):
        subprocess.run(['cargo', 'test', '--locked', '-p', 'horizon-client-lib', '--lib', group,
                        '--', '--include-ignored', '--nocapture'], env=env, check=True)
    subprocess.run(['cargo', 'test', '--locked', '-p', 'horizon-client', '--lib',
                    'game::villagers::reputation_tests', '--', '--include-ignored', '--nocapture'],
                   env=env, check=True)
    subprocess.run(['cargo', 'test', '--locked', '-p', 'horizon-client', '--lib',
                    'game::lan::reputation_baseline_tests', '--', '--include-ignored', '--nocapture'],
                   env=env, check=True)
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(build / 'report.json', {'minecraft': lock['minecraft']['version'],
        'sources_sha256': hashes, 'scope': 'Actual accepted-hurt reputation dispatch and lazy cached witness visibility; isolated entities, no server/world loop.'})
    print('Accepted hurt UUIDs and cached witnesses match vanilla; rejected/fatal-hit regressions pass')


if __name__ == '__main__':
    main()
