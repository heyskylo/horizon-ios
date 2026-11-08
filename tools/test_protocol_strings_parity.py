#!/usr/bin/env python3
"""Packet strings against actual Utf8String.read: replacement grouping, limits and cursors.

No server or game is started. Every one- and two-byte payload is included, plus
surrogate encodings, boundary/malformed longer sequences, deterministic random
bytes and encoded/decoded length failures. Outputs include UTF-16 units and the
reader position on both success and failure, rather than exception text.
"""
import itertools
import os
import random
try:
    from . import lab, test_rust_parity as base, oracle_harness as harness
except ImportError:
    import lab
    import test_rust_parity as base
    import oracle_harness as harness


def varint(value):
    value &= 0xffffffff
    output = bytearray()
    while value > 127:
        output.append((value & 127) | 128)
        value >>= 7
    output.append(value)
    return bytes(output)


def cases():
    def packet(data, limit=16):
        return limit, varint(len(data)) + data + b'\x2a'
    yield packet(b'')
    for length in (1, 2):
        for data in itertools.product(range(256), repeat=length):
            yield packet(bytes(data))
    # Java consumes an encoded surrogate as one malformed sequence, including
    # a truncated prefix and a bad third byte; Rust's lossy decoder differs.
    for second in range(0xa0, 0xc0):
        for third in range(256):
            yield packet(bytes([0xed, second, third]))
    for lead in (0xe0, 0xe1, 0xef, 0xf0, 0xf1, 0xf4, 0xf5):
        for rest in itertools.product((0, 0x7f, 0x80, 0x8f, 0x90, 0x9f, 0xa0, 0xbf, 0xc0, 0xff), repeat=3):
            yield packet(bytes([lead, *rest]))
    rng = random.Random(0x10397)
    for _ in range(20_000):
        yield packet(rng.randbytes(rng.randrange(24)))
    edge_strings = [b'', b'A', b'\xff', b'\xff\xff', b'\xed\xa0', b'\xed\xa0\x80',
                    b'\xf0\x9f\x98\x80', b'A\xf0\x9f\x98\x80', b'\xe2\x82A']
    for data in edge_strings:
        for limit in range(5):
            yield packet(data, limit)
    # Declared lengths must not consume any body on an encoded limit or a
    # short packet; decoded length errors consume exactly the string bytes.
    for limit in (0, 1, 2, 16):
        for declared in (-1, 0, 1, 2, 3, 4, 48, 49):
            for body in (b'', b'\xff', b'ABC'):
                yield limit, varint(declared) + body
    for data in (b'a' * 32767, b'a' * 32768, '😀'.encode() * 16384,
                 b'\xff' * 32767, b'\xed\xa0\x80' * 32767):
        yield packet(data, 32767)


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'protocol-strings-parity'
    build.mkdir(parents=True, exist_ok=True)
    (build / 'report.json').unlink(missing_ok=True)
    source = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaProtocolStringsOracle.java'
    sources = [source, lab.ROOT / 'crates/horizon-protocol/src/buf.rs',
               lab.ROOT / 'crates/horizon-protocol/examples/protocol_strings_probe.rs',
               lab.ROOT / 'tools/test_protocol_strings_parity.py']
    tested = base.source_hashes(sources)
    count = 0
    with (build / 'inputs.txt').open('w') as output:
        for limit, wire in cases():
            output.write(f'{limit} {wire.hex()}\n')
            count += 1
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    run = lambda command, name: harness.run_logged(command, build, name)
    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', cp,
         '-d', classes, source], 'compile-java')
    run(['java', '-cp', str(classes) + os.pathsep + cp,
         'dev.horizonlab.parity.VanillaProtocolStringsOracle', build / 'inputs.txt', build / 'java.txt'], 'java')
    run(['cargo', 'build', '--locked', '-p', 'horizon-protocol', '--example', 'protocol_strings_probe'], 'compile-rust')
    binary = lab.cargo_target() / 'debug/examples' / ('protocol_strings_probe.exe' if os.name == 'nt' else 'protocol_strings_probe')
    run([binary, build / 'inputs.txt', build / 'rust.txt'], 'rust')
    matched = base.compare(build / 'java.txt', build / 'rust.txt')
    assert matched == count, (matched, count)
    base.ensure_sources_unchanged(sources, tested)
    lab.write_json(build / 'report.json', {'pass': True, 'cases': count, 'sources_sha256': tested,
                                         'scope': __doc__})
    print(f'Packet strings: {count} native codec results and cursor positions match')


if __name__ == '__main__':
    main()
