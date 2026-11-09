"""Scenario families for tools/parity_fuzz.py: each builds one scenario from a random generator and its seed.

Field names come from vanilla itself, not from a list here: the first run summons every entity type and places every
block entity bare, reads what vanilla saves for each (`defaults`, cached in the work directory), and mutations then
change those fields' values, keeping each value's NBT type.
"""
import json
import re

try:
    from . import lab
    from .parity_fuzz import read_data
except ImportError:
    import lab
    from parity_fuzz import read_data

# Not summonable alone (a player, a bobber without its owner), gone at once (lightning), or whole fights that
# break the area (dragon, wither).
SKIP_ENTITIES = {'player', 'fishing_bobber', 'lightning_bolt', 'ender_dragon', 'wither'}
# Block entity type -> a block that has it (the same name otherwise); moving pistons and portals have no item form.
BLOCK_FOR = {'banner': 'white_banner', 'mob_spawner': 'spawner', 'sign': 'oak_sign', 'skull': 'skeleton_skull',
             'hanging_sign': 'oak_hanging_sign', 'brushable_block': 'suspicious_sand', 'shelf': 'oak_shelf',
             'piston': None, 'end_portal': None}
# Fields a mutation leaves alone: identity and position (the scenario sets them), and nested data handled below.
FIXED = {'UUID', 'Pos', 'id', 'x', 'y', 'z', 'components', 'Brain', 'attributes', 'Paper'}
# Nested values worth trying, by field name, beside the scalar mutations.
POOLS = {
    'Motion': ['[0.1d,0.25d,-0.3d]', '[0.0d,-1.5d,0.0d]', '[0.6d,0.0d,0.0d]'],
    'Rotation': ['[45.0f,10.0f]', '[-170.5f,-90.0f]'],
    'Tags': ['["fz"]'],
    'CustomName': ['"fz"'],
    'Items': ['[{Slot:0b,id:"minecraft:stone",count:3}]', '[{Slot:1b,id:"minecraft:iron_sword",count:1}]'],
    'Item': ['{id:"minecraft:stone",count:5}', '{id:"minecraft:diamond_pickaxe",count:1}'],
    'active_effects': ['[{id:"minecraft:speed",duration:200,amplifier:1b}]',
                       '[{id:"minecraft:regeneration",duration:-1,amplifier:0b,show_particles:0b}]'],
    'equipment': ['{mainhand:{id:"minecraft:iron_sword",count:1}}', '{head:{id:"minecraft:leather_helmet",count:1}}'],
}
# Fields every entity can load, missing from a bare entity's save until set.
EXTRA = {'NoGravity': '1b', 'Silent': '1b', 'Glowing': '1b', 'CustomName': '"fz"', 'Tags': '["fz"]',
         'HasVisualFire': '1b', 'Motion': '[0.1d,0.25d,-0.3d]'}
NEIGHBOURS = ['water', 'lava', 'sand', 'redstone_block', 'hopper', 'slime_block', 'honey_block', 'soul_sand',
              'magma_block', 'ice', 'cobweb', 'powder_snow', 'scaffolding', 'campfire', 'rail', 'stone']
SCALARS = {'b': ['0b', '1b', '2b', '-1b', '127b'], 's': ['0s', '1s', '5s', '40s', '-1s', '32767s'],
           'L': ['0L', '1L', '200L'], 'f': ['0.0f', '0.5f', '1.0f', '2.5f', '-1.0f', '20.0f'],
           'd': ['0.0d', '0.5d', '1.0d', '-2.5d', '20.0d'], '': ['0', '1', '2', '7', '20', '100', '1200', '-1', '24000']}


def mutate(rng, value):
    if not isinstance(value, str) or value.startswith(('"', "'")):
        return None
    match = re.fullmatch(r'-?[0-9.E-]+([bsLfd]?)', value)
    return rng.choice(SCALARS[match.group(1)]) if match else None


def mutations(rng, saved, extra):
    """Up to four changed fields of a saved compound, each value of the type vanilla saved it with."""
    options = {}
    for key, value in saved.items():
        if key in FIXED:
            continue
        new = rng.choice(POOLS[key]) if key in POOLS else mutate(rng, value)
        if new is not None:
            options[key] = new
    options.update({k: v for k, v in extra.items() if k not in saved and rng.random() < 0.5})
    keys = sorted(options)
    return {k: options[k] for k in sorted(rng.sample(keys, min(len(keys), rng.randint(1, 4))))}


KNOWN = {}  # discover(): what vanilla saves for each bare entity and block entity, {'entity': {...}, 'block': {...}}


