#!/usr/bin/env python3
"""NBT path quote/index lexical rules, result and byte cursor against vanilla.

Uses the real NbtPathArgument/StringReader without a server or prepared resources.
Covers quoted escapes, Unicode keys and prefixes, numeric index tokens, empty and
unterminated controls. Error categories/cursors are compared, not localized text
or invalid-escape argument UTF-16 representation. SNBT filters and path operations
and the vanilla unchecked bare-bracket exception are outside this parser-only corpus.
"""
import itertools
import json
import os
from pathlib import Path
import random
try:
    from . import lab, oracle_harness as h, test_rust_parity as base
except ImportError:
    import lab, oracle_harness as h, test_rust_parity as base


def fixtures():
    values = ['', ' ', 'a', 'a.b', 'a.', 'a..b', '[]', '[0]', '[-1]',
              'a[2147483647]', 'a[-2147483648]', 'a[2147483648]',
              'a[-2147483649]', 'a[1.0]', 'a[.]', 'a[ ]', 'a[+1]']
    for quote, prefix in itertools.product(('"', "'"), ('', 'a', '\u00e9', '\U0001f600')):
        for char in [chr(i) for i in range(128)] + ['\u00e9', '\u4e2d', '\U0001f600']:
            values.append(quote + prefix + '\\' + char + quote)
        for key in ('', 'plain', 'a.b', 'a[b]', 'a b', '\u00e9', '\U0001f600', 'a\u4e2d\U0001f600b'):
            values.extend([quote + key + quote, quote + key, quote + key + '\\'])
    rng = random.Random(0x11039)
    for _ in range(2500):
        token = ''.join(rng.choice('0129-.') for _ in range(rng.randrange(1, 18)))
        for prefix in ('', 'a', '"\u00e9"', '"\U0001f600"'):
            values.append(prefix + '[' + token + ']')
    for prefix, token, suffix in itertools.product(('', 'list'),
            ('', '-', '.', '--1', '1.2', '1', '-2', '0'), ('', ']', '+]', 'x]', ' ]')):
        values.append(prefix + '[' + token + suffix)
    # Vanilla parseNode peeks past a bare terminal '[' and throws an unchecked
    # exception. This corpus checks command syntax errors, not that crash.
    return [value for value in dict.fromkeys(values) if not value.endswith('[')]


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'nbt-path-reader-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    paths = ['crates/horizon-client-core/src/nbt_path.rs',
             'crates/horizon-client-core/examples/nbt_path_reader_probe.rs',
             'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaNbtPathReaderOracle.java',
             'tools/test_nbt_path_reader_parity.py']
    sources = [lab.ROOT / path for path in paths]
    hashes = base.source_hashes(sources)
    cases = fixtures()
    inputs = build / 'inputs.jsonl'
    inputs.write_text(''.join(json.dumps(text) + '\n' for text in cases), encoding='utf-8')
    classes, dependencies, _ = h.compile_oracle(build, 'VanillaNbtPathReaderOracle',
        {'client': jars['server']}, libraries, lock['minecraft']['java_major'])
    h.java_run(build, classes, dependencies, 'VanillaNbtPathReaderOracle',
               [inputs, build / 'java.txt'], 'java')
    h.run_logged(['cargo', 'build', '--offline', '--locked', '-p', 'horizon-client-core',
                  '--example', 'nbt_path_reader_probe'], build, 'compile-rust',
                 env=dict(os.environ, CARGO_BUILD_JOBS=os.environ.get('CARGO_BUILD_JOBS', '2')))
    binary = lab.cargo_target() / 'debug/examples' / (
        'nbt_path_reader_probe.exe' if os.name == 'nt' else 'nbt_path_reader_probe')
    h.run_logged([binary, inputs, build / 'rust.txt'], build, 'rust')
    matched = h.compare(build / 'java.txt', build / 'rust.txt', len(cases))
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        cases=len(cases), matched_output_lines=matched, sources_sha256=hashes,
        input_sha256=lab.digest(inputs, 'sha256'),
        server_jar_sha256=lab.digest(jars['server'], 'sha256'), scope=__doc__))
    print(f'{matched} NBT path reader cases match the pinned server: {report}', flush=True)


if __name__ == '__main__':
    main()
