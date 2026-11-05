"""Allay liked-player damage immunity through the connected owner's commands.

Every hit has a fresh actor. The scene has zero observation ticks, and the
matching memory is copied from the actual command player's UUID.
"""


def allay_player_damage(cmd, setup):
    out = []
    source = '0000013a-0000-0000-0000-000100000001'
    memory = 'Brain.memories."minecraft:liked_player"'

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    def rejected(command):
        op = cmd(command)
        op['expect_callback'] = [False, 0]
        out.append(op)

    def clock():
        out.extend(setup('/execute store result score #clock allay_hit run time query gametime'))
        check('/scoreboard players operation #clock allay_hit -= #start allay_hit', 0)

    check('/tick freeze')
    check('/execute if dimension minecraft:overworld')
    check('/gamerule minecraft:spawn_mobs', 0)
    out.extend(setup('/forceload remove 16 16 31 31'))
    check('/forceload add 16 16 31 31')
    out.append({'op': 'settle'})
    check('/execute if loaded 16 101 16')
    check('/fill 16 100 16 31 104 31 barrier', 1280)
    check('/fill 16 100 16 31 104 31 air', 1280)
    check('/fill 16 99 16 31 99 31 barrier', 256)
    check('/fill 16 99 16 31 99 31 stone', 256)
    for pos, count in [('16 100 16 16 104 31', 80), ('31 100 16 31 104 31', 80),
                       ('17 100 16 30 104 16', 70), ('17 100 31 30 104 31', 70)]:
        check(f'/fill {pos} barrier', count)
    for x, z in [(16, 22), (31, 22), (22, 16), (22, 31)]:
        check(f'/execute if block {x} 102 {z} barrier')
    check('/execute if block 22 99 22 stone')
    check('/execute if block 22 101 22 air')
    check('/tp @s 20.5 102 20.5')
    check('/execute if entity @a')
    check('/execute if entity @s[type=player]')
    player = '{Health:20f,playerGameType:1,Pos:[20.5d,102d,20.5d]}'
    check(f'/execute if data entity @s {player}')
    check('/data get entity @s UUID', 4)
    check('/execute unless data entity @s {UUID:[I;10,20,30,40]}')
    check('/data modify storage allay_hit:identity actual set from entity @s UUID')
    check('/data get storage allay_hit:identity actual', 4)
    check('/summon pig 24.5 101 24.5 {UUID:[I;314,0,1,1],Tags:["allay_hit_source"],'
          'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:10f}')
    check(f'/execute as {source} if entity @s[type=pig,tag=allay_hit_source,'
          'nbt={UUID:[I;314,0,1,1],Health:10f,Pos:[24.5d,101d,24.5d]}]')
    check('/scoreboard objectives add allay_hit dummy')
    out.extend(setup('/execute store result score #start allay_hit run time query gametime'))
    cases = [('player', 'minecraft:player_attack by @s', True),
             ('generic_player', 'minecraft:generic by @s', True),
             ('generic', 'minecraft:generic', False),
             ('player_type_only', 'minecraft:player_attack', False),
             ('mob', f'minecraft:mob_attack by {source}', False)]
    ids = []
    for liked in ('absent', 'matching', 'different'):
        for name, damage, by_player in cases:
            index = len(ids) + 1
            uuid = f'0000013a-0000-0000-0000-{index:012x}'
            ids.append(uuid)
            tag = f'allay_hit_{liked}_{name}'
            check('/summon allay 22.5 101 22.5 '
                  f'{{UUID:[I;314,0,0,{index}],Tags:["{tag}"],NoAI:1b,'
                  'NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:20f}')
            if liked != 'absent':
                check(f'/data modify entity {uuid} {memory} set value {{value:[I;10,20,30,40]}}')
                if liked == 'matching':
                    check(f'/data modify entity {uuid} {memory}.value '
                          'set from storage allay_hit:identity actual')
                check(f'/data get entity {uuid} {memory}.value', 4)
            else:
                check(f'/execute unless data entity {uuid} {memory}')
            check(f'/execute if entity @e[type=allay,tag={tag}]')
            fields = (f'UUID:[I;314,0,0,{index}],Health:20f,HurtTime:0s,NoAI:1b,'
                      'NoGravity:1b,Silent:1b,PersistenceRequired:1b,Pos:[22.5d,101d,22.5d]')
            check(f'/execute as {uuid} if entity @s[type=allay,tag={tag},nbt={{{fields}}}]')
            check(f'/execute as {uuid} unless entity @s[nbt={{Invulnerable:1b}}]')
            check(f'/data get entity {uuid} UUID', 4)
            check(f'/data get entity {uuid} Health', 20)
            check(f'/data get entity {uuid} Pos', 3)
            clock()
            out.extend(setup('/scoreboard players set #r allay_hit 31',
                             '/scoreboard players set #s allay_hit 17'))
            check('/scoreboard players get #r allay_hit', 31)
            check('/scoreboard players get #s allay_hit', 17)
            hit = (f'/execute store result score #r allay_hit store success score #s allay_hit '
                   f'run damage {uuid} 2 {damage}')
            accepted = not (liked == 'matching' and by_player)
            (check if accepted else rejected)(hit)
            check('/scoreboard players get #r allay_hit', int(accepted))
            check('/scoreboard players get #s allay_hit', int(accepted))
            check(f'/data get entity {uuid} Health', 18 if accepted else 20)
            check(f'/data get entity {uuid} HurtTime', 10 if accepted else 0)
            fields = (f'UUID:[I;314,0,0,{index}],Health:{18 if accepted else 20}f,'
                      f'HurtTime:{10 if accepted else 0}s,Pos:[22.5d,101d,22.5d]')
            check(f'/execute if data entity {uuid} {{{fields}}}')
            if liked != 'absent':
                check(f'/data get entity {uuid} {memory}.value', 4)
            else:
                check(f'/execute unless data entity {uuid} {memory}')
            check(f'/execute if data entity @s {player}')
            clock()
    for uuid in ids + [source]:
        out.extend(setup(f'/kill {uuid}'))
    out.extend(setup('/fill 16 99 16 31 104 31 air',
                     '/data remove storage allay_hit:identity actual',
                     '/scoreboard objectives remove allay_hit',
                     '/forceload remove 16 16 31 31', '/tick unfreeze'))
    return out
