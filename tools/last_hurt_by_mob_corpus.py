"""A mob hurt by another mob saves who hurt it.

`LivingEntity.resolveMobResponsibleForDamage`: damage whose source entity is a living entity (and not
`#no_anger`) calls `setLastHurtByMob`, an `EntityReference` and `tickCount` as its timestamp.
`LivingEntity.baseTick` clears it once the attacker is not alive or more than 100 ticks have passed.
`addAdditionalSaveData` writes `last_hurt_by_mob` (the reference) and `ticks_since_last_hurt_by_mob`
while it is set; `readAdditionalSaveData` reads both back.

The world is frozen; ticks are stepped. A NoAI pig (`victim`) and two NoAI cows (`a`, `b`) have fixed
UUIDs, in one loaded chunk. The pig takes no hit, then `generic` damage (no entity), then a hit from `a`,
then one from `b`, 20 ticks apart (past the damage cooldown), then 110 ticks pass. A second NoAI pig
(`echo`) is summoned with `last_hurt_by_mob` naming `a` already in its data: a value read back from the
summon, not from a hit, then ages past 100 ticks.

Label classes, declared before any native run: `target-` rows are the precise `last_hurt_by_mob` and
`ticks_since_last_hurt_by_mob` reads after each action; `body-` rows are the victim's full saved body
after each action (recorded and compared, but holding fields outside this task); `diag-` rows are
recorded diagnostics (clock); every other row is a setup admission or a control.
"""

Y = 100


def uuid(n):
    return f'00006094-0000-0000-0000-{n:012x}'


VICTIM, A, B, ECHO = uuid(1), uuid(2), uuid(3), uuid(4)


def last_hurt_by_mob(full=False):
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

    def read(label, who=VICTIM):
        for field in ('last_hurt_by_mob', 'ticks_since_last_hurt_by_mob'):
            cmd(f'/data get entity {who} {field}', f'target-{label}-{field}', admission=False)
        cmd(f'/data get entity {who} Health', f'{label}-health', admission=False)
        cmd(f'/data get entity {who}', f'body-{label}', admission=False)

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
    actors = [('victim', 'pig', 1, 4.5, ''), ('a', 'cow', 2, 8.5, ''), ('b', 'cow', 3, 12.5, ''),
              ('echo', 'pig', 4, 4.5, ',last_hurt_by_mob:[I;24724,0,0,2]')]
    for name, kind, n, x, extra in actors:
        z = 12.5 if name == 'echo' else 4.5
        cmd(f'/summon {kind} {x} {Y} {z} {{NoAI:1b,PersistenceRequired:1b,Silent:1b,Health:10f,'
            f'Rotation:[0f,0f],Motion:[0d,0d,0d],UUID:[I;24724,0,0,{n}]{extra}}}', f'{name}-summon',
            admission=False)
        cmd(f'/execute as {uuid(n)} if entity @s[type={kind},x={x},y={Y},z={z},distance=..0.001]',
            f'{name}-is', 1)
        cmd(f'/data get entity {uuid(n)} Health', f'{name}-health-start', admission=False)
    cmd('/scoreboard objectives add lh dummy', 'objective', 1)
    cmd('/execute store result score #start lh run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)
    read('none')
    read('echo-0', ECHO)
    step(1, 'settle')
    read('echo-1', ECHO)
    cmd(f'/damage {VICTIM} 1 minecraft:generic', 'generic-hit', admission=False)
    read('generic')
    step(20, 'after-generic')
    cmd(f'/damage {VICTIM} 1 minecraft:mob_attack by {A}', 'a-hit', admission=False)
    read('a')
    step(10, 'after-a')
    read('a-10')
    step(10, 'after-a-more')
    cmd(f'/damage {VICTIM} 1 minecraft:mob_attack by {B}', 'b-hit', admission=False)
    read('b')
    step(110, 'after-b')
    read('b-110')
    read('echo-end', ECHO)
    cmd('/execute store result score #now lh run time query gametime', 'diag-clock-now', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation #now lh -= #start lh', 'clock', 151)
    return ops
