"""Loaded positive/negative glow squid counters across actual world ticks."""

ACTORS = [
    ('noai_positive', 1, 1, 40, 4.5, 4.5),
    ('noai_negative', 2, 1, -2, 10.5, 4.5),
    ('ai_positive', 3, 0, 40, 4.5, 10.5),
    ('ai_negative', 4, 0, -2, 10.5, 10.5),
]


def squid_countdown(full=False):
    ops = []

    def cmd(text, label, result=None, callback=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if callback is not None:
            op['expect_callback'] = callback
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    for rule, value in [('spawn_mobs', 'false'), ('advance_time', 'false'), ('random_tick_speed', '0')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule)
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/fill 0 200 0 15 207 15 glass', 'pool-shell', 2048)
    cmd('/fill 1 201 1 14 206 14 water', 'pool-water', 1176)
    cmd('/tp @s 8.5 210 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h176_clock dummy', 'clock-objective')
    for name, n, noai, dark, x, z in ACTORS:
        cmd(f'/summon glow_squid {x} 203 {z} {{UUID:[I;176,0,0,{n}],Tags:["h176_{name}"],'
            f'NoAI:{noai}b,NoGravity:1b,PersistenceRequired:1b,DarkTicksRemaining:{dark}}}',
            name + '-summon', 1)
    cmd('/execute store result score start h176_clock run time query gametime',
        'clock-start', diagnostic=True)
    elapsed = 0
    for at in [0, 1, 5, 10]:
        if at:
            n = at - elapsed
            cmd(f'/tick step {n}', f'{at}-step')
            ops.append({'op': 'tick', 'n': n})
        elapsed = at
        key = str(at)
        cmd('/execute if entity @s[type=player,name=Player]', key + '-owner', 1)
        for field, value in [('UUID', 4), ('Health', 20), ('Pos', 3), ('playerGameType', 1)]:
            cmd(f'/data get entity @s {field}', key + '-owner-' + field, value)
        for pos, block in [('4 199 4', 'stone'), ('0 203 4', 'glass'), ('4 200 4', 'glass'),
                           ('4 207 4', 'glass'), ('4 203 4', 'water[level=0]')]:
            cmd(f'/execute if block {pos} {block}', key + '-geometry-' + pos, 1)
        cmd('/execute if loaded 4 203 4', key + '-loaded', 1)
        cmd('/execute store result score now h176_clock run time query gametime',
            key + '-clock-raw', diagnostic=True)
        cmd('/scoreboard players operation now h176_clock -= start h176_clock', key + '-clock-delta-op', at)
        cmd('/scoreboard players get now h176_clock', key + '-clock-delta', at)
        for name, n, noai, dark, x, z in ACTORS:
            uuid = f'000000b0-0000-0000-0000-{n:012}'
            label = key + '-' + name
            cmd(f'/execute as {uuid} if entity @s[type=glow_squid,tag=h176_{name}]', label + '-type-tag', 1)
            for field, value in [('UUID', 4), ('Health', 10), ('NoGravity', 1), ('PersistenceRequired', 1)]:
                cmd(f'/data get entity {uuid} {field}', label + '-' + field, value)
            cmd(f'/data get entity {uuid} NoAI', label + '-NoAI',
                result=1 if noai else None, callback=None if noai else [False, 0])
            for dy in ['', '1']:
                cmd(f'/execute as {uuid} at @s if block ~ ~{dy} ~ water[level=0]', label + '-water-' + dy, 1)
            cmd(f'/data get entity {uuid} Pos', label + '-Pos', 3, diagnostic=not noai and at != 0)
            cmd(f'/data get entity {uuid} Motion', label + '-Motion', 3, diagnostic=True)
            cmd(f'/data get entity {uuid} DarkTicksRemaining', label + '-dark',
                max(0, dark - at) if dark > 0 else dark,
                admission=not (noai and dark > 0 and at > 0))
            cmd(f'/data get entity {uuid}', label + '-body', diagnostic=True)
    return ops
