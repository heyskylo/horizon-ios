"""A mob's hurt cooldown is not `invulnerable_time`.

`LivingEntity.hurtServer` keeps the hurt cooldown in `damageCooldownTime` (20 after a hit; while it is
above 10 a hit no stronger than `lastHurt` does nothing and a stronger one deals only the difference).
`baseTick` counts it down; it is never saved. `Entity.invulnerableTime` is another field: `saveWithoutId`
writes it as `invulnerable_time` while it is above 0, `load` reads it, and while it is above 0 the entity
is invulnerable (`isTemporarilyInvulnerable`). A `/data` edit loads the entity in place, so it keeps
the cooldown.

The world is frozen; ticks are stepped. A NoAI pig `victim` (fixed UUID, Health 20) has its whole body
read before a hit, right after a `generic` hit of 2, and at cooldown ticks 9, 10, 11 and 20. A second
pig, `edit`, is hit with 2, then given an unrelated `/data merge` (a custom name) during its cooldown, then
hit with 2 again (equal: nothing) and with 4 (stronger: 2 more). A third, `summoned`, is summoned with
`invulnerable_time:30` and hit with 2 at tick 5 and tick 40.

Label classes, declared before any native run: `target-` rows are the victims' full bodies and their
`invulnerable_time` reads; `diag-` rows are recorded diagnostics (clock); every other row is a setup
admission or a control (the hits, Health, the edit).
"""

Y = 100


def uuid(n):
    return f'00006095-0000-0000-0000-{n:012x}'


VICTIM, EDIT, SUMMONED = uuid(1), uuid(2), uuid(3)


def invulnerable_time(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def step(n, label):
        for i in range(1, n + 1):
            cmd('/tick step 1', f'{label}-{i}', admission=False)
            ops.append({'op': 'tick', 'n': 1})

    def read(label, who):
        cmd(f'/data get entity {who}', f'target-{label}-body', admission=False)
        cmd(f'/data get entity {who} invulnerable_time', f'target-{label}-invulnerable_time', admission=False)
        cmd(f'/data get entity {who} Health', f'{label}-health', admission=False)

    cmd('/tick freeze', 'freeze')
    for rule, value in [('random_tick_speed', '0'), ('spawn_mobs', 'false'), ('advance_time', 'false'),
                        ('advance_weather', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/difficulty normal', 'difficulty-set', admission=False)
    cmd('/difficulty', 'difficulty', 2)
    cmd('/forceload add 0 0 0 0', 'force', admission=False)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/execute if loaded 8 {Y} 8', 'loaded', 1)
    cmd('/gamemode creative', 'creative', admission=False)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd(f'/fill 0 {Y} 0 15 {Y + 3} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    for name, n, x, extra in [('victim', 1, 4.5, ''), ('edit', 2, 8.5, ''),
                              ('summoned', 3, 12.5, ',invulnerable_time:30')]:
        cmd(f'/summon pig {x} {Y} 4.5 {{NoAI:1b,PersistenceRequired:1b,Silent:1b,Health:20f,'
            f'attributes:[{{id:"minecraft:max_health",base:20d}}],'
            f'Rotation:[0f,0f],Motion:[0d,0d,0d],UUID:[I;24725,0,0,{n}]{extra}}}', f'{name}-summon',
            admission=False)
        cmd(f'/execute as {uuid(n)} if entity @s[type=pig,x={x},y={Y},z=4.5,distance=..0.001]', f'{name}-is', 1)
        cmd(f'/data get entity {uuid(n)} Health', f'{name}-health-start', admission=False)
    cmd('/scoreboard objectives add it dummy', 'objective', 1)
    cmd('/execute store result score #start it run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)
    step(1, 'settle')
    read('victim-before', VICTIM)
    read('summoned-1', SUMMONED)
    cmd(f'/damage {VICTIM} 2 minecraft:generic', 'victim-hit', admission=False)
    cmd(f'/damage {EDIT} 2 minecraft:generic', 'edit-hit', admission=False)
    read('victim-after', VICTIM)
    step(4, 'cool')
    # The edit during the cooldown, then the equal and the stronger hit.
    cmd(f'/data merge entity {EDIT} {{CustomName:"edited"}}', 'edit-merge', admission=False)
    read('edit-merged', EDIT)
    cmd(f'/damage {EDIT} 2 minecraft:generic', 'edit-equal-hit', admission=False)
    cmd(f'/data get entity {EDIT} Health', 'edit-health-after-equal', admission=False)
    cmd(f'/damage {EDIT} 4 minecraft:generic', 'edit-stronger-hit', admission=False)
    cmd(f'/data get entity {EDIT} Health', 'edit-health-after-stronger', admission=False)
    cmd(f'/damage {SUMMONED} 2 minecraft:generic', 'summoned-hit-5', admission=False)
    read('summoned-5', SUMMONED)
    step(4, 'to-9')
    read('victim-9', VICTIM)
    step(1, 'to-10')
    read('victim-10', VICTIM)
    step(1, 'to-11')
    read('victim-11', VICTIM)
    step(9, 'to-20')
    read('victim-20', VICTIM)
    read('summoned-21', SUMMONED)
    step(20, 'to-40')
    cmd(f'/damage {SUMMONED} 2 minecraft:generic', 'summoned-hit-40', admission=False)
    read('summoned-40', SUMMONED)
    cmd('/execute store result score #now it run time query gametime', 'diag-clock-now', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation #now it -= #start it', 'clock', 40)
    return ops
