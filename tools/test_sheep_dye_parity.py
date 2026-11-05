#!/usr/bin/env python3
"""Compare native DyeItem.interactLivingEntity with Horizon's sheep-dye interaction."""
import os
from pathlib import Path
import tempfile

try:
    from . import lab, oracle_harness as h, integration_env
except ImportError:
    import lab
    import oracle_harness as h
    import integration_env


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    natives = {v['downloads']['artifact']['path'] for v in manifest['libraries']
               if v['name'].split(':')[-1].startswith('natives-')}
    libraries = []
    for artifact in lab.client_library_artifacts(manifest):
        if artifact['path'] in natives:
            continue
        cached = directory / 'libraries/client' / artifact['path']
        if not cached.is_file() or lab.digest(cached, 'sha1') != artifact['sha1']:
            cached = lab.import_library(artifact, directory / 'libraries/client')
        libraries.append(cached)
    build = lab.LOCAL / 'rust-sheep-dye'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    paths = [
        'tools/test_sheep_dye_parity.py', 'tools/oracle_harness.py',
        'tools/test_rust_parity.py', 'tools/lab.py', 'tools/integration_env.py',
        'Cargo.toml', 'Cargo.lock', 'crates/horizon-client/src/mobs/sheep_dye.rs',
        'crates/horizon-client/src/mobs.rs',
        'crates/horizon-client/examples/probe/main.rs',
        'crates/horizon-client/examples/probe/sheep_dye_fixture.rs',
        'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaSheepDyeOracle.java',
    ]
    sources = [lab.ROOT / p for p in paths]
    hashes = h.base.source_hashes(sources)
    classes, dependencies, _ = h.compile_oracle(
        build, 'VanillaSheepDyeOracle', {'client': client}, libraries,
        lock['minecraft']['java_major'])
    probe = h.build_probe(build, 'sheep_dye_fixture')
    if os.name == 'nt':
        probe[0] = Path(probe[0]).with_suffix('.exe')
    with tempfile.TemporaryDirectory(prefix='sheep-dye-', dir=build) as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        h.run_logged(['java', '-Xmx1G', '-cp', str(classes) + os.pathsep + dependencies,
                      'dev.horizonlab.parity.VanillaSheepDyeOracle', build / 'client.txt'],
                     build, 'client', env=env)
        h.run_logged([*probe, build / 'rust.txt'], build, 'rust', env=env)
    matched = h.compare(build / 'client.txt', build / 'rust.txt', 1152)
    h.base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(
        schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), matched_output_lines=matched,
        output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Actual DyeItem.interactLivingEntity compared with the production sheep-dye function: '
              '576 cases with two consecutive interactions, both hands, client/server, alive/dead, '
              'sheared/woolly, four sheep colors, three dye items and default/overridden/removed DYE components. '
              'Compares success, resulting color/count and sound call. Native sheep properties/setters and '
              'sound delivery are fixture boundaries. Does not compare metadata encoding, Game wiring, '
              'Player.interactOn creative count restoration, playback, network delivery or world ticks.'))
    print(f'{matched} sheep dye snapshots match: {report}', flush=True)


if __name__ == '__main__':
    main()
