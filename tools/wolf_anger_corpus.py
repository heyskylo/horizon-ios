"""Live anger writes, with a separate unmodified NoAI clock-drift diagnostic.

All ends are derived from the observed scene clock. Metadata observations read
actual accessor/writer state; they do not assert network delivery.
"""

ACTORS = [
    ('wolf', 'loaded', 1, 4.5, 4.5, 8, 22),
    ('bee', 'loaded', 2, 10.5, 4.5, 10, 19),
    ('wolf', 'written', 3, 4.5, 10.5, 8, 22),
    ('bee', 'written', 4, 10.5, 10.5, 10, 19),
]


def functions():
    out = {'parity_anger:ready': 'return 23\n',
           'parity_anger:write': '$return run data merge entity $(target) {anger_end_time:$(end)L}\n'}
    for kind, scene, number, x, z, health, _ in ACTORS:
        if scene == 'loaded':
            out[f'parity_anger:load_{kind}'] = (
                f'$return run summon {kind} {x} 200 {z} '
                f'{{UUID:[I;316,0,0,{number}],Tags:["anger_{scene}_{kind}"],'
                'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,'
                f'Health:{health}f,anger_end_time:$(end)L}}\n')
    return out


def groups(cmd, setup, clock_diagnostic=False):
    canonical, diagnostic = [], []

    def check(out, command, result=1):
        out.append(cmd(command, expect_result=result))

    def failed(out, command):
        op = cmd(command)
        op['expect_callback'] = [False, 0]
        out.append(op)

    def clock(out, elapsed=0):
        out.extend(setup('/execute store result score #now anger_write run time query gametime'))
        check(out, '/scoreboard players operation #now anger_write -= #clock anger_write', elapsed)

    def end(out, delta):
        out.extend([cmd('/scoreboard players operation #end anger_write = #clock anger_write'),
                    cmd(f'/scoreboard players add #end anger_write {delta}'),
                    cmd('/scoreboard players operation #delta anger_write = #end anger_write')])
        check(out, '/scoreboard players operation #delta anger_write -= #clock anger_write', delta)
        out.append(cmd('/execute store result storage parity_anger:args end long 1 '
                       'run scoreboard players get #end anger_write'))
        out.append(cmd('/data get storage parity_anger:args end'))

    def snapshot(out, actor, label, delta, elapsed=0):
        kind, scene, number, x, z, health, index = actor
        uuid = f'0000013c-0000-0000-0000-{number:012x}'
        fields = (f'UUID:[I;316,0,0,{number}],Health:{health}f,NoAI:1b,NoGravity:1b,'
                  f'PersistenceRequired:1b,Pos:[{x}d,200d,{z}d],Motion:[0d,0d,0d]')
        check(out, f'/execute as {uuid} if entity @s[type={kind},tag=anger_{scene}_{kind},nbt={{{fields}}}]')
        out.append(cmd(f'/data get entity {uuid} anger_end_time'))
        out.append({'op': 'mob_anger', 'uuid': uuid, 'kind': f'minecraft:{kind}', 'index': index,
                    'label': f'{scene}_{kind}_{label}',
                    'expect_anger': {'uuid': [316, 0, 0, number], 'kind': f'minecraft:{kind}',
                                     'index': index, 'delta': delta}})
        clock(out, elapsed)

    def write(out, command, accepted=True):
        out.extend(setup('/scoreboard players set #r anger_write 31',
                         '/scoreboard players set #s anger_write 17'))
        check(out, '/scoreboard players get #r anger_write', 31)
        check(out, '/scoreboard players get #s anger_write', 17)
        wrapped = ('/execute store result score #r anger_write store success score #s anger_write '
                   f'run {command}')
        (check if accepted else failed)(out, wrapped)
        check(out, '/scoreboard players get #r anger_write', int(accepted))
        check(out, '/scoreboard players get #s anger_write', int(accepted))

    check(canonical, '/tick freeze')
    check(canonical, '/gamerule minecraft:spawn_mobs', 0)
    check(canonical, '/execute if dimension minecraft:overworld')
    canonical.extend(setup('/forceload remove 0 0 31 31'))
    check(canonical, '/forceload add 0 0 31 31', 4)
    check(canonical, '/tp @s 10.5 202 20.5')
    canonical.append({'op': 'settle', 'required_chunks': [[0, 0], [1, 0], [0, 1], [1, 1]]})
    for x, z in [(0, 0), (16, 0), (0, 16), (16, 16)]:
        check(canonical, f'/execute if loaded {x} 200 {z}')
    for block in ['barrier', 'stone']:
        check(canonical, f'/fill 0 199 0 31 199 31 {block}', 1024)
    for block in ['barrier', 'air']:
        check(canonical, f'/fill 0 200 0 31 205 31 {block}', 6144)
    check(canonical, '/execute if data entity @s {playerGameType:1,Health:20f,Pos:[10.5d,202d,20.5d]}')
    canonical.append(cmd('/datapack list enabled'))
    check(canonical, '/function parity_anger:ready', 23)
    check(canonical, '/scoreboard objectives add anger_write dummy')
    # Align through actual pre-actor ticks; startup loading clocks are not assumed equal.
    canonical.extend(setup('/gamerule minecraft:random_tick_speed 0',
                           '/execute store result score #before anger_write run time query gametime'))
    check(canonical, '/execute if score #before anger_write matches 0..1023')
    check(canonical, '/gamerule minecraft:random_tick_speed', 0)
    # Direct permission-bearing commands: functions cannot schedule tick steps here.
    for _ in range(1024):
        canonical.extend(setup('/execute store result score #align anger_write run time query gametime',
                               '/execute if score #align anger_write matches ..1023 run tick step 1'))
        canonical.append({'op': 'tick', 'n': 1})
    canonical.extend(setup('/execute store result score #clock anger_write run time query gametime'))
    check(canonical, '/scoreboard players get #clock anger_write', 1024)
    check(canonical, '/execute if score #clock anger_write matches 0..')
    check(canonical, '/tp @s 10.5 202 20.5')
    check(canonical, '/execute if data entity @s {playerGameType:1,Health:20f,Pos:[10.5d,202d,20.5d]}')
    check(canonical, '/data modify storage parity_anger:args end set value 0L')
    end(canonical, 600)
    for actor in ACTORS:
        kind, scene, number, x, z, health, _ = actor
        uuid = f'0000013c-0000-0000-0000-{number:012x}'
        check(canonical, f'/execute if block {int(x)} 199 {int(z)} stone')
        check(canonical, f'/execute if block {int(x)} 200 {int(z)} air')
        if scene == 'loaded':
            check(canonical, f'/function parity_anger:load_{kind} with storage parity_anger:args')
        else:
            check(canonical, f'/summon {kind} {x} 200 {z} '
                  f'{{UUID:[I;316,0,0,{number}],Tags:["anger_{scene}_{kind}"],'
                  f'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:{health}f}}')
        check(canonical, f'/execute if entity @e[type={kind},tag=anger_{scene}_{kind}]')
        check(canonical, f'/execute if data entity {uuid} {{Silent:1b}}')
        canonical.append(cmd(f'/data get entity {uuid} UUID', expect_result=4))
        snapshot(canonical, actor, 'initial', 600 if scene == 'loaded' else None)
        snapshot(canonical, actor, 'no_write', 600 if scene == 'loaded' else None)
        if scene == 'loaded':
            continue
        write(canonical, f'data merge entity {uuid} {{anger_end_time:-1L}}', False)
        snapshot(canonical, actor, 'unchanged_calm', None)
        check(canonical, f'/data modify storage parity_anger:args target set value "{uuid}"')
        end(canonical, 50)
        write(canonical, 'function parity_anger:write with storage parity_anger:args')
        snapshot(canonical, actor, 'positive50', 50)
        write(canonical, 'function parity_anger:write with storage parity_anger:args', False)
        snapshot(canonical, actor, 'unchanged_positive', 50)
        for air in [299, 300]:
            write(canonical, f'data merge entity {uuid} {{Air:{air}s}}')
            check(canonical, f'/execute if data entity {uuid} {{Air:{air}s}}')
            snapshot(canonical, actor, f'unrelated_air{air}', 50)
        end(canonical, 75)
        write(canonical, 'function parity_anger:write with storage parity_anger:args')
        snapshot(canonical, actor, 'positive75', 75)
        write(canonical, f'data merge entity {uuid} {{anger_end_time:-1L}}')
        snapshot(canonical, actor, 'calmed', None)
    clock(canonical)

    # H262 is outside the live-write gate. Keep its raw absolute ends, unnormalized.
    check(diagnostic, '/tick step 2')
    diagnostic.append({'op': 'tick', 'n': 2, 'record': True, 'label': 'anger-clock-two-ticks'})
    clock(diagnostic, 2)
    for actor in ACTORS:
        snapshot(diagnostic, actor, 'after_two_ticks', 598 if actor[1] == 'loaded' else None, 2)
    for kind, scene, number, *_ in ACTORS:
        diagnostic.extend(setup(f'/kill 0000013c-0000-0000-0000-{number:012x}'))
    cleanup = setup('/fill 0 199 0 31 205 31 air', '/data remove storage parity_anger:args',
                    '/scoreboard objectives remove anger_write', '/forceload remove 0 0 31 31',
                    '/gamerule minecraft:random_tick_speed 3', '/tick unfreeze')
    if clock_diagnostic:
        return [('wolf anger live writes', canonical), ('wolf anger clock diagnostic', diagnostic + cleanup)]
    return [('wolf anger live writes', canonical + cleanup)]
