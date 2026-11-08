#!/usr/bin/env python3
"""Vanilla gateway instance lifecycle and an actual headless replacement regression.

No server constructor, world bootstrap, networking, window or device is used.
This checks replacement/cooldown behavior, not the full command-corpus transcript.
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

TEST = 'game::block_entity_anims::tests::replacing_a_gateway_discards_its_old_cooldown'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resources', type=Path, default=Path(os.environ.get('HORIZON_TEST_RESOURCES', lab.RESOURCES)))
    parser.add_argument('--lab-root', type=Path)
    parser.add_argument('--work-dir', type=Path, default=lab.LOCAL / 'gateway-replacement-parity')
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
    source = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaGatewayReplacementOracle.java'
    sources = [Path(__file__), source, source.parent / 'ParityData.java', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/game.rs',
               lab.ROOT / 'crates/horizon-client/src/game/block_entity_anims.rs',
               lab.ROOT / 'crates/horizon-client/src/game/spill.rs']
    hashes = base.source_hashes(sources)

    def run(command, label, **kwargs):
        path = build / (label + '.log')
        with path.open('w', encoding='utf-8') as log:
            result = subprocess.run([str(c) for c in command], stdout=log, stderr=subprocess.STDOUT,
                                    timeout=600, **kwargs)
        if result.returncode:
            raise RuntimeError(f'{label} failed ({result.returncode}):\n{path.read_text(encoding="utf-8")[-12000:]}')
        return path

    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    with tempfile.TemporaryDirectory(prefix='horizon-gateway-') as tmp:
        scratch = Path(tmp)
        homes = {key: scratch / key for key in ('HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME')}
        env = lab.game_env(os.environ, **homes, CARGO_BUILD_JOBS='2', RUST_TEST_THREADS='2', RUST_MIN_STACK='16777216')
        run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', cp,
             '-d', classes, source, source.parent / 'ParityData.java'], 'javac', cwd=scratch, env=env)
        vanilla = build / 'vanilla.txt'
        vanilla.unlink(missing_ok=True)
        run(['java', '-Xmx512M', '-Djava.awt.headless=true', '-Duser.home=' + tmp, '-cp',
             str(classes) + os.pathsep + cp, 'dev.horizonlab.parity.VanillaGatewayReplacementOracle',
             vanilla], 'java', cwd=scratch, env=env)
        assert vanilla.read_text().splitlines() == [
            'fresh false', 'entered true', 'removed true', 'replacement false', 'untouched true',
        ], 'vanilla lifecycle contract changed'
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
        result = run([binaries[0], TEST, '--exact', '--ignored', '--nocapture'], 'game', cwd=scratch, env=env)
        text = result.read_text(encoding='utf-8')
        assert 'running 1 test' in text and '1 passed; 0 failed' in text, 'regression did not execute'
        assert binary_hash == lab.digest(binaries[0], 'sha256'), 'test artifact changed'
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(sources_sha256=hashes, server_sha256=lab.digest(jars['server'], 'sha256'),
                               rust_binary_sha256=binary_hash, vanilla_queries=5, game_tests=1,
                               scope=__doc__))
    print('5 vanilla lifecycle queries and 1 actual Game regression passed')


if __name__ == '__main__':
    main()
