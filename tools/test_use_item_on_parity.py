#!/usr/bin/env python3
"""Compare a player's right click on a block with vanilla's, for items that are
both food and blocks (sweet and glow berries, carrots, potatoes), for sweet
berry bushes, for spawn eggs used on a monster spawner, for shears carving
pumpkins or cropping a growing plant's head (kelp, weeping and twisting vines,
cave vines) to its top age, for copper golem statues turned to their next pose,
for blocks placed where mobs and armor stands stand, for respawn anchors, for
beds in the Nether and the End, and for doors, trapdoors and fence gates, which a
hand opens unless their material cannot be (`BlockSetType.canOpenByHand`).

Axe, hoe and shovel clicks use the data-driven `minecraft:block_transformer`
registry (`ItemStack.useOn` -> `Item.useOn` -> `BlockTransformer.transformBlock`):
logs stripped and copper scraped, waxed or unwaxed, a double copper chest's other
half told of the change, copper doors and trapdoors, farmland and dirt paths, and
the hanging roots a hoe leaves on rooted dirt's clicked face. The states those
transforms produce are compared by `test_block_transformer_parity.py`; here the
click that uses one is.

Dragon eggs, which a click moves somewhere else nearby
(`DragonEggBlock.useWithoutItem`).

Every case has a block at the origin, something above it, and a player three
blocks south, survival or creative, hungry or full, sneaking or not, who
right-clicks the top face of the origin (for pumpkins, any face, turned any
way) holding an item, sometimes with another (or nothing) in the main hand
and the item in the off hand, the main hand trying first. Vanilla runs the
real `ServerPlayerGameMode.useItemOn` over the headless level of the
dispenser harness (`VanillaDispenserOracle.useItem`): the block's `useItemOn`
and `useWithoutItem` (picking a bush's berries, `SweetBerryBushBlock.useItemOn`
passing bone meal on, `PumpkinBlock.useItemOn` carving with shears and
dropping the seeds from the carved side), then `ItemStack.useOn`
(`BlockItem.place` with its `mayPlaceOn` rules, `BoneMealItem.useOn`,
`ShearsItem.useOn` cropping a growing plant's head, eating a consumable that
could not be placed, and `SpawnEggItem.useOn` setting a spawner's mob). Respawn
anchors are charged with glowstone, set the respawn point in the Nether and
explode elsewhere (`RespawnAnchorBlock`, through the stand-in level's real
`ServerExplosion`, with water at the anchor). Respawn cases call
`ServerPlayer.findRespawnAndUseSpawnBlock` on an anchor among blocks
(`RespawnAnchorBlock.findStandUpPosition` through `DismountHelper`): the
position, the yaw that faces the anchor and the charge it uses; others on a
bed's head set from some yaw (`AbstractBedBlock.findStandUpPosition`: intact,
bunk, only the foot left or gone, crowded around and on top). Villagers lying
in such beds wake up from some yaw (`Villager.stopSleeping`: the same search for
a villager's box, with shelves among the blocks around, #9502). Beds and straw
beds clicked (head or foot, whole or broken) in the Nether and the End blow up
or fall apart (`AbstractBedBlock.useWithoutItem` under the dimension's
`bed_rule`: `BedBlock.destroyOnUse`, `StrawBedBlock.destroyBed`). Horizon runs
`use_item_as_player` on that hit in a headless game
(`HORIZON_DISPENSE_FIXTURE`). Both print block changes, dropped items, level
events, sounds, game events, the used statistic, the held stack, the item the
player started using, spawners' next spawn data, potentials and delay, the
off hand's stack,
player's respawn point, system messages and the level random afterwards.
"""
import json
import os
import random
try:
    from . import block_update_harness as h, lab, test_dispenser_parity as d, test_rust_parity as base
except ImportError:
    import block_update_harness as h
    import lab
    import test_dispenser_parity as d
    import test_rust_parity as base

# Blocks the item is used on: the supports of the planted blocks and others.
GROUNDS = ['grass_block', 'dirt', 'coarse_dirt', 'podzol', 'rooted_dirt', 'moss_block', 'pale_moss_block', 'mud',
           'muddy_mangrove_roots', 'farmland', 'stone', 'sand', 'gravel', 'clay', 'snow_block', 'oak_planks']
ITEMS = ['sweet_berries', 'sweet_berries', 'sweet_berries', 'glow_berries', 'carrot', 'potato', 'bone_meal',
         'wheat_seeds', 'beetroot_seeds', 'apple']
# What is in the way above the clicked block.
ABOVE = ['air'] * 8 + ['short_grass', 'snow', 'stone', 'water', 'dead_bush', 'fern']
EGGS = ['zombie', 'skeleton', 'spider', 'cave_spider', 'pig', 'creeper', 'blaze', 'ghast', 'villager', 'bee', 'cod']
# What stands around an anchor that explodes.
AROUND = ['stone', 'dirt', 'oak_planks', 'cobblestone', 'glass', 'obsidian', 'netherrack', 'water', 'tnt', 'oak_log',
          'grass_block', 'soul_sand', 'gravel', 'crying_obsidian']


STAND = ['air'] * 6 + ['stone', 'stone', 'oak_slab', 'snow', 'lava', 'magma_block', 'fire', 'cactus', 'water',
         'ladder', 'oak_trapdoor', 'short_grass', 'white_carpet', 'campfire', 'sweet_berry_bush',
         'oak_fence', 'powder_snow', 'farmland', 'soul_sand', 'cobweb', 'end_portal']


def respawn_case(rng, states, case):
    """Respawning at an anchor with blocks around it."""
    charges = rng.choice([0, 1, 1, 2, 4])
    cells = [[0, 0, 0, states.get('respawn_anchor', charges=str(charges))]]
    for dy in (-2, -1, 0, 1, 2):
        for dx in range(-1, 2):
            for dz in range(-1, 2):
                if (dx, dy, dz) == (0, 0, 0):
                    continue
                if dy == -1 or dy == -2:
                    name = rng.choice(['stone'] * 6 + STAND)
                else:
                    name = rng.choice(['air'] * 3 + STAND)
                if name == 'air':
                    continue
                state = states.any(rng, name) if name in ('oak_trapdoor', 'oak_slab', 'snow', 'campfire', 'water', 'ladder') else states.get(name)
                cells.append([dx, dy, dz, state])
    if rng.random() < 0.6:
        case['dim'] = 'nether'
    case['respawn_at'] = {'forced': rng.random() < 0.2, 'consume': rng.random() < 0.8}
    case['cells'] = cells


BED_COLORS = ['red', 'white', 'black', 'cyan', 'lime']
FACINGS = {'north': (0, -1), 'south': (0, 1), 'west': (-1, 0), 'east': (1, 0)}
OPPOSITE = {'north': 'south', 'south': 'north', 'west': 'east', 'east': 'west'}


def bed_respawn_case(rng, states, case):
    """Respawning at a bed's head set from some yaw: intact, a bunk bed, only the foot left or
    gone, with blocks around and on it, in the Overworld (and a few in the Nether or the End)."""
    name = 'straw_bed' if rng.random() < 0.15 else rng.choice(BED_COLORS) + '_bed'
    cells = bed_cells(rng, states, name, STAND)
    if rng.random() < 0.1:
        case['dim'] = rng.choice(['nether', 'end'])
    yaw = rng.choice([0.0, 90.0, -90.0, 180.0, round(rng.uniform(-720, 720), rng.choice([0, 2]))])
    case['respawn_at'] = {'forced': rng.random() < 0.15, 'consume': rng.random() < 0.8, 'yaw': yaw}
    case['cells'] = cells


