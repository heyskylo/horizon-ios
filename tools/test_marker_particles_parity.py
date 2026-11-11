#!/usr/bin/env python3
"""Compare creative marker selection, ambient sampling, the block ticks of the
whole `ClientLevel.animateTick` zoo and the campfire block entity's particles
with the pinned ClientLevel."""
import itertools
import json
import random

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab
    import oracle_harness as harness

CAMPFIRES = ['campfire', 'soul_campfire']
FACINGS = ['north', 'south', 'west', 'east']
# Which of the four cooking slots hold an item: the smoke above them and the
# draw each occupied slot makes.
SLOTS = [[0, 0, 0, 0], [1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1],
         [1, 1, 0, 0], [1, 0, 0, 1], [0, 1, 1, 0], [1, 1, 1, 1]]
SEEDS = [0, 1057, 7]
MARKER_ITEMS = ['air', 'barrier', 'light', 'stone', 'structure_void', 'apple']


def state(name, lit, signal, facing):
    return 'minecraft:%s[lit=%s,signal_fire=%s,facing=%s]' % (
        name, str(lit).lower(), str(signal).lower(), facing)


def marker_cases():
    return [dict(mode=mode, main=main, off=off, count=count)
            for mode, main, off, count in itertools.product(
                ['survival', 'creative', 'adventure', 'spectator'], MARKER_ITEMS, MARKER_ITEMS,
                [0, 1, 64])]


def campfire_cases(rng):
    """CampfireBlockEntity.particleTick: the smoke column over the fire and the
    puff above each occupied cooking slot."""
    for name, lit, signal, facing, slots, seed in itertools.product(
            CAMPFIRES, (True, False), (False, True), FACINGS, SLOTS, SEEDS):
        yield dict(kind='campfire', state=state(name, lit, signal, facing), slots=slots,
                   seed=seed, ticks=6)
    for _ in range(240):
        yield dict(kind='campfire',
                   state=state(rng.choice(CAMPFIRES), rng.random() < 0.8, rng.random() < 0.5,
                               rng.choice(FACINGS)),
                   slots=[rng.choice([0, 0, 1]) for _ in range(4)],
                   seed=rng.randrange(-2 ** 63, 2 ** 63), ticks=rng.randrange(1, 9))


def puff_cases(rng):
    """CampfireBlock.makeParticles: the cosy or signal smoke over the fire, and
    the plain smoke a dousing adds with it."""
    for signal, smoking, seed, puffs in itertools.product(
            (False, True), (False, True), SEEDS, (1, 20)):
        yield dict(kind='make', signal=signal, smoking=smoking, seed=seed, puffs=puffs)
    for _ in range(120):
        yield dict(kind='make', signal=rng.random() < 0.5, smoking=rng.random() < 0.5,
                   seed=rng.randrange(-2 ** 63, 2 ** 63), puffs=rng.randrange(1, 24))


def tick_cases(rng):
    """CampfireBlock.animateTick: the crackle sound and the lava particles a lit
    campfire's block tick asks the level random for."""
    for name, lit, signal, facing, seed in itertools.product(
            CAMPFIRES, (True, False), (False, True), FACINGS, SEEDS):
        yield dict(kind='tick', state=state(name, lit, signal, facing), seed=seed)
    for _ in range(96):
        yield dict(kind='tick',
                   state=state(rng.choice(CAMPFIRES), rng.random() < 0.75, rng.random() < 0.5,
                               rng.choice(FACINGS)),
                   seed=rng.randrange(-2 ** 63, 2 ** 63))


