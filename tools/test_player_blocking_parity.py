#!/usr/bin/env python3
"""Compare Game's shield-blocking boundary with native item blocking and durability."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
try:
    from . import lab, test_rust_parity as base
    from .integration_env import run_game
except ImportError:
    import lab
    import test_rust_parity as base
    from integration_env import run_game


def corpus():
    def stack(components=None):
        return '{id:"minecraft:shield",count:1,components:' + json.dumps(components or {}) + '}'
    variants = [stack(), stack({'minecraft:unbreakable': {}}),
                stack({'minecraft:max_damage': 10, 'minecraft:damage': 0}),
                stack({'minecraft:max_damage': 8, 'minecraft:damage': 7}),
                stack({'!minecraft:max_damage': {}}),
                stack({'!minecraft:damage': {}}),
                stack({'minecraft:unbreakable': {}, 'minecraft:enchantments': {'minecraft:unbreaking': 3}}),
                stack({'!minecraft:damage': {}, 'minecraft:enchantments': {'minecraft:unbreaking': 3}})]
    for item in variants:
        for damage in [0, 2, 8]:
            for pierce, behind, creative in [(False, False, False), (True, False, False),
                                             (False, True, False), (False, False, True)]:
                yield {'seed': 0, 'cells': [], 'items': [], 'entities': [],
                       'shield_hit': dict(item=item, damage=damage, pierce=pierce,
                                          behind=behind, creative=creative)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--resources', type=Path)
    parser.add_argument('--timeout', type=float, default=600,
                        help='Game fixture timeout in seconds, including debug resource loading')
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    lock, jars, libraries = base.verify_artifacts()
    work = lab.LOCAL / 'player-blocking-parity'
    work.mkdir(parents=True, exist_ok=True)
    cases = work / 'cases.jsonl'
    cases.write_text(''.join(json.dumps(case) + '\n' for case in corpus()), encoding='utf-8')
    classes = work / 'classes'
    classes.mkdir(exist_ok=True)
    java_root = lab.ROOT / 'mods/horizon/src/test/java'
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-sourcepath', str(java_root), '-cp', cp, '-d', str(classes),
                    str(java_root / 'dev/horizonlab/parity/VanillaPlayerBlockingOracle.java')], check=True)
    native = work / 'native.txt'
    subprocess.run(['java', '-Xmx512M', '-cp', str(classes) + os.pathsep + cp,
                    'dev.horizonlab.parity.VanillaPlayerBlockingOracle', str(cases), str(native)], check=True)
    binary = args.binary
    if binary is None:
        subprocess.run(['cargo', 'build', '-p', 'horizon-client', '--bin', 'horizon-client'], cwd=lab.ROOT, check=True)
        binary = lab.cargo_target() / 'debug' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    resources = args.resources or lab.LOCAL / 'horizon-client' / lock['minecraft']['version']
    rust = work / 'rust.txt'
    rust.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory(prefix='hz-player-blocking-') as game_dir:
        run_game([str(binary.resolve()), '--headless', '--game-dir', game_dir,
                  '--resources', str(resources.resolve())], cwd=game_dir, check=True, timeout=args.timeout,
                 env=lab.game_env(HORIZON_DISPENSE_FIXTURE=f'{cases.resolve()},{rust.resolve()}'))
    expected = native.read_text(encoding='utf-8').splitlines()
    rust_lines = rust.read_text(encoding='utf-8').splitlines()
    actual = [line for line in rust_lines
              if line.startswith(('case ', 'shield '))]
    if actual != expected:
        for index, (left, right) in enumerate(zip(expected, actual)):
            if left != right:
                print(f'line {index}: vanilla {left}\n             Rust {right}')
        raise AssertionError(f'player blocking differs ({len(expected)} vanilla / {len(actual)} Rust lines)')
    # Nondamageable stacks must not advance Unbreaking's RNG. The ordinary
    # block sound can draw, so compare identical hits with/without the enchantment.
    random_states = [line for line in rust_lines if line.startswith('rng ')]
    assert len(random_states) == len(expected) // 2, 'missing Game RNG observations'
    for plain, enchanted in [(1, 6), (5, 7)]:
        assert random_states[plain * 12:(plain + 1) * 12] == random_states[enchanted * 12:(enchanted + 1) * 12], (
            f'nondamageable item advanced Unbreaking RNG: variants {plain}/{enchanted}')
    print(f'{len(expected) // 2} actual Game blocking cases match native dispatch')


if __name__ == '__main__':
    main()
