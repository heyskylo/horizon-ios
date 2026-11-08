#!/usr/bin/env python3
"""Compare where vanilla's nether portals send an entity with Horizon's.

Every case has an entity (an item, a mob, a minecart, an arrow) standing in a
nether portal of one level (the overworld or the Nether), a random destination
around the scaled position (terrain, water, lava, ceilings, other portals) and
a world border, sometimes tight enough to clamp the exit. Vanilla runs
`NetherPortalBlock.getPortalDestination` over two headless levels of the block
update oracle (`VanillaPortalOracle`): `PortalForcer.findClosestPortalPosition`
over the destination's portal points of interest, `createPortal` when there is
none, `BlockUtil.getLargestRectangleAround`, `PortalShape.getRelativePosition`
and `findCollisionFreePosition`. Horizon runs `horizon_world::level::portal_travel`
(`portal_travel_fixture`). Both print the entity's size, the blocks the
destination changed, and the arrival position (bits) and yaw.

The `shape` family searches actual obsidian frames instead of travelling:
`PortalShape.findAnyShape` and `findEmptyPortalShape` from inside, on and
outside valid, clipped, broken and occupied frames on both axes, followed by
`createPortalBlocks`, comparing the rectangle's fields and every changed
block. `--no-shape` leaves the frame family out and reproduces the previous
corpus exactly.
"""
import json
import os
import random
import struct
try:
    from . import block_update_harness as h, lab, test_dispenser_parity as d, test_rust_parity as base
except ImportError:
    import block_update_harness as h
    import lab
    import test_dispenser_parity as d
    import test_rust_parity as base

PARITY_RUST_SCOPE = ['crates/horizon-world/', 'crates/horizon-core/']

# EntityType dimensions (width, height) of the travellers.
ENTITIES = {'minecraft:item': (0.25, 0.25), 'minecraft:zombie': (0.6, 1.95), 'minecraft:skeleton': (0.6, 1.99),
            'minecraft:creeper': (0.6, 1.7), 'minecraft:minecart': (0.98, 0.7), 'minecraft:arrow': (0.5, 0.5),
            'minecraft:horse': (1.3964844, 1.6), 'minecraft:spider': (1.4, 0.9), 'minecraft:ghast': (4.0, 4.0),
            'minecraft:oak_boat': (1.375, 0.5625)}
GROUND = ['stone', 'dirt', 'grass_block', 'netherrack', 'sand', 'gravel', 'oak_planks', 'soul_sand', 'blackstone']
DECOR = ['short_grass', 'snow', 'water', 'lava', 'oak_leaves', 'oak_fence', 'torch', 'glass', 'stone_slab', 'dead_bush']


def portal(states, cells, pois, corner, x_axis, width, height, frame=True):
    """Obsidian around a width by height portal whose bottom left is `corner`."""
    x, y, z = corner
    dx, dz = (1, 0) if x_axis else (0, 1)
    state = states.get('nether_portal', axis='x' if x_axis else 'z')
    for w in range(-1, width + 1):
        for v in range(-1, height + 1):
            at = [x + dx * w, y + v, z + dz * w]
            if 0 <= w < width and 0 <= v < height:
                cells[tuple(at)] = state
                if pois is not None:
                    pois.append(at)
            elif frame:
                cells[tuple(at)] = states.get('obsidian')


def terrain(states, rng, cells, center, nether):
    """Columns around `center`: ground, holes, liquids, plants, a ceiling."""
    cx, cy, cz = center
    lo, hi = (1, 120) if nether else (-60, 300)
    style = rng.choice(['flat', 'flat', 'hills', 'solid', 'empty', 'cave', 'sea'])
    base = max(lo + 2, min(hi - 12, cy + rng.randint(-12, 12)))
    span = rng.choice([8, 20, 24])
    for x in range(cx - span, cx + span + 1):
        for z in range(cz - span, cz + span + 1):
            if style == 'empty':
                continue
            top = base + (rng.randint(-3, 3) if style == 'hills' else 0)
            if style == 'solid':
                for y in range(top - 6, top + 9):
                    cells[(x, y, z)] = states.get(rng.choice(GROUND))
                continue
            if rng.random() < 0.04:
                continue
            for y in range(top - 3, top + 1):
                cells[(x, y, z)] = states.get(rng.choice(GROUND) if y < top else rng.choice(['grass_block', 'stone', 'netherrack']))
            if style == 'sea':
                for y in range(top + 1, top + 3):
                    cells[(x, y, z)] = states.get('water')
            elif rng.random() < 0.08:
                cells[(x, top + 1, z)] = states.get(rng.choice(DECOR))
            if style == 'cave':
                for y in range(top + 4, top + 7):
                    cells[(x, y, z)] = states.get('stone')