def zoo_states():
    """Every block whose own tick animates it from nothing but its state, the
    sampled position and the level random: the rest of `Block.animateTick`'s
    zoo beside the campfire, one world of each."""
    hexes = ['down', 'up', *FACINGS]
    return [
        # A torch burns with a smoke particle and a flame 0.7 up; a wall torch
        # is inset 0.27 into the face it hangs on, 0.22 up.
        'minecraft:torch', 'minecraft:soul_torch', 'minecraft:copper_torch',
        *[f'minecraft:wall_torch[facing={facing}]' for facing in FACINGS],
        *[f'minecraft:soul_wall_torch[facing={facing}]' for facing in FACINGS],
        *[f'minecraft:copper_wall_torch[facing={facing}]' for facing in FACINGS],
        # A lit redstone torch breathes red dust inside a 0.2 cube about it.
        'minecraft:redstone_torch[lit=true]', 'minecraft:redstone_torch[lit=false]',
        *[f'minecraft:redstone_wall_torch[lit=true,facing={facing}]' for facing in FACINGS],
        'minecraft:redstone_wall_torch[lit=false,facing=north]',
        # A lit furnace crackles and smokes out of its front face, a blast
        # furnace without the flame and from higher, a smoker straight up.
        *[f'minecraft:furnace[lit=true,facing={facing}]' for facing in FACINGS],
        *[f'minecraft:blast_furnace[lit=true,facing={facing}]' for facing in FACINGS],
        'minecraft:smoker[lit=true,facing=north]',
        'minecraft:furnace[lit=false,facing=north]', 'minecraft:smoker[lit=false,facing=north]',
        'minecraft:blast_furnace[lit=false,facing=north]',
        # Portals, rods and the ender chest's own portal motes.
        'minecraft:nether_portal[axis=x]', 'minecraft:nether_portal[axis=z]',
        'minecraft:end_portal',
        *[f'minecraft:end_rod[facing={facing}]' for facing in hexes],
        'minecraft:ender_chest[facing=north]', 'minecraft:ender_chest[facing=east]',
        # A spore blossom's falling spores, a mycelium's own specks.
        'minecraft:spore_blossom', 'minecraft:mycelium[snowy=false]', 'minecraft:mycelium[snowy=true]',
        # One flame per candle, each at its own place on the block or the cake.
        *[f'minecraft:candle[candles={count},lit=true,waterlogged=false]' for count in (1, 2, 3, 4)],
        'minecraft:candle[candles=4,lit=false,waterlogged=false]',
        'minecraft:candle_cake[lit=true]',
        # A brewing stand's smoke, from three draws inside its top.
        'minecraft:brewing_stand[has_bottle_0=true,has_bottle_1=true,has_bottle_2=true]',
        'minecraft:brewing_stand[has_bottle_0=false,has_bottle_1=false,has_bottle_2=false]',
        # Leaves: the falling leaf the biome (or the azalea's fixed colour)
        # tints, the poplar blocks that also rustle, the spruce that never falls.
        'minecraft:oak_leaves[distance=1,persistent=false,waterlogged=false]',
        'minecraft:cherry_leaves[distance=7,persistent=true,waterlogged=false]',
        'minecraft:pale_oak_leaves[distance=3,persistent=false,waterlogged=false]',
        'minecraft:red_poplar_leaves[distance=3,persistent=false,waterlogged=false]',
        'minecraft:azalea_leaves[distance=3,persistent=false,waterlogged=false]',
        'minecraft:spruce_leaves[distance=5,persistent=false,waterlogged=false]',
        # Redstone dust along the wire's connections and out of a repeater's
        # torches, and a powered lever's own puff.
        'minecraft:redstone_wire[power=15,north=side,east=up,south=none,west=side]',
        'minecraft:redstone_wire[power=1,north=none,east=none,south=none,west=none]',
        'minecraft:redstone_wire[power=9,north=up,east=up,south=up,west=up]',
        'minecraft:repeater[delay=1,facing=north,locked=false,powered=true]',
        'minecraft:repeater[delay=4,facing=east,locked=false,powered=true]',
        'minecraft:lever[face=floor,facing=north,powered=true]',
        'minecraft:lever[face=wall,facing=east,powered=true]',
        # Fire smoulders, soul fire burns, a sculk sensor clicks and a charged
        # respawn anchor leaks reverse portal motes.
        'minecraft:fire[age=0,east=false,north=false,south=false,up=false,west=false]',
        'minecraft:fire[age=15,east=true,north=true,south=false,up=true,west=false]',
        'minecraft:soul_fire',
        'minecraft:sculk_sensor[sculk_sensor_phase=active,power=0,waterlogged=false]',
        'minecraft:respawn_anchor[charges=1]', 'minecraft:respawn_anchor[charges=4]',
        # Embedded in themselves these only draw: the obsidian's tear roll, a
        # dead bush's and dry grass's idle sound roll, all landing nowhere.
        'minecraft:crying_obsidian', 'minecraft:dead_bush', 'minecraft:short_dry_grass',
        'minecraft:tall_dry_grass',
    ]


