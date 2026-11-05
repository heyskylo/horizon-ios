"""Live boat-holder command contract, with an explicit movement diagnostic.

The gate covers subjects, saved links, clocks, relation feedback and callbacks,
including empty pending/unleashed relations. Initial Motion/Rotation and all
post-tick Pos/Motion/Rotation belong only to the raw diagnostic, not physics
parity. Initial positions remain exact positive prerequisites in both modes.
"""


def boat_leasher(cmd, setup, raw_diagnostic=False):
    holder_words = '[I;1,2,3,4]'
    holder = '00000001-0000-0002-0000-000300000004'
    boat = '00000130-0000-0000-0000-000000000001'
    free = '00000130-0000-0000-0000-000000000002'
    mob = '00000130-0000-0000-0000-000000000003'
    actors = [(holder, 'happy_ghast', 'h304_holder', holder_words, '[0.5d,104d,0.5d]', 20),
              (boat, 'oak_boat', 'h304_boat', '[I;304,0,0,1]', '[8.5d,100d,3.5d]', None),
              (free, 'oak_boat', 'h304_free', '[I;304,0,0,2]', '[-12.5d,100d,8.5d]', None),
              (mob, 'pig', 'h304_mob', '[I;304,0,0,3]', '[3.5d,104d,-3.5d]', 10)]
    out = [cmd('/tick freeze', expect_result=1),
           cmd('/gamemode spectator', expect_result=1),
           cmd('/tp @s 0.5 115 0.5', expect_result=1),
           cmd('/forceload add -32 -32 31 31', expect_result=16),
           {'op': 'settle'}]
    out += [cmd(f'/execute if loaded {x} 100 {z}', expect_result=1)
            for x in (-32, -16, 0, 16) for z in (-32, -16, 0, 16)]
    out += [cmd('/fill -32 99 -32 31 99 31 ice', expect_result=4096)]
    # Seed the slabs first so successful clearing is measured, even if the
    # generated terrain was already air. Each volume is below 32768 blocks.
    for lo, hi, volume in ((100, 106, 28672), (107, 112, 24576)):
        out += [cmd(f'/fill -32 {lo} -32 31 {hi} 31 barrier', expect_result=volume),
                cmd(f'/fill -32 {lo} -32 31 {hi} 31 air', expect_result=volume)]
    out += [cmd('/scoreboard objectives add h304 dummy', expect_result=1)]
    out += setup('/execute store result score start h304 run time query gametime')
    common = 'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b'
    out += [cmd('/summon happy_ghast 0.5 104 0.5 {UUID:' + holder_words +
                ',Tags:["h304_holder"],' + common + ',Health:20f,Rotation:[30f,0f]}', expect_result=1),
            cmd('/summon oak_boat 8.5 100 3.5 {UUID:[I;304,0,0,1],Tags:["h304_boat"],'
                'Rotation:[0f,0f],leash:{UUID:' + holder_words + '}}', expect_result=1),
            cmd('/summon oak_boat -12.5 100 8.5 {UUID:[I;304,0,0,2],Tags:["h304_free"],'
                'Rotation:[0f,0f]}', expect_result=1),
            cmd('/summon pig 3.5 104 -3.5 {UUID:[I;304,0,0,3],Tags:["h304_mob"],'
                + common + ',Health:10f,leash:{UUID:' + holder_words + '}}', expect_result=1)]

    def relation(uid, live):
        out.extend(setup('/scoreboard players set result h304 31',
                         '/scoreboard players set success h304 17',
                         '/scoreboard players set seen h304 0'))
        check = 'if entity @s[nbt={UUID:' + holder_words + '}]'
        out.append(cmd(f'/execute as {uid} on leasher {check}',
                       expect_result=1 if live else None))
        out.append(cmd('/execute store result score result h304 store success score success h304 '
                       f'as {uid} on leasher run execute {check}'))
        # Java leaves both sentinels unchanged when the relation has no source.
        out.extend([cmd('/scoreboard players get result h304', expect_result=1 if live else 31),
                    cmd('/scoreboard players get success h304', expect_result=1 if live else 17),
                    cmd(f'/execute as {uid} on leasher run data get entity @s UUID',
                        expect_result=4 if live else None),
                    cmd(f'/execute as {uid} on leasher run scoreboard players add seen h304 1',
                        expect_result=1 if live else None),
                    cmd('/scoreboard players get seen h304', expect_result=int(live))])

    for elapsed, step in ((0, 0), (10, 10), (20, 10), (40, 20)):
        if step:
            out += [cmd(f'/tick step {step}', expect_result=1), {'op': 'tick', 'n': step}]
        out += setup('/execute store result score elapsed h304 run time query gametime',
                     '/scoreboard players operation elapsed h304 -= start h304')
        out += [cmd('/scoreboard players get elapsed h304', expect_result=elapsed),
                cmd('/data get entity @s playerGameType', expect_result=3),
                cmd('/data get entity @s Health', expect_result=20),
                cmd('/data get entity @s Pos', expect_result=3),
                cmd('/execute if entity @s[nbt={Health:20f,Pos:[0.5d,115d,0.5d]}]', expect_result=1)]
        for uid, kind, tag, words, pos, health in actors:
            fields = f'UUID:{words}'
            if health is not None:
                fields += f',Health:{health}f,{common}'
            if not elapsed:
                fields += ',Pos:' + pos
            out.append(cmd(f'/execute as {uid} if entity @s[type={kind},tag={tag},nbt={{{fields}}}]',
                           expect_result=1))
            out.append(cmd(f'/data get entity {uid} UUID', expect_result=4))
            if health is not None:
                out.append(cmd(f'/data get entity {uid} Health', expect_result=health))
            if not elapsed:
                out.append(cmd(f'/data get entity {uid} Pos', expect_result=3))
            if raw_diagnostic:
                for field in (('Motion', 'Rotation') if not elapsed else ('Pos', 'Motion', 'Rotation')):
                    out.append(cmd(f'/data get entity {uid} {field}'))
        for uid in (boat, mob):
            out.append(cmd(f'/data get entity {uid} leash.UUID', expect_result=4))
        out.append(cmd(f'/data get entity {free} leash'))
        relation(boat, elapsed > 0)
        relation(free, False)
        relation(mob, elapsed > 0)
    out += setup('/kill @e[tag=h304_boat]', '/kill @e[tag=h304_free]',
                 '/kill @e[tag=h304_mob]', '/kill @e[tag=h304_holder]',
                 '/scoreboard objectives remove h304', '/forceload remove -32 -32 31 31',
                 '/gamemode creative', '/tp @s 8.5 230 8.5 0 0', '/tick unfreeze')
    return out
