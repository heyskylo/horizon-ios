"""A summoned dragon fireball flies (H127).

`DragonFireball` is an `AbstractHurtingProjectile`: each tick it moves by its motion, then its motion gains
its direction times `acceleration_power` and is scaled by the inertia (0.95 in air). A `/summon`ed one
takes `Motion` and `acceleration_power` from its NBT, as an ordinary `fireball` does.

The world is frozen; ticks are stepped. In the Overworld, in open air and a forceloaded area, a
`dragon_fireball` and a `fireball` (the control; its known floating-point differences, H222, are not
normalised) are summoned with fixed UUIDs, `Motion:[0.1d,0d,0d]` and `acceleration_power:0.1d`, each in its
own chunk. Their `Pos`, `Motion` and `acceleration_power` are read at ticks 0, 1 and 3, and whether they
are still there.

Label classes, declared before any native run: `target-` rows read the dragon fireball's position and
motion after a tick; `diag-` rows are recorded diagnostics (clocks); every other row is a setup admission
or a control (the ordinary fireball, the tick-0 state).
"""

Y = 120
Z = 8
# name, type, UUID tail, x
BALLS = [('dragon', 'dragon_fireball', 1, 8), ('fireball', 'fireball', 2, 40)]


def uuid(n):
    return f'0000d127-0000-0000-0000-00000000000{n}'


def dragon_fireball_motion(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def reads(tag):
        for name, _, n, _ in BALLS:
            prefix = 'target-' if name == 'dragon' and tag != 't0' else ''
            cmd(f'/execute if entity {uuid(n)}', f'{prefix}{name}-{tag}-alive', admission=False)
            for field in ('Pos', 'Motion', 'acceleration_power'):
                cmd(f'/data get entity {uuid(n)} {field}', f'{prefix}{name}-{tag}-{field.lower()}',
                    admission=False)

    cmd('/tick freeze', 'freeze')
    for rule, value in [('random_tick_speed', '0'), ('spawn_mobs', 'false'), ('advance_time', 'false'),
                        ('advance_weather', 'false'), ('mob_griefing', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/forceload add 0 0 63 15', 'force', 4)
    ops.append({'op': 'settle', 'required_chunks': [[c, 0] for c in range(4)]})
    for c in range(4):
        cmd(f'/execute if loaded {c * 16 + 8} {Y} {Z}', f'loaded-{c}', 1)
    cmd(f'/fill 0 {Y - 4} 0 63 {Y + 4} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    cmd('/scoreboard objectives add df dummy', 'objective')
    cmd('/execute store result score start df run time query gametime', 'diag-clock-start', admission=False,
        diagnostic=True)
    for name, kind, n, x in BALLS:
        cmd(f'/summon {kind} {x + 0.5} {Y} {Z + 0.5} {{Motion:[0.1d,0d,0d],acceleration_power:0.1d,'
            f'UUID:[I;53543,0,0,{n}]}}', f'{name}-summon', admission=False)
        cmd(f'/execute as {uuid(n)} if entity @s[type={kind}]', f'{name}-type', 1)
        cmd(f'/data get entity {uuid(n)} Owner', f'{name}-owner', admission=False)
    reads('t0')
    cmd('/tick step 1', 'step-1')
    ops.append({'op': 'tick', 'n': 1})
    reads('t1')
    cmd('/tick step 2', 'step-3')
    ops.append({'op': 'tick', 'n': 2})
    reads('t3')
    cmd('/execute store result score now df run time query gametime', 'diag-clock-now', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation now df -= start df', 'clock', 3)
    return ops
