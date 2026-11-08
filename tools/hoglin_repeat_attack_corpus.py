"""An adult hoglin that retaliates keeps attacking its attacker.

`HoglinAi.wasHurtBy` → `maybeRetaliate` makes the attacker the `ATTACK_TARGET`; the fight activity's
`MeleeAttack.create(40)` hits it again whenever it is in reach and the 40-tick cooldown is over, and
`StopAttackingIfTargetInvalid` keeps the target while it is alive and attackable. Each hit is
`HoglinBase.hurtAndThrowTarget` (damage, then a throw).

The world is frozen; ticks are stepped. Three glass pens on warped nylium, each in its own chunk, hold an
adult hoglin (AI on, `IsImmuneToZombification`, fixed UUID) and a NoAI pig (fixed UUID) beside it. The
pig has `max_health` 100 and Health 100, so it lives through several hits. As in the original scene, a
neighbouring pen `near` (16 blocks away, behind glass) holds a hoglin hit by its own pig first, in the same
tick. In `hit` the pig hits the
hoglin once (`/damage <hoglin> 1 minecraft:mob_attack by <pig>`); `still` gets no hit (the control). The
creative player stands far away. No repellent is within reach.

The accepted-hit fact is the pig's own: `HurtTime` is 10 on the tick after a hit it took. Its Health is
read too, but each hit's amount is the hoglin's damage roll (`attack / 2 + nextInt(attack)`).

Label classes, declared before any native run: `target-` rows are the hit pen's pig `HurtTime` every
tick and the pig at the end; `rand-` rows are recorded and compared, but hold a random amount (the hit
pen's and the near pen's pig Health); `diag-` rows are recorded diagnostics (positions, raw bodies, clock); every other row
is a setup admission or a control (the still pen, the hits, the near pen's HurtTime and end).
"""

Y = 100
Z = 8
WARMUP = 40
TICKS = 200
# name, UUID tail, pen x0, hit
PENS = [('near', 3, 0, True), ('hit', 1, 16, True), ('still', 2, 32, False)]


def hoglin(n):
    return f'00006093-0000-0000-0000-{n:012x}'


def pig(n):
    return f'00006093-0000-0000-0000-{10 + n:012x}'


