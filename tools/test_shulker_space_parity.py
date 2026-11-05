#!/usr/bin/env python3
"""Compare native shulker attachment queries with Horizon's production block queries."""
import os
from pathlib import Path
import json
import tempfile
try:
    from . import lab, oracle_harness as harness, test_rust_parity as base, integration_env
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base, integration_env


def cases():
    steps = [(0,-1,0),(0,1,0),(0,0,-1),(0,0,1),(-1,0,0),(1,0,0)]
    for x,z in [(0,0),(15,15),(-1,-1)]:
        target=[x,64,z]
        for name in ['air','soul_sand','stone','scaffolding','oak_slab']:
            for face,(dx,dy,dz) in enumerate(steps):
                yield json.dumps(dict(seed=42,cells=[[x+dx,64+dy,z+dz,'minecraft:'+name]],
                                      current=[8.5,64,8.5],target=target,face=face))
        for y in range(60,69):
            yield json.dumps(dict(seed=42,cells=[[x,63,z,'minecraft:scaffolding'],[x,65,z,'minecraft:oak_slab']],
                                  current=[x+.5,y,z+.5],target=target,face=1))
        for face,(dx,dy,dz) in enumerate(steps):
            yield json.dumps(dict(seed=42,cells=[[x+dx,64+dy,z+dz,'minecraft:stone'],[x,64,z,'minecraft:stone']],
                                  current=[8.5,64,8.5],target=target,face=face))


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    natives = {v['downloads']['artifact']['path'] for v in manifest['libraries']
               if v['name'].split(':')[-1].startswith('natives-')}
    libraries = [lab.import_library(a, directory / 'libraries/client')
                 for a in lab.client_library_artifacts(manifest) if a['path'] not in natives]
    build = lab.LOCAL / 'rust-shulker-space-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    sources = [Path(__file__), lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/oracle_harness.py',
               lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/integration_env.py',
               lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/mobs/shulker.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/shulker_space_tests.rs',
               lab.ROOT / 'crates/horizon-client-render/src/collisions.rs',
               lab.ROOT / 'crates/horizon-world/src/block_collision.rs',
               lab.ROOT / 'crates/horizon-world/src/collision_context.rs',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaShulkerSpaceOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaBlockUpdateOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java']
    hashes = base.source_hashes(sources)
    rows = list(cases())
    inputs = build / 'inputs.txt'
    inputs.write_text('\n'.join(rows) + '\n', encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = harness.compile_oracle(build, 'VanillaShulkerSpaceOracle',
                                                     {'client': client}, libraries, lock['minecraft']['java_major'])
    harness.java_run(build, classes, dependencies, 'VanillaShulkerSpaceOracle',
                     [inputs, build / 'client.txt'], 'client')
    with tempfile.TemporaryDirectory(prefix='horizon-shulker-space-parity-') as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env.update(HORIZON_SHULKER_SPACE_INPUT=str(inputs), HORIZON_SHULKER_SPACE_OUTPUT=str(build / 'rust.txt'))
        harness.run_logged(['cargo', 'test', '--locked', '-p', 'horizon-client-lib', '--lib',
                            'mobs::shulker::space_tests::shulker_space_parity_fixture',
                            '--', '--exact', '--ignored'], build, 'rust', env=env)
    matched = harness.compare(build / 'client.txt', build / 'rust.txt', len(rows))
    base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != input_hash:
        raise AssertionError('Input fixtures changed')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), input_sha256=input_hash,
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Actual native Shulker.canStayAt, loadedAndEntityCanStandOnFace and noCollision on a recording level, '
              'compared with the production Horizon block query used by attachment and teleport. '
              'Five support blocks,all six faces,blocked targets,nine current heights and three chunk-boundary positions. '
              'Rust uses original minimal facts/collision fixtures; normal scale and empty equipment. '
              'No nearby entities,world border,random teleport destination selection,AI ticks,networking or rendered interpolation comparison.'))
    print(f'{matched} shulker attachment cases match the pinned client: {report}', flush=True)


if __name__ == '__main__':
    main()
