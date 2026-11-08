"""A loaded sulfur cube's body primes it one tick after it is loaded (H249).

`SulfurCube.tick` runs `tickFuse`, then `primeWhenOnPoweredPosition` (`canExplode()`: the body's
`explosionData`, alive, not primed; and a signal at or beside its block), then `super.tick()`, whose
`LivingEntity.tick` → `detectEquipmentUpdates` → `collectEquipmentChanges` adopts the body (it sets
`explosionData`). A cube that is loaded already holding a TNT body has no `explosionData` on its first
tick, so it can prime only on its second; a cube whose body was adopted earlier primes on the first tick
the power reaches it, after that tick's `tickFuse`.

The world is frozen; ticks are stepped. Each NoAI cube (fixed UUID, a TNT body) stands on stone in its
own region: `immediate` is summoned on a redstone block; `warmed` is summoned unpowered, stepped 5
ticks, then a redstone block is set beside it; `unpowered` never has power. Each cube's `fuse` is read
after every tick, then the immediate cube runs on to its blast (`mob_griefing` off: no block breaks).

Label classes, declared before any native run: `target-` rows read the immediate cube's fuse on its
first three ticks; `diag-` rows are recorded diagnostics (clocks); every other row is a setup admission
or a control (the warmed cube's priming, the unpowered cube, the countdown, the blast's removal).
"""

Y = 100
Z = 8
# name, UUID tail, x
CUBES = [('immediate', 1, 4), ('warmed', 2, 20), ('unpowered', 3, 36)]
BODY = 'equipment:{body:{id:"minecraft:tnt",count:1}}'


def uuid(n):
    return f'0000b249-0000-0000-0000-00000000000{n}'


def sulfur_prime_phase(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def step(label):
        cmd('/tick step 1', label)
        ops.append({'op': 'tick', 'n': 1})

    def reads(tag, names=None):
        for name, n, x in CUBES:
            if names and name not in names:
                continue
            prefix = 'target-' if name == 'immediate' and tag in ('t1', 't2', 't3') else ''
            cmd(f'/data get entity {uuid(n)} fuse', f'{prefix}{name}-{tag}-fuse', admission=False)
            cmd(f'/execute if entity {uuid(n)}', f'{name}-{tag}-alive', admission=False)

    cmd('/tick freeze', 'freeze')
    for rule, value in [('random_tick_speed', '0'), ('spawn_mobs', 'false'), ('advance_time', 'false'),
                        ('advance_weather', 'false'), ('mob_griefing', 'false'), ('tnt_explodes', 'true')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/forceload add 0 0 47 15', 'force', 3)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0], [1, 0], [2, 0]]})
    for c in range(3):
        cmd(f'/execute if loaded {c * 16 + 8} {Y} {Z}', f'loaded-{c}', 1)
    cmd(f'/fill 0 {Y - 2} 0 47 {Y - 1} 15 stone', 'floor', 1536)
    cmd(f'/fill 0 {Y} 0 47 {Y + 4} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    cmd('/kill @e[type=item]', 'clear-items', admission=False, diagnostic=True)
    # The immediate cube's power: the block it stands on.
    cmd(f'/setblock 4 {Y - 1} {Z} redstone_block', 'immediate-power', 1)
    cmd('/scoreboard objectives add sp dummy', 'objective')
    cmd('/execute store result score start sp run time query gametime', 'diag-clock-start', admission=False,
        diagnostic=True)
    for name, n, x in CUBES:
        cmd(f'/summon sulfur_cube {x + 0.5} {Y} {Z + 0.5} {{NoAI:1b,PersistenceRequired:1b,Silent:1b,'
            f'UUID:[I;45641,0,0,{n}],{BODY}}}', f'{name}-summon', admission=False)
        cmd(f'/execute as {uuid(n)} if entity @s[type=sulfur_cube,x={x + 0.5},y={Y},z={Z + 0.5},distance=..0.01]',
            f'{name}-at', 1)
        for field in ('NoAI', 'Size', 'equipment', 'fuse', 'Health'):
            cmd(f'/data get entity {uuid(n)} {field}', f'{name}-{field.lower()}-t0', admission=False)
        cmd(f'/execute if block {x} {Y - 1} {Z} {"redstone_block" if name == "immediate" else "stone"}',
            f'{name}-under', 1)
    for t in (1, 2, 3):
        step(f'step-{t}')
        reads(f't{t}')
    cmd('/execute store result score now sp run time query gametime', 'diag-clock-3', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation now sp -= start sp', 'clock-3', 3)
    # The warmed cube: its body adopted long ago, then power beside it.
    step('step-4')
    step('step-5')
    cmd(f'/setblock 21 {Y} {Z} redstone_block', 'warmed-power', 1)
    for t in (6, 7, 8):
        step(f'step-{t}')
        reads(f't{t}', ('warmed', 'unpowered'))
    # The immediate cube's countdown to its blast.
    for t in range(9, 131):
        step(f'step-{t}')
        if t % 10 == 0 or t >= 115:
            reads(f't{t}', ('immediate',))
    cmd('/execute if entity @e[type=sulfur_cube]', 'cubes-after', admission=False)
    cmd('/execute store result score now sp run time query gametime', 'diag-clock-end', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation now sp -= start sp', 'clock-end', 130)
    return ops
