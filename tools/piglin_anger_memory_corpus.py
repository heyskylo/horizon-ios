"""Live guarded-block anger through normal owner use/break helpers, not NBT setters."""
from command_exec_corpus import cmd


# H191 presentation only: the model has no compound insertion history.
WHOLE_MEMORY_DIAGNOSTICS = {
    'barrel-memory', 'ender-memory', 'shulker-memory', 'guarded_break-memory',
    'chest-memory', 'chest-after80-memory',
}


def piglin_anger_memory(raw_diagnostic=False, whole_memory_diagnostic=False):
    ops = []

    def run(command, expected=None, label=None, record=True):
        op = cmd(command, expect_result=expected, record=record)
        if label:
            op['label'] = label
        ops.append(op)

    def menu(label, slots):
        ops.append({'op': 'menu_state', 'label': label, 'expect_observation': {'own_slots': slots}})

    def step(ticks, label):
        run('/execute store result score step_start pig_time run time query gametime', record=False)
        run(f'/tick step {ticks}', 1)
        ops.append({'op': 'tick', 'n': ticks})
        run('/execute store result score step_end pig_time run time query gametime', record=False)
        run('/scoreboard players operation step_end pig_time -= step_start pig_time', record=False)
        run('/scoreboard players get step_end pig_time', ticks, label)

    def clock(label, start):
        run(f'/execute store result score now pig_time run time query gametime', record=False)
        run(f'/scoreboard players operation now pig_time -= {start} pig_time', record=False)
        run('/scoreboard players get now pig_time', 0, label)

    run('/tick freeze', 1)
    for command in ['/gamerule spawn_mobs false', '/gamerule advance_time false',
                    '/gamerule universal_anger false', '/difficulty normal', '/gamemode survival',
                    '/forceload add 0 0 15 15']:
        run(command)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    run('/execute if loaded 0 200 0', 1)
    run('/fill 0 199 0 12 199 12 minecraft:stone', 169)
    run('/tp @s 6.5 200 9.5 180 0')
    run('/clear @s')
    run('/item replace entity @s armor.head with minecraft:golden_helmet', 1)
    run('/item replace entity @s hotbar.0 with minecraft:netherite_pickaxe[enchantments={"minecraft:efficiency":5}]', 1)
    run('/effect give @s minecraft:resistance infinite 1 true', 1)
    run('/effect give @s minecraft:haste infinite 1 true', 1)
    run('/scoreboard objectives add pig_time dummy', 1)
    run('/scoreboard objectives add pig_damage minecraft.custom:minecraft.damage_taken', 2)
    run('/data modify storage h322:identity owner set from entity @s UUID', record=False)
    run('/data get entity @s UUID', 4)
    run('/data get entity @s Inventory')
    cases = [
        ('barrel', 'barrel', False, 'use', True),
        ('ender', 'ender_chest', False, 'use', True),
        ('shulker', 'shulker_box', False, 'use', True),
        ('walled_chest', 'chest', True, 'use', False),
        ('crafting', 'crafting_table', False, 'use', False),
        ('idle', 'air', False, None, False),
        ('guarded_break', 'gilded_blackstone', True, 'break', True),
        ('ordinary_break', 'blackstone', True, 'break', False),
        ('chest', 'chest', False, 'use', True),
    ]
    for index, (name, block, wall, action, angry) in enumerate(cases, 1):
        ops.append({'op': 'close_menu'})
        menu(name + '-closed', 0)
        run('/kill @e[type=!player]', record=False)
        step(25, name + '-cleanup-ticks')
        run('/kill @e[type=minecraft:item]', record=False)
        run('/clear @s', record=False)
        run('/execute unless data entity @s Inventory[]', 1)
        run('/item replace entity @s armor.head with minecraft:golden_helmet', 1)
        run('/item replace entity @s hotbar.0 with minecraft:netherite_pickaxe[enchantments={"minecraft:efficiency":5}]', 1)
        run('/fill 0 200 0 12 204 12 minecraft:air', record=False)
        for edge in ['0 200 0 0 204 12', '12 200 0 12 204 12', '1 200 0 11 204 0', '1 200 12 11 204 12']:
            run('/fill ' + edge + ' minecraft:stone', record=False)
        run('/execute if block 0 202 6 minecraft:stone', 1)
        run('/execute if block 6 199 7 minecraft:stone', 1)
        run('/execute if block 6 200 7 minecraft:air', 1)
        if wall:
            run('/fill 1 200 4 11 204 4 minecraft:stone', 55)
            run('/execute if block 6 202 4 minecraft:stone', 1)
        else:
            run('/execute if block 6 202 4 minecraft:air', 1)
        if block != 'air':
            run(f'/setblock 6 200 7 minecraft:{block}', 1)
        run(f'/execute if block 6 200 7 minecraft:{block}', 1, name + '-block-before')
        uuid = f'00000142-0000-0000-0000-{index:012x}'
        tag = f'h322_{name}'
        target = f'@e[type=minecraft:piglin,tag={tag},limit=1]'
        run(f'/summon minecraft:piglin 6.5 200 1.5 {{UUID:[I;322,0,0,{index}],Tags:["{tag}"],Health:16f,IsBaby:0b,IsImmuneToZombification:1b,PersistenceRequired:1b,equipment:{{mainhand:{{id:"minecraft:golden_sword",count:1}}}}}}', 1)
        run(f'/execute if entity @e[type=minecraft:piglin,tag={tag},limit=1,nbt={{UUID:[I;322,0,0,{index}],Health:16f,Pos:[6.5d,200d,1.5d],IsImmuneToZombification:1b}}]', 1)
        run(f'/data get entity {uuid} UUID', 4)
        run(f'/data get entity {uuid} Pos')
        run(f'/data get entity {uuid} Health', 16)
        run(f'/execute unless entity @e[tag={tag},nbt={{IsBaby:1b}}]', 1)
        run(f'/execute if items entity {uuid} weapon.mainhand minecraft:golden_sword', 1)
        run(f'/execute unless data entity {uuid} Brain.memories."minecraft:angry_at"', 1)
        step(40, name + '-sense-ticks')
        run(f'/execute if entity @e[type=piglin,tag={tag},nbt={{UUID:[I;322,0,0,{index}],Health:16f,IsImmuneToZombification:1b,PersistenceRequired:1b}}]', 1)
        run(f'/execute if entity @e[tag={tag},x=1,y=200,z=1,dx=10,dy=3,dz={2 if wall else 10}]', 1)
        run(f'/execute as {uuid} at @s if block ~ ~-1 ~ minecraft:stone', 1)
        run(f'/execute if items entity {uuid} weapon.mainhand minecraft:golden_sword', 1)
        run(f'/execute unless data entity {uuid} Brain.memories."minecraft:angry_at"', 1)
        run('/execute if entity @s[type=player,gamemode=survival,nbt={Health:20f,Pos:[6.5d,200d,9.5d]}]', 1)
        run('/execute if items entity @s armor.head minecraft:golden_helmet', 1)
        run('/execute if items entity @s weapon.mainhand minecraft:netherite_pickaxe', 1)
        run('/data get entity @s Inventory', label=name + '-owner-equipment')
        run('/execute store success score identity_changed pig_time run data modify storage h322:identity owner set from entity @s UUID', record=False)
        run('/scoreboard players get identity_changed pig_time', 0)
        run('/execute if loaded 6 200 7', 1)
        menu(name + '-before-action', 0)
        run('/execute store result score action_start pig_time run time query gametime', record=False)
        if raw_diagnostic:
            run(f'/data get entity {uuid} Pos', label=name + '-raw-pos-before')
            run(f'/data get entity {uuid} Motion', label=name + '-raw-motion-before')
        if action == 'use':
            ops.append({'op': 'use_block', 'pos': [6, 200, 7], 'face': 1, 'sequence': 3220 + index})
        elif action == 'break':
            ops.append({'op': 'break', 'pos': [6, 200, 7]})
        menu(name + '-after-action', 10 if block == 'crafting_table' else 27 if action == 'use' else 0)
        run(f'/execute if block 6 200 7 minecraft:{"air" if action == "break" else block}', 1, name + '-block-after')
        run(f'/execute if entity @e[type=piglin,tag={tag},nbt={{UUID:[I;322,0,0,{index}],Health:16f,IsImmuneToZombification:1b}}]', 1)
        clock(name + '-zero-action-ticks', 'action_start')
        run(f'/data get entity {uuid} Brain.memories."minecraft:angry_at"', label=name + '-memory')
        if angry:
            run(f'/data get entity {uuid} Brain.memories."minecraft:angry_at".ttl', 600, name + '-ttl')
            run('/data remove storage h322:identity actual', record=False)
            run(f'/data modify storage h322:identity actual set from entity {uuid} Brain.memories."minecraft:angry_at".value', record=False)
            run('/execute store success score target_changed pig_time run data modify storage h322:identity actual set from entity @s UUID', record=False)
            run('/scoreboard players get target_changed pig_time', 0)
        else:
            run(f'/execute unless data entity {uuid} Brain.memories."minecraft:angry_at"', 1)
        if name == 'chest':
            run('/execute store result score before80 pig_time run time query gametime', record=False)
            step(80, 'chest-extra-ticks')
            run('/execute store result score after80 pig_time run time query gametime', record=False)
            run('/scoreboard players operation after80 pig_time -= before80 pig_time', record=False)
            run('/scoreboard players get after80 pig_time', 80)
            run(f'/execute if entity @e[type=piglin,tag={tag},nbt={{UUID:[I;322,0,0,{index}],Health:16f}}]', 1)
            run(f'/data get entity {uuid} Brain.memories."minecraft:angry_at"', label='chest-after80-memory')
            run(f'/data get entity {uuid} Brain.memories."minecraft:angry_at".ttl', 520, 'chest-after80-ttl')
            run('/execute if score @s pig_damage matches 1..', 1, 'chest-positive-damage')
            run('/execute if entity @s[type=player,gamemode=survival,x=1,y=199,z=1,dx=10,dy=5,dz=10]', 1)
            if raw_diagnostic:
                run('/data get entity @s Health', label='chest-raw-health')
                run('/scoreboard players get @s pig_damage', label='chest-raw-damage')
        if raw_diagnostic:
            run(f'/data get entity {uuid} Pos', label=name + '-raw-pos-after')
            run(f'/data get entity {uuid} Motion', label=name + '-raw-motion-after')
    if not raw_diagnostic and not whole_memory_diagnostic:
        assert sum(op.get('label') in WHOLE_MEMORY_DIAGNOSTICS for op in ops) == 6
        ops = [op for op in ops if op.get('label') not in WHOLE_MEMORY_DIAGNOSTICS]
    return ops
