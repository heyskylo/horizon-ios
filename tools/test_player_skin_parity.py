#!/usr/bin/env python3
"""Compare actual 26.3 skin-future and wing-texture decisions with Horizon.

The native oracle executes SkinManager registration/completion/createLookup and
WingsLayer's private texture selection. Only the download endpoint futures are
controlled (ready/exceptional/pending); no HTTP/cache-expiry claim is made.
"""
import argparse
import json
import os
import subprocess

try:
    from . import lab, test_rust_parity as base, test_model_baking_parity as baking
except ImportError:
    import lab, test_rust_parity as base, test_model_baking_parity as baking


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=('fast', 'dev'), default='fast')
    args = parser.parse_args()
    lock, jars, _ = base.verify_artifacts()
    dependencies = os.pathsep.join(map(str, [jars['client'], *baking.client_libraries()]))
    build = lab.LOCAL / 'rust-player-skin-parity'
    classes = build / 'classes'
    classes.mkdir(parents=True, exist_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaPlayerSkinOracle.java'
    commands = [
        ('compile-java', ['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                          '-nowarn', '-cp', dependencies, '-d', str(classes), str(oracle)]),
        ('vanilla', ['java', '--sun-misc-unsafe-memory-access=allow', '-cp',
                     str(classes) + os.pathsep + dependencies,
                     'dev.horizonlab.parity.VanillaPlayerSkinOracle', str(build / 'skins'), str(build / 'vanilla.jsonl')]),
        ('rust', ['cargo', 'run', '--quiet', '--profile', args.profile, '-p', 'horizon-client-lib',
                  '--example', 'client_probe', '--', 'player_skin_fixture', str(build / 'rust.jsonl')]),
    ]
    for name, command in commands:
        with (build / (name + '.log')).open('w') as log:
            result = subprocess.run(command, cwd=lab.ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=1800)
        if result.returncode:
            raise RuntimeError(f'{name} failed: {(build / (name + ".log")).read_text()[-4000:]}')
    native = [json.loads(line) for line in (build / 'vanilla.jsonl').read_text().splitlines()]
    rust = [json.loads(line) for line in (build / 'rust.jsonl').read_text().splitlines()]
    assert len(native) == len(rust) == 32, (len(native), len(rust))
    for index, (expected, actual) in enumerate(zip(native, rust)):
        assert expected == actual, f'case {index}: native={expected}, Horizon={actual}'
    print('32/32 player-skin registration and wing-selection cases match actual vanilla 26.3')


if __name__ == '__main__':
    main()
