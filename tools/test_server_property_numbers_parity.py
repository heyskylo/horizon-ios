#!/usr/bin/env python3
"""DedicatedServerProperties decimal settings, enums, bounds and seeds against pinned vanilla.

Only constructs settings from in-memory Properties. No server, EULA, window or player data.
"""
import os
import re
import subprocess

try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def number_cases():
    values = ['', '+', '-', '0', '-0', '+12', '12', ' 12', '12 ', '1_2',
              '2147483647', '2147483648', '-2147483648', '-2147483649',
              '9223372036854775807', '9223372036854775808',
              '-9223372036854775808', '-9223372036854775809',
              '0' * 100 + '12', '9' * 100, '\u00b2', '\u2460', '\U0001d7cf', '\u22121']
    # Exercise every BMP decimal-digit block independently against the real JDK parser.
    zeroes = [0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66,
              0xbe6, 0xc66, 0xce6, 0xd66, 0xde6, 0xe50, 0xed0, 0xf20, 0x1040,
              0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90, 0x1b50,
              0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0,
              0xaa50, 0xabf0, 0xff10]
    for zero in zeroes:
        for value in ['12', '-1', '+2147483647', '-2147483648',
                      '9223372036854775807', '-9223372036854775808']:
            values.append(''.join(chr(zero + int(c)) if c.isdigit() else c for c in value))
    return values


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'server-property-numbers-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaServerPropertyNumbersOracle.java'
    sources = [oracle, lab.ROOT / 'crates/horizon-core/src/text.rs',
               lab.ROOT / 'crates/horizon-server-files/src/properties.rs',
               lab.ROOT / 'crates/horizon-server-files/examples/properties_numbers.rs',
               lab.ROOT / 'tools/test_server_property_numbers_parity.py', lab.ROOT / 'Cargo.lock']
    hashes = base.source_hashes(sources)
    values = number_cases()
    inputs = build / 'numbers.txt'
    inputs.write_text('\n'.join(values) + '\n', encoding='utf-8')
    classpath = os.pathsep.join(map(str, [jars['server'], *libraries]))

    def run(command, name):
        result = subprocess.run(list(map(str, command)), cwd=lab.ROOT, capture_output=True,
                                text=True, encoding='utf-8', timeout=900)
        (build / (name + '.log')).write_text(result.stdout + result.stderr, encoding='utf-8')
        result.check_returncode()
        return re.findall(r'(?:RESULT|EXTRA|BROADCAST) .*', result.stdout)

    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none',
         '-cp', classpath, '-d', classes, oracle], 'compile-java')
    expected = run(['java', '-Djava.awt.headless=true', '-cp', str(classes) + os.pathsep + classpath,
                    'dev.horizonlab.parity.VanillaServerPropertyNumbersOracle', inputs], 'vanilla')
    actual = run(['cargo', 'run', '--locked', '-p', 'horizon-server-files', '--example',
                  'properties_numbers', '--', inputs], 'rust')
    if len(expected) != len(actual) or len(actual) != len(values) * 3:
        raise AssertionError(f'incomplete corpus: {len(expected)} vanilla / {len(actual)} native records')
    differences = [(i, a, b) for i, (a, b) in enumerate(zip(expected, actual)) if a != b]
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(passed=not differences, inputs=len(values), records=len(actual),
                               differences=differences, sources_sha256=hashes,
                               server_jar_sha256=lab.digest(jars['server'], 'sha256'), scope=__doc__))
    assert not differences, f'{len(differences)} differences: {differences[:5]}'
    print(f'{len(values)} numeric settings inputs match vanilla ({len(actual)} records): {report}')


if __name__ == '__main__':
    main()
