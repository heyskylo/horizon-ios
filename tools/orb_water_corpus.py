"""Exact water/air orb fields; identity uses registration, never saved UUID NBT."""
try:
    from .command_exec_corpus import cmd, setup
except ImportError:
    from command_exec_corpus import cmd, setup


def arithmetic_inputs():
    return [[0.0, 0.0, 0.0], [0.1, 0.01, -0.2], [-0.25, -0.03, 0.125],
            [0.0, 0.059, 0.0], [0.5, 0.0595, -0.5], [-1.0, 0.2, 1.0]]


def orb_water(raw=False):
    out = setup('/tick freeze', '/gamerule minecraft:spawn_mobs false',
                '/gamerule minecraft:advance_time false', '/gamemode spectator',
                '/tp @s 24.5 210 24.5 0 0')
    out += [cmd('/forceload add 0 0 31 31', expect_result=4), {'op': 'settle'}]
    for x in [0, 16]:
        for z in [0, 16]:
            out.append(cmd(f'/execute if loaded {x} 200 {z}', expect_result=1))
    out += [cmd('/fill 0 200 0 31 207 31 stone', expect_result=8192),
            cmd('/fill 0 200 0 31 207 31 air', expect_result=8192),
            cmd('/fill 0 199 0 31 199 31 stone', expect_result=1024),
            cmd('/fill 2 200 2 4 206 4 glass', expect_result=63),
            cmd('/fill 3 200 3 3 206 3 water', expect_result=7)]
    for pos, block in [('3 200 3', 'water[level=0]'), ('3 201 3', 'water[level=0]'),
                       ('3 206 3', 'water[level=0]'), ('2 201 3', 'glass'),
                       ('4 201 3', 'glass'), ('3 201 2', 'glass'), ('3 201 4', 'glass'),
                       ('3 199 3', 'stone'), ('15 199 3', 'stone'), ('15 205 3', 'air')]:
        out.append(cmd(f'/execute if block {pos} minecraft:{block}', expect_result=1))
    out += setup('/scoreboard objectives add orb_audit dummy',
                 '/data modify storage h303:observer uuid set from entity @s UUID')
    out.append(cmd('/data get entity @s UUID', expect_result=4))
    actors = [('water', '0000012f-0000-0000-0000-000000000001', '3.5 201.2 3.5', 1),
              ('air', '0000012f-0000-0000-0000-000000000002', '15.5 205 3.5', 2)]
    for name, uuid, pos, n in actors:
        out.append(cmd(f'/summon minecraft:experience_orb {pos} '
                       f'{{UUID:[I;303,0,0,{n}],Tags:["h303_{name}"],Health:5s,Age:0s,'
                       'Value:1s,Count:1,NoGravity:0b,Motion:[0d,0d,0d]}', expect_result=1))
    out += setup('/execute store result score start orb_audit run time query gametime')
    for at, n in [(0, 0), (1, 1), (5, 4), (20, 15), (40, 20)]:
        if n:
            out += [cmd(f'/tick step {n}', expect_result=1), {'op': 'tick', 'n': n}]
        out += setup('/execute store result score elapsed orb_audit run time query gametime',
                     '/scoreboard players operation elapsed orb_audit -= start orb_audit')
        out += [cmd('/scoreboard players get elapsed orb_audit', expect_result=at),
                cmd('/data get entity @s playerGameType', expect_result=3),
                cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[24.5d,210d,24.5d]}]', expect_result=1)]
        for name, uuid, _, _ in actors:
            # Direct UUID lookup followed by type/tag/typed live fields. A saved
            # UUID read is intentionally absent: H011 is a separate getter bug.
            out += [cmd(f'/execute as {uuid} if entity @s[type=minecraft:experience_orb,tag=h303_{name},nbt={{Health:5s,Age:{at}s,Value:1s,Count:1}}]', expect_result=1),
                    cmd(f'/execute if entity @e[type=minecraft:experience_orb,tag=h303_{name}]', expect_result=1),
                    cmd(f'/execute as {uuid} at @s if block ~ ~ ~ minecraft:{name if name == "water" else "air"}', expect_result=1),
                    cmd(f'/execute as {uuid} unless entity @s[nbt={{NoGravity:1b}}]', expect_result=1)]
            for field in ['Pos[1]', 'Motion[1]'] + (['Pos', 'Motion'] if raw else []):
                op = cmd(f'/data get entity {uuid} {field}')
                op['label'] = f'h303 {name}/{at}/{field}'
                out.append(op)
    out += setup('/execute store success score changed orb_audit run data modify storage h303:observer uuid set from entity @s UUID')
    out += [cmd('/scoreboard players get changed orb_audit', expect_result=0),
            cmd('/data get entity @s UUID', expect_result=4)]
    return out
