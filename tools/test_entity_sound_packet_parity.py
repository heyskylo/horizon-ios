#!/usr/bin/env python3
"""Compare captured sound packets with the owned client's actual server overloads/codecs."""
import json
import os
from pathlib import Path
import struct
import tempfile

try:
    from . import lab, oracle_harness as h, integration_env
except ImportError:
    import lab
    import oracle_harness as h
    import integration_env


def fixtures():
    f = lambda v: struct.pack('>f', v).hex()
    d = lambda v: struct.pack('>d', v).hex()
    for bound, entity in [(False, 0), (True, 1), (True, 37), (True, 2147483647)]:
        for source in range(11):
            for volume in [f(0), f(.7), f(2.5), '7f7fffff', '7fc00000']:
                for seed in [42, -1, 9223372036854775807]:
                    for sound_id in [None, 0, 37]:
                        for position in [(0.123456789, 64.234567891, -30000000.125), (-.125, -64.75, .999999999)]:
                            yield dict(bound=bound, entity=entity, source=source, volume=volume,
                                       pitch=f(1.2), seed=seed, sound_id=sound_id,
                                       position=[d(v) for v in position])


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
    build = lab.LOCAL / 'rust-entity-sound-packet'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    paths = ['tools/test_entity_sound_packet_parity.py', 'tools/oracle_harness.py', 'tools/lab.py',
             'tools/test_rust_parity.py', 'tools/integration_env.py', 'Cargo.toml', 'Cargo.lock',
             'crates/horizon-client-render/src/audio.rs', 'crates/horizon-client/src/lan/containers.rs',
             'crates/horizon-protocol/src/packets/play.rs', 'crates/horizon-protocol/src/packets/mod.rs',
             'crates/horizon-client/examples/probe/main.rs',
             'crates/horizon-client/examples/probe/entity_sound_packet_fixture.rs',
             'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaEntitySoundPacketOracle.java']
    sources = [lab.ROOT / p for p in paths]
    hashes = h.base.source_hashes(sources)
    inputs = build / 'inputs.jsonl'
    cases = list(fixtures())
    inputs.write_text(''.join(json.dumps(c) + '\n' for c in cases), encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = h.compile_oracle(build, 'VanillaEntitySoundPacketOracle',
                                               {'client': client}, libraries, lock['minecraft']['java_major'])
    probe = h.build_probe(build, 'entity_sound_packet_fixture')
    if os.name == 'nt':
        probe[0] = Path(probe[0]).with_suffix('.exe')
    with tempfile.TemporaryDirectory(prefix='entity-packet-', dir=build) as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        h.run_logged(['java', '-Xmx1G', '-cp', str(classes) + os.pathsep + dependencies,
                      'dev.horizonlab.parity.VanillaEntitySoundPacketOracle', inputs, build / 'client.txt'],
                     build, 'client', env=env)
        h.run_logged([*probe, inputs, build / 'rust.txt'], build, 'rust', env=env)
    matched = h.compare(build / 'client.txt', build / 'rust.txt', len(cases))
    h.base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != input_hash:
        raise AssertionError('Input changed during comparison')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'], sources_sha256=hashes,
        client_jar_sha256=lab.digest(client, 'sha256'), input_sha256=input_hash,
        matched_output_lines=matched, cases=len(cases), output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Actual ServerLevel positional/entity sound overloads and native StreamCodecs compared to '
              'the production captured-sound packet builder and Rust codec, including registry/inline '
              'holders, all categories, volume/range overflow and NaN, seeds and entity ids. '
              'Broadcast is a recording sink: no recipient delivery, random-source ordering, '
              'mob call-site execution or full-world comparison.'))
    print(f'{matched} sound packet payloads and ranges match: {report}', flush=True)


if __name__ == '__main__':
    main()