def zoo_cases(rng):
    """Each of those states through the whole loop at two seeds, and 40 of them
    at a random seed, the case's seed driving both the sampled positions and
    the block's own draws."""
    for block_state in zoo_states():
        for seed in (0, 1057):
            yield dict(kind='tick', state=block_state, seed=seed)
    for _ in range(40):
        yield dict(kind='tick', state=rng.choice(zoo_states()),
                   seed=rng.randrange(-2 ** 63, 2 ** 63))


def wither_rose_cases(rng):
    """WitherRoseBlock.animateTick: three smoke puffs about the centre of the
    block's pick shape, which the flower's `OffsetType.XZ` moves with the column,
    from a world that is all wither rose. Every case carries that offset: a
    flower always has one, so a run without it would compare a shifted shape."""
    for seed in SEEDS:
        yield dict(kind='tick', state='minecraft:wither_rose', seed=seed, offset='xz')
    for _ in range(24):
        yield dict(kind='tick', state='minecraft:wither_rose',
                   seed=rng.randrange(-2 ** 63, 2 ** 63), offset='xz')


def neighbour_cases(rng):
    """The families whose own tick reads a neighbour, a shape or the fluid of the
    block itself, over a world of that block above y = 64 and a floor below it:
    a dried ghast's ambient sound wants a tagged block under it, a hive drips
    only past the face below, a lit redstone ore skips its solid-render
    neighbours and a stalactite tip drips the fluid above its root."""
    dried_ghast = 'minecraft:dried_ghast[facing=north,hydration=0,waterlogged=%s]'
    hive = 'minecraft:beehive[facing=north,honey_level=%d]'
    nest = 'minecraft:bee_nest[facing=south,honey_level=%d]'
    ore = 'minecraft:%sredstone_ore[lit=true]'
    tip = 'minecraft:pointed_dripstone[thickness=%s,vertical_direction=down,waterlogged=%s]'
    floors = ['minecraft:soul_sand', 'minecraft:soul_soil', 'minecraft:stone', 'minecraft:air']
    for waterlogged, floor in itertools.product(('false', 'true'), floors):
        for seed in SEEDS:
            yield dict(kind='tick', state=dried_ghast % waterlogged, floor=floor, seed=seed)
    for block, level in [(hive, 5), (hive, 4), (nest, 5)]:
        for floor in [None, 'minecraft:air']:
            for seed in SEEDS:
                case = dict(kind='tick', state=block % level, seed=seed)
                if floor is not None:
                    case['floor'] = floor
                yield case
    for name, floor in itertools.product(('', 'deepslate_'), [None, 'minecraft:air']):
        for seed in SEEDS:
            case = dict(kind='tick', state=ore % name, seed=seed)
            if floor is not None:
                case['floor'] = floor
            yield case
    for seed in SEEDS:
        yield dict(kind='tick', state='minecraft:redstone_ore[lit=false]', seed=seed)
    for thickness, floor in itertools.product(('tip', 'middle'), [None, 'minecraft:stone', 'minecraft:water[level=0]']):
        for seed in SEEDS:
            case = dict(kind='tick', state=tip % (thickness, 'false'), seed=seed, offset='xz')
            if floor is not None:
                case['floor'] = floor
            yield case
    for seed in SEEDS:
        yield dict(kind='tick', state=tip % ('tip', 'true'), seed=seed, offset='xz')
    # A waterlogged bottom slab has a sturdy down face and no fluid under it: the
    # fluid drip the slab asks for lands on the slab below, 0.05 under its top.
    for seed in SEEDS:
        yield dict(kind='tick',
                   state='minecraft:stone_brick_slab[type=bottom,waterlogged=true]',
                   floor='minecraft:stone_brick_slab[type=bottom,waterlogged=false]', seed=seed)
    for _ in range(12):
        yield dict(kind='tick', state=rng.choice([hive % 5, ore % '']),
                   seed=rng.randrange(-2 ** 63, 2 ** 63),
                   floor=rng.choice(['minecraft:air', 'minecraft:soul_sand', 'minecraft:stone']))


def make_input(build):
    rng = random.Random(0x43414d50)
    cases = (marker_cases() + list(campfire_cases(rng)) + list(puff_cases(rng))
             + list(tick_cases(random.Random(0x414e494d)))
             + list(zoo_cases(random.Random(0x5a4f4f)))
             + list(wither_rose_cases(random.Random(0x524f5345)))
             + list(neighbour_cases(random.Random(0x4e454947))))
    path = build / 'cases.jsonl'
    path.write_text(''.join(json.dumps(case) + '\n' for case in cases))
    return path, len(cases)


