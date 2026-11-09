"""/summon trees with Passengers entries vanilla cannot load.

EntityType.loadPassengersRecursive loads each entry of Passengers with
loadEntityRecursive and keeps only those that load (an unknown or missing id
creates nothing); Entity.saveWithoutId then saves the riders the entity has.
"""
from command_exec_corpus import cmd

PIG = 'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b'


def uuid(n):
    return f'[I;161,0,0,{n}]'


def pig(tag, n, extra=''):
    return f'{{id:"minecraft:pig",UUID:{uuid(n)},Tags:["{tag}"],{PIG}{extra}}}'


def stand(tag, n):
    return f'{{id:"minecraft:armor_stand",UUID:{uuid(n)},Tags:["{tag}"],NoGravity:1b}}'


def summon_passengers(raw=False):
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/tick freeze', 1)
    run('/scoreboard objectives add h161 dummy', 1)
    run('/fill 0 209 0 15 209 15 minecraft:stone', 256)
    run('/execute if loaded 4 210 4', 1)

    # An unrelated live actor, read before and after.
    run(f'/summon minecraft:pig 1.5 210 1.5 {{UUID:{uuid(300)},Tags:["h161_control"],CustomName:"control",{PIG}}}', 1)

    def control():
        run('/execute if entity @e[tag=h161_control]', 1)
        run('/data get entity @e[tag=h161_control,limit=1] UUID', 4)
        run('/data get entity @e[tag=h161_control,limit=1] Pos', 3)
        run('/data get entity @e[tag=h161_control,limit=1] CustomName')

    control()

    unknown = '{id:"minecraft:h161_unknown",Tags:["h161_unknown_entry"]}'
    no_id = '{Tags:["h161_no_id_entry"]}'
    # name, Passengers SNBT or None, [(child tag, kind)] that vanilla loads, in order.
    cases = [
        ('absent', None, []),
        ('empty', '[]', []),
        ('unknown', f'[{unknown}]', []),
        ('missing_id', f'[{no_id}]', []),
        ('noncompound', '[1,2]', []),
        ('mixed', f'[{unknown},{pig("h161_mixed_a", 201)},{no_id},{pig("h161_mixed_b", 202)}]',
         [('h161_mixed_a', 'pig'), ('h161_mixed_b', 'pig')]),
        ('nested', f'[{pig("h161_nested_child", 203, f",Passengers:[{unknown}]")}]',
         [('h161_nested_child', 'pig')]),
        ('nonmob', f'[{stand("h161_nonmob_stand", 204)}]', [('h161_nonmob_stand', 'armor_stand')]),
        ('mixed_nonmob', f'[{unknown},{stand("h161_mixed_nonmob_stand", 205)},{pig("h161_mixed_nonmob_pig", 206)}]',
         [('h161_mixed_nonmob_stand', 'armor_stand'), ('h161_mixed_nonmob_pig', 'pig')]),
    ]
    for i, (name, passengers, children) in enumerate(cases):
        tag = f'h161_{name}'
        riders = f',Passengers:{passengers}' if passengers is not None else ''
        run(f'/summon minecraft:pig {3.5 + i} 210 6.5 {{UUID:{uuid(100 + i)},Tags:["{tag}"],'
            f'CustomName:"{name}",{PIG}{riders}}}')
        run(f'/execute if entity @e[tag={tag}]')
        run(f'/data get entity @e[tag={tag},limit=1] UUID')
        run(f'/data get entity @e[tag={tag},limit=1] CustomName')
        run(f'/execute store result score {tag} h161 run data get entity @e[tag={tag},limit=1] Passengers', record=False)
        run(f'/scoreboard players get {tag} h161')
        for n in range(max(len(children), 1)):
            for field in ['id', 'UUID', 'Tags[0]']:
                run(f'/data get entity @e[tag={tag},limit=1] Passengers[{n}].{field}')
        for child, kind in children:
            run(f'/execute if entity @e[tag={child}]')
            run(f'/execute if entity @e[tag={child},type=minecraft:{kind}]')
            run(f'/data get entity @e[tag={child},limit=1] UUID')
            run(f'/execute as @e[tag={child}] on vehicle if entity @s[tag={tag}]')
        if name == 'nested':
            child = 'h161_nested_child'
            run(f'/execute store result score {child} h161 run data get entity @e[tag={child},limit=1] Passengers', record=False)
            run(f'/scoreboard players get {child} h161')
            run(f'/data get entity @e[tag={child},limit=1] Passengers[0].id')
        for entry in ['h161_unknown_entry', 'h161_no_id_entry']:
            run(f'/execute if entity @e[tag={entry}]')
        if raw:
            run(f'/data get entity @e[tag={tag},limit=1]')
        run('/execute if entity @e[type=minecraft:pig]')
        run('/execute if entity @e[type=minecraft:armor_stand]')

    # Entity.load does not read Passengers: a merge keeps the riders the
    # entity has, whatever the merged compound lists.
    tag = 'h161_merged'
    run(f'/summon minecraft:pig 13.5 210 6.5 {{UUID:{uuid(120)},Tags:["{tag}"],CustomName:"merged",{PIG},'
        f'Passengers:[{unknown},{pig("h161_merged_child", 220)}]}}')
    for merge in ['{CustomName:"merged2"}', f'{{Passengers:[{unknown},{no_id}]}}']:
        run(f'/data merge entity @e[tag={tag},limit=1] {merge}')
        run(f'/data get entity @e[tag={tag},limit=1] CustomName')
        run(f'/execute store result score {tag} h161 run data get entity @e[tag={tag},limit=1] Passengers', record=False)
        run(f'/scoreboard players get {tag} h161')
        for n in range(2):
            for field in ['id', 'UUID', 'Tags[0]']:
                run(f'/data get entity @e[tag={tag},limit=1] Passengers[{n}].{field}')
        run(f'/execute as @e[tag=h161_merged_child] on vehicle if entity @s[tag={tag}]')
        run('/execute if entity @e[tag=h161_unknown_entry]')
    run('/execute if entity @e[type=minecraft:pig]')

    control()
    return ops
