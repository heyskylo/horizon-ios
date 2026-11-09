#!/usr/bin/env python3
"""Compound SNBT uses Java UTF-16 key ordering, including nested compounds."""
import copy
import itertools
import json
import os
from pathlib import Path
import random
import subprocess

try:
    from . import lab, oracle_harness as h, test_rust_parity as base
except ImportError:
    import lab
    import oracle_harness as h
    import test_rust_parity as base


def fixtures():
    rng = random.Random(0x10463)
    keys = ['', 'A', '_', 'true', 'false', chr(0xd7ff), chr(0xe000), chr(0xfffd),
            chr(0xffff), chr(0x10000), chr(0x10001), chr(0x1f600), chr(0x10ffff),
            chr(0x10000) + 'a', chr(0xe000) + 'a']
    cases, groups = [], []

    def add(entries):
        start = len(cases)
        cases.extend([{'entries': entries}, {'entries': list(reversed(entries))}])
        groups.append([start, start + 1])

    cases.append({'entries': []})
    for key in keys:
        cases.append({'entries': [[key, 17]]})
    for left, right in itertools.combinations(keys, 2):
        add([[left, -1], [right, 2]])
    for order in itertools.permutations([(chr(0xe000), 2), (chr(0x10000), 1), (chr(0xd7ff), 0)]):
        cases.append({'entries': list(order)})
    add([['outer', {'entries': [[chr(0xe000), 2], [chr(0x10000), 1]]}], ['ascii', 3]])
    alphabet = ['a', 'Z', '_', ' ', "'", '"', '\\', chr(0x2603), chr(0x4e2d),
                chr(0xd7ff), chr(0xe000), chr(0xffff), chr(0x10000), chr(0x1f600), chr(0x10ffff)]

    def tree(depth):
        entries = {chr(0xe000): rng.randrange(-1000, 1001),
                   chr(0x10000): rng.randrange(-1000, 1001)}
        for _ in range(rng.randrange(8)):
            key = ''.join(rng.choice(alphabet) for _ in range(rng.randrange(6)))
            entries[key] = tree(depth + 1) if depth < 3 and rng.random() < .3 else rng.randrange(-1000, 1001)
        return {'entries': list(entries.items())}

    def shuffle(node):
        if isinstance(node, dict):
            rng.shuffle(node['entries'])
            for _, value in node['entries']:
                shuffle(value)

    for _ in range(500):
        original = tree(0)
        shuffled = copy.deepcopy(original)
        shuffle(shuffled)
        start = len(cases)
        variants = [original, {'entries': list(reversed(original['entries']))}, shuffled]
        unique = []
        for case in variants:
            if case not in unique:
                unique.append(case)
        cases.extend(unique)
        groups.append(list(range(start, len(cases))))
    return cases, groups


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'snbt-unicode-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    paths = ['crates/horizon-world/src/snbt_text.rs', 'crates/horizon-world/src/nbt.rs',
             'crates/horizon-world/tests/snbt_unicode_order.rs',
             'crates/horizon-world/examples/probe/main.rs',
             'crates/horizon-world/examples/probe/snbt_unicode_fixture.rs',
             'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaSnbtUnicodeOracle.java',
             'tools/test_snbt_unicode_parity.py']
    sources = [lab.ROOT / path for path in paths]
    hashes = base.source_hashes(sources)
    cases, groups = fixtures()
    inputs = build / 'inputs.jsonl'
    inputs.write_text(''.join(json.dumps(case) + '\n' for case in cases), encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = h.compile_oracle(build, 'VanillaSnbtUnicodeOracle',
        {'client': jars['server']}, libraries, lock['minecraft']['java_major'])
    h.java_run(build, classes, dependencies, 'VanillaSnbtUnicodeOracle', [inputs, build / 'java.txt'], 'java')
    h.run_logged(['cargo', 'build', '--offline', '--locked', '-p', 'horizon-world',
                  '--example', 'world_probe'], build, 'compile-rust',
                 env=dict(os.environ, CARGO_BUILD_JOBS='2'))
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT))
    binary = Path(metadata['target_directory']) / 'debug/examples' / (
        'world_probe.exe' if os.name == 'nt' else 'world_probe')
    h.run_logged([binary, 'snbt_unicode_fixture', inputs, build / 'rust.txt'], build, 'rust')
    expected = (build / 'java.txt').read_text(encoding='utf-8').splitlines()
    assert len(expected) == len(cases)
    for group in groups:
        assert len({expected[index] for index in group}) == 1, f'Invalid insertion-order controls: {group}'
    matched = h.compare(build / 'java.txt', build / 'rust.txt', len(cases))
    base.ensure_sources_unchanged(sources, hashes)
    assert lab.digest(inputs, 'sha256') == input_hash
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        sources_sha256=hashes, server_jar_sha256=lab.digest(jars['server'], 'sha256'),
        input_sha256=input_hash, cases=len(cases), equivalent_insertion_groups=len(groups),
        matched_output_lines=matched,
        scope='Actual CompoundTag/StringTagVisitor output vs production compact SNBT. Valid Unicode '
              'BMP/supplementary mixtures, prefixes, ASCII quoting controls, insertion orders and '
              'seeded recursive integer compounds. Output compared as UTF-16 units. '
              'Unpaired surrogates, binary NBT order, numeric formatting and full component hashes are outside this scope.'))
    print(f'{matched} Unicode compound SNBT cases match: {report}', flush=True)


if __name__ == '__main__':
    main()
