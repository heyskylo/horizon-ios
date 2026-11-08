#!/usr/bin/env python3
"""Compare ZombieVillager.handleEntityEvent(16)'s cure sound with vanilla 26.3.

The real method runs on a recording ClientLevel, without an audio device or
prepared game resources. Compare sound names, coordinates, volume/pitch bits,
and the entity random afterwards, including silent mobs.
"""
import json
import os
from pathlib import Path
import platform
import random
import subprocess

try:
    from . import lab, oracle_harness as harness, test_rust_parity as base
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base


def main():
    lock, directory = lab.config()
    manifest, client = lab.import_client(lock, directory)
    # This recording oracle uses Java classes, without loading audio or graphics natives.
    native_paths = {library['downloads']['artifact']['path'] for library in manifest['libraries']
                    if library['name'].split(':')[-1].startswith('natives-')}
    libraries = [lab.import_library(a, directory / 'libraries/client')
                 for a in lab.client_library_artifacts(manifest) if a['path'] not in native_paths]
    build = lab.LOCAL / 'rust-zombie-cure-sound-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaZombieCureSoundOracle.java'
    probe = lab.ROOT / 'crates/horizon-client/examples/probe/zombie_cure_sound_probe.rs'
    sources = [Path(__file__), oracle, probe, lab.ROOT / 'tools/lab.py',
               lab.ROOT / 'tools/oracle_harness.py', lab.ROOT / 'tools/test_rust_parity.py',
               lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client-lib/Cargo.toml',
               lab.ROOT / 'crates/horizon-client/src/mobs/zombie_villager.rs']
    tested = base.source_hashes(sources)
    rng = random.Random(0x43555245)
    seeds = [0, 1, -1, 42, -(2**63), 2**63 - 1] + [rng.randrange(-2**63, 2**63) for _ in range(128)]
    rows = [f'{seed} {int(silent)} {x} {y} {z} {eye}'
            for seed in seeds for silent in (False, True)
            for x, y, z, eye in [(0, 0, 0, 1.5), (1.25, 2, -3.75, 1.25),
                                 (-10.5, -64, 20.5, 0.75)]]
    inputs = build / 'inputs.txt'
    inputs.write_text('\n'.join(rows) + '\n')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = harness.compile_oracle(build, 'VanillaZombieCureSoundOracle',
                                                     {'client': client}, libraries, lock['minecraft']['java_major'])
    command = harness.build_probe(build, 'zombie_cure_sound_probe')
    if os.name == 'nt':
        command[0] = command[0].with_suffix('.exe')
    harness.run_logged([*command, inputs, build / 'rust.txt'], build, 'rust')
    harness.java_run(build, classes, dependencies, 'VanillaZombieCureSoundOracle',
                     [inputs, build / 'client.txt'], 'client')
    matched = harness.compare(build / 'client.txt', build / 'rust.txt', len(rows))
    base.ensure_sources_unchanged(sources, tested)
    if lab.digest(inputs, 'sha256') != input_hash:
        raise AssertionError('Input fixtures changed')
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        platform=platform.platform(), java=subprocess.check_output(['java', '--version'], text=True).splitlines()[0],
        rustc=subprocess.check_output(['rustc', '--version'], text=True).strip(),
        sources_sha256=tested, input_sha256=input_hash, client_jar_sha256=lab.digest(client, 'sha256'),
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Real ZombieVillager.handleEntityEvent(16): silent suppression, sound name, eye-position coordinates, '
              'volume/pitch float bits and entity RNG tail. Server infection/conversion triggers and network delivery '
              'are not exercised here; audio engine resolution is not exercised.'))
    print(f'{matched} zombie cure sound cases match the pinned client: {report}')


if __name__ == '__main__':
    main()
