#!/usr/bin/env python3
"""Compare block removal and player-mined experience with vanilla.

A container, jukebox, lectern, campfire, decorated pot, chiseled bookshelf,
shelf, shulker box or another block entity holder at the origin holds random
contents; the case then replaces it with `Level.setBlock` (another block, the
same block in another state, with or without UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS),
destroys it with `Level.destroyBlock` (with or without drops), removes it with
`Level.removeBlock`, or a player mines it (`ServerPlayerGameMode.destroyBlock`,
holding a tool or nothing, #844): in survival, where the block's drops, the tool's wear and the
mined statistic follow, and in creative, where the block itself never drops (a container's
contents still spill) and no statistic is awarded. Vanilla runs over the headless level of the
dispenser oracle
(`VanillaDispenserOracle`, "remove" cases), which replays `LevelChunk.setBlockState`'s
`BlockEntity.preRemoveSideEffects`; Horizon runs its game glue in a headless game
(`HORIZON_DISPENSE_FIXTURE`). Both print the block changes, the contents left,
every item entity (position, motion, stack), level events, sounds, game events
and the level random. Motions from an item's own unseeded random ("~") are
compared within their spread. Deterministic ore, spawner and sculk mining cases
cover XP values and RNG advancement with block drops and Silk Touch on/off.
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


def plain(item, count=1):
    return d.plain(item, count)


def named(item, count, name):
    return '{id:"minecraft:%s",count:%d,components:{"minecraft:custom_name":"%s"}}' % (item, count, name)


STUFF = ['cobblestone', 'diamond', 'stick', 'iron_ingot', 'oak_planks', 'bread', 'arrow', 'redstone', 'ender_pearl', 'torch',
         'iron_sword', 'bow', 'white_wool', 'glass', 'apple']
BOOKS = ['book', 'enchanted_book', 'writable_book', 'written_book', 'knowledge_book']
FOOD = ['porkchop', 'beef', 'chicken', 'mutton', 'rabbit', 'cod', 'salmon', 'potato', 'kelp']
DISCS = ['music_disc_cat', 'music_disc_13', 'music_disc_pigstep', 'music_disc_otherside']

# Blocks with a block entity: the slots a case may fill and what goes in them.
HOLDERS = {
    'chest': (27, STUFF), 'trapped_chest': (27, STUFF), 'barrel': (27, STUFF), 'hopper': (5, STUFF),
    'dispenser': (9, STUFF), 'dropper': (9, STUFF), 'crafter': (9, STUFF), 'furnace': (3, STUFF),
    'blast_furnace': (3, STUFF), 'smoker': (3, FOOD), 'brewing_stand': (5, ['potion', 'blaze_powder', 'nether_wart', 'glass_bottle']),
    'shulker_box': (27, STUFF), 'red_shulker_box': (27, STUFF), 'lime_shulker_box': (27, STUFF),
    'copper_chest': (27, STUFF), 'exposed_copper_chest': (27, STUFF), 'weathered_copper_chest': (27, STUFF),
    'oxidized_copper_chest': (27, STUFF), 'waxed_copper_chest': (27, STUFF), 'waxed_oxidized_copper_chest': (27, STUFF),
    'jukebox': (1, DISCS), 'lectern': (1, ['writable_book', 'written_book']), 'campfire': (4, FOOD), 'soul_campfire': (4, FOOD),
    'decorated_pot': (1, STUFF), 'chiseled_bookshelf': (6, BOOKS), 'oak_shelf': (3, STUFF), 'pale_oak_shelf': (3, STUFF),
    'warped_shelf': (3, STUFF),
    # Block entities without contents.
    'ender_chest': (0, []), 'enchanting_table': (0, []), 'oak_sign': (0, []), 'white_banner': (0, []), 'beacon': (0, []),
    'bell': (0, []), 'skeleton_skull': (0, []), 'daylight_detector': (0, []), 'comparator': (0, []), 'spawner': (0, []),
}
# Blocks whose block entity Horizon models with its slots, which vanilla lists
# when the block entity survives (the others are only ever replaced).
LISTED = {'chest', 'trapped_chest', 'barrel', 'hopper', 'dispenser', 'dropper', 'crafter', 'furnace', 'blast_furnace',
          'smoker', 'brewing_stand', 'shulker_box', 'red_shulker_box', 'lime_shulker_box', 'copper_chest', 'exposed_copper_chest',
          'weathered_copper_chest', 'oxidized_copper_chest', 'waxed_copper_chest', 'waxed_oxidized_copper_chest', 'jukebox'}
REPLACEMENTS = ['air', 'stone', 'glass', 'water', 'oak_planks', 'barrel', 'chest', 'hopper', 'blue_shulker_box', 'dirt', 'tnt',
                'copper_chest', 'oxidized_copper_chest', 'lectern', 'jukebox']
FLAGS = [3, 3, 3, 2, 18, 2 | 256, 816, 3 | 64, 1]
# What a mining player holds: the right tool, a tool too weak or of another kind (swords and maces
# wear 2 a block, shears any block but fire), or nothing.
TOOLS = [plain('diamond_pickaxe'), plain('wooden_pickaxe'), plain('stone_axe'), plain('iron_shovel'), plain('iron_sword'),
         plain('shears'), plain('mace'), '-']


def contents(rng, name, slots, pool):
    """Slot items for the holder (books where books go, discs in jukeboxes, ...)."""
    items = []
    if not slots:
        return items
    fill = rng.choice([0, 1, 1, 2, 3, slots]) if slots > 1 else rng.choice([0, 1, 1])
    for slot in rng.sample(range(slots), min(fill, slots)):
        item = rng.choice(pool)
        count = 1 if name in ('jukebox', 'lectern', 'chiseled_bookshelf') or item in ('iron_sword', 'bow', 'potion',
                                                                                       'writable_book', 'written_book') else rng.randrange(1, 65)
        if item == 'potion':
            stack = d.potion('potion', rng.choice(['water', 'healing', 'swiftness']))
        elif rng.random() < 0.1 and count > 0:
            stack = named(item, count, 'Kept')
        else:
            stack = plain(item, count)
        items.append([0, 0, 0, slot, stack])
    return items


def holder_state(states, rng, name, items):
    """The block's state, with its book, record or slots shown where the state shows them."""
    if name == 'lectern':
        # A lectern shows a book exactly when it holds one.
        return states.get(name, has_book='true' if items else 'false', facing=rng.choice(['north', 'south', 'east', 'west']))
    if name == 'jukebox':
        return states.get(name, has_record='true' if items else 'false')
    if name == 'chiseled_bookshelf':
        filled = {i[3] for i in items}
        return states.get(name, **{f'slot_{i}_occupied': 'true' if i in filled else 'false' for i in range(6)})
    if name in ('chest', 'trapped_chest'):
        return states.get(name, type='single', facing=rng.choice(['north', 'south', 'east', 'west']))
    return states.any(rng, name) if rng.random() < 0.3 else states.get(name)