def bed_cells(rng, states, name, stand):
    """A bed `name` at the origin: intact, a bunk bed, only the foot left or gone, with blocks from
    `stand` around and on it."""
    facing = rng.choice(list(FACINGS))
    fx, fz = FACINGS[facing]
    bed = {}
    kept = rng.random()
    if kept < 0.75:
        bed[(0, 0, 0)] = states.get(name, facing=facing, part='head', occupied='false')
    elif kept < 0.85:
        bed[(0, 0, 0)] = states.get(rng.choice(['stone', 'white_carpet', 'oak_slab']))
    if kept < 0.9 or rng.random() < 0.5:
        bed[(-fx, 0, -fz)] = states.get(name, facing=facing, part='foot', occupied='false')
    if rng.random() < 0.2:
        # A bunk bed: another bed below.
        bed[(0, -1, 0)] = states.get(name, facing=facing, part='head', occupied='false')
        bed[(-fx, -1, -fz)] = states.get(name, facing=facing, part='foot', occupied='false')
    crowd = rng.choice([0.0, 0.1, 0.3, 0.6, 0.95])
    cells = [[*k, v] for k, v in bed.items()]
    if (0, 0, 0) not in bed:
        cells.insert(0, [0, 0, 0, states.get('air')])
    for dy in (-2, -1, 0, 1, 2):
        for dx in range(-3, 4):
            for dz in range(-3, 4):
                if (dx, dy, dz) in bed or (dx, dy, dz) == (0, 0, 0):
                    continue
                if dy == -1 and (dx, 0, dz) in bed:
                    name2 = 'stone'
                elif dy < 0:
                    name2 = rng.choice(['stone'] * 8 + stand)
                elif rng.random() < crowd:
                    name2 = rng.choice(['stone'] * 4 + stand)
                else:
                    continue
                if name2 == 'air':
                    continue
                varied = name2 in ('oak_trapdoor', 'oak_slab', 'snow', 'campfire', 'water', 'ladder') or name2.endswith('_shelf')
                state = states.any(rng, name2) if varied else states.get(name2)
                cells.append([dx, dy, dz, state])
    return cells


# What a villager waking up may stand on or beside: shelves (#9502) beside the blocks a player
# stands up on.
WAKE_STAND = STAND + ['oak_shelf', 'spruce_shelf', 'bamboo_shelf', 'oak_shelf']


def wake_case(rng, states, case):
    """A villager lying in a bed (`Villager.stopSleeping`) wakes up from some yaw: the bed intact, a
    bunk bed, only the foot left or gone, with blocks and shelves around and on it."""
    case['cells'] = bed_cells(rng, states, rng.choice(BED_COLORS) + '_bed', WAKE_STAND)
    yaw = rng.choice([0.0, 90.0, -90.0, 180.0, round(rng.uniform(-720, 720), rng.choice([0, 2]))])
    case['wake'] = {'yaw': yaw}


def bed_case(rng, states, case):
    """A bed (or straw bed) clicked in the Nether or the End, among blocks."""
    name = 'straw_bed' if rng.random() < 0.25 else rng.choice(BED_COLORS) + '_bed'
    facing = rng.choice(list(FACINGS))
    dx, dz = FACINGS[facing]
    # The clicked block is the origin; the other half lies along the facing.
    part = rng.choice(['head', 'foot'])
    other = (-dx, 0, -dz) if part == 'head' else (dx, 0, dz)
    cells = [[0, 0, 0, states.get(name, facing=facing, part=part, occupied='false')],
             [0, -1, 0, states.get(rng.choice(['netherrack', 'end_stone', 'obsidian', 'stone']))],
             [other[0], -1, other[2], states.get('stone')]]
    if rng.random() < 0.9:
        cells.append([*other, states.get(name, facing=facing, part='foot' if part == 'head' else 'head', occupied='false')])
    taken = {tuple(c[:3]) for c in cells}
    for _ in range(rng.choice([0, 3, 10, 30])):
        p = (rng.randrange(-3, 4), rng.randrange(-2, 3), rng.randrange(-3, 4))
        if p in taken:
            continue
        taken.add(p)
        cells.append([*p, states.get(rng.choice(AROUND))])
    case['dim'] = rng.choice(['nether', 'end'])
    item = rng.choice(['-', '-', 'apple', 'stone', 'glowstone'])
    case['use_item'] = {'item': '-' if item == '-' else d_plain(item, 1), 'creative': rng.random() < 0.2,
                        'sneaking': rng.random() < 0.1, 'food': 20, 'distance': 12}
    if rng.random() < 0.2:
        case['rules'] = {'block_explosion_drop_decay': False}
    case['cells'] = cells


def anchor_case(rng, states, case):
    """A respawn anchor clicked in the Nether or the Overworld, with blocks
    and water around it."""
    charges = rng.choice([0, 0, 1, 2, 3, 4, 4])
    cells = [[0, 0, 0, states.get('respawn_anchor', charges=str(charges))],
             [0, -1, 0, states.get(rng.choice(['netherrack', 'stone', 'soul_sand', 'obsidian', 'soul_soil']))]]
    taken = {(0, 0, 0), (0, -1, 0)}
    for _ in range(rng.choice([0, 3, 8, 20, 40])):
        p = (rng.randrange(-3, 4), rng.randrange(-2, 3), rng.randrange(-3, 4))
        if p in taken:
            continue
        taken.add(p)
        name = rng.choice(AROUND)
        state = states.any(rng, 'water') if name == 'water' and rng.random() < 0.5 else states.get(name)
        cells.append([*p, state])
    if rng.random() < 0.2:
        # Water that would flow into the anchor's place.
        d = rng.choice([(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)])
        cells = [c for c in cells if tuple(c[:3]) != d]
        cells.append([*d, states.any(rng, 'water') if rng.random() < 0.4 else states.get('water')])
    nether = rng.random() < 0.5
    if nether:
        case['dim'] = 'nether'
    item = rng.choice(['glowstone'] * 5 + ['apple', '-', 'stone', 'glowstone_dust'])
    count = rng.choice([1, 1, 16, 64])
    use = {'item': '-' if item == '-' else d_plain(item, count), 'creative': rng.random() < 0.2,
           'sneaking': rng.random() < 0.1, 'food': 20,
           # Away from the blast: the stand-in player is not an entity of the level.
           'distance': 3 if nether else 12}
    if rng.random() < 0.3:
        use['respawn'] = rng.choice([[0, 0, 0], [5, 0, 5]])
    if rng.random() < 0.4:
        # Glowstone in the off hand: the main hand passes it on (PASS).
        use['offhand'] = d_plain(rng.choice(['glowstone', 'glowstone', 'apple']), rng.choice([1, 16]))
    if rng.random() < 0.2:
        case['rules'] = {'block_explosion_drop_decay': False}
    case['use_item'] = use
    case['cells'] = cells


