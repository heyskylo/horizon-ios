#!/usr/bin/env python3
"""Resolved login targets and trimmed ASCII host/port syntax against vanilla.

No live DNS, server or game. IDN normalization and raw whitespace semantics are
not claimed: the parsing corpus keeps Horizon's existing outer ASCII trim.
"""
import json
import os
import random
import subprocess

try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def address_cases():
    hosts = ['localhost', 'play.example.org', '127.0.0.1', 'EXAMPLE.COM', '[::1]',
             '[2001:db8::1]', '[foo:bar]', '[example.com]', '[]', '[::1]junk', '']
    ports = ['', '0', '1', '25565', '65535', '65536', '-0', '-00', '-1', '+0',
             '+25565', '2147483647', '2147483648', '-2147483648', '-2147483649',
             '00000000000000025565', ' 25565', '25565 ', '1_0', '0x10',
             '\u0661\u0662\u0663', '\uff11\uff12\uff13', '++1', '--0']
    cases = [host for host in hosts]
    cases += [host + ':' + port for host in hosts for port in ports]
    cases += ['::1', '2001:db8::1', '[::1', '[::1]::', '[::1]]:', '[[::1]]:',
              ' \tplay.example.org:\r\n', ' \t[::1]:\r\n']
    rng = random.Random(0x10465)
    for _ in range(1000):
        port = ''.join(rng.choices('0123456789+-_: abc\u0661\uff11', k=rng.randrange(13)))
        cases.extend(host + ':' + port for host in ('localhost', '127.0.0.1', '[::1]', ''))
    # Multiple unbracketed colons make the whole value a hostname, not a port.
    # Unicode in that shape tests IDN normalization, outside this syntax corpus.
    return [value for value in cases
            if value.startswith('[') or value.count(':') <= 1 or value.isascii()]


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'rust-server-address-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaServerAddressOracle.java'
    sources = [oracle, lab.ROOT / 'crates/horizon-protocol/src/servers.rs',
               lab.ROOT / 'crates/horizon-protocol/examples/server_address_parse_probe.rs',
               lab.ROOT / 'tools/test_server_address_parity.py']
    hashes = base.source_hashes(sources)
    _, game = lab.config()
    manifest = json.loads((game / 'manifest.json').read_text(encoding='utf-8'))
    for artifact in lab.client_library_artifacts(manifest):
        library = game / 'libraries/client' / artifact['path']
        if lab.digest(library, 'sha1') != artifact['sha1']:
            raise ValueError(f'Client dependency checksum changed: {library}')
        libraries.append(library)
    classpath = os.pathsep.join(map(str, [jars['client'], *libraries]))

    def run(command, name):
        result = subprocess.run(list(map(str, command)), cwd=lab.ROOT,
                                capture_output=True, text=True)
        (build / (name + '.log')).write_text(result.stdout + result.stderr, encoding='utf-8')
        result.check_returncode()
        return [line for line in result.stdout.splitlines() if line.startswith('server_address=')]

    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none',
         '-cp', classpath, '-d', classes, oracle], 'compile-java')
    expected = run(['java', '-cp', str(classes) + os.pathsep + classpath,
                    'dev.horizonlab.parity.VanillaServerAddressOracle'], 'vanilla')
    actual = run(['cargo', 'test', '-p', 'horizon-protocol', '--lib',
                  'resolver::tests::login_handshake_address_matches_vanilla', '--', '--nocapture'], 'rust')
    if len(expected) != 3 or actual != expected:
        raise AssertionError(f'vanilla: {expected!r}; Horizon: {actual!r}')
    inputs = address_cases()
    lab.write_json(build / 'addresses.json', inputs)
    run(['java', '-cp', str(classes) + os.pathsep + classpath,
         'dev.horizonlab.parity.VanillaServerAddressOracle', build / 'addresses.json', build / 'java-parse.json'], 'vanilla-parse')
    run(['cargo', 'run', '--locked', '-p', 'horizon-protocol', '--example', 'server_address_parse_probe', '--',
         build / 'addresses.json', build / 'rust-parse.json'], 'rust-parse')
    java = json.loads((build / 'java-parse.json').read_text(encoding='utf-8'))
    rust = json.loads((build / 'rust-parse.json').read_text(encoding='utf-8'))
    assert len(java) == len(rust) == len(inputs)
    differences = [(text, native, result) for text, native, result in zip(inputs, java, rust) if native != result]
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, {'passed': not differences, 'resolved_cases': expected,
                           'parsing_cases': len(inputs), 'differences': differences,
                           'sources_sha256': hashes, 'scope': __doc__})
    assert not differences, f'{len(differences)} address differences: {differences[:5]}'
    print(f'Server address parity: {len(expected)} resolved targets and {len(inputs)} parser cases passed')


if __name__ == '__main__':
    main()
