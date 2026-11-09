"""Actual downward piston pushes onto slime, including the blocked move (#12091)."""


def piston_bounce(cmd, setup):
    ops = setup('/tick freeze', '/gamerule spawn_mobs false',
                '/gamerule advance_time false', '/gamerule random_tick_speed 0',
                '/forceload add 0 0')
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    ops += setup('/tp @s 1.5 200 1.5', '/gamemode creative @s',
                 '/kill @e[type=!player]', '/fill 0 199 0 15 199 15 stone',
                 '/fill 0 200 0 15 208 15 air',
                 '/scoreboard objectives add piston_bounce dummy',
                 '/execute store result score start piston_bounce run time query gametime')
    cases = [('slime_block', 'slime_block', False),
             ('slime_block', 'stone', False),
             ('stone', 'slime_block', False),
             ('slime_block', 'slime_block', True)]
    for index, (floor, top, no_ai) in enumerate(cases):
        x, z = 4 + index % 2 * 6, 4 + index // 2 * 6
        selector = f'@e[tag=piston_bounce_{index},limit=1]'
        for command in (f'/setblock {x} 200 {z} {floor}',
                        f'/setblock {x} 204 {z} {top}',
                        f'/setblock {x} 205 {z} piston[facing=down]',
                        f'/summon wither_skeleton {x}.5 201 {z}.5 '
                        '{PersistenceRequired:1b,Invulnerable:1b,Motion:[0d,0d,0d],'
                        f'NoAI:{int(no_ai)}b,Tags:["piston_bounce_{index}"]' + '}'):
            ops.append(cmd(command, expect_result=1))
        ops += setup(f'/attribute {selector} movement_speed base set 0')
        fields = (f'Pos:[{x}.5d,201d,{z}.5d],Motion:[0d,0d,0d],'
                  'Health:20f' + (',NoAI:1b' if no_ai else ''))
        ops.append(cmd(f'/execute if entity @e[tag=piston_bounce_{index},limit=1,type=wither_skeleton,nbt={{' + fields + '}]',
                       expect_result=1))
        ops.append(cmd(f'/execute unless entity @e[tag=piston_bounce_{index},nbt={{NoGravity:1b}}]',
                       expect_result=1))
        if not no_ai:
            ops.append(cmd(f'/execute unless entity @e[tag=piston_bounce_{index},nbt={{NoAI:1b}}]',
                           expect_result=1))
    for tick in range(7):
        if tick == 1:
            for x, z in [(4, 4), (10, 4), (4, 10), (10, 10)]:
                ops.append(cmd(f'/setblock {x + 1} 205 {z} redstone_block', expect_result=1))
        if tick == 3:
            # Remove the ceiling so the bounce becomes observable travel.
            for x, z in [(4, 4), (10, 4), (4, 10), (10, 10)]:
                ops += setup(f'/fill {x} 203 {z} {x} 205 {z} air')
                for y in (203, 204, 205):
                    ops.append(cmd(f'/execute if block {x} {y} {z} air', expect_result=1))
        if tick:
            ops += setup('/tick step 1')
            ops.append({'op': 'tick', 'n': 1})
        ops += setup('/execute store result score now piston_bounce run time query gametime',
                     '/scoreboard players operation now piston_bounce -= start piston_bounce')
        ops.append(cmd('/scoreboard players get now piston_bounce', expect_result=tick))
        for index in range(len(cases)):
            selector = f'@e[tag=piston_bounce_{index},limit=1]'
            for field in ('Pos', 'Motion', 'OnGround'):
                ops.append(cmd(f'/data get entity {selector} {field}'))
            if tick in (1, 2):
                x, z = 4 + index % 2 * 6, 4 + index // 2 * 6
                ops.append(cmd(f'/execute if block {x} 205 {z} piston[facing=down,extended=true]',
                               expect_result=1))
                ops.append(cmd(f'/data get block {x} 204 {z} progress'))
    return ops + setup('/kill @e[type=!player]', '/fill 0 200 0 15 208 15 air',
                       '/scoreboard objectives remove piston_bounce', '/forceload remove 0 0',
                       '/tick unfreeze')
