"""A mob landing on the edge of a lower block falls on the block that supports it.

`Entity.move` refreshes the supporting block (`setOnGroundWithMovement` → `checkSupportingBlock`:
`findSupportingBlock` over the 1e-6 slab under the box) before `checkFallDamage`, whose block is
`getOnPosLegacy()`, the supporting block when there is one. So a mob whose centre is over a honey block
(collision top 15/16) but whose box rests on the adjacent stone (top 16/16) lands on the stone: the plain
fall, not honey's `fallOn` (`causeFallDamage(fallDistance, 0.2F)`).

The world is frozen; ticks are stepped one at a time. Three pigs with AI (a NoAI mob does not move, not even fall) and a
`movement_speed` of 0 (they cannot walk), with fixed UUIDs and `Health:10f`, are
dropped from 6 blocks above a floor, each in its own chunk: `edge` with its centre 0.4 into a honey block
and its box's edge over the stone beside it; `stone` fully over stone (the support control); `honey`
fully over honey. Every tick each pig's `OnGround`, `Pos`, `Motion`, `fall_distance` and `Health` are
read. Java's resolved support is private: that the edge pig rests on the stone is the geometry of the
two blocks' tops (an inference, not a measurement).

Label classes, declared before any native run: `target-` rows read the edge pig's Health after it lands;
`trace-` rows are the per-tick raw state of every pig; `diag-` rows are recorded diagnostics; every other
row is a setup admission or a control.
"""

Y = 100
Z = 8
DROP = 6
TICKS = 24
# name, UUID tail, block x, pig x, floor blocks [(dx, block)]
SCENES = [
    ('edge', 1, 8, 8.9, [(0, 'honey_block'), (1, 'stone')]),
    ('stone', 2, 24, 24.5, [(0, 'stone'), (1, 'stone')]),
    ('honey', 3, 40, 40.5, [(0, 'honey_block'), (1, 'honey_block')]),
]


def uuid(n):
    return f'00010a4d-0000-0000-0000-00000000000{n}'


def mob_landing_support(full=False):
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
                        ('advance_weather', 'false'), ('fall_damage', 'true')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/forceload add 0 0 47 15', 'force', 3)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0], [1, 0], [2, 0]]})
    for c in range(3):
        cmd(f'/execute if loaded {c * 16 + 8} {Y} {Z}', f'loaded-{c}', 1)
    cmd(f'/fill 0 {Y - 2} 0 47 {Y - 2} 15 stone', 'base', 768)
    cmd(f'/fill 0 {Y} 0 47 {Y + DROP + 3} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    cmd('/kill @e[type=item]', 'clear-items', admission=False, diagnostic=True)
    for name, n, x, px, floor in SCENES:
        for dx, block in floor:
            cmd(f'/setblock {x + dx} {Y - 1} {Z} {block}', f'{name}-floor-{dx}', 1)
            cmd(f'/execute if block {x + dx} {Y - 1} {Z} {block}', f'{name}-floor-{dx}-is', 1)
        cmd(f'/execute if block {x - 1} {Y - 1} {Z} air', f'{name}-floor-left-air', 1)
        cmd(f'/summon pig {px} {Y + DROP} {Z + 0.5} {{PersistenceRequired:1b,Silent:1b,Health:10f,'
            f'attributes:[{{id:"minecraft:movement_speed",base:0.0d}}],'
            f'Rotation:[0f,0f],Motion:[0d,0d,0d],UUID:[I;68173,0,0,{n}]}}', f'{name}-summon', admission=False)
        cmd(f'/execute as {uuid(n)} if entity @s[type=pig,x={px},y={Y + DROP},z={Z + 0.5},distance=..0.0001]',
            f'{name}-at', 1)
        for field in ('Health', 'NoAI', 'OnGround', 'fall_distance', 'Motion', 'Pos', 'attributes[{id:"minecraft:movement_speed"}].base'):
            cmd(f'/data get entity {uuid(n)} {field}', f'{name}-{field.split("[")[0].lower()}-before', admission=False)
    cmd('/scoreboard objectives add ml dummy', 'objective')
    cmd('/execute store result score start ml run time query gametime', 'diag-clock-start', admission=False,
        diagnostic=True)
    for tick in range(1, TICKS + 1):
        cmd('/tick step 1', f'step-{tick}')
        ops.append({'op': 'tick', 'n': 1})
        for name, n, *_ in SCENES:
            for field in ('OnGround', 'Pos', 'Motion', 'fall_distance', 'Health'):
                cmd(f'/data get entity {uuid(n)} {field}', f'trace-{name}-t{tick:02d}-{field.lower()}',
                    admission=False)
    for name, n, *_ in SCENES:
        prefix = 'target-' if name == 'edge' else ''
        cmd(f'/data get entity {uuid(n)} Health', f'{prefix}{name}-health-after', admission=False)
        cmd(f'/data get entity {uuid(n)} HurtTime', f'{prefix}{name}-hurt-time-after', admission=False)
    cmd('/execute store result score now ml run time query gametime', 'diag-clock-now', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation now ml -= start ml', 'clock', TICKS)
    return ops
