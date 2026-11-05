#!/usr/bin/env python3
"""Compare entity-bound engine behavior using an original tiny sound registry.

Requires the owned pinned client and its LWJGL/OpenAL libraries, not prepared
game assets. VanillaSoundRig supplies silent buffers; sample decoding is outside
this test. No audio is sent to an output device.
"""
import json
import os
from pathlib import Path
import tempfile

try:
    from . import lab, oracle_harness as h, integration_env
    from . import test_sound_engine_parity as cases
except ImportError:
    import lab
    import oracle_harness as h
    import integration_env
    import test_sound_engine_parity as cases


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    native_paths = {v['downloads']['artifact']['path'] for v in manifest['libraries']
                    if v['name'].split(':')[-1].startswith('natives-')}
    audio_natives = {v['downloads']['artifact']['path'] for v in manifest['libraries']
                     if v['name'].split(':')[1] in {'lwjgl', 'lwjgl-openal'}}
    libraries = []
    for artifact in lab.client_library_artifacts(manifest):
        if artifact['path'] in native_paths and artifact['path'] not in audio_natives:
            continue
        cached = directory / 'libraries/client' / artifact['path']
        if not cached.is_file() or lab.digest(cached, 'sha1') != artifact['sha1']:
            cached = lab.import_library(artifact, directory / 'libraries/client')
        libraries.append(cached)

    build = lab.LOCAL / 'rust-entity-bound-sound'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    sources = [Path(__file__), lab.ROOT / 'tools/test_sound_engine_parity.py',
               lab.ROOT / 'tools/oracle_harness.py', lab.ROOT / 'tools/lab.py',
               lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/integration_env.py',
               lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client-render/src/sound_engine.rs',
               lab.ROOT / 'crates/horizon-client/examples/probe/sound_engine_fixture.rs',
               lab.ROOT / 'crates/horizon-client/examples/probe/audio_common/mod.rs',
               *[lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity' / (name + '.java')
                 for name in ['VanillaSoundEngineOracle', 'VanillaSoundRig', 'VanillaSoundEventOracle']]]
    hashes = h.base.source_hashes(sources)
    resources = build / 'resources'
    samples = resources / 'assets/minecraft/sounds/fixture'
    samples.mkdir(parents=True, exist_ok=True)
    # Only resource existence is read: SoundRig replaces decoding with its
    # original valid silent buffers, independently of these placeholder bytes.
    for name in ['one', 'two']:
        (samples / (name + '.ogg')).write_bytes(b'original resource-presence fixture')
    registry = resources / 'assets/minecraft/sounds.json'
    registry.write_text(json.dumps({'block.stone.break': {
        'subtitle': 'fixture.bound.subtitle', 'sounds': [
            {'name': 'fixture/one', 'weight': 1, 'volume': .75, 'pitch': .8},
            {'name': 'fixture/two', 'weight': 2, 'volume': 1.25, 'pitch': 1.2}]}}), encoding='utf-8')
    registry_hash = lab.digest(registry, 'sha256')
    script = cases.Script([], [])
    cases.entity_bound_cases(script)
    inputs = build / 'inputs.txt'
    inputs.write_text('\n'.join(script.lines) + '\n', encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = h.compile_oracle(build, 'VanillaSoundEngineOracle',
                                               {'client': client}, libraries, lock['minecraft']['java_major'])
    probe = h.build_probe(build, 'sound_engine_fixture')
    if os.name == 'nt':
        probe[0] = Path(probe[0]).with_suffix('.exe')
    with tempfile.TemporaryDirectory(prefix='entity-bound-', dir=build) as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env['ALSOFT_DRIVERS'] = 'null'
        h.run_logged(['java', '-Xmx1G', '-cp', str(classes) + os.pathsep + dependencies,
                      'dev.horizonlab.parity.VanillaSoundEngineOracle', resources, inputs, build / 'client.txt'],
                     build, 'client', env=env)
        h.run_logged([*probe, resources, inputs, build / 'rust.txt'], build, 'rust', env=env)
    matched = h.base.compare(build / 'client.txt', build / 'rust.txt')
    h.base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != input_hash or lab.digest(registry, 'sha256') != registry_hash:
        raise AssertionError('Input fixtures changed during comparison')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        sources_sha256=hashes, client_jar_sha256=lab.digest(client, 'sha256'),
        input_sha256=input_hash, sounds_json_sha256=registry_hash, steps=len(script.lines),
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Actual SoundEngine on OpenAL NoOutput with inline executor and silent buffers: '
              'entity-bound positions, silence/replay, removal and reused ids, weighted seeded sound choices, '
              'category gain, pause/resume and frozen ticks. Original two-sample registry. '
              'No decoded samples, mixer distance/panning, packet delivery or whole-world tick comparison.'))
    print(f'{matched} entity-bound sound output lines match ({len(script.lines)} steps): {report}', flush=True)


if __name__ == '__main__':
    main()