def discover(runner, work):
    """Run the bare summons and placements once (vanilla's saves are the field lists), cached in the work dir."""
    path = work / 'defaults.json'
    if path.exists():
        KNOWN.update(json.loads(path.read_text(encoding='utf-8')))
        return
    registries = json.loads((lab.RESOURCES / 'registries.json').read_text(encoding='utf-8'))
    names = lambda key: [k.split(':', 1)[1] for k in registries[key]['entries']]  # noqa: E731
    entities = [n for n in names('minecraft:entity_type') if n not in SKIP_ENTITIES]
    blocks = [b for b in (BLOCK_FOR.get(n, n) for n in names('minecraft:block_entity_type')) if b]
    scenarios, subjects = [], []  # subjects: per scenario, what each of its reads looks at
    for start in range(0, len(entities), 12):
        steps = []
        for i, kind in enumerate(entities[start:start + 12]):
            steps += [{'k': 'summon', 'type': kind, 'at': [1.5 + i, 0, 1.5 + (i % 3) * 5], 'n': i,
                       'nbt': {'NoAI': '1b', 'NoGravity': '1b'}}, {'k': 'read_entity', 'n': i}]
        scenarios.append({'family': 'defaults', 'seed': start, 'steps': steps})
        subjects.append([('entity', kind) for kind in entities[start:start + 12]])
    for start in range(0, len(blocks), 12):
        steps = []
        for i, block in enumerate(blocks[start:start + 12]):
            steps += [{'k': 'block', 'block': block, 'at': [1 + i, 0, 8]}, {'k': 'read_block', 'at': [1 + i, 0, 8]}]
        scenarios.append({'family': 'defaults', 'seed': 1000 + start, 'steps': steps})
        subjects.append([('block', block) for block in blocks[start:start + 12]])
    found = {'entity': {}, 'block': {}}
    for kinds, result in zip(subjects, runner.run(scenarios)):
        reads = result.get('reads', [])  # none when the scenario crashed the harness: its subjects stay unknown
        for (kind, name), (_, vanilla, _) in zip(kinds, reads):
            data = read_data(vanilla)
            if data:
                found[kind][name] = data[1]
    lab.write_json(path, found)
    KNOWN.update(found)


def persist(rng, seed):
    """Summon or place one thing with changed fields, maybe beside other blocks; read it, unload and reload, read."""
    known = KNOWN
    steps = [{'k': 'block', 'block': rng.choice(NEIGHBOURS), 'at': [7 + dx, 0, 7 + dz]}
             for dx, dz in rng.sample([(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1)], rng.randint(0, 2))]
    if rng.random() < 0.6:
        kind = rng.choice(sorted(known['entity']))
        nbt = mutations(rng, known['entity'][kind], EXTRA)
        mob = 'PersistenceRequired' in known['entity'][kind]
        if mob and rng.random() < 0.7:
            nbt.setdefault('NoAI', '1b')  # mob AI drift would hide what the reload changes
        if mob and rng.random() < 0.9:
            # Far from the player, a mob despawns on its first tick: what it saved would be gone before a read.
            nbt.setdefault('PersistenceRequired', '1b')
        steps.append({'k': 'summon', 'type': kind, 'at': [7.5, 0, 7.5], 'n': 0, 'nbt': nbt})
        read = {'k': 'read_entity', 'n': 0}
    else:
        block = rng.choice(sorted(known['block']))
        steps.append({'k': 'block', 'block': block, 'at': [7, 0, 7], 'nbt': mutations(rng, known['block'][block], {})})
        read = {'k': 'read_block', 'at': [7, 0, 7]}
    before, after = rng.choice([0, 0, 1, 3]), rng.choice([1, 2, 5])
    steps += ([{'k': 'tick', 'n': before}] if before else []) + [read, {'k': 'reload'}, read,
                                                                    {'k': 'tick', 'n': after}, read]
    return {'family': 'persist', 'seed': seed, 'steps': steps}


# Dispensed item -> the entity it makes (an item entity when the dispenser just drops it).
DISPENSED = {'arrow': 'arrow', 'spectral_arrow': 'spectral_arrow', 'tnt': 'tnt', 'fire_charge': 'small_fireball',
             'egg': 'egg', 'snowball': 'snowball', 'splash_potion': 'splash_potion', 'wind_charge': 'wind_charge',
             'lingering_potion': 'lingering_potion', 'experience_bottle': 'experience_bottle',
             'firework_rocket': 'firework_rocket', 'armor_stand': 'armor_stand', 'pig_spawn_egg': 'pig',
             'stone': 'item', 'oak_boat': 'item', 'minecart': 'item'}
