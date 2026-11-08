#!/usr/bin/env python3
"""Compare FallingBlockEntity's landing branch with Horizon's.

Java (`VanillaFallingBlockOracle`) puts a real `FallingBlockEntity` on the
block update oracle's level, restores the destination block the entity removed
when it started, and ticks it once with its feet on the floor below. Rust
(`falling_block_fixture`) runs `Behaviours::falling_block_landed` with the same
carried state and destination. Both print the landing decision (the placed
state, broken, or still falling on a moving piston), every changed block and
the numeric level events.

The base corpus covers sand, red sand, gravel, suspicious sand and gravel,
pointed dripstone, dragon eggs and sulfur spikes into air, stone, obsidian,
water, lava, fire, plants, leaves, slabs, cobwebs and moving pistons.
`--no-contact` leaves out concrete powder's water contact and the anvil
landing event, reproducing the corpus before those families.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess

try:
    from . import block_update_harness as h, lab, test_dispenser_parity as d, test_rust_parity as base
except ImportError:
    import block_update_harness as h
    import lab
    import test_dispenser_parity as d
    import test_rust_parity as base


def base_cases(facts):
    states = d.States(facts)
    cells = [[x, 63, z, states.get('stone')] for x in range(5, 12) for z in range(5, 12)]

    def case(carried, destination, seed):
        world = [list(c) for c in cells]
        if destination is not None:
            world.append([8, 64, 8, destination])
        return dict(seed=seed, cells=world, state=carried, pos=[8, 64, 8], time=0, drop=True)

    carried = [
        states.get('sand'), states.get('red_sand'), states.get('gravel'),
        states.get('suspicious_sand'), states.get('suspicious_gravel'),
        states.get('dragon_egg'), states.get('sulfur_spike'),
    ]
    for i, state in enumerate(states.facts.states):
        if state[0] == 'minecraft:pointed_dripstone' and state[4].get('properties', {}).get('waterlogged') != 'true':
            carried.append(i)
    destinations = [
        None, states.get('stone'), states.get('obsidian'), states.get('water'),
        states.get('short_grass'), states.get('oak_leaves'), states.get('dead_bush'),
        states.get('oak_slab', type='bottom'), states.get('cobweb'),
        states.get('moving_piston'),
    ]
    seed = 0x46414C4C
    for state in carried:
        for destination in destinations:
            yield case(state, destination, seed)
            seed += 1


def contact_cases(facts):
    states = d.States(facts)
    cells = [[x, 63, z, states.get('stone')] for x in range(5, 12) for z in range(5, 12)]
    concretes = [states.get(name) for name in sorted(states.facts.names)
                 if name.endswith('_concrete_powder')]
    water = states.get('water')
    seed = 0x434F4E54
    for state in concretes:
        for extra in (None, [(8, 64, 8)], [(8, 65, 8)], [(7, 64, 8)], [(9, 64, 8)],
                      [(8, 64, 7)], [(8, 64, 9)], [(8, 65, 8), (7, 64, 8), (9, 64, 8)],
                      [(8, 64, 7), (8, 64, 9)]):
            world = [list(c) for c in cells]
            for at in extra or ():
                world.append([at[0], at[1], at[2], water])
            yield dict(seed=seed, cells=world, state=state, pos=[8, 64, 8], time=0, drop=True)
            seed += 1
    for name in ('anvil', 'chipped_anvil', 'damaged_anvil'):
        for destination in (None, states.get('water'), states.get('stone')):
            world = [list(c) for c in cells]
            if destination is not None:
                world.append([8, 64, 8, destination])
            yield dict(seed=seed, cells=world, state=states.get(name), pos=[8, 64, 8],
                       time=0, drop=True)
            seed += 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--no-contact', action='store_true',
                        help='leave out concrete powder water contact and anvil landings')
    args = parser.parse_args()
    build = lab.LOCAL / 'falling-block-parity'
    build.mkdir(parents=True, exist_ok=True)
    report_path = build / 'report.json'
    report_path.unlink(missing_ok=True)
    if args.prepare:
        lab.prepare()
    lock, jars, libraries = base.verify_artifacts()
    facts = h.load_facts()
    resources = lab.LOCAL / 'horizon-client' / lock['minecraft']['version']
    cases = list(base_cases(facts))
    if not args.no_contact:
        cases += list(contact_cases(facts))
    inputs = build / 'cases.jsonl'
    inputs.write_text(''.join(json.dumps(c) + '\n' for c in cases), encoding='utf-8')
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaFallingBlockOracle.java'
    sources = [oracle, Path(__file__), lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/lab.py',
               lab.ROOT / 'tools/block_update_harness.py', lab.ROOT / 'tools/test_dispenser_parity.py',
               lab.ROOT / 'crates/horizon-client/examples/probe/falling_block_fixture.rs',
               lab.ROOT / 'crates/horizon-client/src/falling_block.rs',
               lab.ROOT / 'crates/horizon-world/src/level/falling.rs']
    tested = base.source_hashes(sources)
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    java_root = lab.ROOT / 'mods/horizon/src/test/java'

    def run(command, name, timeout=900):
        with (build / (name + '.log')).open('w', encoding='utf-8') as log:
            subprocess.run(list(map(str, command)), cwd=lab.ROOT, check=True, timeout=timeout,
                           stdout=log, stderr=subprocess.STDOUT)

    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-sourcepath', str(java_root),
         '-cp', dependencies, '-d', classes, oracle], 'compile-java')
    run(['java', '-Xmx2g', '-cp', str(classes) + os.pathsep + dependencies,
         'dev.horizonlab.parity.VanillaFallingBlockOracle', resources, inputs, build / 'client.txt'], 'client')
    run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client-lib', '--example', 'client_probe'], 'compile-rust', timeout=3600)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
                                                  cwd=lab.ROOT, text=True))
    binary = Path(metadata['target_directory']) / 'fast/examples/client_probe'
    run([binary, 'falling_block_fixture', resources, inputs, build / 'rust.txt'], 'rust')
    matched = base.compare(build / 'client.txt', build / 'rust.txt')
    base.ensure_sources_unchanged(sources, tested)
    lab.write_json(report_path, {
        'schema': 1, 'minecraft': lock['minecraft']['version'], 'platform': platform.platform(),
        'cases': len(cases), 'matched': matched, 'sources_sha256': tested,
        'scope': 'FallingBlockEntity.tick landing branch: carried falling-block states into replaceable and '
                 'non-replaceable destinations, concrete powder water contact and solidification, anvil onLand '
                 'level event, moving piston continuation, waterlogged and plant destinations, blocks changed by '
                 'the landing and numeric level events. Movement, impact damage, item drops and block-entity data '
                 'remain separate.'})
    print(f'{len(cases)} falling-block landing cases match ({matched} output lines): {report_path}', flush=True)


if __name__ == '__main__':
    main()