def pumpkin_case(rng, states, case):
    """Shears (or something else) used on a pumpkin (`PumpkinBlock.useItemOn`):
    any face, the player turned any way, worn shears (the stand-in player
    has no entity id for the break event, so they keep one use)."""
    cells = [[0, 0, 0, states.get(rng.choice(['pumpkin'] * 6 + ['carved_pumpkin', 'melon']))]]
    if rng.random() < 0.3:
        cells.append([0, 1, 0, states.get(rng.choice(['stone', 'snow', 'short_grass']))])
    damage = rng.choice([0, 0, 0, 100, 236])
    shears = d_plain('shears', 1) if damage == 0 else \
        '{id:"minecraft:shears",count:1,components:{"minecraft:damage":%d}}' % damage
    item = rng.choice([shears] * 6 + [d_plain('stick', 1), d_plain('pumpkin', 3), '-'])
    use = {'item': item, 'creative': rng.random() < 0.2, 'sneaking': rng.random() < 0.15, 'food': 20,
           'yaw': rng.choice([0, 30, 90, 135, 180, 200, 270, 315, -45, -100]),
           'face': rng.choice(['up', 'up', 'down', 'north', 'south', 'west', 'east'])}
    if rng.random() < 0.3:
        # Shears in the off hand, something else or nothing in the main hand.
        use.update(offhand=item, item=rng.choice(['-', '-', d_plain('stick', 1), d_plain('stone', 16)]))
    case['use_item'] = use
    case['cells'] = cells


# Growing plant heads (kelp, the two nether vines, cave vines) and the plants' bodies.
HEADS = ['kelp', 'weeping_vines', 'twisting_vines', 'cave_vines']
BODIES = ['kelp_plant', 'weeping_vines_plant', 'twisting_vines_plant', 'cave_vines_plant']


def crop_case(rng, states, case):
    """Shears (or something else) on a growing plant's head (`ShearsItem.useOn`): a head below its
    top age is cropped to `GrowingPlantHeadBlock.getMaxAgeState` (age 25) with a
    `block.growing_plant.crop` sound, a `block_change` game event and one point of wear; a head
    already at 25, or a plant's body (`GrowingPlantBodyBlock`), takes nothing (`Item.useOn`)."""
    head = rng.random() < 0.85
    name = rng.choice(HEADS) if head else rng.choice(BODIES)
    props = {}
    if name in ('cave_vines', 'cave_vines_plant'):
        # Berry-less: a berry-bearing vine picks its berries first (`useWithoutItem`), which is
        # another arm's.
        props['berries'] = 'false'
    if head:
        # Heads carry `AGE_25`; the bodies below them carry no age at all.
        props['age'] = str(rng.choice([0, 3, 11, 24, 25, 25]))
    state = states.get(name, **props) if props else states.any(rng, name)
    cells = [[0, 0, 0, state]]
    if rng.random() < 0.3:
        cells.append([0, -1, 0, states.get(rng.choice(['stone', 'dirt', 'sand', 'water']))])
    damage = rng.choice([0, 0, 0, 100, 236])
    shears = d_plain('shears', 1) if damage == 0 else \
        '{id:"minecraft:shears",count:1,components:{"minecraft:damage":%d}}' % damage
    item = rng.choice([shears] * 6 + [d_plain('stick', 1), d_plain('stick', 1), '-'])
    use = {'item': item, 'creative': rng.random() < 0.2, 'sneaking': rng.random() < 0.15, 'food': 20}
    if rng.random() < 0.2:
        # Shears in the off hand, something else or nothing in the main hand.
        use.update(offhand=item, item=rng.choice(['-', d_plain('stick', 1)]))
    case['use_item'] = use
    case['cells'] = cells


WIRE_SIDES = ['none', 'side', 'up']


def wire_case(rng, states, case):
    """A redstone wire clicked with an empty hand (`RedstoneWireBlock.useWithoutItem`): a wire that
    is neither a `cross` (connected to all four sides) nor a `dot` (connected to none) passes the
    click on, and one that is either takes the opposite shape, keeping its `POWER` and its
    connections, which `getConnectionState` recomputes while `updatesOnShapeChange` notifies the
    neighbours whose connection changed. Nothing is played and no game event is sent."""
    props = dict(zip(('north', 'east', 'south', 'west'), (rng.choice(WIRE_SIDES) for _ in range(4))))
    props['power'] = str(rng.choice([0, 0, 1, 7, 15]))
    cells = [[0, 0, 0, states.get('redstone_wire', **props)],
             [0, -1, 0, states.get(rng.choice(['stone', 'dirt', 'oak_planks', 'grass_block']))]]
    for direction, at in (('north', (0, 0, -1)), ('east', (1, 0, 0)), ('south', (0, 0, 1)), ('west', (-1, 0, 0))):
        if rng.random() < 0.6:
            name = rng.choice(['redstone_wire', 'redstone_wire', 'lever', 'stone', 'repeater', 'redstone_block'])
            # Wire, levers and repeaters have too many states to name one; stone and
            # a redstone block have a single default.
            varied = ('redstone_wire', 'lever', 'repeater')
            cells.append([*at, states.any(rng, name) if name in varied else states.get(name)])
    case['use_item'] = {'item': '-', 'creative': rng.random() < 0.2, 'sneaking': rng.random() < 0.15,
                        'food': 20, 'face': rng.choice(['up', 'north', 'south', 'west', 'east'])}
    case['cells'] = cells


STATUES = [f'{wax}{age}copper_golem_statue' for wax in ('', 'waxed_')
           for age in ('', 'exposed_', 'weathered_', 'oxidized_')]


def statue_case(rng, states, case):
    """A copper golem statue right-clicked (`CopperGolemStatueBlock.useItemOn`, the weathering
    one's): anything but an axe, an empty hand too, turns it to its next pose; honeycomb waxes a
    statue that still weathers instead (`HoneycombItem.useOn`), and an axe scrapes or unwaxes it."""
    statue = rng.choice(STATUES)
    case['cells'] = [[0, 0, 0, states.any(rng, statue)]]
    items = ['-', '-', 'stick', 'honeycomb', 'honeycomb', 'stone', 'copper_ingot', 'diamond_pickaxe',
             'copper_golem_statue']
    if statue != 'copper_golem_statue':
        # An axe scrapes or unwaxes the others; on the unaffected one it brings the golem back
        # (`CopperGolemStatueBlockEntity.removeStatue`), which Horizon does not do yet.
        items.append('iron_axe')
    item = rng.choice(items)
    use = {'item': '-' if item == '-' else d_plain(item, rng.choice([1, 16])), 'creative': rng.random() < 0.2,
           'sneaking': rng.random() < 0.15, 'food': 20,
           'face': rng.choice(['up', 'up', 'north', 'south', 'west', 'east'])}
    if rng.random() < 0.2:
        # The item in the off hand, nothing in the main hand.
        use.update(offhand=use['item'], item='-')
    case['use_item'] = use


