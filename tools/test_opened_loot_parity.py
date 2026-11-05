#!/usr/bin/env python3
"""Compare native chest opening/save/reload with Horizon's chunk persistence."""
import os
from pathlib import Path
import random
import tempfile
try:
    from . import lab, oracle_harness as harness, test_rust_parity as base, integration_env
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base, integration_env


def cases():
    rng = random.Random(0x4C4F4F54)
    seeds = [0, 1, -1, 42, -(1 << 63), (1 << 63) - 1]
    seeds.extend(rng.randrange(-(1 << 63), 1 << 63) for _ in range(58))
    for table in ['ruined_portal', 'simple_dungeon', 'abandoned_mineshaft']:
        for seed in seeds:
            for mode in range(3):
                yield f'minecraft:chests/{table} {seed} {mode}'


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    natives = {v['downloads']['artifact']['path'] for v in manifest['libraries']
               if v['name'].split(':')[-1].startswith('natives-')}
    libraries = [lab.import_library(a, directory / 'libraries/client')
                 for a in lab.client_library_artifacts(manifest) if a['path'] not in natives]
    build = lab.LOCAL / 'rust-opened-loot-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    sources = [Path(__file__), lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/oracle_harness.py',
               lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/integration_env.py',
               lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/game/commands.rs',
               lab.ROOT / 'crates/horizon-client/src/save.rs',
               lab.ROOT / 'crates/horizon-client/src/save/loot_tests.rs',
               lab.ROOT / 'crates/horizon-client-render/src/chunk.rs',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaOpenedLootOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaBlockUpdateOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java']
    hashes = base.source_hashes(sources)
    rows = list(cases())
    inputs = build / 'inputs.txt'
    inputs.write_text('\n'.join(rows) + '\n', encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = harness.compile_oracle(build, 'VanillaOpenedLootOracle',
                                                     {'client': client}, libraries, lock['minecraft']['java_major'])
    harness.java_run(build, classes, dependencies, 'VanillaOpenedLootOracle',
                     [inputs, build / 'client.txt'], 'client')
    with tempfile.TemporaryDirectory(prefix='horizon-opened-loot-parity-') as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env.update(HORIZON_OPENED_LOOT_INPUT=str(inputs), HORIZON_OPENED_LOOT_OUTPUT=str(build / 'rust.txt'))
        harness.run_logged(['cargo', 'test', '--locked', '-p', 'horizon-client-lib', '--lib',
                            'save::loot_tests::opened_loot_parity_fixture',
                            '--', '--exact', '--ignored'], build, 'rust', env=env)
    matched = harness.compare(build / 'client.txt', build / 'rust.txt', len(rows))
    base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != input_hash:
        raise AssertionError('Input fixtures changed')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), input_sha256=input_hash,
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Native chest unpackLootTable, save and reload on a recording level versus Horizon opening bookkeeping, '
              'chunk encoding, binary NBT roundtrip and chunk decoding. Compares loot table/seed retention and opened inventory counts. '
              'Unopened inventory contents are intentionally not compared: Horizon eagerly rolls while vanilla defers. '
              'Three real native chest tables,64 seeds,unopened/emptied/replaced contents. Native dirty notifications suppressed; '
              'Rust begins with an original synthetic eager-roll inventory. No loot-output/RNG comparison,full world/UI run or region-file I/O.'))
    print(f'{matched} opened-loot save/reload cases match the pinned client: {report}', flush=True)


if __name__ == '__main__':
    main()
