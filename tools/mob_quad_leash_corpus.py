"""Guarded live mob leash dispatch; all numeric feedback compares strictly."""
try:
    from .command_exec_corpus import cmd, setup
except ImportError:
    from command_exec_corpus import cmd, setup


def mob_quad_leash(raw=False):
    out = setup('/tick freeze', '/gamerule minecraft:spawn_mobs false',
                '/gamerule minecraft:advance_time false', '/gamemode spectator',
                '/tp @s 0.5 115 0.5 0 0')
    out += [cmd('/forceload add -32 -32 31 31', expect_result=16), {'op': 'settle'}]
    for x in [-32, -16, 0, 16]:
        for z in [-32, -16, 0, 16]:
            out.append(cmd(f'/execute if loaded {x} 100 {z}', expect_result=1))
    out.append(cmd('/fill -32 99 -32 31 99 31 ice', expect_result=4096))
    # Each valid slab is below32768, unlike the historical53248-block clear.
    for lo, hi, count in [(100, 106, 28672), (107, 112, 24576)]:
        out += [cmd(f'/fill -32 {lo} -32 31 {hi} 31 stone', expect_result=count),
                cmd(f'/fill -32 {lo} -32 31 {hi} 31 air', expect_result=count)]
    out += setup('/scoreboard objectives add leash_audit dummy',
                 '/data modify storage h302:observer uuid set from entity @s UUID')
    out.append(cmd('/data get entity @s UUID', expect_result=4))
    for scene, holder_type in enumerate(['happy_ghast', 'cow'], 1):
        holder_health = 20 if holder_type == 'happy_ghast' else 10
        holder_pos = '0.5 104 0.5' if scene == 1 else '-5.5 100 -4.5'
        holder_nbt_pos = '[0.5d,104d,0.5d]' if scene == 1 else '[-5.5d,100d,-4.5d]'
        holder = f'00000001-0000-0002-0000-00030000000{scene}'
        horse = f'0000012e-0000-0000-0000-00000000000{scene}'
        out += setup('/kill @e[type=!minecraft:player]')
        out += [cmd(f'/summon minecraft:{holder_type} {holder_pos} '
                    f'{{UUID:[I;1,2,3,{scene}],Tags:["h302_holder"],NoAI:1b,NoGravity:1b,'
                    f'Health:{holder_health}f,Rotation:[30f,0f],PersistenceRequired:1b}}', expect_result=1),
                cmd('/summon minecraft:horse -7.5 100 -4.5 '
                    f'{{UUID:[I;302,0,0,{scene}],Tags:["h302_horse"],NoAI:1b,Health:53f,attributes:[{{id:"minecraft:max_health",base:53d}}],'
                    f'Rotation:[90f,0f],PersistenceRequired:1b,leash:{{UUID:[I;1,2,3,{scene}]}}}}', expect_result=1)]
        def guards():
            return [cmd('/data get entity @s playerGameType', expect_result=3),
                cmd('/execute if entity @s[type=player,nbt={playerGameType:3,Health:20f,Pos:[0.5d,115d,0.5d]}]', expect_result=1),
                cmd(f'/data get entity {holder} UUID', expect_result=4),
                cmd(f'/execute as {holder} if entity @s[type=minecraft:{holder_type},tag=h302_holder,nbt={{Health:{holder_health}f,NoAI:1b,NoGravity:1b,Pos:{holder_nbt_pos},Rotation:[30f,0f]}}]', expect_result=1),
                cmd(f'/data get entity {horse} UUID', expect_result=4),
                cmd(f'/execute as {horse} if entity @s[type=minecraft:horse,tag=h302_horse,nbt={{Health:53f,NoAI:1b,Pos:[-7.5d,100d,-4.5d]}}]', expect_result=1),
                cmd('/execute if block -8 99 -5 minecraft:ice', expect_result=1),
                cmd('/execute if block 0 104 0 minecraft:air', expect_result=1)]
        out += guards()
        out += [cmd(f'/data get entity {horse} leash.UUID', expect_result=4),
                cmd(f'/execute as {horse} on leasher run data get entity @s UUID')]
        out += setup('/execute store result score start leash_audit run time query gametime')
        for at, n in [(10, 10), (20, 10), (40, 20)]:
            out += [cmd(f'/tick step {n}', expect_result=1), {'op': 'tick', 'n': n}]
            out += setup('/execute store result score now leash_audit run time query gametime',
                         '/scoreboard players operation now leash_audit -= start leash_audit')
            out += [cmd('/scoreboard players get now leash_audit', expect_result=at)] + guards()
            out += [cmd(f'/execute as {horse} on leasher if entity @s[nbt={{UUID:[I;1,2,3,{scene}]}}]', expect_result=1),
                    cmd(f'/data get entity {horse} leash.UUID', expect_result=4)]
            # Exact three-decimal command contract, not a comparison tolerance.
            # Each successful numeric read has a separately observed success callback.
            for field in ['Rotation[0]', 'Motion[0]', 'Motion[1]', 'Motion[2]']:
                out += setup('/scoreboard players set value leash_audit -99',
                             '/scoreboard players set success leash_audit 17',
                             f'/execute store success score success leash_audit store result score value leash_audit run data get entity {horse} {field} 1000')
                out.append(cmd('/scoreboard players get success leash_audit', expect_result=1))
                op = cmd('/scoreboard players get value leash_audit')
                op['label'] = f'h302 {holder_type}/{at}/{field} x1000'
                out.append(op)
            out.append(cmd(f'/data get entity {horse} Pos'))
            if raw:
                for field in ['Rotation[0]', 'Motion']:
                    op = cmd(f'/data get entity {horse} {field}')
                    op['label'] = f'h302 raw/{holder_type}/{at}/{field}'
                    out.append(op)
    out += setup('/execute store success score changed leash_audit run data modify storage h302:observer uuid set from entity @s UUID')
    out += [cmd('/scoreboard players get changed leash_audit', expect_result=0),
            cmd('/data get entity @s UUID', expect_result=4)]
    return out
