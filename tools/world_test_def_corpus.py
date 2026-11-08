"""`/test` with a test instance defined only in an enabled world data pack (H275).

Test instances are read from the data packs the world enables, not only
from the game's own data. `h275:broken` is a block-based test on the empty
structure, which has no test block: it runs and fails at once, the same
way on both servers. `minecraft:always_pass` is the built-in positive and
`h275:missing` a name no pack defines. The test's own outcome is read from
the test instance block it places (`data.status`).
"""

PACK_TESTS = {
    'h275:broken': {
        'type': 'minecraft:block_based', 'structure': 'minecraft:empty',
        'environment': 'minecraft:default', 'max_ticks': 20, 'required': True,
    },
}
Y = 100


def world_test_def(full=False):
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
    cmd('/forceload add -32 -32 47 47', 'force', 25)
    ops.append({'op': 'settle', 'required_chunks': [[cx, cz] for cx in range(-2, 3) for cz in range(-2, 3)]})
    for cx in range(-2, 3):
        for cz in range(-2, 3):
            cmd(f'/execute if loaded {cx * 16} {Y} {cz * 16}', f'loaded-{cx}-{cz}', 1)
    cmd(f'/fill -16 {Y - 1} -16 31 {Y - 1} 31 stone', 'floor', 2304)
    cmd(f'/tp @s 8.5 {Y} 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/datapack list enabled', 'packs-enabled', admission=False)
    cmd('/scoreboard objectives add h275 dummy', 'objective')
    cmd('/execute store result score start h275 run time query gametime', 'clock-start', diagnostic=True)

    def clock(label, ticks):
        cmd('/execute store result score now h275 run time query gametime', label + '-clock-raw', diagnostic=True)
        cmd('/scoreboard players operation now h275 -= start h275', label + '-clock-op', ticks)
        cmd('/scoreboard players get now h275', label + '-clock', ticks)

    clock('start', 0)
    # Each test runs from its own spot, so their structures stay apart; the
    # test instance block lands 3 blocks south of the runner.
    runs = [('minecraft:always_pass', 8), ('h275:broken', 24), ('h275:missing', 40)]
    for name, x in runs:
        key = name.split(':')[1]
        cmd(f'/tp @s {x}.5 {Y} 8.5 0 0', f'{key}-runner', 1)
        cmd(f'/test locate {name}', f'{key}-locate-before', admission=False)
        cmd(f'/execute store success score {key} h275 run test run {name}', f'{key}-run', admission=False)
        cmd(f'/scoreboard players get {key} h275', f'{key}-run-store', admission=False)
    cmd(f'/tp @s 8.5 {Y} 8.5 0 0', 'owner-back', 1)

    def read_blocks(key):
        for name, x in runs:
            short = name.split(':')[1]
            for field in ('test', 'status', 'error_message'):
                cmd(f'/data get block {x} {Y} 11 data.{field}', f'{key}-{short}-{field}', admission=False)
            cmd(f'/data get block {x} {Y} 11', f'{key}-{short}-body', diagnostic=True)

    read_blocks('at0')
    elapsed = 0
    for at in (1, 2, 5):
        cmd(f'/tick step {at - elapsed}', f'step-to-{at}')
        ops.append({'op': 'tick', 'n': at - elapsed})
        elapsed = at
        clock(f'at{at}', at)
        read_blocks(f'at{at}')
    return ops