def timed_case(rng, states, case):
    """A repeater, a comparator or a daylight detector clicked with an empty hand, none of which may
    be turned where `Player.getAbilities().mayBuild` is false (`RepeaterBlock.useWithoutItem`,
    `ComparatorBlock.useWithoutItem`, `DaylightDetectorBlock.useWithoutItem` all answer PASS there).
    A repeater cycles its `delay` through 1 to 4 with `setBlockAndUpdate` and nothing else; a
    comparator cycles `mode` between `compare` and `subtract` with a `block.comparator.click` at
    volume 0.3 and pitch 0.55 for `subtract` / 0.5 for `compare`, `Level.setBlock` writes the state
    and the comparator refreshes its output; a daylight detector cycles `inverted`, writes the state,
    sends `block_change` and recomputes the `power` it emits."""
    name = rng.choice(['repeater'] * 2 + ['comparator'] * 2 + ['daylight_detector'])
    props = {'facing': rng.choice(['north', 'south', 'west', 'east'])}
    if name == 'repeater':
        props.update(delay=str(rng.randrange(1, 5)), locked=rng.choice(['true', 'false']))
    elif name == 'comparator':
        props.update(mode=rng.choice(['compare', 'subtract']))
    else:
        props.pop('facing')
        props.update(inverted=rng.choice(['true', 'false']), power=str(rng.randrange(16)))
    cells = [[0, 0, 0, states.get(name, **props)],
             [0, -1, 0, states.get(rng.choice(['stone', 'dirt', 'oak_planks', 'grass_block']))]]
    if name != 'daylight_detector' and rng.random() < 0.5:
        # Something for the diode to read, on the side it faces.
        face = {'north': (0, 0, -1), 'south': (0, 0, 1), 'west': (-1, 0, 0), 'east': (1, 0, 0)}[props['facing']]
        cells.append([*face, states.get(rng.choice(['redstone_wire', 'lever', 'stone', 'redstone_block']))])
    case['use_item'] = {'item': '-', 'creative': rng.random() < 0.2, 'sneaking': rng.random() < 0.15,
                        'food': 20, 'face': rng.choice(['up', 'up', 'north', 'south', 'west', 'east'])}
    case['cells'] = cells


# Entities a block may be placed into (`Entity.blocksBuilding`), and blocks to place.
BLOCKERS = ['skeleton', 'zombie', 'pig', 'chicken', 'cow', 'armor_stand', 'slime', 'bat']
PLACED = ['obsidian', 'stone', 'oak_slab', 'oak_fence', 'white_carpet', 'torch', 'glass', 'oak_trapdoor', 'snow']


def blocked_case(rng, states, case):
    """A block placed on top of the origin where a mob or an armor stand stands, in its box or beside it
    (`BlockItem.canPlace`'s `Level.isUnobstructed`)."""
    case['cells'] = [[0, 0, 0, states.get(rng.choice(['stone', 'grass_block', 'dirt']))]]
    entities = []
    for _ in range(rng.choice([1, 1, 2])):
        kind = rng.choice(BLOCKERS)
        offset = [rng.choice([0, 0, 0.3, -0.45, 0.7, -1.2]), rng.choice([0, 0, 0.5, 0.95, 1.02, -0.5]),
                  rng.choice([0, 0, 0.3, -0.6, 0.9])]
        e = {'type': f'minecraft:{kind}', 'pos': [0.5 + offset[0], 1 + offset[1], 0.5 + offset[2]]}
        if kind == 'armor_stand' and rng.random() < 0.3:
            e['marker'] = True
        if kind in ('pig', 'cow', 'chicken') and rng.random() < 0.3:
            e['baby'] = True
        entities.append(e)
    case['entities'] = entities
    case['use_item'] = {'item': d_plain(rng.choice(PLACED), rng.choice([1, 16])), 'creative': rng.random() < 0.3,
                        'sneaking': False, 'food': 20}


BUTTONS = ['stone_button', 'oak_button', 'spruce_button', 'birch_button', 'jungle_button', 'acacia_button',
           'cherry_button', 'dark_oak_button', 'mangrove_button', 'pale_oak_button', 'bamboo_button',
           'crimson_button', 'warped_button', 'polished_blackstone_button']


def switch_case(rng, states, case):
    """A lever or a button clicked with an empty hand. `LeverBlock.useWithoutItem` always answers
    SUCCESS and calls `pull`, which cycles `powered` both ways, writes it with `setBlockAndUpdate`,
    updates the neighbours at the lever and at the block it hangs on, plays `block.lever.click` at
    volume 0.3 with pitch 0.6 when the lever is now on and 0.5 when it is off, and sends
    `block_activate` or `block_deactivate` (`LeverBlock.pull`, `LeverBlock.playSound`).
    `ButtonBlock.useWithoutItem` answers CONSUME and touches nothing while the button is `powered`,
    and otherwise `ButtonBlock.press`es it: `powered` on, the same neighbour updates, a release
    scheduled `ticksToStayPressed` away (the block set type's own), the button type's own sound and
    `block_activate`. Neither reads the held item, the hit or the game mode, so the case varies the
    `face` and `facing` the block is attached by, every button type and `powered` either way."""
    name = rng.choice(['lever'] * 2 + BUTTONS)
    props = {'facing': rng.choice(['north', 'south', 'west', 'east']),
             'face': rng.choice(['floor', 'wall', 'ceiling']),
             'powered': rng.choice(['true', 'false'])}
    cells = [[0, 0, 0, states.get(name, **props)],
             [0, -1, 0, states.get(rng.choice(['stone', 'dirt', 'oak_planks', 'grass_block']))]]
    case['use_item'] = {'item': '-', 'creative': rng.random() < 0.2, 'sneaking': rng.random() < 0.15,
                        'food': 20, 'face': rng.choice(['up', 'up', 'north', 'south', 'west', 'east', 'down'])}
    case['cells'] = cells


def jukebox_case(rng, states, case):
    """A jukebox placed from a Creative pick that carries a record: the block entity takes the disc
    and `JukeboxBlock.setPlacedBy` sets `has_record`, so the model shows it and a right click pops it
    out. Without the state the box is empty while it holds one."""
    case['cells'] = [[0, 0, 0, states.get(rng.choice(['stone', 'dirt', 'grass_block']))]]
    data = ''
    if rng.random() < 0.75:
        record = rng.choice(['music_disc_cat', 'music_disc_13', 'music_disc_pigstep'])
        data = ',components:{"minecraft:block_entity_data":{id:"minecraft:jukebox",' \
               'RecordItem:{id:"minecraft:%s",count:1}}}' % record
    case['use_item'] = {'item': '{id:"minecraft:jukebox",count:1%s}' % data,
                        'creative': True, 'sneaking': False, 'food': 20}


# Doors, trapdoors and fence gates of every material, and the ones whose `BlockSetType` cannot be
# opened by hand (`DoorBlock.useWithoutItem`'s `type.canOpenByHand`).
DOORS = ['oak', 'spruce', 'birch', 'jungle', 'acacia', 'dark_oak', 'mangrove', 'cherry', 'pale_oak', 'poplar',
         'bamboo', 'crimson', 'warped', 'copper', 'exposed_copper', 'weathered_copper', 'oxidized_copper',
         'waxed_copper', 'iron']
GATES = ['oak', 'spruce', 'birch', 'jungle', 'acacia', 'dark_oak', 'mangrove', 'cherry', 'pale_oak', 'poplar',
         'bamboo', 'crimson', 'warped']


