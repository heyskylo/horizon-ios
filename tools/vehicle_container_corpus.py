"""Class-owned vehicle container save fields through actual commands."""
import uuid

ITEMS = 'Items:[{Slot:0b,id:"minecraft:diamond",count:3,components:{"minecraft:custom_data":{h052:9}}}]'


def cases():
    return [
        ('boat_plain', 'oak_boat', ITEMS + ',LootTable:"minecraft:empty",LootTableSeed:73L', None, None),
        ('cart_plain', 'minecart', ITEMS + ',LootTable:"minecraft:empty",LootTableSeed:73L', None, None),
        ('boat_items', 'oak_chest_boat', ITEMS + ',LootTableSeed:73L', 1, None),
        ('cart_items', 'chest_minecart', ITEMS + ',LootTableSeed:73L', 1, None),
        ('boat_table', 'oak_chest_boat', 'LootTable:"minecraft:empty",LootTableSeed:73L', None, "minecraft:empty"),
        ('cart_table', 'chest_minecart', 'LootTable:"minecraft:empty",LootTableSeed:73L', None, "minecraft:empty"),
        ('boat_empty_table', 'oak_chest_boat', 'LootTable:"",LootTableSeed:73L', None, 'minecraft:'),
        ('cart_empty_table', 'chest_minecart', 'LootTable:"",LootTableSeed:73L', None, 'minecraft:'),
        ('boat_empty', 'oak_chest_boat', '', 0, None),
        ('cart_empty', 'chest_minecart', '', 0, None),
    ]


def identity(index):
    return str(uuid.UUID(int=(52 << 96) + index))


def vehicle_containers(cmd, diagnostic=False):
    out = [cmd('/tick freeze'), cmd('/scoreboard objectives add h052 dummy', expect_result=1),
           cmd('/forceload add 0 0 31 31', expect_result=4), {'op': 'settle'}]
    for x in [0, 16]:
        for z in [0, 16]:
            out.append(cmd(f'/execute if loaded {x} 200 {z}', expect_result=1))
    out += [cmd('/fill 0 199 0 31 204 15 barrier', expect_result=3072),
            cmd('/fill 0 200 0 31 204 15 air', expect_result=2560),
            cmd('/fill 0 199 0 31 199 15 stone', expect_result=512),
            cmd('/execute store result score #start h052 store success score #clock h052 run time query gametime', False),
            cmd('/scoreboard players get #clock h052', expect_result=1)]
    for i, (name, kind, fields, items, pending) in enumerate(cases(), 1):
        actor = identity(i)
        x = i * 2 + .5
        tag = '{UUID:[I;52,0,0,' + str(i) + '],Tags:["h052","' + name + '"],NoGravity:1b,Motion:[0d,0d,0d]'
        tag += (',' + fields if fields else '') + '}'
        out += [cmd(f'/summon minecraft:{kind} {x} 200 4.5 ' + tag, expect_result=1),
                cmd(f'/execute as {actor} if entity @s[type=minecraft:{kind},tag={name}]', expect_result=1),
                cmd(f'/execute if block {int(x)} 199 4 stone', expect_result=1),
                cmd(f'/execute if loaded {int(x)} 200 4', expect_result=1),
                cmd(f'/execute as {actor} if entity @s[nbt={{UUID:[I;52,0,0,{i}],NoGravity:1b,Pos:[{x}d,200d,4.5d]}}]', expect_result=1),
                cmd(f'/data get entity {actor} UUID', expect_result=4),
                *([cmd(f'/data get entity {actor} Tags', expect_result=2)] if diagnostic else []),
                cmd(f'/data get entity {actor} Pos', expect_result=3),
                cmd('/scoreboard players set #r h052 31', expect_result=31),
                cmd('/scoreboard players set #s h052 17', expect_result=17),
                cmd(f'/execute store result score #r h052 store success score #s h052 run data get entity {actor} Items'),
                cmd('/scoreboard players get #r h052', expect_result=items or 0),
                cmd('/scoreboard players get #s h052', expect_result=int(items is not None)),
                cmd(f'/data get entity {actor} LootTable', expect_result=len(pending) if pending else None),
                cmd(f'/data get entity {actor} LootTableSeed', expect_result=73 if pending else None)]
        if items:
            out += [cmd(f'/data get entity {actor} Items[0].count', expect_result=3),
                    cmd(f'/data get entity {actor} Items[0].components."minecraft:custom_data".h052', expect_result=9)]
        # Predeclared whole-body diagnostics: values/order outside the saved-field contract.
        if diagnostic:
            out.append(cmd(f'/data get entity {actor}'))
    out += [cmd('/execute store result score #end h052 store success score #clock h052 run time query gametime', False),
            cmd('/scoreboard players get #clock h052', expect_result=1),
            cmd('/execute if score #start h052 = #end h052', expect_result=1),
            cmd('/kill @e[tag=h052]', expect_result=10),
            cmd('/scoreboard objectives remove h052'), cmd('/forceload remove 0 0 31 31', expect_result=4),
            cmd('/tick unfreeze')]
    return out
