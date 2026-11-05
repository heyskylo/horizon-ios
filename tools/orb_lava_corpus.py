"""Server lava damage: exact health/absence; random trajectories stay diagnostic."""
try:
    from .command_exec_corpus import cmd, setup
except ImportError:
    from command_exec_corpus import cmd, setup


def orb_lava(raw=False):
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
            cmd('/fill 8 200 2 10 201 4 glass', expect_result=18),
            cmd('/setblock 9 201 3 lava', expect_result=1)]
    for p, b in [('9 201 3', 'lava[level=0]'), ('9 200 3', 'glass'),
                 ('8 201 3', 'glass'), ('10 201 3', 'glass'),
                 ('9 201 2', 'glass'), ('9 201 4', 'glass'),
                 ('15 199 3', 'stone'), ('15 205 3', 'air')]:
        out.append(cmd(f'/execute if block {p} minecraft:{b}', expect_result=1))
    out += setup('/scoreboard objectives add orb_audit dummy')
    actors = [('lava', '9.5 201.1 3.5', 1), ('air', '15.5 205 3.5', 2)]
    for name, pos, n in actors:
        out.append(cmd(f'/summon experience_orb {pos} '
                       f'{{UUID:[I;305,0,0,{n}],Tags:["h305_{name}"],Health:5s,Age:0s,'
                       'Value:1s,Count:1,NoGravity:0b,Motion:[0d,0d,0d]}', expect_result=1))
    out += setup('/execute store result score start orb_audit run time query gametime')
    for at, n in [(0, 0), (1, 1), (2, 1), (5, 3), (20, 15), (40, 20)]:
        if n:
            out += [cmd(f'/tick step {n}', expect_result=1), {'op': 'tick', 'n': n}]
        out += setup('/execute store result score elapsed orb_audit run time query gametime',
                     '/scoreboard players operation elapsed orb_audit -= start orb_audit')
        out += [cmd('/scoreboard players get elapsed orb_audit', expect_result=at),
                cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[24.5d,210d,24.5d],playerGameType:3}]', expect_result=1)]
        for name, _, ident in actors:
            uuid = f'00000131-0000-0000-0000-{ident:012x}'
            alive = name == 'air' or at < 2
            health = 1 if name == 'lava' and at == 1 else 5
            op = cmd(f'/execute if entity {uuid}')
            op['expect_callback'] = [alive, int(alive)]
            out.append(op)
            out += setup(f'/scoreboard players set count orb_audit 0',
                         f'/execute as @e[type=experience_orb,tag=h305_{name}] run scoreboard players add count orb_audit 1')
            out.append(cmd('/scoreboard players get count orb_audit', expect_result=int(alive)))
            if alive:
                out.append(cmd(f'/execute as {uuid} if entity @s[type=experience_orb,tag=h305_{name},nbt={{Health:{health}s,Age:{at}s,Value:1s,Count:1}}]', expect_result=1))
                out.append(cmd(f'/execute as {uuid} unless entity @s[nbt={{NoGravity:1b}}]', expect_result=1))
                for field in ['Health', 'Age', 'Value', 'Count']:
                    out.append(cmd(f'/data get entity {uuid} {field}'))
                if name == 'air' or at == 0 or raw:
                    for field in ['Pos', 'Motion']:
                        out.append(cmd(f'/data get entity {uuid} {field}'))
            else:
                out.append(cmd(f'/execute unless entity {uuid}', expect_result=1))
                if raw:
                    for field in ['Health', 'Pos', 'Motion']:
                        out.append(cmd(f'/data get entity {uuid} {field}'))
    # One-tick contact boundaries distinguish the swept box from a feet test.
    for x in [24, 28]:
        out += [cmd(f'/setblock {x} 200 12 glass', expect_result=1),
                cmd(f'/setblock {x} 201 12 lava', expect_result=1),
                cmd(f'/execute if block {x} 201 12 lava[level=0]', expect_result=1)]
    for name, pos, n in [('side', '23.8 201.1 12.5', 3),
                         ('above', '28.5 201.95 12.5', 4)]:
        out.append(cmd(f'/summon experience_orb {pos} '
                       f'{{UUID:[I;305,0,0,{n}],Tags:["h305_{name}"],Health:5s,Age:0s,'
                       'Value:1s,Count:1,Motion:[0d,0d,0d]}', expect_result=1))
        out.append(cmd(f'/execute as 00000131-0000-0000-0000-{n:012x} if entity @s[type=experience_orb,tag=h305_{name},nbt={{Health:5s,Age:0s,Value:1s,Count:1}}]', expect_result=1))
    out += [cmd('/tick step 1', expect_result=1), {'op': 'tick', 'n': 1}]
    out += setup('/execute store result score elapsed orb_audit run time query gametime',
                 '/scoreboard players operation elapsed orb_audit -= start orb_audit')
    out.append(cmd('/scoreboard players get elapsed orb_audit', expect_result=41))
    for name, n, health in [('side', 3, 1), ('above', 4, 5)]:
        uuid = f'00000131-0000-0000-0000-{n:012x}'
        out.append(cmd(f'/execute as {uuid} if entity @s[type=experience_orb,tag=h305_{name},nbt={{Health:{health}s,Age:1s,Value:1s,Count:1}}]', expect_result=1))
        out.append(cmd(f'/data get entity {uuid} Health'))
        if raw:
            out += [cmd(f'/data get entity {uuid} Pos'), cmd(f'/data get entity {uuid} Motion')]
    return out
