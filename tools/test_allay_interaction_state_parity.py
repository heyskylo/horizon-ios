#!/usr/bin/env python3
"""Compare native client Allay.mobInteract state with Horizon's client predictor."""
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
    build = lab.LOCAL / 'rust-allay-interaction-state'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    paths = [
        'tools/test_allay_interaction_state_parity.py', 'tools/oracle_harness.py',
        'tools/test_rust_parity.py', 'tools/lab.py', 'tools/integration_env.py',
        'Cargo.toml', 'Cargo.lock', 'crates/horizon-client/src/mobs/allay.rs',
        'crates/horizon-client/src/mobs/allay/interaction.rs',
        'crates/horizon-client/src/mobs.rs',
        'crates/horizon-client/examples/probe/main.rs',
        'crates/horizon-client/examples/probe/allay_interaction_fixture.rs',
        'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaAllayInteractionStateOracle.java',
    ]
    sources = [lab.ROOT / p for p in paths]
    hashes = h.base.source_hashes(sources)
    classes, dependencies, _ = h.compile_oracle(
        build, 'VanillaAllayInteractionStateOracle', {'client': client}, libraries,
        lock['minecraft']['java_major'])
    probe = h.build_probe(build, 'allay_interaction_fixture')
    if os.name == 'nt':
        probe[0] = Path(probe[0]).with_suffix('.exe')
    with tempfile.TemporaryDirectory(prefix='allay-state-', dir=build) as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        h.run_logged(['java', '-Xmx1G', '-cp', str(classes) + os.pathsep + dependencies,
                      'dev.horizonlab.parity.VanillaAllayInteractionStateOracle', build / 'client.txt'],
                     build, 'client', env=env)
        h.run_logged([*probe, build / 'rust.txt'], build, 'rust', env=env)
    matched = h.compare(build / 'client.txt', build / 'rust.txt', 864)
    h.base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(
        schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), matched_output_lines=matched,
        output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Actual client Allay.mobInteract compared to the production client state predictor: '
              '288 cases with three consecutive interactions, both hands, survival/creative, '
              'held/offered items, dancing, duplication readiness and three duplication-tag sets. '
              'Compares held/offer counts, liked-player presence, duplication flag, returned stack '
              'and sound call. Entity construction and player inventory insertion are fixture sinks; '
              'item defaults are empty, carried inventory is empty. Does not compare inventory slot '
              'placement, sound playback, network delivery, world ticks or the Game wrapper.'))
    print(f'{matched} Allay interaction snapshots match: {report}', flush=True)


if __name__ == '__main__':
    main()
