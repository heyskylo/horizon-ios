"""Loaded husks saved with FallFlying across actual world ticks (H168).

`LivingEntity.aiStep` calls `updateFallFlying` while the glide flag is set:
the flag clears when the mob cannot glide (no glider, or one whose next
damage breaks it), and a glider loses one durability every 20 glide ticks.
Each husk is bound to its own UUID and tag. The NoAI healthy husk is the
stationary control: it keeps its position, so its wear does not depend on
glide travel. The moving husks' position and motion are travel (H169)
diagnostics, recorded but never compared.
"""

# name, UUID suffix, NoAI, chest damage (None: no elytra), x, z
ACTORS = [
    ('bare_ai', 1, 0, None, 2.5, 4.5),
    ('bare_noai', 2, 1, None, 2.5, 10.5),
    ('worn_ai', 3, 0, 431, 7.5, 4.5),
    ('worn_noai', 4, 1, 431, 7.5, 10.5),
    ('healthy_ai', 5, 0, 0, 12.5, 4.5),
    ('healthy_noai', 6, 1, 0, 12.5, 10.5),
]
Y = 300
TICKS = [0, 1, 20, 40]


def uuid_of(n):
    return f'000000a8-0000-0000-0000-{n:012}'


def glide_state(full=False):
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
    for rule, value in [('spawn_mobs', 'false'), ('advance_time', 'false'), ('random_tick_speed', '0'),
                        ('advance_weather', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule)
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 8.5 200 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h168_clock dummy', 'clock-objective')
    for name, n, noai, damage, x, z in ACTORS:
        equipment = ''
        if damage is not None:
            components = f',components:{{"minecraft:damage":{damage}}}' if damage else ''
            equipment = f',equipment:{{chest:{{id:"minecraft:elytra",count:1{components}}}}}'
        cmd(f'/summon husk {x} {Y} {z} {{UUID:[I;168,0,0,{n}],Tags:["h168_{name}"],NoAI:{noai}b,'
            f'PersistenceRequired:1b,FallFlying:1b,Motion:[0d,0d,0d],Rotation:[0f,0f]{equipment}}}',
            name + '-summon', 1)
    cmd('/execute store result score start h168_clock run time query gametime',
        'clock-start', diagnostic=True)
    elapsed = 0
    for at in TICKS:
        if at:
            n = at - elapsed
            cmd(f'/tick step {n}', f'{at}-step')
            ops.append({'op': 'tick', 'n': n})
        elapsed = at
        key = str(at)
        cmd('/execute if entity @s[type=player,name=Player]', key + '-owner', 1)
        for field, value in [('UUID', 4), ('Health', 20), ('playerGameType', 1)]:
            cmd(f'/data get entity @s {field}', key + '-owner-' + field, value)
        cmd('/execute if block 4 199 4 stone', key + '-floor', 1)
        cmd('/execute if loaded 8 250 8', key + '-loaded', 1)
        cmd('/execute store result score now h168_clock run time query gametime',
            key + '-clock-raw', diagnostic=True)
        cmd('/scoreboard players operation now h168_clock -= start h168_clock', key + '-clock-delta-op', at)
        cmd('/scoreboard players get now h168_clock', key + '-clock-delta', at)
        for name, n, noai, damage, x, z in ACTORS:
            uuid = uuid_of(n)
            label = key + '-' + name
            cmd(f'/execute as {uuid} if entity @s[type=husk,tag=h168_{name}]', label + '-type-tag', 1)
            for field, value in [('UUID', 4), ('Health', 20), ('PersistenceRequired', 1)]:
                cmd(f'/data get entity {uuid} {field}', label + '-' + field, value)
            cmd(f'/data get entity {uuid} NoAI', label + '-NoAI',
                result=1 if noai else None, callback=None if noai else [False, 0])
            # The worn equipment, its id guarded; its damage is the wear under test.
            if damage is None:
                cmd(f'/execute unless data entity {uuid} equipment.chest', label + '-no-chest', 1)
            else:
                cmd(f'/execute if data entity {uuid} equipment.chest{{id:"minecraft:elytra",count:1}}',
                    label + '-chest-elytra', 1)
                cmd(f'/data get entity {uuid} equipment.chest.components."minecraft:damage"',
                    label + '-chest-damage', admission=False)
            cmd(f'/data get entity {uuid} FallFlying', label + '-FallFlying', admission=False)
            # NoAI husks stay where they were summoned: the stationary control.
            if noai:
                cmd(f'/execute if data entity {uuid} {{Pos:[{x}d,{Y}.0d,{z}d]}}', label + '-stationary', 1)
            cmd(f'/data get entity {uuid} Pos', label + '-Pos', 3, diagnostic=not noai)
            cmd(f'/data get entity {uuid} Motion', label + '-Motion', 3, diagnostic=True)
            cmd(f'/data get entity {uuid}', label + '-body', diagnostic=True)
    return ops
