"""Commands see the leash knot a restored fence leash ties (`LeashFenceKnotEntity`).

A mob saved with a fence leash (`leash:[I;x,y,z]`) restores it on its first tick (`Leashable.tickLeash`
→ `restoreLeashFromSave` → `LeashFenceKnotEntity.getOrCreateKnot`): a real knot entity at the fence's
block, centred at x+0.5, y+0.375, z+0.5, which entity selectors and `/data` find like any entity.

The world is frozen; ticks are stepped. A NoAI pig (fixed UUID, ordinary gravity) on stone is saved with
a leash to the oak fence at 8 201 0. A second oak fence at 40 201 0 has an explicitly summoned knot (fixed
UUID), the H129 control, which no leash uses. Reads at tick 0 (before restoration), 1 and 2.

Label classes, declared before any native run: `target-` rows are the restored knot's command view at the
pig's fence (its count, typed selection, position and body fields); `diag-` rows are recorded diagnostics
(the knot's random UUID and full body, clock); every other row is a setup admission or a control (the
explicit knot, the pig, the fences).
"""

Y = 200
PIG = '0000609a-0000-0000-0000-000000000001'
EXPLICIT = '0000609a-0000-0000-0000-000000000002'
NATURAL = f'@e[type=minecraft:leash_knot,x=8.5,y={Y + 1.375},z=0.5,distance=..0.001]'
NATURAL_ANY = f'@e[x=8.5,y={Y + 1.375},z=0.5,distance=..0.001]'
ALL_KNOTS = '@e[type=minecraft:leash_knot]'


def knot_command_view(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'rule-spawn', admission=False)
    cmd('/forceload add -16 -16 63 15', 'force', 10)
    ops.append({'op': 'settle', 'required_chunks': [[x, z] for x in (-1, 0, 1, 2, 3) for z in (-1, 0)]})
    for x in (-1, 0, 1, 2, 3):
        for z in (-1, 0):
            cmd(f'/execute if loaded {x * 16 + 8} {Y} {z * 16 + 8}', f'loaded-{x}-{z}', 1)
    cmd(f'/fill -8 {Y} -8 60 {Y} 8 minecraft:stone', 'floor', 69 * 17)
    cmd(f'/fill -8 {Y + 1} -8 60 {Y + 4} 8 minecraft:air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    for fx in (8, 40):
        cmd(f'/setblock {fx} {Y + 1} 0 minecraft:oak_fence', f'fence-{fx}', 1)
        cmd(f'/execute if block {fx} {Y + 1} 0 minecraft:oak_fence', f'fence-{fx}-is', 1)
    cmd(f'/execute if block 16 {Y} 0 minecraft:stone', 'pig-support', 1)
    cmd(f'/summon minecraft:leash_knot 40.5 {Y + 1.375} 0.5 {{UUID:[I;24730,0,0,2]}}', 'explicit-summon', 1)
    cmd(f'/execute as {EXPLICIT} if entity @s[type=leash_knot,x=40.5,y={Y + 1.375},z=0.5,distance=..0.001]',
        'explicit-at', 1)
    cmd(f'/summon minecraft:pig 16.5 {Y + 1} 0.5 {{NoAI:1b,PersistenceRequired:1b,Health:10f,Rotation:[0f,0f],'
        f'Motion:[0d,0d,0d],leash:[I;8,{Y + 1},0],UUID:[I;24730,0,0,1]}}', 'pig-summon', 1)
    cmd(f'/execute as {PIG} if entity @s[type=pig,x=16.5,y={Y + 1},z=0.5,distance=..0.001]', 'pig-at', 1)
    cmd(f'/execute as {PIG} if entity @s[nbt=!{{NoGravity:1b}}]', 'pig-gravity', 1)
    cmd(f'/data get entity {PIG} leash', 'pig-leash-pending', admission=False)
    cmd('/scoreboard objectives add kv dummy', 'objective', 1)
    cmd('/execute store result score #start kv run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)

    def read(tick):
        cmd(f'/execute if entity {NATURAL}', f'target-t{tick}-natural-knot', admission=False)
        cmd(f'/execute if entity {NATURAL_ANY}', f'target-t{tick}-natural-any-type', admission=False)
        cmd(f'/execute store result score #all kv if entity {ALL_KNOTS}', f'target-t{tick}-all-knots-store',
            admission=False)
        cmd('/scoreboard players get #all kv', f'target-t{tick}-all-knots', admission=False)
        cmd(f'/data get entity {NATURAL.replace("]", ",limit=1]")} Pos', f'target-t{tick}-natural-pos',
            admission=False)
        cmd(f'/data get entity {NATURAL.replace("]", ",limit=1]")} Motion', f'target-t{tick}-natural-motion',
            admission=False)
        cmd(f'/data get entity {NATURAL.replace("]", ",limit=1]")} UUID', f'diag-t{tick}-natural-uuid',
            admission=False, diagnostic=True)
        cmd(f'/data get entity {NATURAL.replace("]", ",limit=1]")}', f'diag-t{tick}-natural-body',
            admission=False, diagnostic=True)
        cmd(f'/execute if entity {EXPLICIT}', f't{tick}-explicit-exists', admission=False)
        cmd(f'/data get entity {EXPLICIT} Pos', f't{tick}-explicit-pos', admission=False)
        cmd(f'/data get entity {PIG} leash', f't{tick}-pig-leash', admission=False)

    read(0)
    for tick in (1, 2):
        cmd('/tick step 1', f'step-{tick}', admission=False)
        ops.append({'op': 'tick', 'n': 1})
        read(tick)
    cmd('/execute store result score #now kv run time query gametime', 'diag-clock-now', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation #now kv -= #start kv', 'clock', 2)
    return ops
