#!/usr/bin/env python3
"""Identifier.STREAM_CODEC packet reads, independent of game resources."""
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
    def packet(value):
        raw = value.encode('utf-8')
        length, prefix = len(raw), bytearray()
        while length > 127:
            prefix.append((length & 127) | 128)
            length >>= 7
        prefix.append(length)
        return bytes(prefix) + raw

    values = ['', ':', 'stone', ':stone', 'minecraft:stone', 'mod:', 'a:b:c',
              'A:b', 'a:B', 'a/b:c', 'a+b:c', 'a b:c', 'a:b/c.d-e_f09',
              'stone' * 6553, 'a' * 32767, 'a' * 32768]
    for code in range(256):
        char = chr(code)
        values.extend([char, 'a' + char + ':path', 'ns:dir/' + char])
    rng = random.Random(0x1DE17)
    alphabet = 'abcxyz01239._-/:'
    for _ in range(2500):
        values.append(''.join(rng.choice(alphabet) for _ in range(rng.randrange(32))))
    cases = [packet(value) for value in values]
    cases.extend([bytes.fromhex(x) for x in ['', '01', '036162', 'ffffffff0f',
                  '8080808080', '03eda080', '02c080', '0261ff']])
    return list(dict.fromkeys(cases))


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'protocol-identifiers-parity'
    build.mkdir(parents=True, exist_ok=True)
    paths = ['crates/horizon-protocol/src/buf.rs',
             'crates/horizon-protocol/src/packets/configuration.rs',
             'crates/horizon-protocol/src/packets/play.rs',
             'crates/horizon-protocol/examples/protocol_identifiers_probe.rs',
             'crates/horizon-protocol/tests/identifier_codec.rs',
             'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaProtocolIdentifierOracle.java',
             'tools/test_protocol_identifiers_parity.py']
    sources = [lab.ROOT / path for path in paths]
    hashes = base.source_hashes(sources)
    cases = fixtures()
    inputs = build / 'inputs.txt'
    inputs.write_text(''.join(case.hex() + '\n' for case in cases), encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = h.compile_oracle(build, 'VanillaProtocolIdentifierOracle',
        {'client': jars['server']}, libraries, lock['minecraft']['java_major'])
    h.java_run(build, classes, dependencies, 'VanillaProtocolIdentifierOracle',
               [inputs, build / 'java.txt'], 'java')
    h.run_logged(['cargo', 'build', '--locked', '-p', 'horizon-protocol',
                  '--example', 'protocol_identifiers_probe'], build, 'compile-rust',
                 env=dict(os.environ, CARGO_BUILD_JOBS='2'))
    binary = lab.cargo_target() / 'debug/examples' / (
        'protocol_identifiers_probe.exe' if os.name == 'nt' else 'protocol_identifiers_probe')
    h.run_logged([binary, inputs, build / 'rust.txt'], build, 'rust')
    matched = base.compare(build / 'java.txt', build / 'rust.txt')
    base.ensure_sources_unchanged(sources, hashes)
    if lab.digest(inputs, 'sha256') != input_hash:
        raise AssertionError('Input fixtures changed')
    report = {'minecraft': lock['minecraft']['version'], 'cases': len(cases),
              'manifest_sha1': lock['minecraft']['manifest_sha1'],
              'server_jar_sha256': lab.digest(jars['server'], 'sha256'),
              'matched': matched, 'sources_sha256': hashes,
              'input_sha256': input_hash,
              'scope': 'Identifier.STREAM_CODEC decoding, normalized text and reader position; '
                       'includes real CustomPayload regression tests separately; encoding unchanged'}
    lab.write_json(build / 'report.json', report)
    print(f'{matched} packet identifier cases match pinned Minecraft: {build / "report.json"}')


if __name__ == '__main__':
    main()
