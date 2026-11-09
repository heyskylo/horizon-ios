#!/usr/bin/env python3
"""Compare direct trade modifier/cost/payment methods without starting a server."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def corpus():
    for seed in (0, 1, -1, 12345):
        for level in (0, 1, 7, 255, 300):
            for add in (False, True):
                yield {'seed': seed, 'trade': {
                    'gives': {'id': 'minecraft:book', 'count': 2},
                    'given_item_modifier': [
                        {'function': 'minecraft:set_enchantments', 'enchantments': {'minecraft:sharpness': 3}},
                        {'function': 'minecraft:set_enchantments', 'enchantments': {'minecraft:sharpness': level}, 'add': add}],
                    'wants': {'id': 'minecraft:emerald', 'count': {'type': 'minecraft:uniform', 'min': 2, 'max': 7}},
                    'additional_wants': {'id': 'minecraft:book', 'count': {'type': 'minecraft:constant', 'value': 2}}}}
        for count in (0, 1, 2, 64, 100):
            yield {'seed': seed, 'trade': {'gives': {'id': 'minecraft:emerald'},
                'wants': {'id': 'minecraft:potion', 'count': {'type': 'minecraft:constant', 'value': count},
                          'components': {'minecraft:potion_contents': {'potion': 'minecraft:water'}}}}}
        for count in (None, 1):
            wants = {'id': 'minecraft:potion', 'components': {'minecraft:potion_contents': {'potion': 'minecraft:water'}}}
            if count is not None:
                wants['count'] = count
            yield {'seed': seed, 'trade': {'gives': {'id': 'minecraft:emerald'}, 'wants': wants}}
        yield {'seed': seed, 'trade': {'gives': {'id': 'minecraft:book'},
            'given_item_modifier': {'function': 'minecraft:set_enchantments', 'enchantments': {
                'minecraft:sharpness': {'type': 'minecraft:uniform', 'min': 1, 'max': 5}}},
            'wants': {'id': 'minecraft:emerald', 'count': {'type': 'minecraft:uniform', 'min': 1, 'max': 12}}}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, default=lab.GAME)
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    parser.add_argument('--java-only', action='store_true')
    parser.add_argument('--no-build', action='store_true')
    args = parser.parse_args()
    lab.GAME = args.game
    lab.config = lambda: (json.loads(lab.LOCK.read_text(encoding='utf-8')), args.game)
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'trade-components-parity'
    build.mkdir(parents=True, exist_ok=True)
    cases = list(corpus())
    inputs = build / 'inputs.jsonl'
    inputs.write_text(''.join(json.dumps(c) + '\n' for c in cases), encoding='utf-8')
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaTradeComponentsOracle.java'
    sources = [Path(__file__), oracle,
               lab.ROOT / 'crates/horizon-client/examples/probe/trade_components_probe.rs',
               lab.ROOT / 'crates/horizon-client/src/merchant.rs']
    tested = base.source_hashes(sources)
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none', '-cp', cp,
                    '-d', str(classes), str(oracle)], check=True)
    with tempfile.TemporaryDirectory(prefix='horizon-trade-components-java-') as scratch:
        subprocess.run(['java', '-Xmx1G', '-cp', str(classes) + os.pathsep + cp,
                        'dev.horizonlab.parity.VanillaTradeComponentsOracle', str(inputs), str(build / 'java.jsonl')],
                       check=True, cwd=scratch, env=lab.game_env())
    expected = [json.loads(s) for s in (build / 'java.jsonl').read_text(encoding='utf-8').splitlines()]
    if args.java_only:
        print(f'{len(expected)} headless vanilla cases')
        return
    if not args.no_build:
        subprocess.run(['cargo', 'build', '--locked', '-p', 'horizon-client-lib', '--example', 'client_probe'], check=True)
    binary = lab.cargo_target() / 'debug/examples/client_probe'
    with tempfile.TemporaryDirectory(prefix='horizon-trade-components-rust-') as scratch:
        subprocess.run([str(binary), 'trade_components_probe', str(args.resources.resolve()), str(inputs), str(build / 'rust.jsonl')],
                       check=True, cwd=scratch, env=lab.game_env())
    actual = [json.loads(s) for s in (build / 'rust.jsonl').read_text(encoding='utf-8').splitlines()]
    bad = [(i, a, b) for i, (a, b) in enumerate(zip(expected, actual)) if a != b]
    if bad or len(actual) != len(expected):
        raise SystemExit(f'{len(bad)} differing cases of {len(expected)} (Rust {len(actual)}): {bad[:3]}')
    base.ensure_sources_unchanged(sources, tested)
    lab.write_json(build / 'report.json', {'minecraft': lock['minecraft']['version'], 'cases': len(expected),
                   'sources_sha256': tested,
                   'scope': 'Direct SetEnchantmentsFunction.run, TradeCost.toItemCost and MerchantOffer.take; no server/world.'})
    print(f'{len(expected)} headless trade component cases match vanilla')


if __name__ == '__main__':
    main()