def cases(facts, count, seed):
    rng = random.Random(seed)
    states = d.States(facts)
    for n in range(count):
        from_nether = rng.random() < 0.5
        x_axis = rng.random() < 0.5
        width, height = rng.choice([2, 2, 3, 4]), rng.choice([3, 3, 4, 5])
        spread = rng.choice([30, 300, 3000])
        lo, hi = (5, 100) if from_nether else (-50, 250)
        corner = (rng.randint(-spread, spread), rng.randint(lo, hi), rng.randint(-spread, spread))
        source = {}
        portal(states, source, None, corner, x_axis, width, height)
        kind = rng.choice(sorted(ENTITIES))
        dims = ENTITIES[kind]
        dx, dz = (1, 0) if x_axis else (0, 1)
        along = rng.uniform(0, width)
        up = rng.uniform(0, height - 0.1)
        across = rng.uniform(0.1, 0.9)
        position = [corner[0] + dx * along + dz * across, corner[1] + up, corner[2] + dz * along + dx * across]
        entry = [int(position[0] // 1), int(position[1] // 1), int(position[2] // 1)]
        if tuple(entry) not in source:
            entry = list(corner)
        scale = 8.0 if from_nether else 1 / 8
        approx = (int((position[0] * scale) // 1), int(position[1] // 1), int((position[2] * scale) // 1))
        target, pois = {}, []
        terrain(states, rng, target, approx, not from_nether)
        radius = 128 if from_nether else 16
        for _ in range(rng.choice([0, 0, 1, 1, 2, 3])):
            reach = rng.choice([4, radius, radius + 6])
            other = (approx[0] + rng.randint(-reach, reach), approx[1] + rng.randint(-10, 10), approx[2] + rng.randint(-reach, reach))
            portal(states, target, pois, other, rng.random() < 0.5, rng.choice([2, 3]), rng.choice([3, 4]), rng.random() < 0.8)
        if rng.random() < 0.25:
            # A tight border near the exit.
            border = {'x': approx[0] + rng.randint(-20, 20) + 0.5 * rng.randint(0, 1), 'z': approx[2] + rng.randint(-20, 20),
                      'size': rng.choice([4, 9, 16, 30, 60])}
        else:
            border = {'x': 0.0, 'z': 0.0, 'size': 59999968.0}
        yield {'seed': rng.randrange(1 << 40), 'from': 'nether' if from_nether else 'overworld',
               'source': [[*p, s] for p, s in source.items()], 'target': [[*p, s] for p, s in target.items()],
               'pois': pois, 'entry': entry, 'entity': {'type': kind, 'pos': position, 'bits': [struct.pack('>d', v).hex() for v in position], 'dims': list(dims),
                                                                    'reset_forward': kind not in ('minecraft:item', 'minecraft:arrow')}, 'border': border}


def shape_cases(facts, seed):
    """PortalShape frames: valid, clipped, broken and occupied frames searched
    from inside, on and outside the rectangle, on both axes. Each case is one
    frame world and one probe; nothing is mutated before `findEmptyPortalShape`
    and its `createPortalBlocks`."""
    rng = random.Random(seed ^ 0x5348415045)
    states = d.States(facts)
    obsidian = states.get('obsidian')

    def frame_cells(corner, x_axis, width, height, missing=(), interior='air', extra=()):
        x, y, z = corner
        dx, dz = (1, 0) if x_axis else (0, 1)
        cells = {}
        for w in range(-1, width + 1):
            for v in range(-1, height + 1):
                at = (x + dx * w, y + v, z + dz * w)
                if w in (-1, width) or v in (-1, height):
                    if (w, v) not in missing:
                        cells[at] = obsidian
                elif interior == 'nether_portal':
                    cells[at] = states.get('nether_portal', axis='x' if x_axis else 'z')
                elif interior != 'air':
                    cells[at] = states.get(interior)
        for at, name in extra:
            cells[at] = states.get(name)
        return cells

    def cell(corner, x_axis, w, v):
        x, y, z = corner
        dx, dz = (1, 0) if x_axis else (0, 1)
        return (x + dx * w, y + v, z + dz * w)

    def case(cells, probe, x_axis):
        return dict(kind='shape', seed=rng.randrange(1 << 40),
                    source=[[*p, s] for p, s in cells.items()],
                    probe=dict(pos=list(probe), axis='x' if x_axis else 'z'))

    def valid(width, height, x_axis, y):
        corner = (rng.randrange(-20, 21), y, rng.randrange(-20, 21))
        cells = frame_cells(corner, x_axis, width, height)
        for w, v in ((0, 0), (width // 2, height // 2), (width - 1, height - 1)):
            yield case(cells, cell(corner, x_axis, w, v), x_axis)
        # The preferred axis is the frame's other axis, so the search must
        # fall back to the frame's own axis to find it.
        if width != height:
            yield case(cells, cell(corner, x_axis, 0, 0), not x_axis)
            yield case(cells, cell(corner, x_axis, width - 1, height - 1), not x_axis)
        # Just outside the frame, and on its left wall.
        yield case(cells, (corner[0] - 2, corner[1], corner[2]), x_axis)
        yield case(cells, cell(corner, x_axis, -1, 0), x_axis)

    # Every legal size the shape search accepts, plus the seam at 21.
    for width in (2, 3, 4, 5, 21):
        for height in (3, 4, 5, 21):
            for x_axis in (True, False):
                for y in (64, -60, 300):
                    if y - 1 >= -63 and y + height <= 319:
                        yield from valid(width, height, x_axis, y)
    # One block too narrow, too short, and both limits overshot.
    for width, height in ((1, 3), (22, 3), (2, 2), (2, 22), (21, 22), (22, 21)):
        for x_axis in (True, False):
            corner = (4, 64, 4)
            cells = frame_cells(corner, x_axis, width, height)
            for w, v in ((0, 0), (width // 2, height // 2), (width - 1, height - 1)):
                yield case(cells, cell(corner, x_axis, w, v), x_axis)
    # A hole anywhere on the rim, and a missing top row, make the frame invalid.
    for width, height in ((2, 3), (3, 4)):
        for x_axis in (True, False):
            corner = (4, 64, 4)
            holes = [(w, v) for w in range(-1, width + 1) for v in range(-1, height + 1)
                     if w in (-1, width) or v in (-1, height)]
            for hole in holes:
                cells = frame_cells(corner, x_axis, width, height, missing=(hole,))
                yield case(cells, cell(corner, x_axis, 0, 0), x_axis)
            top = tuple((w, height) for w in range(width))
            cells = frame_cells(corner, x_axis, width, height, missing=top)
            yield case(cells, cell(corner, x_axis, 0, 0), x_axis)
    # Occupied and obstructed interiors: fire is empty, portal blocks count.
    for interior in ('stone', 'fire', 'water', 'obsidian', 'nether_portal'):
        for width, height in ((2, 3), (3, 4)):
            for x_axis in (True, False):
                corner = (-2, 64, 7)
                cells = frame_cells(corner, x_axis, width, height, interior=interior)
                for w, v in ((0, 0), (width // 2, height // 2)):
                    yield case(cells, cell(corner, x_axis, w, v), x_axis)
    # A floor, a ceiling and blocks around the frame change the bottom-left search.
    for x_axis in (True, False):
        corner = (10, 64, 10)
        for extra in (
            [((10, 63, 10), 'stone')],
            [((10, 68, 10), 'stone')],
            [((8, 64, 10), 'stone')],
            [((8, 65, 10), 'stone'), ((8, 66, 10), 'stone')],
            [((10, 64, 8), 'obsidian')],
            [((12, 64, 10), 'crying_obsidian')],
            [((10, 64, 10), 'fire')],
        ):
            cells = frame_cells(corner, x_axis, 3, 4, extra=extra)
            yield case(cells, cell(corner, x_axis, 1, 1), x_axis)
    # A non-obsidian rim, which is not a portal frame.
    for x_axis in (True, False):
        cells = frame_cells((3, 70, 3), x_axis, 3, 4)
        for at in list(cells):
            cells[at] = states.get('stone')
        yield case(cells, cell((3, 70, 3), x_axis, 1, 1), x_axis)
    # Random frames: holes, obstructions and probes at every edge.
    for _ in range(240):
        width, height = rng.randrange(1, 23), rng.randrange(1, 23)
        x_axis = rng.random() < 0.5
        y = rng.randrange(-60, 316 - height)
        corner = (rng.randrange(-40, 41), y, rng.randrange(-40, 41))
        holes = set()
        for _ in range(rng.randrange(4)):
            w = rng.choice([-1, width, rng.randrange(width)])
            v = rng.choice([-1, height, rng.randrange(height)])
            holes.add((w, v))
        interior = rng.choice(['air', 'air', 'stone', 'fire', 'water', 'nether_portal'])
        extra = [((corner[0] + rng.randrange(-3, 4), corner[1] - 1, corner[2] + rng.randrange(-3, 4)),
                  rng.choice(['stone', 'dirt', 'obsidian']))]
        cells = frame_cells(corner, x_axis, width, height, missing=holes, interior=interior, extra=extra)
        w, v = rng.choice([(0, 0), (width - 1, height - 1), (rng.randrange(width), rng.randrange(height)),
                           (-2, 0), (width + 1, 0), (0, -2), (0, height + 1)])
        yield case(cells, cell(corner, x_axis, w, v), x_axis)


def main():
    parser = h.argument_parser(__doc__)
    parser.add_argument('--no-shape', action='store_true',
                        help='leave out the PortalShape frame family, reproducing the previous corpus')
    args = parser.parse_args()
    if args.count == 40000:
        args.count = 1500
    lock, jars, libraries = base.verify_artifacts()
    facts = h.load_facts()
    resources = lab.LOCAL / 'horizon-client' / lock['minecraft']['version']
    work = lab.LOCAL / 'nether-portal-parity'
    work.mkdir(parents=True, exist_ok=True)
    all_cases = list(cases(facts, args.count, args.seed))
    if not args.no_shape:
        all_cases += list(shape_cases(facts, args.seed))
    shapes = sum(case.get('kind') == 'shape' for case in all_cases)
    (work / 'cases.jsonl').write_text(''.join(json.dumps(case) + '\n' for case in all_cases))
    classes = work / 'classes'
    classes.mkdir(exist_ok=True)
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    java_root = lab.ROOT / 'mods/horizon/src/test/java'
    h.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none', '-sourcepath', str(java_root), '-nowarn',
           '-cp', dependencies, '-d', str(classes), str(java_root / 'dev/horizonlab/parity/VanillaPortalOracle.java')])
    h.run(['java', '-Xmx3G', '-cp', str(classes) + os.pathsep + dependencies, 'dev.horizonlab.parity.VanillaPortalOracle',
           str(resources), str(work / 'cases.jsonl'), str(work / 'native.txt')])
    h.run(['cargo', 'run', '-q', '--release', '-p', 'horizon-world', '--example', 'world_probe', '--',
           'portal_travel_fixture', str(resources), str(work / 'cases.jsonl'), str(work / 'rust.txt')], cwd=lab.ROOT)
    native = h.split_cases((work / 'native.txt').read_text(), ())
    rust = h.split_cases((work / 'rust.txt').read_text(), ())
    assert len(native) == len(rust) == len(all_cases), (len(native), len(rust), len(all_cases))
    groups, errors = {}, {}
    created = found = 0
    for case, a, b in zip(all_cases, native, rust):
        if a and a[0].startswith('error'):
            errors.setdefault(a[0], []).append(case)
            continue
        shape = case.get('kind') == 'shape'
        if shape:
            key = 'shape'
        else:
            if any(line.startswith('set ') for line in a):
                created += 1
            elif not any(line == 'dest none' for line in a):
                found += 1
            key = f"{case['from']} {case['entity']['type']} {'pois' if case['pois'] else 'no pois'}"
        if sorted(a) != sorted(b):
            groups.setdefault(key, []).append((case, a, b))
    for name, entries in sorted(errors.items(), key=lambda e: -len(e[1])):
        print(f'oracle error x{len(entries)}: {name}')
    for name, entries in sorted(groups.items(), key=lambda e: -len(e[1])):
        print(f'{name}: {len(entries)} differing')
        for case, a, b in entries[:args.show]:
            if case.get('kind') == 'shape':
                print(f"  shape probe={case['probe']} cells={len(case['source'])}")
            else:
                print(f"  case entry={case['entry']} entity={case['entity']} border={case['border']} pois={len(case['pois'])}")
            print('   native ' + ' | '.join(sorted(a))[:1500])
            print('   rust   ' + ' | '.join(sorted(b))[:1500])
    total = sum(len(v) for v in groups.values())
    skipped = sum(len(v) for v in errors.values())
    shape_note = f'; {shapes} of them PortalShape frame searches' if shapes else ''
    print(f'{len(all_cases) - total - skipped}/{len(all_cases) - skipped} cases match ({skipped} oracle errors); '
          f'vanilla linked to an existing portal {found} times and built one {created} times{shape_note}')
    h.finish(total + skipped)


if __name__ == '__main__':
    main()
