#!/usr/bin/env python3
"""Run the native wind-burst block-filter regression without a display or server.

Build `cargo build -p horizon-client --example wind_burst_hook_probe`, then pass
--binary and --resources. Uses only temporary synthetic fixture worlds.
"""
import argparse
import json
import os
from pathlib import Path
import tempfile

try:
    from .block_update_harness import Facts
    from .integration_env import run_game
except ImportError:
    from block_update_harness import Facts
    from integration_env import run_game


def state(facts, name, **properties):
    for index in facts.by_block['minecraft:' + name]:
        actual = facts.states[index][4].get('properties', {})
        if all(actual.get(key) == value for key, value in properties.items()):
            return index
    raise ValueError((name, properties))


def check(binary, resources):
    facts = Facts(resources)
    cells = [
        [0, 100, 0, state(facts, 'lever', face='floor', powered='false')],
        [1, 100, 0, state(facts, 'candle', lit='true', waterlogged='false')],
        [-1, 100, 0, state(facts, 'lever', face='floor', powered='false')],
    ]
    stone = state(facts, 'stone')
    cells += [[x, 99, 0, stone] for x in [-1, 0, 1]]
    cases = []
    for breeze, griefing in [(False, True), (True, True), (True, False)]:
        cases.append(dict(seed=123, cells=cells, pos=[0, 100, 0],
                          rules={'mob_griefing': griefing},
                          wind_burst=dict(center=[0.5, 100.5, 0.5], radius=3.0, breeze=breeze)))
    with tempfile.TemporaryDirectory(prefix='hz-wind-hook-') as folder:
        work = Path(folder)
        inputs = work / 'cases.jsonl'
        inputs.write_text(''.join(json.dumps(case) + '\n' for case in cases))
        results = {}
        for mode in ['no_mod', 'noop', 'protect']:
            output = work / (mode + '.txt')
            env = dict(os.environ, HORIZON_DISPENSE_FIXTURE=f'{inputs},{output}')
            env.pop('HORIZON_WIND_NO_MOD', None)
            env.pop('HORIZON_WIND_PROTECT', None)
            if mode == 'no_mod':
                env['HORIZON_WIND_NO_MOD'] = '1'
            elif mode == 'protect':
                env['HORIZON_WIND_PROTECT'] = '1'
            run_game([str(binary), '--headless', '--resources', str(resources)],
                     cwd=work, env=env, check=True, timeout=120)
            text = output.read_text()
            assert 'failed ' not in text, text
            results[mode] = [part.splitlines() for part in text.split('case ')[1:]]
        # The fixture records sound identity/position, not random client pitch.
        for i in range(3):
            assert results['no_mod'][i] == results['noop'][i], results
            normal = results['noop'][i]
            protected = results['protect'][i]
            changed = lambda lines, pos: any(line.startswith('set ' + pos + ' ') for line in lines)
            if i < 2:
                assert changed(normal, '0,100,0') and changed(normal, '1,100,0'), normal
                assert not changed(protected, '0,100,0'), protected
                assert not changed(protected, '1,100,0'), protected
                assert changed(protected, '-1,100,0'), protected
                assert [line for line in normal if line.startswith('rng ')] == [
                    line for line in protected if line.startswith('rng ')], results
            else:
                assert not any(line.startswith('set ') for line in protected), protected
        print('PASS: player/breeze burst protection, unprotected control, no-mod/noop equivalence, RNG and mob_griefing')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--resources', type=Path, required=True)
    args = parser.parse_args()
    check(args.binary.resolve(), args.resources.resolve())


if __name__ == '__main__':
    main()
