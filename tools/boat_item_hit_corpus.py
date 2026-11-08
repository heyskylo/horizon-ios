"""BoatItem use: exact placement and one-tick identity, no ray mocks."""
from command_exec_corpus import cmd, setup


def boat_item_hits(motion_diagnostic=False):
    out = setup('/tick freeze', '/gamerule spawn_mobs false', '/gamerule advance_time false',
                '/scoreboard objectives add boat_hit dummy')
    out += [cmd('/forceload add 0 0 15 15', expect_result=1), {'op':'settle'},
            cmd('/execute if loaded 5 200 5',expect_result=1)]
    cases=[('stone',5.5,5.5,0,50,True),('turned',5.3,5.8,37.5,41.25,True),
           ('water-near',5.5,5.5,0,38,False),('water-far',5.5,5.5,0,30,True),
           ('slab',5.5,5.5,0,45,True),('pig',5.5,5.5,0,50,False),
           ('wall',5.5,5.5,0,10,False)]
    for sequence,(name,x,z,yaw,pitch,placed) in enumerate(cases,1):
        out += setup('/gamemode creative','/kill @e[type=!player]', '/clear @s',
                     '/fill 0 200 0 15 204 15 air','/fill 0 199 0 15 199 15 stone')
        if name.startswith('water'):
            out += [cmd('/fill 4 200 6 7 200 9 water[level=0]',expect_result=16),
                    cmd('/execute if block 5 200 7 water[level=0]',expect_result=1)]
        if name=='slab':
            out += [cmd('/fill 4 200 6 7 200 9 stone_slab[type=bottom,waterlogged=false]',expect_result=16),
                    cmd('/execute if block 5 200 7 stone_slab[type=bottom,waterlogged=false]',expect_result=1)]
        if name=='wall':
            out += [cmd('/fill 4 200 7 7 203 7 stone',expect_result=16),
                    cmd('/execute if block 5 203 7 stone',expect_result=1)]
        if name=='pig':
            out += [cmd('/summon pig 5.5 200 6.9 {UUID:[I;310,0,0,1],NoAI:1b,Tags:["boat_blocker"]}',expect_result=1),
                    cmd('/execute if entity @e[type=pig,tag=boat_blocker,nbt={UUID:[I;310,0,0,1],Health:10f,NoAI:1b}]',expect_result=1)]
        out += setup(f'/tp @s {x} 200 {z} {yaw} {pitch}', '/gamemode survival',
                     '/item replace entity @s weapon.mainhand with oak_boat')
        out += [cmd('/execute if entity @s[type=player,gamemode=survival]',expect_result=1),
                cmd('/execute if data entity @s UUID',expect_result=1),
                cmd('/data get entity @s Pos'),cmd('/data get entity @s Rotation'),cmd('/data get entity @s Health',expect_result=20),
                cmd('/clear @s oak_boat 0',expect_result=1),cmd('/execute unless entity @e[type=oak_boat]',expect_result=1),
                cmd('/execute if block 5 199 5 stone',expect_result=1)]
        out += setup('/execute store result score start boat_hit run time query gametime')
        out += [{'op':'use_item','yaw':yaw,'pitch':pitch,'sequence':sequence},cmd('/execute if entity @s[type=player]',expect_result=1)]
        out += [cmd('/execute '+('if' if placed else 'unless')+' entity @e[type=oak_boat]',expect_result=1),
                cmd('/clear @s oak_boat 0')]
        if placed:
            out += [cmd('/tag @e[type=oak_boat,limit=1] add placed_boat',expect_result=1),
                    cmd('/data modify storage boat_hit:identity uuid set from entity @e[tag=placed_boat,limit=1] UUID',expect_result=1),
                    cmd('/data get entity @e[tag=placed_boat,limit=1] Pos'),cmd('/data get entity @e[tag=placed_boat,limit=1] Rotation')]
        if placed and motion_diagnostic:
            out.append(cmd('/data get entity @e[tag=placed_boat,limit=1] Motion'))
        out += setup('/execute store result score elapsed boat_hit run time query gametime','/scoreboard players operation elapsed boat_hit -= start boat_hit')
        out += [cmd('/scoreboard players get elapsed boat_hit',expect_result=0),cmd('/tick step 1',expect_result=1),{'op':'tick','n':1}]
        out += setup('/execute store result score elapsed boat_hit run time query gametime','/scoreboard players operation elapsed boat_hit -= start boat_hit')
        out += [cmd('/scoreboard players get elapsed boat_hit',expect_result=1)]
        if placed:
            out += [cmd('/execute if entity @e[type=oak_boat,tag=placed_boat]',expect_result=1),
                    cmd('/data modify storage boat_hit:identity uuid set from entity @e[tag=placed_boat,limit=1] UUID'),
                    cmd('/data get entity @e[tag=placed_boat,limit=1] Pos')]
        else:
            out += [cmd('/execute unless entity @e[type=oak_boat]',expect_result=1)]
        if placed and motion_diagnostic:
            out.append(cmd('/data get entity @e[tag=placed_boat,limit=1] Motion'))
        out += setup('/data remove storage boat_hit:identity uuid')
    return out
