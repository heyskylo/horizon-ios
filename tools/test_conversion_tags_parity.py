#!/usr/bin/env python3
"""Compare command tags inherited by conversion, without a game window or server."""
import json
import os
from pathlib import Path
import subprocess

try:
    from . import lab, oracle_harness as h, test_rust_parity as base
except ImportError:
    import lab
    import oracle_harness as h
    import test_rust_parity as base


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'conversion-tags-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    paths = ['crates/horizon-client/src/entity_nbt.rs',
             'crates/horizon-client/src/game/creatures.rs',
             'crates/horizon-client/examples/probe/conversion_tags_fixture.rs',
             'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaConversionTagsOracle.java',
             'tools/test_conversion_tags_parity.py']
    sources = [lab.ROOT / p for p in paths]
    hashes = base.source_hashes(sources)
    cases = [[], ['subject'], ['quest', 'subject', 'phase_2'],
             ['', 'space in tag', 'punctuation:!'], [chr(0x96ea), chr(0x1f600)],
             [f'tag_{i}' for i in range(1024)]]
    inputs = build / 'inputs.jsonl'
    inputs.write_text(''.join(json.dumps(c) + '\n' for c in cases), encoding='utf-8')
    classes, dependencies, _ = h.compile_oracle(
        build, 'VanillaConversionTagsOracle', {'client': jars['server']},
        libraries, lock['minecraft']['java_major'])
    h.java_run(build, classes, dependencies, 'VanillaConversionTagsOracle',
               [inputs, build / 'vanilla.txt'], 'vanilla')
    h.run_logged(['cargo', 'build', '--locked', '-p', 'horizon-client-lib',
                  '--example', 'client_probe'], build, 'build-rust',
                 env=dict(os.environ, CARGO_BUILD_JOBS='2'))
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT))
    binary = Path(metadata['target_directory']) / 'debug/examples' / (
        'client_probe.exe' if os.name == 'nt' else 'client_probe')
    h.run_logged([binary, 'conversion_tags_fixture', inputs, build / 'rust.txt'], build, 'rust')
    matched = h.compare(build / 'vanilla.txt', build / 'rust.txt', len(cases))
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        sources_sha256=hashes, server_jar_sha256=lab.digest(jars['server'], 'sha256'),
        matched_output_lines=matched, cases=len(cases),
        scope='Actual ConversionType.convertCommon on inert allocated mobs versus the '
              'production raw-data conversion helper. Empty, named, Unicode and 1024-tag '
              'sets. No world, conversion tick, native callback, save/reload or selector execution.'))
    print(f'{matched} conversion tag sets match: {report}', flush=True)


if __name__ == '__main__':
    main()
