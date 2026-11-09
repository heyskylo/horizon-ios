#!/usr/bin/env python3
"""Compare native air/frozen metadata changes with Horizon's tracker, wire codec and client."""
import os
from pathlib import Path
import random
import tempfile
try:
    from . import lab, oracle_harness as harness, test_rust_parity as base, integration_env
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base, integration_env


def cases():
    rng = random.Random(0x414952)
    for kind, maximum in [('cow', 300), ('cod', 300), ('dolphin', 4800), ('axolotl', 6000)]:
        sequences = [[(maximum, 0)], [(100, -3)],
                     [(maximum, 0), (100, -3), (100, -3), (maximum, 0), (maximum, 0)],
                     [(-2147483648, -2147483648), (2147483647, 2147483647), (maximum, 0)]]
        for _ in range(64):
            sequence = []
            for _ in range(16):
                sequence.append(rng.choice([(maximum, 0), (100, -3), (-20, 140),
                                            (rng.randrange(-100, 7000), rng.randrange(-100, 300))]))
            sequences.append(sequence)
        for sequence in sequences:
            yield kind + ' ' + ' '.join(f'{air},{frozen}' for air, frozen in sequence)


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    natives = {v['downloads']['artifact']['path'] for v in manifest['libraries']
               if v['name'].split(':')[-1].startswith('natives-')}
    libraries = [lab.import_library(a, directory / 'libraries/client')
                 for a in lab.client_library_artifacts(manifest) if a['path'] not in natives]
    build = lab.LOCAL / 'rust-mob-air-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    sources = [Path(__file__), lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/oracle_harness.py',
               lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/integration_env.py',
               lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/game/lan.rs',
               lab.ROOT / 'crates/horizon-client/src/game/remote_mob_data.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs.rs',
               lab.ROOT / 'crates/horizon-client/src/lan/entity.rs',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaMobAirOracle.java']
    hashes = base.source_hashes(sources)
    rows = list(cases())
    inputs = build / 'inputs.txt'
    inputs.write_text('\n'.join(rows) + '\n', encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = harness.compile_oracle(build, 'VanillaMobAirOracle',
                                                     {'client': client}, libraries, lock['minecraft']['java_major'])
    harness.java_run(build, classes, dependencies, 'VanillaMobAirOracle',
                     [inputs, build / 'client.txt'], 'client')
    with tempfile.TemporaryDirectory(prefix='horizon-mob-air-parity-') as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env.update(HORIZON_MOB_AIR_INPUT=str(inputs), HORIZON_MOB_AIR_OUTPUT=str(build / 'rust.txt'))
        harness.run_logged(['cargo', 'test', '--locked', '-p', 'horizon-client', '--lib',
                            'game::remote_mob_data::tests::mob_air_parity_fixture',
                            '--', '--exact', '--ignored'], build, 'rust', env=env)
    matched = harness.compare(build / 'client.txt', build / 'rust.txt', len(rows))
    base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != input_hash:
        raise AssertionError('Input fixtures changed')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), input_sha256=input_hash,
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Native Entity air/frozen setters and spawn/dirty metadata on real cow, cod, dolphin and axolotl instances, '
              'compared with Horizon metadata generation, entity tracking, encoded packets and client apply. '
              'Covers non-default spawn values, repeats and resets, signed extremes and species air defaults. '
              'Full constructors/world ticks, drowning damage, rendered frozen effects and live transport are not exercised.'))
    print(f'{matched} mob air/frozen metadata sequences match the pinned client: {report}', flush=True)


if __name__ == '__main__':
    main()
