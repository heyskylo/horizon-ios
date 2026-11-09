#!/usr/bin/env python3
"""Compare native raider metadata changes with Horizon's tracker, wire codec and client."""
import itertools
import os
from pathlib import Path
import random
import tempfile
try:
    from . import lab, oracle_harness as harness, test_rust_parity as base, integration_env
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base, integration_env


def cases():
    sequences = [bits for size in range(1, 6) for bits in itertools.product((0, 1), repeat=size)]
    rng = random.Random(0x52414944)
    sequences.extend(tuple(rng.randrange(2) for _ in range(64)) for _ in range(16))
    for kind in ('pillager', 'vindicator', 'evoker', 'illusioner', 'witch', 'ravager'):
        for sequence in sequences:
            yield kind + ' ' + ' '.join(map(str, sequence))


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    natives = {v['downloads']['artifact']['path'] for v in manifest['libraries']
               if v['name'].split(':')[-1].startswith('natives-')}
    libraries = [lab.import_library(a, directory / 'libraries/client')
                 for a in lab.client_library_artifacts(manifest) if a['path'] not in natives]
    build = lab.LOCAL / 'rust-raider-celebration-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    sources = [Path(__file__), lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/oracle_harness.py',
               lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/integration_env.py',
               lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/game/lan.rs',
               lab.ROOT / 'crates/horizon-client/src/game/remote_mob_data.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/raider.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs.rs',
               lab.ROOT / 'crates/horizon-client/src/lan/entity.rs',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaRaiderCelebrationOracle.java']
    hashes = base.source_hashes(sources)
    rows = list(cases())
    inputs = build / 'inputs.txt'
    inputs.write_text('\n'.join(rows) + '\n', encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = harness.compile_oracle(build, 'VanillaRaiderCelebrationOracle',
                                                     {'client': client}, libraries, lock['minecraft']['java_major'])
    harness.java_run(build, classes, dependencies, 'VanillaRaiderCelebrationOracle',
                     [inputs, build / 'client.txt'], 'client')
    with tempfile.TemporaryDirectory(prefix='horizon-raider-parity-') as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env.update(HORIZON_RAIDER_SYNC_INPUT=str(inputs), HORIZON_RAIDER_SYNC_OUTPUT=str(build / 'rust.txt'))
        harness.run_logged(['cargo', 'test', '--locked', '-p', 'horizon-client', '--lib',
                            'game::remote_mob_data::tests::raider_celebration_parity_fixture',
                            '--', '--exact', '--ignored'], build, 'rust', env=env)
    matched = harness.compare(build / 'client.txt', build / 'rust.txt', len(rows))
    base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != input_hash:
        raise AssertionError('Input fixtures changed')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), input_sha256=input_hash,
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Native Raider setters, spawn non-default values and dirty metadata for all six types, '
              'compared with production Horizon metadata generation, entity tracking, encoded packets and client apply. '
              'Covers joining while celebrating, repeated values and clearing the flag. '
              'Raid AI, rendered arm geometry and a live network transport are not exercised.'))
    print(f'{matched} raider metadata sequences match the pinned client: {report}', flush=True)


if __name__ == '__main__':
    main()
