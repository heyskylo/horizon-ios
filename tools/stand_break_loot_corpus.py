"""A burnt-out armor stand rolls its entity loot table.

`ArmorStand.hurtServer` takes 4 health off a burning stand (`#burns_armor_stands`, `on_fire`, from
`Entity.baseTick` every 20 fire ticks); at 0.5 or below `causeDamage` calls `brokenByAnything` and `kill`.
`brokenByAnything` plays the break sound, calls `dropAllDeathLoot` (the entity loot table
`minecraft:entities/armor_stand`, when `mob_drops` is on) and then pops each equipment stack without
`PREVENT_EQUIPMENT_DROP` (curse of vanishing) above the stand's block.

A world pack overrides `minecraft:entities/armor_stand` with one stick. The world is frozen; ticks are
stepped. Each stand has a fixed UUID, `Fire:200s`, `Health:0.6f` and a diamond helmet, on a stone floor in
its own region: a normal stand (with vanishing-cursed boots too), a small one, one that burns while
`mob_drops` is off, and one removed by `/kill`. A direct `/loot spawn` of the same table, in a region of
its own, shows the table the pack installed.

Label classes, declared before any native run: `target-` rows read the sticks a burnt stand drops; `diag-`
rows are recorded diagnostics (clocks, random item motion); every other row is a setup admission or a
control (the helmet and boots, the removal, the direct loot, `mob_drops` off, `/kill`).
"""

Y = 100
Z = 8
TABLE = {'type': 'minecraft:entity', 'pools': [{'rolls': 1, 'entries': [
    {'type': 'minecraft:item', 'name': 'minecraft:stick'}]}]}
# name, UUID tail, x, extra NBT
STANDS = [
    ('normal', 1, 4, ',feet:{id:"minecraft:leather_boots",count:1,components:{"minecraft:enchantments":'
                     '{"minecraft:vanishing_curse":1}}}'),
    ('small', 2, 12, ''),
    ('nodrops', 3, 20, ''),
    ('kill', 4, 28, ''),
]
LOOT_X = 40


def uuid(n):
    return f'00005b1e-0000-0000-0000-00000000000{n}'


def stand_break_loot(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def step(n, label):
        cmd(f'/tick step {n}', label)
        ops.append({'op': 'tick', 'n': n})

    cmd('/tick freeze', 'freeze')
    for rule, value in [('random_tick_speed', '0'), ('spawn_mobs', 'false'), ('advance_time', 'false'),
                        ('advance_weather', 'false'), ('mob_drops', 'true')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/datapack list enabled', 'pack-enabled', admission=False)
    cmd('/forceload add 0 0 47 15', 'force', 3)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0], [1, 0], [2, 0]]})
    for c in range(3):
        cmd(f'/execute if loaded {c * 16 + 8} {Y} {Z}', f'loaded-{c}', 1)
    cmd(f'/fill 0 {Y - 1} 0 47 {Y - 1} 15 stone', 'floor', 768)
    cmd(f'/fill 0 {Y} 0 47 {Y + 4} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    cmd('/kill @e[type=item]', 'clear-items', admission=False, diagnostic=True)
    cmd('/execute if entity @e[type=item]', 'no-items')
    # The direct table: a stick far from every stand.
    cmd(f'/loot spawn {LOOT_X + 0.5} {Y} {Z + 0.5} loot minecraft:entities/armor_stand', 'direct-loot',
        admission=False)
    cmd(f'/execute if entity @e[type=item,x={LOOT_X + 0.5},y={Y},z={Z + 0.5},distance=..2]', 'direct-loot-count',
        admission=False)
    cmd(f'/data get entity @e[type=item,limit=1,x={LOOT_X + 0.5},y={Y},z={Z + 0.5},distance=..2] Item',
        'direct-loot-item', admission=False)
    cmd('/scoreboard objectives add sb dummy', 'objective')
    cmd('/execute store result score start sb run time query gametime', 'diag-clock-start', admission=False,
        diagnostic=True)

    def summon(name, n, x, extra):
        small = ',Small:1b' if name == 'small' else ''
        cmd(f'/summon armor_stand {x + 0.5} {Y} {Z + 0.5} {{NoGravity:1b,Fire:200s,Health:0.6f,'
            f'UUID:[I;23326,0,0,{n}]{small},equipment:{{head:{{id:"minecraft:diamond_helmet",count:1}}{extra}}}}}',
            f'{name}-summon', admission=False)
        cmd(f'/execute as {uuid(n)} if entity @s[type=armor_stand,x={x + 0.5},y={Y},z={Z + 0.5},distance=..0.01]',
            f'{name}-at', 1)
        cmd(f'/execute if block {x} {Y - 1} {Z} stone', f'{name}-floor', 1)
        for field in ('Health', 'Fire', 'Small', 'Marker', 'Invisible', 'equipment'):
            cmd(f'/data get entity {uuid(n)} {field}', f'{name}-{field.lower()}', admission=False)
        near = f'x={x + 0.5},y={Y},z={Z + 0.5},distance=..3'
        cmd(f'/execute if entity @e[type=item,{near}]', f'{name}-no-items-before', admission=False)

    def reads(name, n, x):
        near = f'x={x + 0.5},y={Y},z={Z + 0.5},distance=..3'
        prefix = 'target-' if name in ('normal', 'small') else ''
        cmd(f'/execute if entity {uuid(n)}', f'{name}-alive-after', admission=False)
        cmd(f'/execute if entity @e[type=item,{near}]', f'{name}-items', admission=False)
        cmd(f'/execute if entity @e[type=item,{near},nbt={{Item:{{id:"minecraft:stick"}}}}]',
            f'{prefix}{name}-sticks', admission=False)
        cmd(f'/data get entity @e[type=item,limit=1,{near},nbt={{Item:{{id:"minecraft:stick"}}}}] Item',
            f'{prefix}{name}-stick-item', admission=False)
        cmd(f'/execute if entity @e[type=item,{near},nbt={{Item:{{id:"minecraft:diamond_helmet"}}}}]',
            f'{name}-helmets', admission=False)
        cmd(f'/data get entity @e[type=item,limit=1,{near},nbt={{Item:{{id:"minecraft:diamond_helmet"}}}}] Item',
            f'{name}-helmet-item', admission=False)
        cmd(f'/execute if entity @e[type=item,{near},nbt={{Item:{{id:"minecraft:leather_boots"}}}}]',
            f'{name}-boots', admission=False)
        cmd(f'/data get entity @e[type=item,limit=1,{near},nbt={{Item:{{id:"minecraft:diamond_helmet"}}}}] Motion',
            f'diag-{name}-helmet-motion', admission=False, diagnostic=True)

    for name, n, x, extra in STANDS[:2]:
        summon(name, n, x, extra)
    # The fire's first hurt: Fire is 200, a multiple of 20.
    step(1, 'burn-step')
    for name, n, x, _ in STANDS[:2]:
        reads(name, n, x)
    cmd('/gamerule mob_drops false', 'mob-drops-off', admission=False)
    name, n, x, extra = STANDS[2]
    summon(name, n, x, extra)
    step(1, 'nodrops-step')
    reads(name, n, x)
    cmd('/gamerule mob_drops true', 'mob-drops-on', admission=False)
    name, n, x, extra = STANDS[3]
    summon(name, n, x, extra)
    cmd(f'/kill {uuid(n)}', 'kill-command', admission=False)
    reads(name, n, x)
    cmd('/execute if entity @e[type=armor_stand]', 'stands-after', admission=False)
    cmd('/execute store result score now sb run time query gametime', 'diag-clock-now', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation now sb -= start sb', 'clock', 2)
    return ops