# Entities a piston pushes or a command summons, with what they need to stay put.
MOVABLE = {'item': {'Item': '{id:"minecraft:stone",count:1}', 'PickupDelay': '100s'}, 'armor_stand': {},
           'pig': {'NoAI': '1b'}, 'minecart': {}, 'oak_boat': {}, 'tnt': {'fuse': '80s'}, 'chest_minecart': {},
           'falling_block': {'BlockState': '{Name:"minecraft:sand"}', 'Time': '1'},
           'experience_orb': {'Value': '3s'}, 'arrow': {'pickup': '0b'}, 'snowball': {}, 'villager': {'NoAI': '1b'},
           'slime': {'Size': '1', 'NoAI': '1b'}}
DROPPED = ['dirt', 'oak_log', 'gravel', 'oak_leaves', 'glass', 'potted_dandelion', 'chest', 'sand']
FACINGS = ['north', 'south', 'east', 'west', 'up', 'down']


def spawn(rng, seed):
    """One spawn mechanism, then the new entity read before the first tick and after each of a few ticks."""
    mechanism = rng.choice(['summon', 'dispense', 'pulse', 'drop', 'explode', 'piston'])
    c = [7, 0, 7]
    steps = []
    if mechanism in ('dispense', 'pulse'):
        item = rng.choice(sorted(DISPENSED))
        kind = DISPENSED[item]
        facing = rng.choice(FACINGS if mechanism == 'dispense' else FACINGS[:4])
        # The oracle's direct activation (`crafter_tick`) takes dispensers only; a dropper needs the real signal.
        dropper = kind == 'item' and mechanism == 'pulse' and rng.random() < 0.5
        steps.append({'k': 'block', 'block': f'{"dropper" if dropper else "dispenser"}[facing={facing}]', 'at': c, 'nbt': {'Items': f'[{{Slot:0b,id:"minecraft:{item}",count:2}}]'}})
        if mechanism == 'pulse':  # the real path: a neighbour signal schedules the dispenser's tick 4 ticks later
            steps += [{'k': 'block', 'block': 'redstone_block', 'at': [7, 1, 7]}, {'k': 'tick', 'n': 3}]
        else:
            steps.append({'k': 'dispense', 'at': c})
    elif mechanism == 'drop':
        block = rng.choice(DROPPED)
        kind = 'item'
        nbt = {'Items': '[{Slot:0b,id:"minecraft:stone",count:4},{Slot:3b,id:"minecraft:egg",count:16}]'} \
            if block == 'chest' else None
        steps += [{'k': 'block', 'block': block, 'at': c, 'nbt': nbt},
                  {'k': 'cmd', 'cmd': '/setblock {0} minecraft:air destroy', 'at': [c]}]
    elif mechanism == 'explode':
        kind = rng.choice(['item', 'armor_stand', 'tnt'])
        steps += [{'k': 'block', 'block': rng.choice(['dirt', 'oak_planks', 'sand']), 'at': [7 + dx, 0, 7 + dz]}
                  for dx, dz in [(1, 0), (0, 1), (-1, 0)]]
        steps.append({'k': 'summon', 'type': kind, 'at': [7.5, 0, 9.5], 'n': 1, 'nbt': dict(MOVABLE[kind])})
        steps.append({'k': 'summon', 'type': 'tnt', 'at': [7.5, 0, 7.5], 'n': 0, 'nbt': {'fuse': rng.choice(['0s', '1s'])}})
    elif mechanism == 'piston':
        kind = rng.choice(sorted(MOVABLE))
        facing = rng.choice(['east', 'up'])
        piston = [5, 0, 7] if facing == 'east' else [7, -1, 7]
        steps += [{'k': 'block', 'block': f'{rng.choice(["piston", "sticky_piston"])}[facing={facing}]', 'at': piston},
                  {'k': 'summon', 'type': kind, 'at': [6.5, 0, 7.5] if facing == 'east' else [7.5, 0, 7.5], 'n': 0,
                   'nbt': dict(MOVABLE[kind])},
                  {'k': 'block', 'block': 'redstone_block', 'at': [piston[0] - 1, piston[1], piston[2]]
                   if facing == 'east' else [6, -1, 7]}]
    else:
        kind = rng.choice(sorted(MOVABLE))
        nbt = dict(MOVABLE[kind])
        if rng.random() < 0.6:
            nbt['Motion'] = rng.choice(POOLS['Motion'])
        steps.append({'k': 'summon', 'type': kind, 'at': [7.5, rng.choice([0, 0.5, 2]), 7.5], 'n': 0, 'nbt': nbt})
    read = {'k': 'read_near', 'type': kind, 'at': c}
    steps.append(read)
    for _ in range(rng.choice([2, 3, 4])):
        steps += [{'k': 'tick', 'n': 1}, read]
    return {'family': 'spawn', 'seed': seed, 'steps': steps}


FAMILIES = {'persist': persist, 'spawn': spawn}