def openable_case(rng, states, case):
    """A door, trapdoor or fence gate right-clicked by hand (`DoorBlock.useWithoutItem`,
    `TrapDoorBlock.useWithoutItem`, `FenceGateBlock.useWithoutItem`): the ones whose
    `BlockSetType.canOpenByHand` turn to their other `open` with their open or close sound and
    `block_open`/`block_close`, a waterlogged trapdoor also schedules the water again, a fence gate
    closed across the player turns to face them first (`player.getDirection`) and draws its sound's
    pitch, and an iron one passes, so what the hand holds is placed against it instead."""
    kind = rng.random()
    facing = rng.choice(list(FACINGS))
    if kind < 0.4:
        material = rng.choice(DOORS)
        props = {'facing': facing, 'half': 'lower', 'hinge': rng.choice(['left', 'right']),
                 'open': rng.choice(['false', 'false', 'true']), 'powered': rng.choice(['false', 'true'])}
        cells = [[0, 0, 0, states.get(material + '_door', **props)],
                 [0, 1, 0, states.get(material + '_door', **{**props, 'half': 'upper'})]]
        if rng.random() < 0.5:
            cells.append([0, -1, 0, states.get(rng.choice(['stone', 'dirt', 'oak_planks']))])
        face = rng.choice([facing, OPPOSITE[facing], 'up'])
    elif kind < 0.75:
        material = rng.choice(DOORS)
        cells = [[0, 0, 0, states.get(material + '_trapdoor', facing=facing, half=rng.choice(['bottom', 'top']),
                                      open=rng.choice(['false', 'false', 'true']),
                                      powered=rng.choice(['false', 'true']),
                                      waterlogged=rng.choice(['false', 'false', 'false', 'true']))]]
        if rng.random() < 0.5:
            cells.append([0, -1, 0, states.get(rng.choice(['stone', 'dirt', 'oak_planks']))])
        face = rng.choice([facing, OPPOSITE[facing], 'up', 'down'])
    else:
        material = rng.choice(GATES)
        cells = [[0, 0, 0, states.get(material + '_fence_gate', facing=facing,
                                      open=rng.choice(['false', 'false', 'true']),
                                      powered=rng.choice(['false', 'true']),
                                      in_wall=rng.choice(['false', 'false', 'true']))]]
        if rng.random() < 0.5:
            cells.append([0, -1, 0, states.get(rng.choice(['stone', 'dirt', 'oak_planks']))])
        face = rng.choice([facing, OPPOSITE[facing], 'up', 'down'])
    held = rng.choice(['-', '-', 'stone', 'oak_planks', 'stick', 'iron_ingot', 'oak_door', 'oak_fence_gate'])
    case['use_item'] = {'item': '-' if held == '-' else d_plain(held, rng.choice([1, 16])),
                        'creative': rng.random() < 0.2, 'sneaking': rng.random() < 0.25, 'food': 20,
                        'yaw': rng.choice([0, 45, 90, 135, 180, -90, -135, -180, 270, 315, 30, 200]),
                        'face': face}
    case['cells'] = cells


def dragon_egg_case(rng, states, case):
    """A dragon egg clicked with anything, or an empty hand (`DragonEggBlock.useWithoutItem`, which
    takes the click before the item is placed): `teleport` draws 16/8/16-wide offsets up to 1000
    times and moves the egg to the first air block inside the build height that has a block under it,
    sending level event 2015 with `BlockUtil.packDifferenceInPosition` of the move. A floor gives the
    search somewhere to land; in a bare sky only the cell above the egg has a block under it, and a
    block in that cell leaves the 1000 draws with nowhere to go and the egg where it was."""
    cells = [[0, 0, 0, states.get('dragon_egg')]]
    if rng.random() < 0.8:
        # A floor for the egg to land on: it only moves to air with a block under it.
        cells.append([0, -1, 0, states.get(rng.choice(['stone', 'dirt', 'grass_block', 'netherrack', 'sand']))])
        for dx in range(-2, 3):
            for dz in range(-2, 3):
                if (dx, dz) == (0, 0) or rng.random() < 0.15:
                    continue
                cells.append([dx, -1, dz, states.get(rng.choice(['stone', 'dirt', 'oak_planks', 'glass',
                                                                 'snow_block', 'gravel']))])
    if rng.random() < 0.3:
        # Something above it, whose own block below is air, so the search never takes it.
        cells.append([0, 1, 0, states.get(rng.choice(['stone', 'oak_slab', 'glass']))])
    item = rng.choice(['-', '-', 'stick', 'stone', 'torch', 'dragon_egg'])
    case['use_item'] = {'item': '-' if item == '-' else d_plain(item, rng.choice([1, 16])),
                        'creative': rng.random() < 0.2, 'sneaking': rng.random() < 0.15, 'food': 20}
    case['cells'] = cells


# A block placed from an item carrying `minecraft:block_state`: the block and the properties the
# component sets, none of them the ones the click would have chosen. The last two name a property the
# block has no such thing as, and no property at all, which `BlockItemStateProperties.apply` skips.
STATED = [('oak_stairs', '{half:"top",facing:"east",shape:"outer_right"}'),
          ('oak_slab', '{type:"top"}'),
          ('oak_log', '{axis:"x"}'),
          ('oak_trapdoor', '{half:"top",open:"true"}'),
          ('hopper', '{enabled:"false"}'),
          ('repeater', '{delay:"4",facing:"west",locked:"true"}'),
          ('comparator', '{mode:"subtract",powered:"true"}'),
          ('beehive', '{honey_level:"5"}'),
          ('chest', '{facing:"east"}'),
          ('composter', '{level:"6"}'),
          ('oak_fence_gate', '{in_wall:"true",open:"true"}'),
          ('oak_stairs', '{half:"sideways"}'),
          ('stone', '{facing:"east"}'),
          ('oak_stairs', '{}')]


def block_state_case(rng, states, case):
    """A block placed from an item that carries a `minecraft:block_state` component (Ctrl + Pick Block
    of a turned stair, a half slab, a disabled hopper): `BlockItem.updateBlockStateFromTag` sets those
    properties on the state `getStateForPlacement` chose, so the block ends up in the state the item
    names. Ignoring the component leaves the state the click alone would have placed."""
    case['cells'] = [[0, 0, 0, states.get(rng.choice(['stone', 'dirt', 'grass_block']))]]
    block, props = rng.choice(STATED)
    case['use_item'] = {'item': '{id:"minecraft:%s",count:1,components:{"minecraft:block_state":%s}}'
                                % (block, props),
                        'creative': rng.random() < 0.7, 'sneaking': rng.random() < 0.2, 'food': 20}


# Axes, hoes and shovels transform blocks through the data-driven
# `minecraft:block_transformer` registry. `test_block_transformer_parity.py`
# compares the states that registry produces; this compares the click that uses
# one: the state change, the sound, the scrub particle, the wear, the statistic,
# the `block_change` game event, the loot rooted dirt leaves on the clicked face,
# and the second game event and particle a double copper chest's other half gets.
AXES = ['wooden_axe', 'stone_axe', 'iron_axe', 'golden_axe', 'diamond_axe', 'netherite_axe']
HOES = ['wooden_hoe', 'stone_hoe', 'iron_hoe', 'golden_hoe', 'diamond_hoe', 'netherite_hoe']
SHOVELS = ['wooden_shovel', 'stone_shovel', 'iron_shovel', 'golden_shovel', 'diamond_shovel', 'netherite_shovel']
LOGS = ['oak_log', 'spruce_log', 'birch_log', 'jungle_log', 'acacia_log', 'dark_oak_log', 'mangrove_log',
        'cherry_log', 'pale_oak_log', 'poplar_log', 'bamboo_block', 'crimson_stem', 'warped_stem',
        'oak_wood', 'spruce_wood', 'birch_wood', 'crimson_hyphae', 'warped_hyphae']
