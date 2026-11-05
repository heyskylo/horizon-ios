"""Generic Mob home codec and one real fence-leash home, independent of AI motion."""
try:
    from .command_exec_corpus import cmd, setup
except ImportError:
    from command_exec_corpus import cmd, setup


def mob_home(holder_removal_diagnostic=False, late_detach_control=False):
    out = setup('/tick freeze', '/gamerule minecraft:spawn_mobs false',
                '/gamerule minecraft:advance_time false', '/gamemode spectator',
                '/tp @s 12.5 210 12.5 0 0')
    out += [cmd('/forceload add 0 0 15 15', expect_result=1), {'op': 'settle'},
            cmd('/execute if loaded 0 201 0', expect_result=1),
            cmd('/fill 0 201 0 15 205 15 stone', expect_result=1280),
            cmd('/fill 0 201 0 15 205 15 air', expect_result=1280),
            cmd('/fill 0 200 0 15 200 15 stone', expect_result=256)]
    out += setup('/scoreboard objectives add home_audit dummy',
                 '/execute store result score start home_audit run time query gametime')

    def actor(n, extra='', species='pig'):
        uuid = f'00000132-0000-0000-0000-{n:012x}'
        pos = '3.5 201 0.5' if n == 30 else '8.5 201 8.5'
        out.append(cmd(f'/summon {species} {pos} '
                       f'{{UUID:[I;306,0,0,{n}],Tags:["h306_{n}"],NoAI:1b,PersistenceRequired:1b'
                       f'{"," if extra else ""}{extra}}}', expect_result=1))
        return uuid

    def identity(uuid, n, species='pig', health=10):
        pos = '3.5d,201d,0.5d' if n == 30 else '8.5d,201d,8.5d'
        out.append(cmd(f'/execute as {uuid} if entity @s[type={species},tag=h306_{n},nbt={{NoAI:1b,Health:{health}f,Pos:[{pos}]}}]', expect_result=1))

    def home(uuid, radius=None, pos=None):
        for key in ['home_radius', 'home_pos']:
            out.append(cmd(f'/data get entity {uuid} {key}'))
        if radius is None:
            for key in ['home_radius', 'home_pos']:
                op = cmd(f'/execute if data entity {uuid} {key}')
                op['expect_callback'] = [False, 0]
                out.append(op)
        else:
            coords = ','.join(map(str,pos))
            out.append(cmd(f'/execute as {uuid} if entity @s[nbt={{home_radius:{radius},home_pos:[I;{coords}]}}]', expect_result=1))

    cases = [('',None,None), ('home_pos:[I;4,5,6]',None,None),
             ('home_radius:0,home_pos:[I;4,5,6]',0,[4,5,6]),
             ('home_radius:5,home_pos:[I;4,5,6]',5,[4,5,6]),
             ('home_radius:-1,home_pos:[I;4,5,6]',None,None),
             ('home_radius:-2,home_pos:[I;4,5,6]',-2,[0,0,0]),
             ('home_radius:0',0,[0,0,0]),
             ('home_radius:5,home_pos:[I;4,5]',5,[0,0,0]),
             ('home_radius:5,home_pos:"bad"',5,[0,0,0]),
             ('home_radius:-2',-2,[0,0,0]),
             ('home_radius:5,home_pos:[4,5,6]',5,[4,5,6]),
             ('home_radius:5,home_pos:[-2.5d,5.5d,6.5d]',5,[-2,5,6]),
             ('home_radius:-2.5f,home_pos:[I;4,5,6]',-3,[0,0,0]),
             ('home_radius:"bad",home_pos:[I;4,5,6]',None,None)]
    for n,(extra,radius,pos) in enumerate(cases,1):
        uuid=actor(n,extra);identity(uuid,n);home(uuid,radius,pos)
        out.append(cmd(f'/kill {uuid}',expect_result=1))
    uuid=actor(20,'home_radius:5,home_pos:[I;4,5,6]')
    identity(uuid,20);home(uuid,5,[4,5,6])
    for suffix,radius,pos in [
        ('modify home_radius set value -2',-2,[4,5,6]),
        ('remove home_radius',None,None),
        ('modify home_radius set value -2',-2,[4,5,6]),
        ('modify home_radius set value 0',0,[4,5,6]),
        ('remove home_pos',0,[0,0,0]),
        ('merge {home_radius:5,home_pos:[I;7,8,9]}',5,[7,8,9]),
        ('modify home_radius set value -1',None,None),
        ('modify home_radius set value -2',-2,[7,8,9])]:
        if suffix.startswith('merge '):
            command=f'/data merge entity {uuid} '+suffix[6:]
        elif suffix.startswith('remove '):
            command=f'/data remove entity {uuid} '+suffix[7:]
        else:
            command=f'/data modify entity {uuid} '+suffix[7:]
        out += setup('/scoreboard players set result home_audit -99',
                     '/scoreboard players set success home_audit -99')
        out += [cmd('/scoreboard players get result home_audit',expect_result=-99),
                cmd('/scoreboard players get success home_audit',expect_result=-99),
                cmd(f'/execute store result score result home_audit store success score success home_audit run {command[1:]}'),
                cmd('/scoreboard players get result home_audit'),cmd('/scoreboard players get success home_audit')]
        identity(uuid,20);home(uuid,radius,pos)
    out.append(cmd(f'/kill {uuid}',expect_result=1))
    out += setup('/execute store result score elapsed home_audit run time query gametime',
                 '/scoreboard players operation elapsed home_audit -= start home_audit')
    out.append(cmd('/scoreboard players get elapsed home_audit',expect_result=0))
    # The only observation tick belongs to the actual fence-leash route.
    out.append(cmd('/setblock 0 201 0 oak_fence',expect_result=1))
    uuid=actor(30,'leash:[I;0,201,0]')
    identity(uuid,30);home(uuid)
    out.append(cmd(f'/data get entity {uuid} leash'))
    out += [cmd('/tick step 1',expect_result=1),{'op':'tick','n':1}]
    identity(uuid,30);home(uuid,5,[0,201,0])
    if holder_removal_diagnostic:
        out.append(cmd('/setblock 0 201 0 air',expect_result=1))
        out += [cmd('/tick step 1',expect_result=1),{'op':'tick','n':1}]
        identity(uuid,30)
        # Java's knot still exists at this measured boundary. Keep its raw
        # getters and presence checks as a separate timing diagnostic.
        out += [cmd(f'/data get entity {uuid} home_radius'),
                cmd(f'/data get entity {uuid} home_pos')]
        for key in ['home_radius', 'home_pos']:
            out.append(cmd(f'/execute if data entity {uuid} {key}',expect_result=1))
    if late_detach_control:
        out.append(cmd('/setblock 0 201 0 air',expect_result=1))
        out += [cmd('/tick step 110',expect_result=1),{'op':'tick','n':110}]
        identity(uuid,30);home(uuid)
    out += setup('/execute store result score elapsed home_audit run time query gametime',
                 '/scoreboard players operation elapsed home_audit -= start home_audit')
    out.append(cmd('/scoreboard players get elapsed home_audit',expect_result=111 if late_detach_control else 2 if holder_removal_diagnostic else 1))
    return out
