"""Saved spectral duration, with independent full read/style diagnostics retained."""
GROUP = 'spectral saved duration'
DIAGNOSTIC_RECORDS = 28


def cases(cmd, setup):
    def diagnostic(command):
        op = cmd(command, record=False)
        op['trace'] = True
        return op
    ops = [cmd('/tick freeze'), cmd('/forceload add 160 160 175 175'),
           {'op': 'wait_chunks', 'chunks': [[10, 10]]},
           cmd('/fill 160 199 160 175 199 175 stone', expect_result=256),
           cmd('/execute if loaded 164 200 164', expect_result=1)]
    ops += setup('/scoreboard objectives add spectral_time dummy',
                 '/execute store result score start spectral_time run time query gametime')
    for n, supplied in enumerate([None, 77, 0, -1, 200, 2147483647, 'ordinary'], 1):
        ordinary = supplied == 'ordinary'
        kind = 'minecraft:arrow' if ordinary else 'minecraft:spectral_arrow'
        expected = None if ordinary else (200 if supplied is None else supplied)
        uuid = f'000000b7-0000-0001-0000-{n:012x}'
        words = [183, 1, 0, n]
        x = 163.5 + n * .25
        tag = f'h183_{n}'
        extra = '' if supplied is None or ordinary else f',Duration:{supplied}'
        ops += [diagnostic(f'/summon {kind} {x} 200 164.5 {{UUID:[I;183,1,0,{n}],Tags:["{tag}"],NoGravity:1b,Motion:[0d,0d,0d]{extra}}}'),
                {'op':'projectile_saved_type','uuid':uuid,'label':tag,'expect_observation':{
                    'uuid':words,'kind':kind,'pos':[x,200.0,164.5],'no_gravity':True}},
                cmd(f'/execute as {uuid} if entity @s[tag={tag}]', expect_result=1),
                cmd(f'/execute if data entity {uuid} {{UUID:[I;183,1,0,{n}],Pos:[{x}d,200d,164.5d],NoGravity:1b}}', expect_result=1),
                diagnostic(f'/execute as {uuid} if entity @s[type={kind}]'),
                diagnostic(f'/data get entity {uuid}')]
        ops += setup('/scoreboard players set duration spectral_time 317',
                     '/scoreboard players set success spectral_time 17')
        # This is a real read with callback stores; never a projectile NBT write.
        ops += [diagnostic(f'/execute store result score duration spectral_time store success score success spectral_time run data get entity {uuid} Duration'),
                cmd('/scoreboard players get duration spectral_time', expect_result=0 if ordinary else expected),
                cmd('/scoreboard players get success spectral_time', expect_result=0 if ordinary else 1)]
        if ordinary:
            ops += [cmd(f'/execute unless data entity {uuid} Duration', expect_result=1)]
        else:
            ops += [cmd(f'/execute if data entity {uuid} {{Duration:{expected}}}', expect_result=1)]
        ops += [cmd(f'/execute if data entity {uuid} {{Pos:[{x}d,200d,164.5d],NoGravity:1b}}', expect_result=1)]
    ops += setup('/execute store result score elapsed spectral_time run time query gametime',
                 '/scoreboard players operation elapsed spectral_time -= start spectral_time')
    return ops + [cmd('/scoreboard players get elapsed spectral_time', expect_result=0)]