COPPERS = ['exposed_copper', 'weathered_copper', 'oxidized_copper', 'exposed_cut_copper', 'weathered_cut_copper',
           'oxidized_cut_copper', 'exposed_copper_bulb', 'weathered_copper_bulb', 'oxidized_copper_bulb',
           'exposed_copper_grate', 'weathered_copper_grate', 'oxidized_copper_grate', 'exposed_copper_bars',
           'weathered_copper_bars', 'oxidized_copper_bars', 'exposed_copper_chain', 'weathered_copper_chain',
           'oxidized_copper_chain', 'exposed_lightning_rod', 'weathered_lightning_rod', 'oxidized_lightning_rod',
           'waxed_copper_block', 'waxed_exposed_copper', 'waxed_weathered_copper', 'waxed_oxidized_copper',
           'waxed_cut_copper', 'waxed_exposed_cut_copper', 'waxed_weathered_cut_copper', 'waxed_oxidized_cut_copper',
           'waxed_copper_bulb', 'waxed_exposed_copper_bulb', 'waxed_weathered_copper_bulb',
           'waxed_oxidized_copper_bulb', 'waxed_copper_grate', 'waxed_exposed_copper_grate',
           'waxed_weathered_copper_grate', 'waxed_oxidized_copper_grate', 'waxed_copper_bars',
           'waxed_exposed_copper_bars', 'waxed_weathered_copper_bars', 'waxed_oxidized_copper_bars',
           'waxed_copper_chain', 'waxed_exposed_copper_chain', 'waxed_weathered_copper_chain',
           'waxed_oxidized_copper_chain', 'waxed_lightning_rod', 'waxed_exposed_lightning_rod']
COPPER_CHESTS = ['exposed_copper_chest', 'weathered_copper_chest', 'oxidized_copper_chest',
                 'waxed_copper_chest', 'waxed_exposed_copper_chest', 'waxed_weathered_copper_chest',
                 'waxed_oxidized_copper_chest']
COPPER_DOORS = ['exposed_copper_door', 'weathered_copper_door', 'oxidized_copper_door',
                'waxed_copper_door', 'waxed_exposed_copper_door', 'waxed_weathered_copper_door',
                'waxed_oxidized_copper_door']
COPPER_TRAPDOORS = ['exposed_copper_trapdoor', 'weathered_copper_trapdoor', 'oxidized_copper_trapdoor',
                    'waxed_copper_trapdoor', 'waxed_exposed_copper_trapdoor',
                    'waxed_weathered_copper_trapdoor', 'waxed_oxidized_copper_trapdoor']
SOIL = {'grass_block': {'snowy': 'false'}, 'podzol': {'snowy': 'false'}, 'mycelium': {'snowy': 'false'},
        'dirt': {}, 'coarse_dirt': {}, 'dirt_path': {}, 'rooted_dirt': {}}
FACINGS = {'north': (0, -1), 'south': (0, 1), 'west': (-1, 0), 'east': (1, 0)}


def damage_stack(item, damage):
    if damage:
        return '{id:"minecraft:%s",count:1,components:{"minecraft:damage":%d}}' % (item, damage)
    return d_plain(item, 1)


def transformer_case(rng, states, case):
    """One click that uses a block transformer: an axe on a log, a copper block,
    its door, trapdoor or chest, or a hoe or shovel on soil. The chest and door
    clicks sneak, because their own use (opening) would otherwise take the click
    (`suppressUsingBlock`)."""
    kind = rng.choices(('log', 'copper', 'chest', 'door', 'trapdoor', 'soil'), (3, 4, 2, 1, 1, 3))[0]
    creative = rng.random() < 0.15
    damage = rng.choice([0, 0, 0, 1, 10])
    face = rng.choice(['up', 'up', 'down', 'north', 'south', 'west', 'east'])
    sneaking = rng.random() < 0.3
    cells = [[0, -1, 0, states.get(rng.choice(['stone', 'dirt', 'oak_planks']))]]
    items = []
    if kind == 'log':
        name = rng.choice(LOGS)
        cells.insert(0, [0, 0, 0, states.get(name, axis=rng.choice(['x', 'y', 'z']))])
        item = rng.choice(AXES) if rng.random() < 0.9 else rng.choice(['iron_sword', 'stick'])
    elif kind == 'copper':
        name = rng.choice(COPPERS)
        cells.insert(0, [0, 0, 0, states.get(name)])
        item = rng.choice(AXES) if rng.random() < 0.9 else 'iron_sword'
    elif kind == 'chest':
        name = rng.choice(COPPER_CHESTS)
        facing = rng.choice(list(FACINGS))
        double = rng.random() < 0.5
        if double:
            own = rng.choice(['left', 'right'])
            other = 'right' if own == 'left' else 'left'
            dx, dz = FACINGS[facing]
            # ChestBlock.getConnectedDirection: LEFT is facing.getClockWise().
            dx, dz = (-dz, dx) if own == 'left' else (dz, -dx)
            cells.insert(0, [0, 0, 0, states.get(name, type=own, facing=facing, waterlogged='false')])
            cells.insert(1, [dx, 0, dz, states.get(name, type=other, facing=facing, waterlogged='false')])
        else:
            cells.insert(0, [0, 0, 0, states.get(name, type='single', facing=facing, waterlogged='false')])
        for slot in rng.sample(range(27), rng.choice([0, 1, 2])):
            items.append([0, 0, 0, slot, d.plain(rng.choice(['stone', 'diamond', 'bread']), rng.choice([1, 3, 16]))])
        item = rng.choice(['iron_axe', 'diamond_axe', 'netherite_axe'])
        sneaking = True
    elif kind == 'door':
        name = rng.choice(COPPER_DOORS)
        facing = rng.choice(list(FACINGS))
        props = {'facing': facing, 'hinge': rng.choice(['left', 'right']),
                 'open': rng.choice(['false', 'true']), 'powered': rng.choice(['false', 'true'])}
        cells.insert(0, [0, 0, 0, states.get(name, half='lower', **props)])
        cells.insert(1, [0, 1, 0, states.get(name, half='upper', **props)])
        item = rng.choice(['iron_axe', 'diamond_axe'])
        sneaking = True
    elif kind == 'trapdoor':
        name = rng.choice(COPPER_TRAPDOORS)
        cells.insert(0, [0, 0, 0, states.get(name, facing=rng.choice(list(FACINGS)),
                                              half=rng.choice(['bottom', 'top']),
                                              open=rng.choice(['false', 'true']),
                                              powered=rng.choice(['false', 'true']),
                                              waterlogged=rng.choice(['false', 'true']))])
        item = rng.choice(['iron_axe', 'diamond_axe'])
        sneaking = True
    else:
        hoe = rng.random() < 0.5
        # Rooted dirt is the one transform with a loot table, so give it a
        # third of the hoe cases.
        names = list(SOIL) + (['rooted_dirt'] * 3 if hoe else [])
        name = rng.choice(names)
        cells.insert(0, [0, 0, 0, states.get(name, **SOIL[name])])
        above = rng.choice(['air', 'air', 'air', 'stone', 'water'])
        if above != 'air':
            cells.append([0, 1, 0, states.get(above)])
        item = rng.choice(HOES if hoe else SHOVELS) if rng.random() < 0.9 else 'iron_sword'
    case['cells'] = cells
    case['items'] = items
    case['use_item'] = {'item': damage_stack(item, damage), 'creative': creative, 'sneaking': sneaking,
                        'food': 20, 'face': face}


