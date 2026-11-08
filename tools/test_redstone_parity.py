#!/usr/bin/env python3
"""Compare vanilla redstone circuits with horizon-world, tick by tick.

A small floor carries randomly placed redstone components (wire, repeaters,
comparators, torches, observers, pistons, lamps, rails, plates, buttons...). A power
source is placed and later removed through `Level.setBlock` and the scheduled-tick
queue is run for N game ticks; the block changes and pending ticks after every
tick (and the RNG fingerprint at the end) must match. Deterministic border cases
also execute piston extension/retraction events and compare the changed blocks.
"""
import random
try:
    from . import block_update_harness as h
except ImportError:
    import block_update_harness as h

COMPONENTS = [
    'redstone_wire', 'redstone_wire', 'redstone_wire', 'redstone_wire', 'repeater', 'repeater', 'comparator', 'redstone_torch',
    'redstone_wall_torch', 'observer', 'piston', 'sticky_piston', 'redstone_lamp', 'lever', 'stone_button', 'oak_button',
    'stone_pressure_plate', 'note_block', 'target', 'dropper', 'dispenser', 'hopper', 'rail', 'powered_rail', 'activator_rail',
    'detector_rail', 'oak_door', 'iron_door', 'oak_trapdoor', 'oak_fence_gate', 'tnt', 'slime_block', 'honey_block', 'stone',
    'redstone_block', 'daylight_detector', 'trapped_chest', 'copper_bulb', 'sculk_sensor', 'tripwire_hook', 'lectern',
    'oak_slab', 'iron_block', 'glass', 'crafter',
]


def build(facts, rng):
    floor = facts.defaults['minecraft:stone']
    names = [n for n in ('minecraft:' + c for c in COMPONENTS) if n in facts.by_block]
    cells = []
    for x in range(-4, 5):
        for z in range(-4, 5):
            cells.append([x, -1, z, floor])
            if rng.random() < 0.5:
                name = rng.choice(names)
                states = facts.by_block[name]
                state = rng.choice(states)
                cells.append([x, 0, z, state])
                if rng.random() < 0.15:
                    cells.append([x, 1, z, rng.choice(facts.by_block[rng.choice(names)])])
    return cells


def border_cases(facts):
    def state(name, **props):
        return next(i for i in facts.by_block['minecraft:' + name]
                    if all(facts.states[i][4]['properties'].get(k) == str(v) for k, v in props.items()))
    stone = facts.defaults['minecraft:stone']
    power = facts.defaults['minecraft:redstone_block']
    air = facts.defaults['minecraft:air']
    for dx, dz, facing, direction in [(1, 0, 'east', 5), (-1, 0, 'west', 4),
                                       (0, 1, 'south', 3), (0, -1, 'north', 2)]:
        for size in (2, 2.5, 4, 4.5, 8):
            for sticky in (False, True):
                for retract in (False, True):
                    piston = state('sticky_piston' if sticky else 'piston', facing=facing,
                                   extended='true' if retract else 'false')
                    cells = [[0, 64, 0, piston]]
                    if retract:
                        cells.extend([[dx, 64, dz, state('piston_head', facing=facing,
                                      type='sticky' if sticky else 'normal', short='false')],
                                      [dx * 2, 64, dz * 2, stone]])
                    else:
                        cells.extend([[0, 63, 0, power], [dx, 64, dz, stone]])
                    yield dict(seed=1, mode='piston_event', pos=[0, 64, 0], cells=cells,
                               border=[0, 0, size], event=1 if retract else 0, dir=direction)
            # An air source outside the border must also refuse an extension.
            yield dict(seed=1, mode='piston_event', pos=[0, 64, 0],
                       cells=[[0, 64, 0, state('piston', facing=facing, extended='false')],
                              [0, 63, 0, power], [dx, 64, dz, air]],
                       border=[0, 0, size], event=0, dir=direction)


def cases(facts, count, seed, only):
    yield from border_cases(facts)
    rng = random.Random(seed)
    source = facts.defaults['minecraft:redstone_block']
    air = facts.defaults['minecraft:air']
    for n in range(count):
        cells = build(facts, rng)
        at = [rng.randrange(-4, 5), 0, rng.randrange(-4, 5)]
        cells = [c for c in cells if c[:3] != at]
        cells.append([*at, air])
        case = {**h.environment(rng), 'mode': 'run', 'pos': [0, -1, 0], 'cells': cells, 'ops': [
            {'op': 'set', 'pos': at, 'state': source, 'flags': 3},
            {'op': 'ticks', 'n': 12, 'snap': True},
            {'op': 'set', 'pos': at, 'state': air, 'flags': 3},
            {'op': 'ticks', 'n': 12, 'snap': True},
        ]}
        case.pop('raining', None)
        yield case


def main():
    args = h.argument_parser(__doc__).parse_args()
    args.count = min(args.count, 4000) if args.count == 40000 else args.count
    facts = h.load_facts()
    differing = h.execute('redstone-parity', list(cases(facts, args.count, args.seed, args.only)), args, facts)
    h.finish(differing)


if __name__ == '__main__':
    main()
