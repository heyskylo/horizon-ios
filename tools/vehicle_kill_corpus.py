"""Actual zero-tick vehicle kill, explicit contents only.

Fresh Java validates every positive prerequisite before native comparison.
Drop identity/motion and complete vehicle bodies are predeclared diagnostics.
"""
import uuid


def vehicle_kill_contents(cmd, diagnostic=False):
    out = [cmd('/tick freeze'),
           cmd('/scoreboard objectives add h051 dummy', expect_result=1),
           cmd('/forceload add 0 0 31 31', expect_result=4), {'op': 'settle'}]
    for x in (0, 16):
        for z in (0, 16):
            out.append(cmd(f'/execute if loaded {x} 200 {z}', expect_result=1))
    out += [cmd('/fill 0 199 0 31 204 15 barrier', expect_result=3072),
            cmd('/fill 0 200 0 31 204 15 air', expect_result=2560),
            cmd('/fill 0 199 0 31 199 15 stone', expect_result=512),
            cmd('/gamerule minecraft:entity_drops false'),
            cmd('/execute store result score #start h051 store success score #clock h051 run time query gametime', False),
            cmd('/scoreboard players get #clock h051', expect_result=1)]
    sentinel = str(uuid.UUID(int=(51 << 96) + 99))
    out += [cmd('/summon item 30.5 200 12.5 {UUID:[I;51,0,0,99],Tags:["h051_witness"],NoGravity:1b,Motion:[0d,0d,0d],PickupDelay:32767s,Item:{id:"minecraft:emerald",count:11,components:{"minecraft:custom_data":{h051:99}}}}', expect_result=1),
            cmd(f'/execute as {sentinel} if entity @s[type=item,tag=h051_witness,nbt={{Pos:[30.5d,200d,12.5d],Item:{{id:"minecraft:emerald",count:11}}}}]', expect_result=1)]
    cases = ('oak_chest_boat', 'bamboo_chest_raft', 'chest_minecart', 'hopper_minecart', 'oak_boat')
    for i, kind in enumerate(cases, 1):
        x = i * 5 + .25
        actor = str(uuid.UUID(int=(51 << 96) + i))
        stacks = [('diamond', 3), ('dirt', 5), ('stick', 7)] if i < 5 else []
        items = ','.join('{Slot:' + str(slot) + 'b,id:"minecraft:' + item + '",count:' + str(count) + ',components:{"minecraft:custom_data":{h051:' + str(i) + '}}}' for slot, (item, count) in enumerate(stacks))
        tag = '{UUID:[I;51,0,0,' + str(i) + '],Tags:["h051_actor"],NoGravity:1b,Motion:[0d,0d,0d]' + (',Items:[' + items + ']' if stacks else '') + '}'
        out += [cmd(f'/summon minecraft:{kind} {x} 200 4.75 ' + tag, expect_result=1),
                cmd(f'/execute as {actor} if entity @s[type=minecraft:{kind},tag=h051_actor]', expect_result=1),
                cmd(f'/execute as {actor} if entity @s[nbt={{UUID:[I;51,0,0,{i}],Pos:[{x}d,200d,4.75d],NoGravity:1b}}]', expect_result=1),
                cmd(f'/execute if block {int(x)} 199 4 stone', expect_result=1),
                cmd(f'/execute if loaded {int(x)} 200 4', expect_result=1)]
        for slot, (item, count) in enumerate(stacks):
            out += [cmd(f'/execute if data entity {actor} Items[{{Slot:{slot}b,id:"minecraft:{item}",count:{count},components:{{"minecraft:custom_data":{{h051:{i}}}}}}}]', expect_result=1),
                    cmd(f'/data get entity {actor} Items[{{Slot:{slot}b}}]', expect_result=4)]
        if diagnostic:
            out.append(cmd(f'/data get entity {actor}'))
        out += [cmd('/scoreboard players set #r h051 31', expect_result=31),
                cmd('/scoreboard players set #s h051 17', expect_result=17),
                cmd(f'/execute store result score #r h051 store success score #s h051 run kill {actor}', expect_result=1),
                cmd('/scoreboard players get #r h051', expect_result=1),
                cmd('/scoreboard players get #s h051', expect_result=1),
                cmd(f'/execute unless entity {actor}', expect_result=1)]
        nearby = f'@e[type=minecraft:item,x={x},y=200,z=4.75,distance=..2]'
        out.append(cmd(f'/execute if entity {nearby}', expect_result=3) if stacks else cmd(f'/execute unless entity {nearby}', expect_result=1))
        for item, count in stacks:
            selector = f'@e[type=minecraft:item,x={x},y=200,z=4.75,distance=..2,nbt={{Item:{{id:"minecraft:{item}",count:{count},components:{{"minecraft:custom_data":{{h051:{i}}}}}}}}}]'
            out.append(cmd(f'/execute if entity {selector}', expect_result=1))
            out.append(cmd(f'/execute as {selector} run data get entity @s Item', expect_result=3))
            if diagnostic:
                out.append(cmd(f'/execute as {selector} run data get entity @s'))
        out.append(cmd(f'/execute as {sentinel} if entity @s[type=item,tag=h051_witness,nbt={{Pos:[30.5d,200d,12.5d],Item:{{id:"minecraft:emerald",count:11,components:{{"minecraft:custom_data":{{h051:99}}}}}}}}]', expect_result=1))
    out += [cmd('/execute store result score #end h051 store success score #clock h051 run time query gametime', False),
            cmd('/scoreboard players get #clock h051', expect_result=1),
            cmd('/execute if score #start h051 = #end h051', expect_result=1),
            cmd('/kill @e[type=item,x=0,y=198,z=0,dx=31,dy=8,dz=15]'),
            cmd('/scoreboard objectives remove h051'),
            cmd('/forceload remove 0 0 31 31', expect_result=4), cmd('/tick unfreeze')]
    return out
