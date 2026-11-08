"""A sulfur cube's body archetype attribute modifiers, read with /attribute.

SulfurCube.collectEquipmentChanges (during a tick, when the body item
changed) removes the previous body's archetype modifiers and adds the new
body's as transient modifiers (addOrUpdateTransientModifier):
knockback_resistance, explosion_knockback_resistance and bounciness
(add_value), friction_modifier and air_drag_modifier (add_multiplied_total).
Before that first tick the attributes read their defaults.
"""
from command_exec_corpus import cmd

BODIES = ['none', 'white_concrete_powder', 'oak_planks', 'andesite', 'iron_block', 'tube_coral_block',
          'white_wool', 'blue_ice', 'mushroom_stem', 'honeycomb_block', 'soul_sand', 'tnt', 'magma_block']
ATTRS = ['knockback_resistance', 'explosion_knockback_resistance', 'bounciness', 'friction_modifier',
         'air_drag_modifier', 'step_height']
CUBE = 'NoGravity:1b,Silent:1b,PersistenceRequired:1b,Size:1'


def uuid(n):
    return f'[I;192,0,0,{n}]'


def sulfur_body_attributes(raw=False):
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/gamerule spawn_mobs false')
    run('/forceload add 0 0 31 31', 4)
    for x, z in [(0, 0), (16, 0), (0, 16), (16, 16)]:
        run(f'/execute if loaded {x} 200 {z}', 1)
    run('/fill 0 199 0 31 199 31 minecraft:stone', 1024)
    run('/scoreboard objectives add h192 dummy', 1)

    subjects = []

    def summon(tag, n, body, noai, x, z):
        extra = '' if body == 'none' else f',equipment:{{body:{{id:"minecraft:{body}"}}}}'
        run(f'/summon minecraft:sulfur_cube {x} 200 {z} {{UUID:{uuid(n)},Tags:["{tag}"],NoAI:{noai}b,{CUBE}{extra}}}', 1)
        subjects.append((tag, body, noai))

    n = 0
    for i, body in enumerate(BODIES):
        n += 1
        summon(f'h192_n_{body}', n, body, 1, 1.5 + (i % 7) * 4, 1.5 + (i // 7) * 4)
    for i, body in enumerate(['none', 'oak_planks', 'white_wool']):
        n += 1
        summon(f'h192_ai_{body}', n, body, 0, 1.5 + i * 4, 17.5)
    # Replacement and removal, an explicit base, a permanent modifier with an archetype id.
    for i, name in enumerate(['change', 'base', 'collide']):
        n += 1
        summon(f'h192_{name}', n, 'oak_planks', 1, 17.5 + i * 4, 17.5)
    run('/attribute @e[tag=h192_base,limit=1] minecraft:knockback_resistance base set 0.5')
    # The bouncy archetype's own id, permanent, before the body is adopted.
    run('/attribute @e[tag=h192_collide,limit=1] minecraft:knockback_resistance modifier add '
        'minecraft:bouncy_add_knockback_resistance 0.25 add_value')

    def read(tag, body, noai):
        run(f'/execute if entity @e[tag={tag},type=minecraft:sulfur_cube]')
        run(f'/data get entity @e[tag={tag},limit=1] UUID')
        run(f'/data get entity @e[tag={tag},limit=1] Health')
        run(f'/data get entity @e[tag={tag},limit=1] Size')
        run(f'/data get entity @e[tag={tag},limit=1] equipment.body.id')
        if noai:
            run(f'/data get entity @e[tag={tag},limit=1] Pos')
        for a in ATTRS:
            run(f'/attribute @e[tag={tag},limit=1] minecraft:{a} get 1000')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:knockback_resistance base get 1000')
        if raw:
            run(f'/data get entity @e[tag={tag},limit=1] attributes')
            run(f'/data get entity @e[tag={tag},limit=1]')

    def modifiers(tag, archetype):
        for a, op in [('knockback_resistance', 'add'), ('explosion_knockback_resistance', 'add'),
                      ('bounciness', 'add'), ('friction_modifier', 'mul'), ('air_drag_modifier', 'mul')]:
            run(f'/attribute @e[tag={tag},limit=1] minecraft:{a} modifier value get minecraft:{archetype}_{op}_{a} 1000')

    def ticks(k, label):
        run(f'/execute store result score #{label}0 h192 run time query gametime', record=False)
        ops.append({'op': 'tick', 'n': k})
        run(f'/execute store result score #{label}1 h192 run time query gametime', record=False)
        run(f'/scoreboard players operation #{label}1 h192 -= #{label}0 h192', record=False)
        run(f'/scoreboard players get #{label}1 h192')

    # Tick 0: the defaults (the body is not adopted yet).
    for s in subjects:
        read(*s)
    modifiers('h192_n_oak_planks', 'bouncy')
    ticks(2, 't')
    for s in subjects:
        read(*s)
    # oak_planks is #sulfur_cube_archetype/bouncy; white_wool is light.
    modifiers('h192_n_oak_planks', 'bouncy')
    modifiers('h192_collide', 'bouncy')
    # Replace the body, then take it off.
    run('/item replace entity @e[tag=h192_change,limit=1] armor.body with minecraft:white_wool')
    ticks(2, 'u')
    read('h192_change', 'white_wool', 1)
    modifiers('h192_change', 'bouncy')
    modifiers('h192_change', 'light')
    run('/item replace entity @e[tag=h192_change,limit=1] armor.body with minecraft:air')
    ticks(2, 'v')
    read('h192_change', 'none', 1)
    modifiers('h192_change', 'light')
    return ops


def sulfur_body_replace():
    """Body replacement and removal through /data (Entity.load sets the
    equipment; the next tick's collectEquipmentChanges swaps the archetype
    modifiers), with a same-id permanent modifier and an explicit base."""
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/gamerule spawn_mobs false')
    run('/forceload add 0 0 15 15', 1)
    run('/execute if loaded 4 200 4', 1)
    run('/fill 0 199 0 15 199 15 minecraft:stone', 256)
    run('/scoreboard objectives add h192 dummy', 1)
    tags = ['h192_r_swap', 'h192_r_collide', 'h192_r_base']
    for i, tag in enumerate(tags):
        run(f'/summon minecraft:sulfur_cube {2.5 + 4 * i} 200 4.5 {{UUID:{uuid(41 + i)},Tags:["{tag}"],NoAI:1b,{CUBE},'
            f'equipment:{{body:{{id:"minecraft:oak_planks"}}}}}}', 1)
    run('/attribute @e[tag=h192_r_collide,limit=1] minecraft:knockback_resistance modifier add '
        'minecraft:bouncy_add_knockback_resistance 0.25 add_value')
    run('/attribute @e[tag=h192_r_base,limit=1] minecraft:knockback_resistance base set 0.5')

    def read(tag):
        run(f'/execute if entity @e[tag={tag},type=minecraft:sulfur_cube]')
        run(f'/data get entity @e[tag={tag},limit=1] UUID')
        run(f'/data get entity @e[tag={tag},limit=1] equipment.body')
        run(f'/data get entity @e[tag={tag},limit=1] Pos')
        for a in ATTRS:
            run(f'/attribute @e[tag={tag},limit=1] minecraft:{a} get 1000')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:knockback_resistance base get 1000')
        for archetype in ['bouncy', 'light']:
            for a, op in [('knockback_resistance', 'add'), ('bounciness', 'add'), ('friction_modifier', 'mul')]:
                run(f'/attribute @e[tag={tag},limit=1] minecraft:{a} modifier value get minecraft:{archetype}_{op}_{a} 1000')
        run(f'/data get entity @e[tag={tag},limit=1] attributes[{{id:"minecraft:knockback_resistance"}}].modifiers')

    def ticks(k, label):
        run(f'/execute store result score #{label}0 h192 run time query gametime', record=False)
        ops.append({'op': 'tick', 'n': k})
        run(f'/execute store result score #{label}1 h192 run time query gametime', record=False)
        run(f'/scoreboard players operation #{label}1 h192 -= #{label}0 h192', record=False)
        run(f'/scoreboard players get #{label}1 h192')

    ticks(2, 'a')
    for tag in tags:
        read(tag)
    for tag in tags:
        run(f'/data merge entity @e[tag={tag},limit=1] {{equipment:{{body:{{id:"minecraft:white_wool",count:1}}}}}}')
    # Before the next tick the old modifiers stay (the body is not adopted yet).
    for tag in tags:
        read(tag)
    ticks(2, 'b')
    for tag in tags:
        read(tag)
    for tag in tags:
        run(f'/data remove entity @e[tag={tag},limit=1] equipment.body')
    ticks(2, 'c')
    for tag in tags:
        read(tag)
    return ops
