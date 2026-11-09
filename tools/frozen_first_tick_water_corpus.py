"""Water placed in newly forceloaded chunks of a frozen world (H175).

`LevelChunk.registerTickContainerInLevel` registers a chunk's block and
fluid tick containers when it becomes a full chunk, before a command can
reach it. So water a command places before the level's first world tick
schedules its flow tick like water placed later. The cold source is placed
before any world tick in those chunks; the warm source after one measured
`/tick step`. Both are read at 5, 6 and 16 world ticks after the cold
placement (the warm source is one tick younger).
"""

CHUNKS = [(cx, cz) for cx in (79, 80, 81) for cz in (79, 80, 81)]
Y = 200
Z = 1283
SOURCES = [('cold', 1283), ('warm', 1291)]


def frozen_first_tick_water(full=False):
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
    for rule, value in [('random_tick_speed', '0'), ('spawn_mobs', 'false'), ('advance_time', 'false'),
                        ('advance_weather', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/scoreboard objectives add h175 dummy', 'objective')
    cmd('/execute store result score start h175 run time query gametime', 'clock-start', diagnostic=True)
    cmd('/forceload add 1264 1264 1311 1311', 'force', 9)
    ops.append({'op': 'settle', 'required_chunks': [list(c) for c in CHUNKS]})
    for cx, cz in CHUNKS:
        cmd(f'/execute if loaded {cx * 16} {Y} {cz * 16}', f'loaded-{cx}-{cz}', 1)

    def clock(label, ticks):
        cmd('/execute store result score now h175 run time query gametime', label + '-clock-raw', diagnostic=True)
        cmd('/scoreboard players operation now h175 -= start h175', label + '-clock-op', ticks)
        cmd('/scoreboard players get now h175', label + '-clock', ticks)

    # No world tick has run in the frozen level since the chunks loaded.
    clock('cold', 0)
    cmd(f'/fill 1270 {Y - 1} 1270 1310 {Y - 1} 1310 stone', 'floor', 1681)
    cmd(f'/fill 1280 {Y} 1280 1295 {Y + 2} 1287 air strict', 'clear', admission=False)
    for name, x in SOURCES:
        cmd(f'/execute if block {x} {Y - 1} {Z} stone', name + '-floor', 1)
        cmd(f'/execute if block {x} {Y} {Z} air', name + '-initial-air', 1)
        cmd(f'/execute if block {x + 1} {Y} {Z} air', name + '-initial-east-air', 1)

    def place(name, x):
        cmd(f'/setblock {x} {Y} {Z} water', name + '-place', 1)
        cmd(f'/execute if block {x} {Y} {Z} water[level=0]', name + '-source', 1)

    place('cold', SOURCES[0][1])
    cmd('/tick step 1', 'warm-step')
    ops.append({'op': 'tick', 'n': 1})
    clock('warm', 1)
    place('warm', SOURCES[1][1])
    elapsed = 1
    for delta in (4, 1, 10):
        cmd(f'/tick step {delta}', f'step-{delta}')
        ops.append({'op': 'tick', 'n': delta})
        elapsed += delta
        key = f'after{elapsed}'
        clock(key, elapsed)
        for name, x in SOURCES:
            label = f'{key}-{name}'
            cmd(f'/execute if block {x} {Y} {Z} water[level=0]', label + '-source', 1)
            cmd(f'/execute if block {x + 1} {Y} {Z} water[level=1]', label + '-flow', admission=False)
            cmd(f'/execute if block {x + 1} {Y} {Z} air', label + '-air', admission=False)
            cmd(f'/execute store success score {name} h175 if block {x + 1} {Y} {Z} water',
                label + '-store-op', admission=False)
            cmd(f'/scoreboard players get {name} h175', label + '-store', admission=False)
            for dx, dz in ((-1, 0), (0, 1), (0, -1)):
                cmd(f'/execute if block {x + dx} {Y} {Z + dz} water', f'{label}-around-{dx}-{dz}',
                    admission=False)
    return ops