def other_state(states, rng, name, current):
    """Another state of the same block, when it has one."""
    choices = [i for i in states.facts.by_block['minecraft:' + name] if i != current]
    return rng.choice(choices) if choices else None


def cases(facts, count, seed, only, creative=True):
    rng = random.Random(seed)
    states = d.States(facts)
    names = list(HOLDERS)
    for n in range(count):
        name = names[n % len(names)] if rng.random() < 0.7 else rng.choice(names)
        if only and only not in name:
            continue
        slots, pool = HOLDERS[name]
        items = contents(rng, name, slots, pool)
        state = holder_state(states, rng, name, items)
        cells = [[0, 0, 0, state]]
        # A few neighbours: supports, comparators reading the container, fluids.
        for _ in range(rng.randrange(0, 4)):
            at = (rng.randrange(-1, 2), rng.randrange(-1, 2), rng.randrange(-1, 2))
            if at != (0, 0, 0):
                h.put(cells, at, states.get(rng.choice(['stone', 'dirt', 'glass', 'oak_planks', 'water', 'comparator', 'redstone_wire'])))
        roll = rng.random()
        if roll < 0.45:
            if name in LISTED and rng.random() < 0.15:
                target = other_state(states, rng, name, state)
                target = target if target is not None else states.get(rng.choice(REPLACEMENTS))
            else:
                target = states.get(rng.choice(REPLACEMENTS))
            op = {'op': 'set', 'state': target, 'flags': rng.choice(FLAGS)}
        elif roll < 0.75:
            op = {'op': 'destroy', 'drop': True}
        elif roll < 0.85:
            op = {'op': 'destroy', 'drop': False}
        elif roll < 0.92:
            op = {'op': 'remove'}
        else:
            # ServerPlayerGameMode.destroyBlock: a player mines it (#844), in survival or, every
            # other mine, in creative, where the block itself never drops and no statistic is
            # awarded. The tool comes from the roll, not another draw, so the cases after this
            # one stay the same.
            item = TOOLS[int(roll * 1000) % len(TOOLS)]
            if creative and n % 2 == 0:
                # A sword or mace carries `can_destroy_blocks_in_creative: false`, so a creative
                # player holding one cannot mine at all (`Item.canDestroyBlock`). That predicate
                # has its own corpus (test_creative_mining_parity); these cases need a tool that
                # does remove the block, and the empty hand does too.
                if item in (plain('iron_sword'), plain('mace')):
                    item = plain('diamond_pickaxe')
                op = {'op': 'mine', 'item': item, 'creative': True}
            else:
                op = {'op': 'mine', 'item': item}
                if creative:
                    op['creative'] = False
        case = {**h.environment(rng), 'mode': 'remove', 'pos': [0, 0, 0], 'cells': cells, 'items': items, 'remove': op}
        case.pop('raining', None)
        case.pop('thundering', None)
        yield case


