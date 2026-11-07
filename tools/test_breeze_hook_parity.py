#!/usr/bin/env python3
"""Compare native Breeze/fishing-hook collision responses without running a world tick."""
import os
from pathlib import Path
import random
import tempfile
try:
    from . import lab, oracle_harness as harness, test_rust_parity as base, integration_env
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base, integration_env


def cases():
    rng = random.Random(0x484f4f4b)
    velocities = [(0.0, 0.0, -0.0), (2.0, -0.4, 1.0), (-2.0, 3.0, -4.0),
                  (1e-100, -1e100, 0.5)]
    velocities.extend(tuple(rng.uniform(-50, 50) for _ in range(3)) for _ in range(128))
    sequences = [(0,), (1,), (0, 0), (0, 1), (0, 0, 1, 1, 0), (1, 0, 1, 0)]
    for i, velocity in enumerate(velocities):
        for sequence in sequences:
            yield ' '.join(map(str, (i - 64, *velocity, *sequence)))


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    natives = {v['downloads']['artifact']['path'] for v in manifest['libraries']
               if v['name'].split(':')[-1].startswith('natives-')}
    libraries = [lab.import_library(a, directory / 'libraries/client')
                 for a in lab.client_library_artifacts(manifest) if a['path'] not in natives]
    build = lab.LOCAL / 'rust-breeze-hook-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    sources = [Path(__file__), lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/oracle_harness.py',
               lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/integration_env.py',
               lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/game/fishing.rs',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaBreezeHookOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java']
    hashes = base.source_hashes(sources)
    rows = list(cases())
    inputs = build / 'inputs.txt'
    inputs.write_text('\n'.join(rows) + '\n', encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = harness.compile_oracle(build, 'VanillaBreezeHookOracle',
                                                     {'client': client}, libraries, lock['minecraft']['java_major'])
    harness.java_run(build, classes, dependencies, 'VanillaBreezeHookOracle',
                     [inputs, build / 'client.txt'], 'client')
    with tempfile.TemporaryDirectory(prefix='horizon-breeze-hook-') as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env.update(HORIZON_BREEZE_HOOK_INPUT=str(inputs), HORIZON_BREEZE_HOOK_OUTPUT=str(build / 'rust.txt'))
        harness.run_logged(['cargo', 'test', '--locked', '-p', 'horizon-client', '--lib',
                            'game::fishing::tests::breeze_hook_parity_fixture', '--', '--exact', '--ignored'],
                           build, 'rust', env=env)
    matched = harness.compare(build / 'client.txt', build / 'rust.txt', len(rows))
    base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != input_hash:
        raise AssertionError('Input fixtures changed')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), input_sha256=input_hash,
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Real Projectile.hitTargetOrDeflectSelf on a FishingHook hitting two default-tagged Breezes. '
              'Compare velocity bits, new/repeated deflection sound decisions, no landing/attachment and immediate sync. '
              'World-tick movement, collision detection, transport, hook yaw/RNG and ordinary-target native behavior '
              'are not compared. Rust uses the production hook collision transition, not a full Game.'))
    print(f'{matched} Breeze hook collision sequences match the pinned client: {report}', flush=True)


if __name__ == '__main__':
    main()