def cases(facts, count, seed, transformers=True):
    rng = random.Random(seed)
    states = d.States(facts)
    for n in range(count):
        case = {**h.environment(rng), 'pos': [0, 0, 0]}
        case.pop('raining', None)
        case.pop('thundering', None)
        if transformers and rng.random() < 0.08:
            transformer_case(rng, states, case)
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.08:
            wire_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.1:
            bed_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.1:
            respawn_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.1:
            bed_respawn_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.15:
            anchor_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.08:
            timed_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.1:
            blocked_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.08:
            statue_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.12:
            pumpkin_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.08:
            crop_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.08:
            switch_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.08:
            jukebox_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.1:
            openable_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.1:
            dragon_egg_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        if rng.random() < 0.06:
            block_state_case(rng, states, case)
            case['items'] = []
            case['mode'] = 'dispense'
            yield case
            continue
        cells = []
        item = rng.choice(ITEMS)
        if rng.random() < 0.15:
            # A spawn egg on a fresh monster spawner (SpawnEggItem.useOn).
            cells.append([0, 0, 0, states.get('spawner')])
            if rng.random() < 0.3:
                cells.append([0, 1, 0, states.get(rng.choice(['stone', 'water', 'short_grass']))])
            item = rng.choice([f'{e}_spawn_egg' for e in EGGS] + ['apple', '-'])
            if rng.random() < 0.15:
                case['rules'] = {'spawner_blocks_work': False}
        elif rng.random() < 0.3:
            # A sweet berry bush on grass: pick it, bone meal it, or plant against it.
            age = rng.randrange(4)
            cells.append([0, 0, 0, states.get('sweet_berry_bush', age=str(age))])
            cells.append([0, -1, 0, states.get(rng.choice(['grass_block', 'dirt', 'podzol', 'farmland']))])
            item = rng.choice(['bone_meal', 'bone_meal', 'sweet_berries', 'apple', '-', 'stick'])
        else:
            ground = rng.choice(GROUNDS)
            cells.append([0, 0, 0, states.any(rng, ground) if ground == 'farmland' else states.get(ground)])
            above = rng.choice(ABOVE)
            if above != 'air':
                cells.append([0, 1, 0, states.get(above)])
            if item == 'glow_berries' and rng.random() < 0.5:
                # A ceiling two blocks up for the cave vines to hang from.
                cells.append([0, 2, 0, states.get(rng.choice(['stone', 'dirt', 'oak_leaves']))])
            if item == 'bone_meal' and ground in ('grass_block', 'moss_block', 'pale_moss_block'):
                # Bone meal on spreading blocks is the bone meal harness's.
                item = 'sweet_berries'
        held = '-' if item == '-' else d.plain(item, rng.choice([1, 1, 2, 16, 64]))
        case['use_item'] = {'item': held, 'creative': rng.random() < 0.2, 'sneaking': rng.random() < 0.2,
                            'food': rng.choice([20, 20, 19, 10, 0])}
        if rng.random() < 0.3:
            # The item in the off hand, something else or nothing in the main hand.
            main = rng.choice(['-', '-', 'stick', 'apple', 'stone', 'bone_meal', 'shield'])
            case['use_item'].update(offhand=held, item='-' if main == '-' else d.plain(main, rng.choice([1, 16])))
        case['cells'] = cells
        case['items'] = []
        case['mode'] = 'dispense'
        yield case


def wake_cases(facts, count, seed):
    """Villagers waking up, from a stream of their own so the other cases stay as they were."""
    rng = random.Random(f'wake {seed}')
    states = d.States(facts)
    for _ in range(count):
        case = {**h.environment(rng), 'pos': [0, 0, 0]}
        case.pop('raining', None)
        case.pop('thundering', None)
        wake_case(rng, states, case)
        case['items'] = []
        case['mode'] = 'dispense'
        yield case


# Items whose `Item.use` counts the used statistic (#9775): books, the spyglass, boats, a carrot or
# warped fungus on a stick off its mount, armor and other equippables swapped from the hand, and
# flint and steel or a fire charge on TNT (or lighting a fire). An empty map needs the server's
# saved data and a fishing rod a hook, which the stand-in level and the fixture leave out.
USED = ['writable_book', 'written_book', 'spyglass', 'oak_boat', 'birch_chest_boat', 'bamboo_raft',
        'carrot_on_a_stick', 'warped_fungus_on_a_stick', 'iron_helmet', 'diamond_chestplate',
        'leather_leggings', 'golden_boots', 'turtle_helmet', 'carved_pumpkin', 'flint_and_steel', 'fire_charge']
# What the player looks down on (three blocks south of the clicked block), for the boats: two
# blocks below its feet, so the new boat clears the player (vanilla's `noCollision` counts the
# player, which is not in the oracle's level).
FLOORS = ['stone', 'water', 'water', 'air', 'oak_planks']


def used_stat_case(rng, states, case):
    """An item whose `use` counts the used statistic, in either hand, over the clicked block (TNT for
    flint and steel and fire charges, most of the time), with a floor under the player for boats."""
    item = rng.choice(USED)
    lights = item in ('flint_and_steel', 'fire_charge')
    origin = 'tnt' if lights and rng.random() < 0.8 else rng.choice(['stone', 'dirt', 'oak_planks'])
    cells = [[0, 0, 0, states.get(origin)]]
    floor = rng.choice(FLOORS)
    if floor != 'air':
        cells.append([0, -2, 3, states.get(floor)])
    case['cells'] = cells
    # One of each: a second would go to the inventory, which the oracle's player lacks.
    held = d.plain(item, 1)
    case['use_item'] = {'item': held, 'creative': rng.random() < 0.2, 'sneaking': rng.random() < 0.15,
                        'food': 20, 'yaw': rng.choice([0.0, 90.0, 180.0, 270.0, 33.0])}
    if rng.random() < 0.3:
        main = rng.choice(['-', '-', 'stick', 'stone'])
        case['use_item'].update(offhand=held, item='-' if main == '-' else d.plain(main, 1))


def used_stat_cases(facts, count, seed):
    """`used_stat_case`s, from a stream of their own so the other cases stay as they were."""
    rng = random.Random(f'used {seed}')
    states = d.States(facts)
    for _ in range(count):
        case = {**h.environment(rng), 'pos': [0, 0, 0]}
        case.pop('raining', None)
        case.pop('thundering', None)
        used_stat_case(rng, states, case)
        case['items'] = []
        case['mode'] = 'dispense'
        yield case


# Entities an item places, each with its ENTITY_PLACE game event and used statistic (#9776):
# paintings and item frames on a wall, armor stands, end crystals on obsidian or bedrock, minecarts
# on rails.
HANGING = ['painting', 'painting', 'item_frame', 'glow_item_frame']
RAILS = ['rail', 'powered_rail', 'detector_rail', 'activator_rail']
CARTS = ['minecart', 'chest_minecart', 'furnace_minecart', 'tnt_minecart', 'hopper_minecart']


