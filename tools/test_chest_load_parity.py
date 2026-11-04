#!/usr/bin/env python3
"""Compare chest inventory loading with the pinned client, without unpacking loot."""
import json
import os
from pathlib import Path
try:
    from . import lab, oracle_harness as harness, test_rust_parity as base
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base


def cases():
    tables = [None, '"minecraft:chests/simple_dungeon"', '"chests/simple_dungeon"',
              '"example:missing"', '"INVALID TABLE"', '42', '""', '":chests/simple_dungeon"']
    for table in tables:
        for items in [None, '[]', '[{Slot:3b,id:"minecraft:diamond",count:7}]',
                      '[{Slot:0b,id:"minecraft:stone",count:1},{Slot:26b,id:"minecraft:diamond",count:64}]']:
            for seed in [None, '42L', '-9223372036854775808L']:
                fields = ['id:"minecraft:chest"', 'x:1', 'y:2', 'z:3']
                if items is not None:
                    fields.append('Items:' + items)
                if table is not None:
                    fields.append('LootTable:' + table)
                if seed is not None:
                    fields.append('LootTableSeed:' + seed)
                yield '{' + ','.join(fields) + '}'


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    natives = {v['downloads']['artifact']['path'] for v in manifest['libraries']
               if v['name'].split(':')[-1].startswith('natives-')}
    libraries = [lab.import_library(a, directory / 'libraries/client')
                 for a in lab.client_library_artifacts(manifest) if a['path'] not in natives]
    build = lab.LOCAL / 'rust-chest-load-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    sources = [Path(__file__), lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/oracle_harness.py',
               lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/save.rs',
               lab.ROOT / 'crates/horizon-client-core/src/identifier.rs',
               lab.ROOT / 'crates/horizon-client/examples/probe/chest_load_fixture.rs',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaChestLoadOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java']
    hashes = base.source_hashes(sources)
    rows = list(cases())
    inputs = build / 'inputs.txt'
    inputs.write_text('\n'.join(rows) + '\n', encoding='utf-8')
    inputs_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = harness.compile_oracle(build, 'VanillaChestLoadOracle',
                                                     {'client': client}, libraries, lock['minecraft']['java_major'])
    harness.java_run(build, classes, dependencies, 'VanillaChestLoadOracle',
                     [inputs, build / 'client.txt'], 'client')
    command = harness.build_probe(build, 'chest_load_fixture')
    if os.name == 'nt':
        command[0] = command[0].with_suffix('.exe')
    harness.run_logged([*command, inputs, build / 'rust.txt'], build, 'rust')
    matched = harness.compare(build / 'client.txt', build / 'rust.txt', len(rows))
    base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != inputs_hash:
        raise AssertionError('Input fixtures changed')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), input_sha256=inputs_hash,
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Real ChestBlockEntity.loadWithComponents on a previously filled chest. Compare decoded inventory '
              'for present/absent/invalid loot-table keys, item lists and seeds without unpacking loot. '
              'Loot generation, command integration and network synchronization are not exercised.'))
    print(f'{matched} chest load cases match the pinned client: {report}', flush=True)


if __name__ == '__main__':
    main()
