#!/usr/bin/env python3
"""World-spawn packet codec contract and actual headless Game command regressions.

The Java oracle only constructs and round-trips respawn packets. The Game tests
exercise spawn context selection and command delivery to an in-memory guest.
No game window, listening socket, server constructor or world bootstrap is used.
This is not a full connected-client join/respawn test.
"""
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

TESTS = 'game::lan::world_spawn_sync_tests::'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resources', type=Path, default=Path(os.environ.get('HORIZON_TEST_RESOURCES', lab.RESOURCES)))
    parser.add_argument('--lab-root', type=Path)
    parser.add_argument('--work-dir', type=Path, default=lab.LOCAL / 'world-spawn-sync-parity')
    args = parser.parse_args()
    build = args.work_dir.resolve()
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    if not (args.resources / 'movement-properties.nbt').is_file():
        parser.error('prepared resources are required')
    if args.lab_root:
        lab.LOCAL = args.lab_root.resolve()
        lab.GAME = lab.LOCAL / 'minecraft' / lab.VERSION
    lock, jars, libraries = base.verify_artifacts()
    source = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaWorldSpawnPacketOracle.java'
    sources = [Path(__file__), source, lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/game/commands.rs',
               lab.ROOT / 'crates/horizon-client/src/game/lan_actions.rs',
               lab.ROOT / 'crates/horizon-client/src/lan/hub.rs',
               lab.ROOT / 'crates/horizon-client/src/game/lan.rs',
               lab.ROOT / 'crates/horizon-client/src/game/native_player_fx.rs',
               lab.ROOT / 'crates/horizon-client/src/game/server_travel.rs']
    hashes = base.source_hashes(sources)
    lab.write_json(build / 'input.json', dict(sources_sha256=hashes))

    def run(command, label, **kwargs):
        path = build / (label + '.log')
        with path.open('w', encoding='utf-8') as log:
            result = subprocess.run([str(c) for c in command], stdout=log, stderr=subprocess.STDOUT,
                                    timeout=900, **kwargs)
        if result.returncode:
            raise RuntimeError(f'{label} failed ({result.returncode}):\n{path.read_text(encoding="utf-8")[-12000:]}')
        return path

    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    with tempfile.TemporaryDirectory(prefix='horizon-world-spawn-') as tmp:
        scratch = Path(tmp)
        homes = {key: scratch / key for key in ('HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME')}
        env = lab.game_env(os.environ, **homes, CARGO_BUILD_JOBS='2', RUST_TEST_THREADS='1', RUST_MIN_STACK='16777216')
        run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', cp,
             '-d', classes, source], 'javac', cwd=scratch, env=env)
        vanilla = run(['java', '-Xmx512M', '-Djava.awt.headless=true', '-Duser.home=' + tmp, '-cp',
                       str(classes) + os.pathsep + cp, 'dev.horizonlab.parity.VanillaWorldSpawnPacketOracle'],
                      'java', cwd=scratch, env=env)
        rows = [r for r in vanilla.read_text(encoding='utf-8').splitlines() if r.startswith('world-spawn\t')]
        assert len(rows) == 7 and len(set(rows)) == 7, 'vanilla oracle did not emit all seven cases'
        compiled = run(['cargo', 'test', '--locked', '-p', 'horizon-client', '--lib', '--no-run',
                        '--message-format=json'], 'compile', cwd=lab.ROOT, env=env)
        binaries = []
        for line in compiled.read_text(encoding='utf-8').splitlines():
            if not line.startswith('{'):
                continue
            row = json.loads(line)
            if (row.get('reason') == 'compiler-artifact' and row.get('executable')
                    and row.get('target', {}).get('name') == 'horizon_client'
                    and row.get('profile', {}).get('test')):
                binaries.append(Path(row['executable']))
        assert len(binaries) == 1, 'expected one actual Game test binary'
        binary_hash = lab.digest(binaries[0], 'sha256')
        env['HORIZON_TEST_RESOURCES'] = str(args.resources.resolve())
        result = run([binaries[0], TESTS, '--ignored', '--nocapture'], 'game', cwd=scratch, env=env)
        text = result.read_text(encoding='utf-8')
        assert 'running 3 tests' in text and '3 passed; 0 failed' in text, 'regressions did not execute'
        rust_rows = [r[r.index('world-spawn\t'):] for r in text.splitlines() if 'world-spawn\t' in r]
        assert sorted(rows) == sorted(rust_rows), (rows, rust_rows)
        assert binary_hash == lab.digest(binaries[0], 'sha256'), 'test artifact changed'
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(sources_sha256=hashes, server_sha256=lab.digest(jars['server'], 'sha256'),
                               rust_binary_sha256=binary_hash, vanilla_packets=7, game_tests=3,
                               scope=__doc__))
    print('7 vanilla packet rows and 3 actual Game regressions passed')


if __name__ == '__main__':
    main()
