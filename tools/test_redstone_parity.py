#!/usr/bin/env python3
"""Compare vanilla redstone circuits with horizon-world, tick by tick.

A small floor carries randomly placed redstone components (wire, repeaters,
comparators, torches, observers, pistons, lamps, rails, plates, buttons...). A power
source is placed and later removed through `Level.setBlock` and the scheduled-tick
queue is run for N game ticks; the block changes and pending ticks after every
tick (and the RNG fingerprint at the end) must match. Deterministic border cases
also execute piston extension/retraction events and compare the changed blocks.

Repeated pulses also drive a torch's burnout: a redstone torch extinguished
eight times within 60 game ticks emits level event 1502 and stays dark for 160
ticks, while a slower pulse train (more than 60 ticks for eight), one faster
than the 2-tick delay, or only seven extinguishings does not. `--no-burnout` leaves that family out and
reproduces the corpus before it, byte for byte.
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


def torch_cases(facts):
    """Repeated pulses: the eighth extinguishing inside 60 ticks burns the torch out."""
    def state(name, **props):
        return next(i for i in facts.by_block['minecraft:' + name]
                    if all(facts.states[i][4]['properties'].get(k) == str(v) for k, v in props.items()))
    stone = facts.defaults['minecraft:stone']
    power = facts.defaults['minecraft:redstone_block']

    def case(pulses, spacing, standing=False, facing='east'):
        if standing:
            support, torch = [1, 63, 0], [1, 64, 0]
            cells = [[1, 62, 0, stone], [1, 63, 0, stone],
                     [1, 64, 0, state('redstone_torch', lit='true')]]
        else:
            support, torch = [0, 64, 0], [1, 64, 0]
            cells = [[0, 63, 0, stone], [0, 64, 0, stone],
                     [1, 64, 0, state('redstone_wall_torch', facing=facing, lit='true')]]
        ops = []
        for _ in range(pulses):
            # The torch's own tick runs two ticks after the neighbour change; a
            # pulse shorter than that coalesces and toggles nothing.
            ops.append({'op': 'set', 'pos': support, 'state': power, 'flags': 3})
            ops.append({'op': 'ticks', 'n': spacing, 'snap': True})
            ops.append({'op': 'set', 'pos': support, 'state': stone, 'flags': 3})
            ops.append({'op': 'ticks', 'n': spacing, 'snap': True})
        # A burnt-out torch relights from its 160-tick scheduled tick once the
        # recent-toggle window has emptied; leave enough ticks for that.
        ops.append({'op': 'ticks', 'n': 200, 'snap': True})
        return dict(seed=101, time=1000, mode='run', pos=torch, cells=cells, ops=ops)

    yield case(7, 2)
    yield case(8, 2)
    yield case(8, 4)
    yield case(8, 5)
    yield case(16, 1)
    yield case(7, 2, standing=True)
    yield case(8, 2, standing=True)


def cases(facts, count, seed, only, burnout=True):
    yield from border_cases(facts)
    if burnout:
        yield from torch_cases(facts)
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
    parser = h.argument_parser(__doc__)
    parser.add_argument('--no-burnout', action='store_true',
                        help='leave out the repeated-pulse torch burnout family, as the corpus was before it')
    args = parser.parse_args()
    args.count = min(args.count, 4000) if args.count == 40000 else args.count
    facts = h.load_facts()
    differing = h.execute('redstone-parity',
                          list(cases(facts, args.count, args.seed, args.only, not args.no_burnout)), args, facts)
    h.finish(differing)


if __name__ == '__main__':
    main()
