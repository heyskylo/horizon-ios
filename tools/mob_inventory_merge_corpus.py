"""A Piglin's and a Villager's saved Inventory, loaded.

InventoryCarrier.readInventoryFromTag -> SimpleContainer.fromItemList:
clearContent, then addItem for each saved stack. addItem first fills
earlier stacks of the same item and components (up to the stack limit),
then empty slots; what does not fit in the eight slots is dropped.
"""
from command_exec_corpus import cmd

MOBS = {
    'piglin': 'NoAI:1b,PersistenceRequired:1b,Silent:1b,IsImmuneToZombification:1b',
    'villager': 'NoAI:1b,PersistenceRequired:1b,Silent:1b',
}


def item(id, count, extra=''):
    return f'{{id:"minecraft:{id}",count:{count}{extra}}}'


def tagged(id, count, value):
    return item(id, count, f',components:{{"minecraft:custom_data":{{h138:{value}}}}}')


KINDS = ['stone', 'dirt', 'cobblestone', 'oak_planks', 'sand', 'gravel', 'oak_log', 'glass', 'clay']

CASES = [
    ('two_three', [item('stone', 2), item('stone', 3)]),
    ('forty', [item('stone', 40), item('stone', 40)]),
    ('different', [item('stone', 2), item('dirt', 3)]),
    ('components', [tagged('stone', 2, 1), tagged('stone', 3, 2)]),
    ('same_components', [tagged('stone', 2, 1), tagged('stone', 3, 1)]),
    ('pearls', [item('ender_pearl', 10), item('ender_pearl', 10)]),
    ('interleaved', [item('stone', 40), item('dirt', 1), item('stone', 40)]),
    ('nine_kinds', [item(k, 1) for k in KINDS]),
    ('nine_full', [item('stone', 64)] * 9),
    ('single', [item('stone', 5)]),
    ('empty', []),
    ('absent', None),
]


def uuid(n):
    return f'[I;138,0,0,{n}]'


def mob_inventory_merge(raw=False):
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/fill 0 209 0 15 209 15 minecraft:stone', 256)
    run('/fill 0 209 16 15 209 31 minecraft:stone', 256)
    run('/execute if loaded 4 210 4', 1)
    run('/execute if loaded 4 210 20', 1)
    run('/scoreboard objectives add h138 dummy', 1)
    tags = []
    n = 0
    for m, (kind, fields) in enumerate(MOBS.items()):
        for i, (name, items) in enumerate(CASES):
            n += 1
            tag = f'h138_{kind}_{name}'
            tags.append(tag)
            inventory = '' if items is None else ',Inventory:[' + ','.join(items) + ']'
            x, z = 1.5 + (i % 6) * 2.5, 2.5 + (i // 6) * 4 + m * 16
            run(f'/summon minecraft:{kind} {x} 210 {z} {{UUID:{uuid(n)},Tags:["{tag}"],{fields}{inventory}}}', 1)
            run(f'/execute if entity @e[tag={tag},type=minecraft:{kind}]', 1)
    for tag in tags:
        run(f'/data get entity @e[tag={tag},limit=1] UUID', 4)
        run(f'/data get entity @e[tag={tag},limit=1] Health')
        run(f'/data get entity @e[tag={tag},limit=1] NoAI')
        run(f'/data get entity @e[tag={tag},limit=1] Inventory')
        run(f'/execute store result score {tag} h138 run data get entity @e[tag={tag},limit=1] Inventory', record=False)
        run(f'/scoreboard players get {tag} h138')
        if raw:
            run(f'/data get entity @e[tag={tag},limit=1]')
    # One tick later the inventories are the same (no pickup: NoAI, no items around).
    run('/execute store result score #t0 h138 run time query gametime', record=False)
    ops.append({'op': 'tick', 'n': 1})
    run('/execute store result score #t1 h138 run time query gametime', record=False)
    run('/scoreboard players operation #t1 h138 -= #t0 h138', record=False)
    run('/scoreboard players get #t1 h138')
    for tag in tags:
        run(f'/data get entity @e[tag={tag},limit=1] Inventory')
        run(f'/data get entity @e[tag={tag},limit=1] Pos')
    return ops
