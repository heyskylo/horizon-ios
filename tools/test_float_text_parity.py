#!/usr/bin/env python3
"""Pinned numeric/SNBT text for subnormals and normal-value controls.

--broad also checks uniformly seeded normal bit patterns. It is diagnostic:
existing nearest-decimal tie-breaking differences are outside this patch.
"""
import argparse
import json
import os
from pathlib import Path
import random
import struct
import subprocess

try:
    from . import lab, oracle_harness as h, test_rust_parity as base
except ImportError:
    import lab
    import oracle_harness as h
    import test_rust_parity as base


def fixtures(broad=False):
    rng = random.Random(0x10401)
    floats, doubles = set(range(4096)), set(range(4096))
    floats.update([0x007fffff, 0x00800000, 0x3f800000, 0x7f7fffff, 0x7f800000, 0x7fc00000])
    doubles.update([0xfffffffffffff, 0x10000000000000, 0x3ff0000000000000,
                    0x7fefffffffffffff, 0x7ff0000000000000, 0x7ff8000000000000])
    for exponent in range(-45, 39):
        try:
            bits = struct.unpack('>I', struct.pack('>f', 10.0 ** exponent))[0]
            floats.update(max(0, min(0x7f800000, bits + i))
                          for i in (range(-3, 4) if broad else (0,)))
        except OverflowError:
            pass
    for exponent in range(-323, 309):
        bits = struct.unpack('>Q', struct.pack('>d', 10.0 ** exponent))[0]
        doubles.update(max(0, min(0x7ff0000000000000, bits + i))
                       for i in (range(-3, 4) if broad else (0,)))
    floats.update(rng.getrandbits(32 if broad else 23) for _ in range(10000))
    doubles.update(rng.getrandbits(64 if broad else 52) for _ in range(10000))
    if not broad:
        floats.update(0x00800000 + i for i in range(-3, 4))
        doubles.update(0x10000000000000 + i for i in range(-3, 4))
        for value in [0.001, 0.01, 0.1, 1.0, 1.5, 42.0, 12345.125, 9999999.0, 1e7]:
            floats.add(struct.unpack('>I', struct.pack('>f', value))[0])
            doubles.add(struct.unpack('>Q', struct.pack('>d', value))[0])
    floats.update(bits | 0x80000000 for bits in list(floats))
    doubles.update(bits | 0x8000000000000000 for bits in list(doubles))
    return [f'f {bits:08x}' for bits in sorted(floats)] + [f'd {bits:016x}' for bits in sorted(doubles)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--broad', action='store_true', help='diagnose the full seeded normal-bit corpus too')
    args = parser.parse_args()
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / ('float-text-broad' if args.broad else 'float-text-parity')
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    paths = ['crates/horizon-world/src/snbt_text.rs', 'crates/horizon-world/src/datafix/json.rs',
             'crates/horizon-world/src/nbt.rs', 'crates/horizon-world/examples/probe/main.rs',
             'crates/horizon-world/examples/probe/float_text_fixture.rs',
             'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaFloatTextOracle.java',
             'tools/test_float_text_parity.py']
    sources = [lab.ROOT / path for path in paths]
    hashes = base.source_hashes(sources)
    cases = fixtures(args.broad)
    inputs = build / 'inputs.txt'
    inputs.write_text(''.join(case + '\n' for case in cases), encoding='utf-8')
    classes, dependencies, _ = h.compile_oracle(build, 'VanillaFloatTextOracle',
        {'client': jars['server']}, libraries, lock['minecraft']['java_major'])
    h.java_run(build, classes, dependencies, 'VanillaFloatTextOracle', [inputs, build / 'java.txt'], 'java')
    # The release world_probe every other horizon-world harness builds, so CI reuses it.
    h.run_logged(['cargo', 'build', '--release', '--locked', '-p', 'horizon-world',
                  '--example', 'world_probe'], build, 'compile-rust')
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT))
    binary = Path(metadata['target_directory']) / 'release/examples' / (
        'world_probe.exe' if os.name == 'nt' else 'world_probe')
    h.run_logged([binary, 'float_text_fixture', inputs, build / 'rust.txt'], build, 'rust')
    expected = (build / 'java.txt').read_text(encoding='utf-8').splitlines()
    actual = (build / 'rust.txt').read_text(encoding='utf-8').splitlines()
    assert len(expected) == len(actual) == len(cases)
    differences = [{'java': left, 'rust': right} for left, right in zip(expected, actual) if left != right]
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        sources_sha256=hashes, server_jar_sha256=lab.digest(jars['server'], 'sha256'),
        input_sha256=lab.digest(inputs, 'sha256'), cases=len(cases),
        matched_output_lines=len(cases) - len(differences), differences=differences,
        scope='Java Float/Double.toString, actual pinned FloatTag/DoubleTag.toString and Rust '
              'SNBT/DataFixer number text. Strict scope: dense and seeded subnormal bits, both signs, '
              'normal/subnormal boundaries, decimal powers, zero signs, nonfinite values and named normal '
              'controls. --broad diagnoses other normal patterns; known decimal tie-breaking is not fixed.'))
    print(f'{len(cases) - len(differences)}/{len(cases)} float-text cases match: {report}', flush=True)
    assert not differences, f'{len(differences)} text differences; first cases: {differences[:3]}'


if __name__ == '__main__':
    main()
