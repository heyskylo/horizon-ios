"""Allay passive healing through the real entity tick, with and without AI.

Only health/cadence/cap are measured. Positions are guarded initially and
remain within the loaded test area; random wandering trajectories are not
an exact-coordinate contract.
"""


def allay_healing(cmd, setup):
    out = []
    elapsed = 0
    actors = [(f'00000138-0000-0000-0000-{i + 1:012x}',
               f'allay_heal_{i}', 16.5 + i * 4, no_ai, health)
              for i, (health, no_ai) in enumerate(
                  [(10, True), (10, False), (19.5, True), (19.5, False),
                   (20, True), (20, False)])]

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    def sample():
        for uuid, tag, x, no_ai, initial in actors:
            health = min(20, initial + elapsed // 10)
            check(f'/execute if entity @e[type=allay,tag={tag}]')
            check(f'/execute as {uuid} if entity @s[type=allay,tag={tag},'
                  'nbt={NoGravity:1b,Silent:1b,PersistenceRequired:1b}]')
            sign = 'if' if no_ai else 'unless'
            check(f'/execute as {uuid} {sign} entity @s[nbt={{NoAI:1b}}]')
            check(f'/execute as {uuid} at @s if loaded ~ ~ ~')
            check(f'/execute as {uuid} if entity @s[x=0,y=100,z=0,dx=63,dy=12,dz=63]')
            check(f'/execute if data entity {uuid} {{Health:{health:g}f}}')
            check(f'/data get entity {uuid} Health 100', int(health * 100))
            if elapsed in (0, 10, 20):
                out.extend(setup('/scoreboard players set #r allay_heal 31',
                                 '/scoreboard players set #s allay_heal 17'))
                check('/scoreboard players get #r allay_heal', 31)
                check('/scoreboard players get #s allay_heal', 17)
                check('/execute store result score #r allay_heal '
                      'store success score #s allay_heal '
                      f'run data get entity {uuid} Health 100', int(health * 100))
                check('/scoreboard players get #r allay_heal', int(health * 100))
                check('/scoreboard players get #s allay_heal')

    check('/tick freeze')
    check('/execute if dimension minecraft:overworld')
    check('/gamerule minecraft:spawn_mobs', 0)
    out.extend(setup('/forceload remove 0 0 63 63'))
    check('/forceload add 0 0 63 63', 16)
    out.append({'op': 'settle'})
    for x in range(0, 64, 16):
        for z in range(0, 64, 16):
            check(f'/execute if loaded {x} 101 {z}')
    check('/fill 0 100 0 63 103 63 barrier', 16384)
    check('/fill 0 100 0 63 103 63 air', 16384)
    check('/fill 0 99 0 63 99 63 stone', 4096)
    check('/scoreboard objectives add allay_heal dummy')
    for i, (uuid, tag, x, no_ai, health) in enumerate(actors):
        check(f'/execute if block {int(x)} 99 16 stone')
        check(f'/execute if block {int(x)} 101 16 air')
        check(f'/summon allay {x} 101 16.5 '
              f'{{UUID:[I;312,0,0,{i + 1}],Tags:["{tag}"],'
              f'NoAI:{int(no_ai)}b,NoGravity:1b,Silent:1b,'
              f'PersistenceRequired:1b,Health:{health:g}f}}')
        check(f'/execute if data entity {uuid} '
              f'{{UUID:[I;312,0,0,{i + 1}],Pos:[{x}d,101d,16.5d]}}')
        check(f'/data get entity {uuid} Pos', 3)
    out.extend(setup('/execute store result score #start allay_heal run time query gametime'))
    sample()
    for target in (1, 9, 10, 11, 19, 20):
        step = target - elapsed
        check(f'/tick step {step}')
        out.append({'op': 'tick', 'n': step})
        elapsed = target
        out.extend(setup('/execute store result score #clock allay_heal run time query gametime'))
        check('/scoreboard players operation #clock allay_heal -= #start allay_heal', elapsed)
        sample()
    for uuid, *_ in actors:
        out.extend(setup(f'/kill {uuid}'))
    out.extend(setup('/fill 0 99 0 63 103 63 air',
                     '/scoreboard objectives remove allay_heal',
                     '/forceload remove 0 0 63 63', '/tick unfreeze'))
    return out
