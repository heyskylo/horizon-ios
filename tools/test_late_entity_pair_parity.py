#!/usr/bin/env python3
"""Compare actual ServerEntity late pairing and first updates with Horizon's tracker."""
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
    build = lab.LOCAL / 'rust-late-entity-pair'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    paths = [
        'tools/test_late_entity_pair_parity.py', 'tools/oracle_harness.py',
        'tools/test_rust_parity.py', 'tools/lab.py', 'tools/integration_env.py',
        'Cargo.toml', 'Cargo.lock', 'crates/horizon-client/src/lan/entity.rs',
        'crates/horizon-client/src/lan/tracking.rs',
        'crates/horizon-protocol/src/packets/play.rs',
        'crates/horizon-protocol/src/packets/mod.rs',
        'crates/horizon-protocol/src/buf.rs',
        'crates/horizon-client/examples/probe/main.rs',
        'crates/horizon-client/examples/probe/late_entity_pair_fixture.rs',
        'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaLateEntityPairOracle.java',
    ]
    sources = [lab.ROOT / p for p in paths]
    hashes = h.base.source_hashes(sources)
    classes, dependencies, _ = h.compile_oracle(
        build, 'VanillaLateEntityPairOracle', {'client': client}, libraries,
        lock['minecraft']['java_major'])
    probe = h.build_probe(build, 'late_entity_pair_fixture')
    if os.name == 'nt':
        probe[0] = Path(probe[0]).with_suffix('.exe')
    with tempfile.TemporaryDirectory(prefix='late-pair-', dir=build) as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        h.run_logged(['java', '-Xmx1G', '-cp', str(classes) + os.pathsep + dependencies,
                      'dev.horizonlab.parity.VanillaLateEntityPairOracle', build / 'client.txt'],
                     build, 'client', env=env)
        native_type = (build / 'client.txt').read_text(encoding='utf-8').splitlines()[0].removeprefix('type ')
        int(native_type)
        h.run_logged([*probe, native_type, build / 'rust.txt'], build, 'rust', env=env)
    matched = h.compare(build / 'client.txt', build / 'rust.txt', 37)
    h.base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(
        schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), matched_output_lines=matched,
        output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Real ThrownTrident construction and actual ServerEntity.sendPairingData/sendChanges '
              'compared with Entities.pair_after_tick/tick: 12 cases, pairing plus two updates each. '
              'Checks ordered packet types and exact native StreamCodec payloads for fractional positions, '
              'three initial velocities, landing metadata and the unchanged next update. Native pairing '
              'also asserts tickCount remains zero. World ID allocation and packet delivery are sinks; '
              'uses the native entity-type registry ID to align fixtures. Does not compare ChunkMap range '
              'decisions, physics, Game wiring, callbacks, sound playback or network transport.'))
    print(f'{matched} late-pair output lines match: {report}', flush=True)


if __name__ == '__main__':
    main()
