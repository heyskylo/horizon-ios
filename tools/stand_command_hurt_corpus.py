"""/damage on an armor stand: the side effects of a refused hurt.

`DamageCommand.damage` reports success only when `hurtServer` returns
true. `ArmorStand.hurtServer` returns false after some damage sources have
already acted on the stand: an explosion (`#is_explosion`) breaks it
(`brokenByAnything`: its equipment pops above its block) and kills it;
`#ignites_armor_stands` (`in_fire`) sets a stand that is not burning on fire
for 5 seconds; `#burns_armor_stands` (`on_fire`) takes 4 health off a
burning one. The command then fails with "Target is invulnerable".

The world is frozen and no tick runs. Each stand stands on a stone floor in
one forced chunk, with a fixed UUID. Every `/damage` stores its success and
result in fresh scores (set to -1 first). The stand's removal, its dropped
equipment, `Fire` and `Health` are read apart from the command's feedback.

Label classes, declared before any native run: `target-` rows read the
state an explosion leaves (removal, the dropped helmet); `fire-` rows read
the state `in_fire` and `on_fire` leave (`Fire`, `Health`); `diag-` rows are
recorded diagnostics; every other row is a setup admission or a control
that must hold (a refused `generic` hurt and an invisible stand's blast
change nothing, `/kill` removes a stand, a pig takes an explosion's damage).
"""

Y = 100
Z = 8
# name, UUID tail, x, summon data, damage type (None: /kill)
STANDS = [
    ('blast', 1, 2, ',equipment:{head:{id:"minecraft:diamond_helmet",count:1}}', 'explosion'),
    ('ignite', 2, 5, '', 'in_fire'),
    ('burn', 3, 8, ',Fire:200s', 'on_fire'),
    ('generic', 4, 11, ',equipment:{head:{id:"minecraft:iron_helmet",count:1}}', 'generic'),
    ('hidden', 5, 14, ',Invisible:1b,equipment:{head:{id:"minecraft:golden_helmet",count:1}}', 'explosion'),
    ('kill', 6, 17, ',equipment:{head:{id:"minecraft:leather_helmet",count:1}}', None),
]
PIG = ('pig', 7, 20)


def uuid(n):
    return f'00000ad5-0000-0000-0000-00000000000{n}'


def stand_command_hurt(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    for rule, value in [('random_tick_speed', '0'), ('spawn_mobs', 'false'), ('advance_time', 'false'),
                        ('advance_weather', 'false'), ('mob_griefing', 'true')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/forceload add 0 0 31 15', 'force', 2)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0], [1, 0]]})
    for cx in range(2):
        cmd(f'/execute if loaded {cx * 16 + 8} {Y} {Z}', f'loaded-{cx}', 1)
    cmd(f'/fill 0 {Y - 1} 0 31 {Y - 1} 15 stone', 'floor', 512)
    cmd(f'/fill 0 {Y} 0 31 {Y + 3} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    # The loot of what that killed.
    cmd('/kill @e[type=item]', 'clear-items', admission=False, diagnostic=True)
    still = 'NoGravity:1b,Rotation:[0f,0f],Motion:[0d,0d,0d]'
    for name, n, x, extra, _ in STANDS:
        cmd(f'/summon armor_stand {x + 0.5} {Y} {Z + 0.5} {{{still},UUID:[I;2773,0,0,{n}]{extra}}}',
            f'{name}-summon', admission=False)
    name, n, x = PIG
    cmd(f'/summon pig {x + 0.5} {Y} {Z + 0.5} {{{still},NoAI:1b,PersistenceRequired:1b,Silent:1b,Health:10f,'
        f'UUID:[I;2773,0,0,{n}]}}', 'pig-summon', admission=False)
    for name, n, x, *_ in STANDS + [PIG]:
        kind = 'pig' if name == 'pig' else 'armor_stand'
        cmd(f'/execute as {uuid(n)} if entity @s[type={kind},x={x + 0.5},y={Y},z={Z + 0.5},distance=..0.01]',
            f'{name}-at', 1)
        cmd(f'/execute if block {x} {Y - 1} {Z} stone', f'{name}-floor', 1)
        for field in ('Health', 'Fire', 'Invisible', 'Marker', 'equipment'):
            cmd(f'/data get entity {uuid(n)} {field}', f'{name}-{field.lower()}-before', admission=False)
    cmd('/execute if entity @e[type=item]', 'no-items-before')
    cmd('/scoreboard objectives add h_s dummy', 'objective-success')
    cmd('/scoreboard objectives add h_r dummy', 'objective-result')
    cmd('/execute store result score start h_r run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)

    for name, n, x, extra, ty in STANDS:
        prefix = {'blast': 'target-', 'ignite': 'fire-', 'burn': 'fire-'}.get(name, '')
        # One hurt per stand: its result and success stored as it runs.
        cmd(f'/scoreboard players set {name} h_s -1', f'{name}-success-fresh', admission=False)
        cmd(f'/scoreboard players set {name} h_r -1', f'{name}-result-fresh', admission=False)
        run = f'kill {uuid(n)}' if ty is None else f'damage {uuid(n)} 1 minecraft:{ty}'
        cmd(f'/execute store success score {name} h_s store result score {name} h_r run {run}',
            f'{name}-command', admission=False)
        cmd(f'/scoreboard players get {name} h_s', f'{name}-success', admission=False)
        cmd(f'/scoreboard players get {name} h_r', f'{name}-result', admission=False)
        cmd(f'/execute if entity {uuid(n)}', f'{prefix}{name}-alive-after', admission=False)
        for field in ('Health', 'Fire', 'equipment'):
            cmd(f'/data get entity {uuid(n)} {field}', f'{prefix}{name}-{field.lower()}-after', admission=False)
        near = f'@e[type=item,x={x + 0.5},y={Y + 1},z={Z + 0.5},distance=..1.5]'
        cmd(f'/execute if entity {near}', f'{prefix}{name}-drops', admission=False)
        cmd(f'/data get entity {near.replace("]", ",limit=1]")} Item', f'{prefix}{name}-drop-item',
            admission=False)
    name, n, x = PIG
    cmd(f'/scoreboard players set {name} h_s -1', f'{name}-success-fresh', admission=False)
    cmd(f'/scoreboard players set {name} h_r -1', f'{name}-result-fresh', admission=False)
    cmd(f'/execute store success score {name} h_s store result score {name} h_r run '
        f'damage {uuid(n)} 1 minecraft:explosion', f'{name}-command', admission=False)
    cmd(f'/scoreboard players get {name} h_s', f'{name}-success', admission=False)
    cmd(f'/scoreboard players get {name} h_r', f'{name}-result', admission=False)
    cmd(f'/data get entity {uuid(n)} Health', f'{name}-health-after', admission=False)
    cmd('/execute if entity @e[type=item]', 'items-after', admission=False)
    cmd('/execute if entity @e[type=armor_stand]', 'stands-after', admission=False)
    cmd('/execute store result score now h_r run time query gametime', 'diag-clock-now',
        admission=False, diagnostic=True)
    cmd('/scoreboard players operation now h_r -= start h_r', 'clock', 0)
    return ops
