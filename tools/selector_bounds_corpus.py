"""Strict zero-tick box selection versus independent point distance/sorting."""
from command_exec_corpus import cmd


def selector_bounds():
    ops = []

    def run(text, expected=None, label=None, record=True):
        op = cmd(text, record=record, expect_result=expected)
        if label:
            op['label'] = label
        ops.append(op)

    run('/tick freeze', 1)
    run('/gamerule spawn_mobs false')
    run('/gamerule advance_time false')
    run('/forceload add 0 0 63 31', 8)
    ops.append({'op': 'settle', 'required_chunks': [[x, z] for x in range(4) for z in range(2)]})
    for x in range(4):
        for z in range(2):
            run(f'/execute if loaded {x*16} 220 {z*16}', 1)
    run('/fill 0 219 0 63 219 31 minecraft:stone', 2048)
    run('/fill 0 220 0 63 224 31 minecraft:air')
    for x in [10, 20, 30, 40, 50]:
        run(f'/execute if block {x} 219 10 minecraft:stone', 1)
    run('/execute if block 10 220 10 minecraft:air', 1)
    run('/tp @s 0.5 220 0.5')
    run('/gamemode creative')
    run('/execute if entity @s[type=minecraft:player,gamemode=creative,nbt={Health:20f,Pos:[0.5d,220d,0.5d]}]', 1)
    run('/data get entity @s UUID', 4)
    run('/kill @e[type=!player]', record=False)
    run('/scoreboard objectives add h030 dummy', 1)
    run('/execute store result score start h030 run time query gametime', record=False)
    actors = [
        ('pig', 'pig', 10.5, 'Health:10f,NoAI:1b,', 'Health:10f,NoAI:1b,'),
        ('golem', 'iron_golem', 20.5, 'Health:100f,NoAI:1b,', 'Health:100f,NoAI:1b,'),
        ('stand', 'armor_stand', 30.5, 'Health:20f,', 'Health:20f,'),
        ('item', 'item', 40.5, 'Health:5s,PickupDelay:32767s,Item:{id:"minecraft:stone",count:3},', 'Health:5s,Item:{id:"minecraft:stone",count:3},'),
        ('boat', 'oak_boat', 50.5, '', ''),
        ('far_pig', 'pig', 12.5, 'Health:10f,NoAI:1b,', 'Health:10f,NoAI:1b,'),
    ]
    for n, (name, kind, x, extra, guard) in enumerate(actors, 1):
        tags = f'"h030_{name}"' + (',"h030_pigs"' if kind == 'pig' else '')
        run(f'/summon minecraft:{kind} {x} 220 10.5 {{UUID:[I;30,0,0,{n}],Tags:[{tags}],{extra}NoGravity:1b}}', 1)
        run(f'/execute if entity @e[type=minecraft:{kind},tag=h030_{name},nbt={{UUID:[I;30,0,0,{n}],{guard}NoGravity:1b,Pos:[{x}d,220d,10.5d]}}]', 1)
        run(f'/data get entity 0000001e-0000-0000-0000-{n:012x} UUID', 4)
        run(f'/data get entity 0000001e-0000-0000-0000-{n:012x} Pos')
    checks = [
        ('pig-overlap-x', '9.5 220 10.5', 'tag=h030_pig,dx=0,dy=0,dz=0', 1),
        ('pig-separated-x', '11 220 10.5', 'tag=h030_pig,dx=0,dy=0,dz=0', 0),
        ('pig-point-distance', '9.5 220 10.5', 'tag=h030_pig,dx=0,dy=0,dz=0,distance=..0.9', 0),
        ('pig-distance-one', '9.5 220 10.5', 'tag=h030_pig,dx=0,dy=0,dz=0,distance=..1', 1),
        ('pig-overlap-z', '10.5 220 9.5', 'tag=h030_pig,dz=0', 1),
        ('pig-positive-x', '8.5 220 10.5', 'tag=h030_pig,dx=1', 1),
        ('pig-negative-x', '11.5 220 10.5', 'tag=h030_pig,dx=-1', 1),
        ('pig-positive-z', '10.5 220 8.5', 'tag=h030_pig,dz=1', 1),
        ('pig-negative-z', '10.5 220 11.5', 'tag=h030_pig,dz=-1', 1),
        ('pig-positive-y', '10.5 218.5 10.5', 'tag=h030_pig,dy=1', 1),
        ('pig-negative-y', '10.5 221.5 10.5', 'tag=h030_pig,dy=-1', 1),
        ('pig-bottom-touch', '10.5 219 10.5', 'tag=h030_pig,dy=0', 0),
        ('pig-top-touch', '10.5 220.89999997615814 10.5', 'tag=h030_pig,dy=0', 0),
        ('pig-type-positive', '9.5 220 10.5', 'tag=h030_pig,type=minecraft:pig,dx=0', 1),
        ('pig-type-negative', '9.5 220 10.5', 'tag=h030_pig,type=!minecraft:pig,dx=0', 0),
        ('pig-type-other-negative', '9.5 220 10.5', 'tag=h030_pig,type=!minecraft:cow,dx=0,limit=1', 1),
        ('golem-wide', '19 220 10.5', 'tag=h030_golem,dx=0', 1),
        ('golem-tall', '20.5 222 10.5', 'tag=h030_golem,dy=0', 1),
        ('golem-separated', '22 220 10.5', 'tag=h030_golem,dx=0', 0),
        ('stand-overlap', '29.5 220 10.5', 'tag=h030_stand,dx=0', 1),
        ('stand-left-touch', '29.25 220 10.5', 'tag=h030_stand,dx=0', 0),
        ('stand-right-touch', '30.75 220 10.5', 'tag=h030_stand,dx=0', 0),
        ('stand-left-inside', '29.250000000000004 220 10.5', 'tag=h030_stand,dx=0', 1),
        ('stand-tall', '30.5 221.5 10.5', 'tag=h030_stand,dy=0', 1),
        ('item-overlap', '39.5 220 10.5', 'tag=h030_item,dx=0', 1),
        ('item-left-touch', '39.375 220 10.5', 'tag=h030_item,dx=0', 0),
        ('item-right-touch', '40.625 220 10.5', 'tag=h030_item,dx=0', 0),
        ('item-front-touch', '40.5 220 9.375', 'tag=h030_item,dz=0', 0),
        ('item-back-touch', '40.5 220 10.625', 'tag=h030_item,dz=0', 0),
        ('item-top-touch', '40.5 220.25 10.5', 'tag=h030_item,dy=0', 0),
        ('boat-overlap', '49 220 10.5', 'tag=h030_boat,dx=0', 1),
        ('boat-left-touch', '48.8125 220 10.5', 'tag=h030_boat,dx=0', 0),
        ('boat-right-touch', '51.1875 220 10.5', 'tag=h030_boat,dx=0', 0),
        ('pigs-both', '9.5 220 10.5', 'tag=h030_pigs,dx=3', 2),
        ('pigs-limit', '9.5 220 10.5', 'tag=h030_pigs,dx=3,limit=1', 1),
    ]
    for label, origin, options, expected in checks:
        run('/scoreboard players set result h030 -99', -99)
        run('/scoreboard players set success h030 -99', -99)
        run('/scoreboard players get result h030', -99)
        run('/scoreboard players get success h030', -99)
        run(f'/execute positioned {origin} store result score result h030 store success score success h030 run execute if entity @e[{options}]', expected, label)
        if expected == 0:
            ops[-1].pop('expect_result')
            ops[-1]['expect_callback'] = [False, 0]
        run('/scoreboard players get result h030', expected)
        run('/scoreboard players get success h030', int(expected > 0))
    for sort, tag in [('nearest', 'pig'), ('furthest', 'far_pig')]:
        run(f'/execute positioned 9.5 220 10.5 as @e[tag=h030_pigs,dx=3,sort={sort},limit=1] if entity @s[tag=h030_{tag}]', 1, 'point-sort-' + sort)
        run(f'/execute positioned 9.5 220 10.5 as @e[tag=h030_pigs,distance=..4,sort={sort},limit=1] if entity @s[tag=h030_{tag}]', 1, 'unboxed-point-sort-' + sort)
    for n, (name, kind, x, _, guard) in enumerate(actors, 1):
        run(f'/execute if entity @e[type=minecraft:{kind},tag=h030_{name},nbt={{UUID:[I;30,0,0,{n}],{guard}NoGravity:1b,Pos:[{x}d,220d,10.5d]}}]', 1)
        run(f'/data get entity 0000001e-0000-0000-0000-{n:012x} Pos')
    run('/execute store result score end h030 run time query gametime', record=False)
    run('/scoreboard players operation end h030 -= start h030', record=False)
    run('/scoreboard players get end h030', 0, 'zero-world-ticks')
    run('/execute if entity @s[type=minecraft:player,gamemode=creative,nbt={Health:20f,Pos:[0.5d,220d,0.5d]}]', 1)
    return ops