def main():
    harness.differential(
        'marker-particles-parity', 'VanillaMarkerParticlesOracle', 'marker_particles_probe',
        make_input, 'Creative main-hand marker selection and barrier marker positions, velocities and random draws, '
        'the real CampfireBlock.animateTick for both campfire blocks, lit and unlit, signal fire on and off and '
        'all four facings, run through the whole ClientLevel.animateTick loop around the player (667 rounds of the '
        '16- and 32-block sample) on a world that is all campfire, with the crackle sound and the lava particles '
        'compared by name and exact position, volume and pitch bits in call order, and the level random compared '
        'through the draw after the last one, and the same loop on a world that is all of one other state for the 79 '
        'block states whose own tick reads nothing but that state, the sampled position and the level random: the '
        'three torches and their wall forms in all four facings, both redstone torches, the lit and unlit furnace, '
        'blast furnace and smoker in all four facings, both nether portal axes, the end portal, the end rod in all six '
        'facings, the ender chest, the spore blossom, the mycelium, one to four candles and the candle cake, the '
        'brewing stand, six leaf blocks, three redstone wire states, two repeaters, two levers, fire, soul fire, a '
        'blinking sculk sensor and a charged respawn anchor, each at two seeds and forty of them at a random one, '
        'and the wither rose, whose three smoke puffs sit about the centre of the pick shape the flower\'s '
        'model offset moves with the column, at three seeds and twenty-four more at random ones, '
        'and the families whose own tick reads a neighbour, a shape or the fluid of the block itself, '
        'each over a world of that block above y = 64 and a named floor below it: a dried ghast, '
        'waterlogged and not, over soul sand, soul soil, stone and air; a hive full of honey and one '
        'short of it, and a bee nest, embedded in themselves and hanging over air; a lit redstone ore '
        'and a deepslate one embedded and over air, and an unlit one; and stalactite tips and a middle '
        'segment embedded, over stone and over water, a waterlogged tip, and a waterlogged slab whose '
        'fluid drip lands on the dry slab below it; each at two seeds and twelve more at random '
        'seeds and floors; '
        'and the campfire block entity: the real '
        'CampfireBlockEntity.particleTick for both campfire blocks, lit and unlit, signal '
        'fire on and off, all four facings and every one of the four cooking slots held or empty, ticked six times '
        'from a seeded level random, and the real CampfireBlock.makeParticles for the cosy and the signal smoke with '
        'and without the plain smoke puff a douse adds. Every particle the level is asked for is compared by its '
        'type and exact position bits, in the order it was asked for, and the level random continues to be compared '
        'through the draw after the last one. The lit filter the client tick loop applies before it ticks a campfire '
        'and the rain every leaf block asks about are supplied to the oracle, as dry weather; the biome ambient '
        'sampling the loop runs under a block that is not solid-render reads the default biome both stand-in worlds '
        'report, whose ambient list is empty; the always-visible '
        'flag, the particle-set limiter, the velocities (each particle draws its own random for them) and the '
        'particles past their creation are outside this comparison. Families whose own tick needs something the '
        'all-one-state world cannot give it are not here: a falling block wants air below it, a drip wants a face '
        'that is not solid, the firefly bush wants a light level, the end gateway counts its visible faces and the '
        'lightning rod the level height, the potent sulfur wants water above, the bubble column and the '
        'fluid drips want a fluid, and the enchanting table needs air between itself and its power '
        'providers. The stand-in world holds every sampled position and one block of neighbour either '
        'way, its blocks below y = 64 are the case\'s floor and its fluid state is the state\'s own, so a '
        'waterlogged block hands the loop the water source it has in the game; and the block tags these '
        'ticks ask about (the dried ghast\'s ambient-sound tag, the hives\' impermeable tag, the speleothem '
        'tag the stalactite branch needs) are bound from the pinned client jar the way a datapack load '
        'binds them, where a bare bootstrap leaves every tag empty.',
        sources=[lab.ROOT / 'tools/test_marker_particles_parity.py',
                 lab.ROOT / 'crates/horizon-client/src/animate_tick.rs'], expected_lines=462960)


if __name__ == '__main__':
    main()
