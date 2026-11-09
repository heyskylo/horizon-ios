"""Actual hopper insertion into the existing decorated-pot saved slot.

Full block entities and saved cooldowns remain categorical diagnostics. The
strict gate checks real item counts/components, conservation and tick cadence.
"""

def cases(cmd):
    out = []
    def add(command, result=None, admission=False, diagnostic=None):
        op = cmd(command) if result is None else cmd(command, expect_result=result)
        if admission: op['h282_admission'] = True
        if diagnostic: op['diagnostic'] = diagnostic
        out.append(op)
    def step(n):
        add(f'/tick step {n}', 1, True)
        out.append({'op': 'tick', 'n': n})
    def clock(elapsed):
        add('/execute store result score #now h282 run time query gametime', diagnostic='absolute scene epoch')
        add('/scoreboard players operation #now h282 -= #start h282')
        add('/scoreboard players get #now h282', elapsed, True)
    def stack(item, count, slot=None):
        fields = f'id:"minecraft:{item}",count:{count},components:{{"minecraft:custom_data":{{h282:1}}}}'
        if slot is not None: fields = f'Slot:{slot}b,' + fields
        return '{' + fields + '}'
    add('/tick freeze', admission=True)
    add('/gamerule minecraft:spawn_mobs false', admission=True)
    add('/gamerule minecraft:random_tick_speed 0', admission=True)
    add('/gamemode spectator', admission=True)
    add('/scoreboard objectives add h282 dummy', 1, True)
    add('/forceload add 0 0 15 15', 1, True)
    out.append({'op': 'settle'})
    add('/execute if loaded 5 200 12', 1, True)
    add('/fill 0 199 0 15 206 15 barrier', 2048, True)
    add('/fill 0 200 0 15 206 15 air', 1792, True)
    add('/fill 0 199 0 15 199 15 stone', 256, True)
    add('/tp @s 12.5 204 8.5', 1, True)
    add('/execute if entity @s[type=minecraft:player,nbt={Health:20f,Pos:[12.5d,204d,8.5d],playerGameType:3}]', 1, True)
    add('/data get entity @s UUID', 4, True)
    for z, item, target in [(4, 'wheat', 'decorated_pot'), (8, 'carrot', 'decorated_pot'), (12, 'wheat', 'chest')]:
        inventory = stack(item, 40, 0) + ',' + '{Slot:4b,id:"minecraft:diamond",count:2}'
        add(f'/setblock 4 200 {z} hopper[facing=east,enabled=true]{{Items:[{inventory}],TransferCooldown:0}}', 1, True)
        content = 'item:' + stack('wheat', 3) if target == 'decorated_pot' else 'Items:[' + stack('wheat', 3, 0) + ']'
        add(f'/setblock 5 200 {z} {target}{{{content}}}', 1, True)
        add(f'/execute if block 4 200 {z} hopper[facing=east,enabled=true]', 1, True)
        add(f'/execute if block 5 200 {z} {target}', 1, True)
        add(f'/execute if block 5 199 {z} stone', 1, True)
        add(f'/execute if data block 4 200 {z} {{id:"minecraft:hopper",Items:[{stack(item,40,0)}]}}', 1, True)
        add(f'/execute if data block 5 200 {z} {{id:"minecraft:{target}",x:5,y:200,z:{z}}}', 1, True)
        path = 'item' if target == 'decorated_pot' else 'Items[0]'
        add(f'/data get block 5 200 {z} {path}.count', 3, True)
        add(f'/data get block 5 200 {z} {path}.components."minecraft:custom_data".h282', 1, True)
    add('/execute store result score #start h282 run time query gametime', diagnostic='absolute scene epoch')
    for tick, delta in [(0, 0), (1, 1), (9, 8), (217, 208)]:
        if delta: step(delta)
        clock(tick)
        transferred = 0 if tick == 0 else (tick - 1) // 8 + 1
        for z in [4,8,12]:
            expected = transferred if z != 8 else 0
            path = 'item' if z != 12 else 'Items[0]'
            add(f'/data get block 4 200 {z} Items[0].count', 40 - expected)
            add(f'/data get block 5 200 {z} {path}.count', 3 + expected)
            add(f'/execute if data block 4 200 {z} Items[{{Slot:4b,id:"minecraft:diamond",count:2}}]', 1, True)
            add(f'/data get block 5 200 {z} {path}.components."minecraft:custom_data".h282', 1)
            add(f'/execute store result score #donor h282 run data get block 4 200 {z} Items[0].count')
            add(f'/execute store result score #target h282 run data get block 5 200 {z} {path}.count')
            add('/scoreboard players operation #donor h282 += #target h282')
            add('/scoreboard players get #donor h282', 43)
            add(f'/data get block 4 200 {z} TransferCooldown', diagnostic='saved cooldown projection; cadence is measured through real transfers')
            add(f'/data get block 4 200 {z}', diagnostic='complete source block entity')
            add(f'/data get block 5 200 {z}', diagnostic='complete target block entity')
    add('/scoreboard players set #result h282 31', 31, True)
    add('/scoreboard players set #success h282 17', 17, True)
    add('/execute store result score #result h282 store success score #success h282 run data get block 5 200 4 item.count', 31)
    add('/scoreboard players get #result h282', 31)
    add('/scoreboard players get #success h282', 1)
    add('/execute if entity @s[type=minecraft:player,nbt={Health:20f,Pos:[12.5d,204d,8.5d],playerGameType:3}]', 1, True)
    clock(217)
    add('/forceload remove 0 0 15 15', 1)
    add('/scoreboard objectives remove h282')
    add('/tick unfreeze')
    return out
