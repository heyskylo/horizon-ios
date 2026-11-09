"""Pending chest-boat loot through real hopper ticks and destroying removals."""
import uuid


def boat_loot(cmd):
    out = [cmd('/tick freeze'), cmd('/scoreboard objectives add boatloot dummy', expect_result=1),
           cmd('/forceload add 0 0 15 15', expect_result=1), {'op': 'settle'},
           cmd('/execute if loaded 8 200 8', expect_result=1)]
    for index, (kind, mode, seed) in enumerate(
            ((kind, mode, seed) for kind in ('oak_chest_boat', 'bamboo_chest_raft')
             for mode in ('pull', 'push', 'kill', 'damage') for seed in (73, 112)), 1):
        actor = str(uuid.UUID(int=(13666 << 96) + index))
        out += [cmd('/fill 4 198 4 12 203 12 air', False),
                cmd('/kill @e[type=item,x=4,y=198,z=4,dx=8,dy=6,dz=8]', False),
                cmd('/summon minecraft:' + kind + ' 8.5 200 8.5 '
                    + '{UUID:[I;13666,0,0,' + str(index) + '],Tags:["boatloot"],NoGravity:1b,'
                    + 'LootTable:"minecraft:chests/spawn_bonus_chest",LootTableSeed:' + str(seed) + 'L}', expect_result=1),
                cmd(f'/execute as {actor} if entity @s[type=minecraft:{kind},tag=boatloot]', expect_result=1),
                cmd(f'/data get entity {actor} LootTableSeed', expect_result=seed),
                cmd(f'/execute unless data entity {actor} Items', expect_result=1)]
        if mode in ('pull', 'push'):
            y = 199 if mode == 'pull' else 201
            extra = '{TransferCooldown:0}' if mode == 'pull' else '{TransferCooldown:0,Items:[{Slot:0b,id:"minecraft:diamond",count:1}]}'
            out += [cmd(f'/setblock 8 {y} 8 hopper[facing=down]' + extra, expect_result=1),
                    cmd(f'/data get block 8 {y} 8 TransferCooldown', expect_result=0),
                    cmd('/execute store result score #start boatloot run time query gametime', False),
                    cmd('/tick step 1'), {'op': 'tick', 'n': 1},
                    cmd('/execute store result score #elapsed boatloot run time query gametime', False),
                    cmd('/scoreboard players operation #elapsed boatloot -= #start boatloot', False),
                    cmd('/scoreboard players get #elapsed boatloot', expect_result=1),
                    cmd(f'/execute unless data entity {actor} LootTable', expect_result=1),
                    cmd(f'/data get entity {actor} Items'),
                    cmd(f'/data get block 8 {y} 8 Items'),
                    cmd(f'/data modify storage boatloot:check items set from entity {actor} Items', False),
                    cmd('/tick step 1'), {'op': 'tick', 'n': 1},
                    cmd(f'/execute store success score #changed boatloot run data modify storage boatloot:check items set from entity {actor} Items', False),
                    cmd('/scoreboard players get #changed boatloot', expect_result=0),
                    cmd(f'/setblock 8 {y} 8 air', False),
                    cmd(f'/kill {actor}', expect_result=1)]
        else:
            out += [cmd(f'/kill {actor}' if mode == 'kill' else f'/damage {actor} 5 minecraft:generic', expect_result=1),
                    cmd(f'/execute unless entity {actor}', expect_result=1),
                    cmd('/scoreboard players set #total boatloot 0', False),
                    cmd('/execute as @e[type=item,x=4,y=198,z=4,dx=8,dy=6,dz=8] store result score @s boatloot run data get entity @s Item.count', False),
                    cmd('/execute as @e[type=item,x=4,y=198,z=4,dx=8,dy=6,dz=8] run scoreboard players operation #total boatloot += @s boatloot', False),
                    cmd('/execute if score #total boatloot matches 1..', expect_result=1),
                    cmd('/scoreboard players get #total boatloot')]
    out += [cmd('/kill @e[type=item,x=4,y=198,z=4,dx=8,dy=6,dz=8]', False),
            cmd('/scoreboard objectives remove boatloot'),
            cmd('/forceload remove 0 0 15 15', expect_result=1), cmd('/tick unfreeze')]
    return out
