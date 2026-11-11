"""Actual ordinary destruction carries only the shulker's authoritative lock.

Full source/item bodies and random entity UUID/position/motion remain raw
executable diagnostics. No custom loot-table policy or trajectory claim.
"""

def cases(cmd):
    out = []
    def add(command, result=None, admission=False, diagnostic=None):
        op = cmd(command) if result is None else cmd(command, expect_result=result)
        if admission: op['h211_admission'] = True
        if diagnostic: op['diagnostic'] = diagnostic
        out.append(op)
    def clock():
        add('/execute store result score #now h211 run time query gametime', diagnostic='absolute scene epoch')
        add('/scoreboard players operation #now h211 -= #start h211')
        add('/scoreboard players get #now h211', 0, True)
    add('/tick freeze', admission=True)
    add('/gamerule minecraft:spawn_mobs false', admission=True)
    add('/gamerule minecraft:random_tick_speed 0', admission=True)
    add('/gamerule minecraft:block_drops true', admission=True)
    add('/gamemode spectator', admission=True)
    add('/scoreboard objectives add h211 dummy', 1, True)
    add('/forceload add 0 0 15 15', 1, True)
    out.append({'op': 'settle'})
    add('/execute if loaded 11 200 11', 1, True)
    add('/fill 0 199 0 15 205 15 barrier', 1792, True)
    add('/fill 0 200 0 15 205 15 air', 1536, True)
    add('/fill 0 199 0 15 199 15 stone', 256, True)
    add('/tp @s 7.5 204 7.5', 1, True)
    add('/execute if entity @s[type=player,nbt={Health:20f,Pos:[7.5d,204d,7.5d],playerGameType:3}]', 1, True)
    add('/data get entity @s UUID', 4, True)
    add('/execute store result score #start h211 run time query gametime', diagnostic='absolute scene epoch')
    contents = 'Items:[{Slot:0b,id:"minecraft:diamond",count:3,components:{"minecraft:custom_data":{h211:1}}},{Slot:26b,id:"minecraft:apple",count:5}]'
    for x,z,kind,locked in [(3,3,'shulker_box',True),(11,3,'red_shulker_box',False),(3,11,'chest',True),(11,11,'crafter',True)]:
        pos = f'{x} 200 {z}'
        shulker = kind.endswith('shulker_box')
        fields = ([contents, 'CustomName:"Box"'] if shulker else []) + (['lock:{items:"minecraft:diamond"}'] if locked else [])
        add(f'/setblock {pos} minecraft:{kind}' + '{' + ','.join(fields) + '}', 1, True)
        add(f'/execute if loaded {pos}', 1, True)
        add(f'/execute if block {x} 199 {z} stone', 1, True)
        add(f'/execute if block {pos} minecraft:{kind}', 1, True)
        entity_id = 'shulker_box' if shulker else kind
        add(f'/execute if data block {pos} {{id:"minecraft:{entity_id}",x:{x},y:200,z:{z}}}', 1, True)
        add(f'/execute {"if" if locked else "unless"} data block {pos} lock', 1, True)
        if locked:
            add(f'/data get block {pos} lock', 1, True)
            add(f'/execute if data block {pos} {{lock:{{items:"minecraft:diamond"}}}}', 1, True)
        if shulker:
            add(f'/data get block {pos} CustomName', 3, True)
            add(f'/data get block {pos} Items[{{Slot:0b}}].count', 3, True)
            add(f'/data get block {pos} Items[{{Slot:26b}}].count', 5, True)
        add(f'/data get block {pos}', diagnostic='complete source block entity')
        clock()
        add('/scoreboard players set #r h211 99', 99, True)
        add('/scoreboard players set #s h211 17', 17, True)
        add(f'/execute store result score #r h211 store success score #s h211 run setblock {pos} air destroy', 1, True)
        add('/scoreboard players get #r h211', 1, True)
        add('/scoreboard players get #s h211', 1, True)
        add(f'/execute if block {pos} air', 1, True)
        item=f'@e[type=minecraft:item,x={x},y=199,z={z},dx=1,dy=2,dz=1]'
        one=item[:-1]+',limit=1]'
        add(f'/execute if entity {item}', 1, True)
        add(f'/execute if entity {item[:-1]},nbt={{Item:{{id:"minecraft:{kind}",count:1}}}}]', 1, True)
        add(f'/data get entity {one} Item.id', admission=True)
        add(f'/data get entity {one} Item.count', 1, True)
        add(f'/data get entity {one} Age', 0, True)
        add(f'/data get entity {one} PickupDelay', 10, True)
        if shulker:
            add(f'/data get entity {one} Item.components."minecraft:custom_name"', 3, True)
            add(f'/data get entity {one} Item.components."minecraft:container"[{{slot:0}}].item.count', 3, True)
            add(f'/data get entity {one} Item.components."minecraft:container"[{{slot:26}}].item.count', 5, True)
            add(f'/data get entity {one} Item.components."minecraft:container"[{{slot:0}}].item.components."minecraft:custom_data".h211', 1, True)
        if shulker and locked:
            add('/scoreboard players set #r h211 99', 99, True)
            add('/scoreboard players set #s h211 17', 17, True)
            add(f'/execute store result score #r h211 store success score #s h211 run data get entity {one} Item.components."minecraft:lock"', 1)
            add('/scoreboard players get #r h211', 1)
            add('/scoreboard players get #s h211', 1)
            add(f'/execute if data entity {one} Item.components{{"minecraft:lock":{{items:"minecraft:diamond"}}}}', 1)
        else:
            add(f'/execute unless data entity {one} Item.components."minecraft:lock"', 1)
        for path in ['Item','UUID','Pos','Motion','']:
            add(f'/data get entity {one}' + (' '+path if path else ''), diagnostic='full item or random identity/kinematics; no trajectory equality')
        clock()
    add('/execute if entity @s[type=player,nbt={Health:20f,Pos:[7.5d,204d,7.5d],playerGameType:3}]', 1, True)
    clock()
    add('/forceload remove 0 0 15 15', 1)
    add('/scoreboard objectives remove h211')
    add('/tick unfreeze')
    return out
