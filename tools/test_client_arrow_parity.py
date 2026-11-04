#!/usr/bin/env python3
"""Compare native client arrow ticks with Horizon's production client arrow simulation."""
import os
from pathlib import Path
import json
import tempfile
try:
    from . import lab, oracle_harness as harness, test_rust_parity as base, integration_env
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base, integration_env


def cases():
    water=[[x,y,z,'minecraft:water'] for x in range(-2,6) for y in range(16,21) for z in range(-2,3)]
    entering=[[x,y,z,'minecraft:water'] for x in range(2,6) for y in range(16,21) for z in range(-2,3)]
    for cells in [[],water,entering,[[2,18,0,'minecraft:stone']]]:
        yield json.dumps(dict(cells=cells,position=[.5,18.5,.5],velocity=[1,.2,0],ticks=5))
    for cells in [[],water]:
        for shared in [False,True]:
            yield json.dumps(dict(cells=cells,position=[.5,18.5,.5],velocity=[1,.2,0],ticks=5,fireTicks=80,sharedFire=shared))

    # Exit, every water level, sloping currents and both bubble-column directions.
    exit_water=[[x,y,z,'minecraft:water'] for x in range(-2,2) for y in range(16,21) for z in range(-2,3)]
    yield json.dumps(dict(cells=exit_water,position=[.5,18.5,.5],velocity=[1,.2,0],ticks=8))
    for level in range(16):
        cells=[[x,18,z,'minecraft:water',level] for x in range(-2,8) for z in range(-2,3)]
        yield json.dumps(dict(cells=cells,position=[.5,18.5,.5],velocity=[1,.2,0],ticks=5))
    for drag in [False,True]:
        for top in [18,20]:
            cells=[[x,y,z,'minecraft:bubble_column',drag] for x in range(-2,6) for y in range(16,top+1) for z in range(-2,3)]
            yield json.dumps(dict(cells=cells,position=[.5,18.5,.5],velocity=[1,.2,0],ticks=5))
    cells=[[x,18,z,'minecraft:water',max(0,min(7,x+2))] for x in range(-2,8) for z in range(-2,3)]
    yield json.dumps(dict(cells=cells,position=[.5,18.5,.5],velocity=[1,.2,0],ticks=5))

    for cells in [[],water,[[2,18,0,'minecraft:stone']]]:
        for physics,gravity in [(False,True),(True,False),(False,False)]:
            yield json.dumps(dict(cells=cells,position=[.5,18.5,.5],velocity=[1,.2,0],ticks=5,noPhysics=not physics,noGravity=not gravity))
    for cells in [[],[[0,18,0,'minecraft:stone']]]:
        yield json.dumps(dict(cells=cells,position=[.5,18.5,.5],velocity=[1,.2,0],ticks=5,inGround=True))


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    natives = {v['downloads']['artifact']['path'] for v in manifest['libraries']
               if v['name'].split(':')[-1].startswith('natives-')}
    libraries = [lab.import_library(a, directory / 'libraries/client')
                 for a in lab.client_library_artifacts(manifest) if a['path'] not in natives]
    build = lab.LOCAL / 'rust-client-arrow-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    sources = [Path(__file__), lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/oracle_harness.py',
               lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/integration_env.py',
               lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/arrows.rs',
               lab.ROOT / 'crates/horizon-client/src/arrows_client_tests.rs',
               lab.ROOT / 'crates/horizon-client/src/game/remote_projectiles.rs',
               lab.ROOT / 'crates/horizon-client/src/motion/fluids.rs',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaClientArrowOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaArrowOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaRegistryCollisionScanOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaSceneCollisionOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java']
    hashes = base.source_hashes(sources)
    rows = list(cases())
    inputs = build / 'inputs.txt'
    inputs.write_text('\n'.join(rows) + '\n', encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = harness.compile_oracle(build, 'VanillaClientArrowOracle',
                                                     {'client': client}, libraries, lock['minecraft']['java_major'])
    harness.java_run(build, classes, dependencies, 'VanillaClientArrowOracle',
                     [inputs, build / 'client.txt'], 'client')
    with tempfile.TemporaryDirectory(prefix='horizon-client-arrow-parity-') as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env.update(HORIZON_CLIENT_ARROW_INPUT=str(inputs), HORIZON_CLIENT_ARROW_OUTPUT=str(build / 'rust.txt'))
        harness.run_logged(['cargo', 'test', '--locked', '-p', 'horizon-client-lib', '--lib',
                            'arrows::client_tests::client_arrow_parity_fixture',
                            '--', '--exact', '--ignored'], build, 'rust', env=env)
    matched = harness.compare(build / 'client.txt', build / 'rust.txt', len(rows))
    base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != input_hash:
        raise AssertionError('Input fixtures changed')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), input_sha256=input_hash,
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Actual client Arrow.tick in a recording ClientLevel, compared with shared production client simulation. '
              'Air,source water,water entry/exit,all water levels,sloping currents,bubble columns,stone impact and synced fire controls. '
              'Exact position/motion bits,water state and fire state. Original minimal Rust facts and collision fixtures. '
              'NoPhysics/NoGravity and client ground retention at despawn threshold. No entity targets,rendering or network comparison.'))
    print(f'{matched} client-arrow cases match the pinned client: {report}', flush=True)


if __name__ == '__main__':
    main()