def hoglin_repeat_attack(full=False):
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
                        ('advance_weather', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/difficulty normal', 'difficulty-set', admission=False)
    cmd('/difficulty', 'difficulty', 2)
    cmd('/forceload add 0 0 47 15', 'force', 3)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0], [1, 0], [2, 0]]})
    for c in range(3):
        cmd(f'/execute if loaded {c * 16 + 8} {Y} {Z}', f'loaded-{c}', 1)
    cmd('/gamemode creative', 'creative', admission=False)
    cmd(f'/tp @s 16.5 {Y} 120.5 0 0', 'player-away', admission=False)
    cmd(f'/execute if entity @s[x=16.5,y={Y},z=120.5,distance=..0.001]', 'player-at', 1)
    cmd(f'/fill 0 {Y - 1} 0 47 {Y - 1} 15 warped_nylium', 'floor', 768)
    cmd(f'/fill 0 {Y} 0 47 {Y + 4} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    for name, n, x0, _ in PENS:
        cmd(f'/fill {x0 + 4} {Y} 4 {x0 + 10} {Y + 2} 12 glass', f'{name}-walls', admission=False)
        cmd(f'/fill {x0 + 5} {Y} 5 {x0 + 9} {Y + 2} 11 air', f'{name}-inside', 105)
        cmd(f'/execute if block {x0 + 4} {Y + 1} 8 glass', f'{name}-wall-is', 1)
        cmd(f'/execute if block {x0 + 6} {Y - 1} {Z} warped_nylium', f'{name}-floor-is', 1)
        for bx in (x0 + 6, x0 + 7):
            for by in (Y, Y + 1):
                cmd(f'/execute if block {bx} {by} {Z} air', f'{name}-air-{bx - x0}-{by - Y}', 1)
        cmd(f'/summon hoglin {x0 + 6.5} {Y} {Z + 0.5} {{PersistenceRequired:1b,IsImmuneToZombification:1b,'
            f'Health:40f,Rotation:[0f,0f],Motion:[0d,0d,0d],UUID:[I;24723,0,0,{n}]}}', f'{name}-hoglin',
            admission=False)
        sturdy = 'attributes:[{id:"minecraft:max_health",base:100d}],Health:100f,'
        cmd(f'/summon pig {x0 + 7.5} {Y} {Z + 0.5} {{NoAI:1b,PersistenceRequired:1b,Silent:1b,'
            f'{sturdy}'
            f'Rotation:[0f,0f],Motion:[0d,0d,0d],UUID:[I;24723,0,0,{10 + n}]}}', f'{name}-pig', admission=False)
        cmd(f'/execute as {hoglin(n)} if entity @s[type=hoglin,x={x0 + 6.5},y={Y},z={Z + 0.5},distance=..0.001]',
            f'{name}-hoglin-is', 1)
        cmd(f'/execute as {pig(n)} if entity @s[type=pig,x={x0 + 7.5},y={Y},z={Z + 0.5},distance=..0.001]',
            f'{name}-pig-is', 1)
        # AI on: an absent NoAI reads as no element, so the admission is a predicate.
        cmd(f'/execute as {hoglin(n)} if entity @s[nbt=!{{NoAI:1b}}]', f'{name}-hoglin-ai', 1)
        for field in ('Health', 'Age', 'IsImmuneToZombification'):
            cmd(f'/data get entity {hoglin(n)} {field}', f'{name}-hoglin-{field.lower()}', admission=False)
        for field in ('Health', 'NoAI', 'attributes[{id:"minecraft:max_health"}].base'):
            cmd(f'/data get entity {pig(n)} {field}', f'{name}-pig-{field.split("[")[0].lower()}', admission=False)
    cmd('/scoreboard objectives add hp dummy', 'objective', 1)
    cmd('/execute store result score #start hp run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)
    for tick in range(1, WARMUP + 1):
        cmd('/tick step 1', f'warm-{tick}', admission=False)
        ops.append({'op': 'tick', 'n': 1})
    # As in the original scene: the neighbouring pen's hoglin is hit first, in the same tick.
    cmd(f'/damage {hoglin(3)} 1 minecraft:mob_attack by {pig(3)}', 'near-hit', admission=False)
    cmd(f'/data get entity {hoglin(3)} Health', 'near-hoglin-health-after-hit', admission=False)
    cmd(f'/damage {hoglin(1)} 1 minecraft:mob_attack by {pig(1)}', 'hit-hit', admission=False)
    cmd(f'/data get entity {hoglin(1)} Health', 'hit-hoglin-health-after-hit', admission=False)
    cmd(f'/data get entity {hoglin(1)} HurtTime', 'hit-hoglin-hurttime-after-hit', admission=False)
    for tick in range(1, TICKS + 1):
        cmd('/tick step 1', f'step-{tick}', admission=False)
        ops.append({'op': 'tick', 'n': 1})
        cmd(f'/data get entity {pig(1)} HurtTime', f'target-hit-t{tick:03d}-pig-hurttime', admission=False)
        cmd(f'/data get entity {pig(1)} Health', f'rand-hit-t{tick:03d}-pig-health', admission=False)
        # The neighbouring pen's pig: its hits and Health every tick.
        cmd(f'/data get entity {pig(3)} HurtTime', f'near-t{tick:03d}-pig-hurttime', admission=False)
        cmd(f'/data get entity {pig(3)} Health', f'rand-near-t{tick:03d}-pig-health', admission=False)
        if tick % 10 == 0:
            cmd(f'/data get entity {pig(2)} Health', f'still-t{tick:03d}-pig-health', admission=False)
            cmd(f'/data get entity {pig(2)} HurtTime', f'still-t{tick:03d}-pig-hurttime', admission=False)
            for name, n, _, _ in PENS:
                cmd(f'/data get entity {hoglin(n)} Pos', f'diag-{name}-t{tick:03d}-hoglin-pos', admission=False,
                    diagnostic=True)
                cmd(f'/data get entity {pig(n)} Pos', f'diag-{name}-t{tick:03d}-pig-pos', admission=False,
                    diagnostic=True)
    for name, n, _, _ in PENS:
        prefix = 'target-' if name == 'hit' else ''
        cmd(f'/execute if entity {pig(n)}', f'{prefix}{name}-pig-exists-end', admission=False)
        cmd(f'/execute as {hoglin(n)} if entity @s[type=hoglin]', f'{name}-hoglin-exists-end', admission=False)
        cmd(f'/data get entity {hoglin(n)}', f'diag-{name}-hoglin-body-end', admission=False, diagnostic=True)
        cmd(f'/data get entity {pig(n)}', f'diag-{name}-pig-body-end', admission=False, diagnostic=True)
    cmd('/execute store result score #now hp run time query gametime', 'diag-clock-now', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation #now hp -= #start hp', 'clock', WARMUP + TICKS)
    return ops