def placement_case(rng, states, case):
    """An item that places an entity, clicked where it fits or does not."""
    roll = rng.random()
    if roll < 0.35:
        # A wall facing the player (south), up to 4x4 so the larger paintings fit; the click on its
        # south face, or another face.
        width, height = rng.choice([(1, 1), (2, 2), (4, 3), (4, 4)])
        wall = rng.choice(['stone', 'oak_planks', 'glass', 'stone'])
        cells = [[x, y, 0, states.get(wall)] for x in range(-(width // 2), width - width // 2)
                 for y in range(height)]
        item = rng.choice(HANGING)
        # Something in front of the wall, for the frames only: a painting picks among the largest
        # variants that fit with its own (unseeded) random, so each wall here fits one size alone.
        if item != 'painting' and rng.random() < 0.3:
            cells.append([0, 1 if height > 1 else 0, 1, states.get(rng.choice(['torch', 'stone', 'oak_slab']))])
        case['use_item'] = {'item': d.plain(item, 1), 'face': rng.choice(['south', 'south', 'south', 'up', 'east'])}
    elif roll < 0.6:
        cells = [[0, 0, 0, states.get(rng.choice(['stone', 'grass_block', 'short_grass', 'oak_slab']))]]
        if rng.random() < 0.3:
            cells.append([0, rng.choice([1, 2]), 0, states.get(rng.choice(['stone', 'glass']))])
        case['use_item'] = {'item': d.plain('armor_stand', 1)}
    elif roll < 0.8:
        cells = [[0, 0, 0, states.get(rng.choice(['obsidian', 'obsidian', 'bedrock', 'stone']))]]
        if rng.random() < 0.25:
            cells.append([0, 1, 0, states.get(rng.choice(['fire', 'stone']))])
        case['use_item'] = {'item': d.plain('end_crystal', 1)}
    else:
        rail = rng.choice(RAILS)
        cells = [[0, -1, 0, states.get('stone')], [0, 0, 0, states.any(rng, rail)]]
        case['use_item'] = {'item': d.plain(rng.choice(CARTS), 1)}
    case['cells'] = cells
    case['use_item'].update(creative=rng.random() < 0.2, sneaking=rng.random() < 0.15, food=20,
                            yaw=rng.choice([0.0, 90.0, 180.0, 270.0, 33.0]))
    if rng.random() < 0.25:
        held = case['use_item']['item']
        case['use_item'].update(offhand=held, item=rng.choice(['-', '-', d.plain('stick', 1)]))


def placement_cases(facts, count, seed):
    """`placement_case`s, from a stream of their own."""
    rng = random.Random(f'placed {seed}')
    states = d.States(facts)
    for _ in range(count):
        case = {**h.environment(rng), 'pos': [0, 0, 0]}
        case.pop('raining', None)
        case.pop('thundering', None)
        placement_case(rng, states, case)
        case['items'] = []
        case['mode'] = 'dispense'
        yield case


def forced_respawn_cases(facts):
    """Non-bed forced points: Block.isPossibleToRespawnInThis at feet and head.

    Plants containing water are not liquid blocks; bubble columns are. Keep both
    cases even in a small run so fluid-containing is not substituted for liquid.
    """
    states = d.States(facts)
    for name in ('air', 'water', 'lava', 'bubble_column', 'kelp', 'seagrass', 'stone'):
        for height in (0, 1):
            cells = [[0, 0, 0, states.get('air')], [0, 1, 0, states.get('air')]]
            cells[height][3] = states.get(name)
            yield {**h.environment(random.Random(10044)), 'pos': [0, 0, 0],
                   'cells': cells, 'items': [], 'mode': 'dispense',
                   'respawn_at': {'forced': True, 'consume': False, 'yaw': 37.0}}


def d_plain(item, count):
    return d.plain(item, count)


def kind(case, facts):
    first = facts.states[case['cells'][0][3]][0].split(':')[1]
    if 'respawn_at' in case:
        return f'respawn at {first}'
    if 'wake' in case:
        return f'villager waking at {first}'
    item = case['use_item']['item']
    item = item.split('"')[1].split(':')[1] if item != '-' else 'empty hand'
    if 'entities' in case:
        return f'{item} placed by ' + ', '.join(e['type'].split(':')[1] for e in case['entities'])
    return f'{item} on {first}'


def main():
    parser = h.argument_parser(__doc__)
    parser.add_argument('--no-transformers', action='store_true',
                        help='leave out the axe, hoe and shovel transforms, as the corpus did before them')
    args = parser.parse_args()
    if args.count == 40000:
        args.count = 4000
    lock, jars, libraries = base.verify_artifacts()
    facts = h.load_facts()
    resources = lab.LOCAL / 'horizon-client' / lock['minecraft']['version']
    work = lab.LOCAL / 'use-item-on-parity'
    work.mkdir(parents=True, exist_ok=True)
    all_cases = [*cases(facts, args.count, args.seed, not args.no_transformers),
                 *wake_cases(facts, max(args.count // 10, 1), args.seed),
                 *used_stat_cases(facts, max(args.count // 10, 1), args.seed),
                 *placement_cases(facts, max(args.count // 10, 1), args.seed), *forced_respawn_cases(facts)]
    (work / 'cases.jsonl').write_text(''.join(json.dumps(case) + '\n' for case in all_cases))
    classes = work / 'classes'
    classes.mkdir(exist_ok=True)
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    java_root = lab.ROOT / 'mods/horizon/src/test/java'
    d.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none', '-sourcepath', str(java_root), '-nowarn',
           '-cp', dependencies, '-d', str(classes), str(java_root / 'dev/horizonlab/parity/VanillaDispenserOracle.java')])
    d.run(['java', '-Xmx3G', '-cp', str(classes) + os.pathsep + dependencies, 'dev.horizonlab.parity.VanillaDispenserOracle',
           str(resources), str(work / 'cases.jsonl'), str(work / 'native.txt')])
    d.run(['cargo', 'build', '-q', '--profile', 'fast', '-p', 'horizon-client', '--bin', 'horizon-client'], cwd=lab.ROOT,
          env={**os.environ, 'CARGO_BUILD_JOBS': os.environ.get('CARGO_BUILD_JOBS', '3')})
    d.run([str(lab.cargo_target() / 'fast/horizon-client'), '--resources', str(resources)], cwd=work,
          env=lab.game_env(HORIZON_DISPENSE_FIXTURE=f'{work / "cases.jsonl"},{work / "rust.txt"}'))
    ignore = tuple(prefix + ' ' for prefix in args.ignore.split(',') if prefix) + ('D ',)
    native = h.split_cases((work / 'native.txt').read_text(), ignore)
    rust = h.split_cases((work / 'rust.txt').read_text(), ignore)
    assert len(native) == len(rust) == len(all_cases), (len(native), len(rust), len(all_cases))
    groups, errors = {}, {}
    planted = eaten = respawned = exploded = moved = 0
    for case, a, b in zip(all_cases, native, rust):
        if a and a[0].startswith('error'):
            errors.setdefault(a[0], []).append(case)
            continue
        planted += any(line.startswith('set 0,1,0') for line in a)
        eaten += any(line.startswith('held') and not line.endswith('using -') for line in a)
        respawned += any(line.startswith('respawn ') and line != 'respawn none' and 'respawn_at' in case for line in a)
        exploded += any(line.endswith(' minecraft:explode') for line in a)
        # Off the bed: not where the villager lay (y 0.6875).
        moved += any(line.startswith('wake ') and line.split()[2] != '0.687500' for line in a)
        if not d.same(a, b):
            groups.setdefault(kind(case, facts), []).append((case, a, b))
    for name, entries in sorted(errors.items(), key=lambda e: -len(e[1])):
        print(f'oracle error x{len(entries)}: {name}')
    for name, entries in sorted(groups.items(), key=lambda e: -len(e[1])):
        print(f'{name}: {len(entries)} differing')
        for case, a, b in entries[:args.show]:
            print('  case ' + d.describe(case, facts) + ' use='
                  + json.dumps(case.get('use_item', case.get('respawn_at', case.get('wake')))))
            print('   native ' + ' | '.join(sorted(a)))
            print('   rust   ' + ' | '.join(sorted(b)))
    total = sum(len(v) for v in groups.values())
    skipped = sum(len(v) for v in errors.values())
    print(f'{len(all_cases) - total - skipped}/{len(all_cases) - skipped} cases match ({skipped} oracle errors); '
          f'vanilla planted in {planted}, started eating in {eaten}, exploded {exploded} times and found a respawn '
          f'spot in {respawned}; villagers woke off the bed in {moved}')
    h.finish(total + skipped)


if __name__ == '__main__':
    main()
