#!/usr/bin/env python3
"""Dropped items retain blast damage and spill destroyed contents, against vanilla.

Runs ItemEntity.hurtServer on an isolated vanilla level and Horizon's production
explosion entity walk on an empty headless world. No server or renderer starts.
Blast damage uses radius one and full exposure; positions, motion, world block
callbacks and mob-griefing attribution are outside this fixture's scope.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
try:
    from . import lab, test_rust_parity as base, test_model_baking_parity as baking
except ImportError:
    import lab
    import test_rust_parity as base
    import test_model_baking_parity as baking

FIXTURE = 'game::explosion::tests::explosion_items_parity_fixture'


def cases():
    out = []
    for health in (1, 2, 3, 4, 5, 10):
        for distances in ([1.5], [1.5, 1.5], [2.0] * 5, [0.0], [2.25]):
            out.append(dict(name=f'health-{health}-{len(out)}', stack='{id:"minecraft:stone",count:1}',
                            health=health, distances=distances))
    for item in ('nether_star', 'stone'):
        out.append(dict(name=f'{item}-control', stack=f'{{id:"minecraft:{item}",count:1}}',
                        health=1, distances=[0.0, 0.0]))
    for item, component, contents in (
        ('bundle', 'bundle_contents', '[{id:"minecraft:diamond",count:3}]'),
        ('shulker_box', 'container', '[{slot:0,item:{id:"minecraft:diamond",count:3}}]'),
    ):
        for distances in ([1.5, 1.5], [0.0]):
            out.append(dict(name=f'{item}-{len(out)}',
                            stack=f'{{id:"minecraft:{item}",count:1,components:{{"minecraft:{component}":{contents}}}}}',
                            health=5, distances=distances, sibling=True))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    parser.add_argument('--test-binary', type=Path, help='already built gameplay test executable')
    args = parser.parse_args()
    source_paths = [lab.ROOT / path for path in (
        'crates/horizon-client/src/entities.rs',
        'crates/horizon-client/src/game/explosion.rs',
        'crates/horizon-client/src/game.rs',
        'crates/horizon-client/src/game/native_machines.rs',
        'mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java',
        'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaVehicleContainerOracle.java',
        'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaExplosionItemsOracle.java',
        'tools/test_explosion_items_parity.py')]
    source_hashes = base.source_hashes(source_paths)
    try:
        lock, jars, _ = base.verify_artifacts()
        work = lab.LOCAL / 'explosion-items-parity'
        work.mkdir(parents=True, exist_ok=True)
        (work / 'report.json').unlink(missing_ok=True)
        (work / 'rust.json').unlink(missing_ok=True)
        inputs = cases()
        lab.write_json(work / 'inputs.json', inputs)
        classes = work / 'classes'
        classes.mkdir(exist_ok=True)
        sources = lab.ROOT / 'mods/horizon/src/test/java'
        package = sources / 'dev/horizonlab/parity'
        dependencies = os.pathsep.join(map(str, [jars['client'], *baking.client_libraries()]))

        def run(command, name):
            with (work / f'{name}.log').open('w') as log:
                result = subprocess.run(list(map(str, command)), cwd=lab.ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=900)
            if result.returncode:
                raise RuntimeError(f'{name} failed: {(work / f"{name}.log").read_text()[-4000:]}')

        run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-nowarn',
             '-sourcepath', sources, '-cp', dependencies, '-d', classes,
             *(package / f'{name}.java' for name in ('ParityData', 'VanillaVehicleContainerOracle', 'VanillaExplosionItemsOracle'))], 'compile-java')
        run(['java', '-Xmx512M', '-cp', str(classes) + os.pathsep + dependencies, 'dev.horizonlab.parity.VanillaExplosionItemsOracle',
             work / 'inputs.json', work / 'vanilla.json'], 'vanilla')
        binary = args.test_binary
        if binary is None:
            # Build from the workspace, then execute from scratch so Game cannot
            # read or write the checkout's options, caches or worlds.
            run(['cargo', 'test', '-p', 'horizon-client', '--lib', '--no-run',
                 '--message-format=json'], 'compile-rust')
            for line in (work / 'compile-rust.log').read_text(encoding='utf-8').splitlines():
                try:
                    message = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if (message.get('reason') == 'compiler-artifact' and message.get('executable')
                        and message.get('profile', {}).get('test')
                        and message.get('target', {}).get('name') == 'horizon_client'
                        and message.get('target', {}).get('kind') == ['lib']):
                    binary = Path(message['executable'])
            if binary is None:
                raise RuntimeError('Cargo did not report a gameplay test executable')
        with (work / 'rust.log').open('w') as log, tempfile.TemporaryDirectory(prefix='explosion-items-') as game_dir:
            env = lab.game_env(HORIZON_EXPLOSION_ITEMS_FIXTURE=work,
                               HORIZON_TEST_RESOURCES=args.resources.resolve())
            command = [str(binary.resolve()), FIXTURE, '--exact', '--ignored', '--nocapture']
            subprocess.run(command, cwd=game_dir, env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=900)
        assert 'test result: ok. 1 passed; 0 failed;' in (work / 'rust.log').read_text(encoding='utf-8'), 'gameplay fixture did not run exactly once'
        native = json.loads((work / 'vanilla.json').read_text())
        rust = json.loads((work / 'rust.json').read_text())
        assert len(native) == len(rust) == len(inputs)
        errors = [(c['name'], n, r) for c, n, r in zip(inputs, native, rust) if n != r]
        if errors:
            raise AssertionError(f'{len(errors)}/{len(inputs)} item explosion cases differ: {errors[:3]}')
        lab.write_json(work / 'report.json', dict(minecraft=lock['minecraft']['version'], cases=len(inputs),
                                                sources_sha256=source_hashes,
                                                client_jar_sha256=lab.digest(jars['client'], 'sha256'),
                                                output_sha256=lab.digest(work / 'rust.json', 'sha256'), scope=__doc__))
        print(f'item explosion: {len(inputs)}/{len(inputs)} cases match vanilla')
    finally:
        base.ensure_sources_unchanged(source_paths, source_hashes)


if __name__ == '__main__':
    main()