def mining_experience_cases(facts, only):
    """Actual player mining: sampling precedes Silk Touch and the drops rule."""
    states = d.States(facts)
    for name in ('diamond_ore', 'spawner', 'sculk', 'sculk_catalyst'):
        if only and only not in name:
            continue
        for seed in (0, 1, 42, 123456789):
            for drops in (False, True):
                for silk in (False, True):
                    item = 'diamond_hoe' if name.startswith('sculk') else 'diamond_pickaxe'
                    tool = plain(item)
                    if silk:
                        tool = ('{id:"minecraft:%s",count:1,components:'
                                '{"minecraft:enchantments":{"minecraft:silk_touch":1}}}' % item)
                    yield {'seed': seed, 'mode': 'remove', 'pos': [0, 0, 0],
                           'cells': [[0, 0, 0, states.get(name)]], 'items': [],
                           'rules': {'block_drops': drops},
                           'remove': {'op': 'mine', 'item': tool}}


def describe(case, facts):
    cells = ' '.join(f'{c[0]},{c[1]},{c[2]}={facts.describe(c[3])}' for c in case['cells'])
    op = dict(case['remove'])
    if 'state' in op:
        op['state'] = facts.describe(op['state'])
    return f'{cells} items={json.dumps(case["items"])} remove={json.dumps(op)} seed={case["seed"]} rules={case.get("rules", {})}'


def main():
    parser = h.argument_parser(__doc__)
    parser.add_argument('--survival-only', action='store_true',
                        help='mine in survival only, as the corpus did before creative mining')
    args = parser.parse_args()
    if args.count == 40000:
        args.count = 4000
    lock, jars, libraries = base.verify_artifacts()
    facts = h.load_facts()
    resources = lab.LOCAL / 'horizon-client' / lock['minecraft']['version']
    work = lab.LOCAL / 'block-removal-parity'
    work.mkdir(parents=True, exist_ok=True)
    all_cases = list(mining_experience_cases(facts, args.only))
    all_cases.extend(cases(facts, args.count, args.seed, args.only, not args.survival_only))
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
    # Loot table names (D) are vanilla's bookkeeping; the drops themselves are compared.
    ignore = tuple(prefix + ' ' for prefix in args.ignore.split(',') if prefix) + ('D ',)
    native = h.split_cases((work / 'native.txt').read_text(), ignore)
    rust = h.split_cases((work / 'rust.txt').read_text(), ignore)
    assert len(native) == len(rust) == len(all_cases), (len(native), len(rust), len(all_cases))
    groups, errors = {}, {}
    for case, a, b in zip(all_cases, native, rust):
        if a and a[0].startswith('error'):
            errors.setdefault(a[0], []).append(case)
            continue
        if not d.same(a, b):
            block = facts.states[case['cells'][0][3]][0].split(':')[1]
            groups.setdefault(f'{block} {case["remove"]["op"]}', []).append((case, a, b))
    for kind, entries in sorted(errors.items(), key=lambda e: -len(e[1])):
        print(f'oracle error x{len(entries)}: {kind}')
    for name, entries in sorted(groups.items(), key=lambda e: -len(e[1])):
        print(f'{name}: {len(entries)} differing')
        for case, a, b in entries[:args.show]:
            print('  case ' + describe(case, facts))
            print('   native ' + ' | '.join(sorted(a)))
            print('   rust   ' + ' | '.join(sorted(b)))
    total = sum(len(v) for v in groups.values())
    skipped = sum(len(v) for v in errors.values())
    print(f'{len(all_cases) - total - skipped}/{len(all_cases) - skipped} cases match ({skipped} oracle errors)')
    (work / 'differences.json').write_text(json.dumps({k: [[c, a, b] for c, a, b in v[:5]] for k, v in groups.items()}))
    h.finish(total + skipped)


if __name__ == '__main__':
    main()
