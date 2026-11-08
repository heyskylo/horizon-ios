"""Which data packs a world's `/test` reads its test instances from, over time (H275 lifecycle).

A second, predeclared program next to `world_test_def_corpus.py` (whose 96
records stay as they are). Three world packs:

- the harness pack: `h275:dup`, a function test that passes (`always_pass`);
- `h275-late`: `h275:late`, a block-based test that fails;
- `h275-override`: `h275:dup` again and `minecraft:always_pass`, both as
  block-based tests on the empty structure, which fail.

Each step runs a test from its own spot and reads the test instance block it
placed (`test`, `status`, `error_message`) after stepped ticks: a pass, a
failure or no block at all is the witness, not a name list.

1. a name no pack has (`h275:never`);
2. `h275:late`, then after `/datapack disable "file/h275-late"` (which reloads),
   then after `/datapack enable "file/h275-late"`;
3. `h275:dup`, defined by two enabled packs;
4. `minecraft:always_pass` with the override pack enabled, then after
   `/datapack disable "file/h275-override"`.
"""

FAIL = {'type': 'minecraft:block_based', 'structure': 'minecraft:empty',
        'environment': 'minecraft:default', 'max_ticks': 20, 'required': True}
PASS = {'type': 'minecraft:function', 'function': 'minecraft:always_pass', 'structure': 'minecraft:empty',
        'environment': 'minecraft:default', 'max_ticks': 1, 'setup_ticks': 1, 'required': False}
PACKS = {
    None: {'h275:dup': PASS},
    'h275-late': {'h275:late': FAIL},
    'h275-override': {'h275:dup': FAIL, 'minecraft:always_pass': FAIL},
}
Y = 100


def world_test_def_lifecycle(full=False):
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
    cmd('/forceload add -16 -16 191 31', 'force', 39)
    ops.append({'op': 'settle', 'required_chunks': [[cx, cz] for cx in range(-1, 12) for cz in range(-1, 2)]})
    for cx in range(-1, 12):
        cmd(f'/execute if loaded {cx * 16 + 8} {Y} 8', f'loaded-{cx}', 1)
    cmd(f'/fill -16 {Y - 1} 0 191 {Y - 1} 15 stone', 'floor', 3328)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h275 dummy', 'objective')
    cmd('/datapack list enabled', 'packs-start', admission=False)
    x = [8]

    def run(name, label):
        spot = x[0]
        x[0] += 16
        cmd(f'/tp @s {spot}.5 {Y} 8.5 0 0', f'{label}-runner', 1)
        cmd(f'/execute store success score {label} h275 run test run {name}', f'{label}-run', admission=False)
        cmd(f'/scoreboard players get {label} h275', f'{label}-store', admission=False)
        for n in (2, 5):
            cmd(f'/tick step {n}', f'{label}-step-{n}')
            ops.append({'op': 'tick', 'n': n})
        cmd(f'/execute if loaded {spot} {Y} 11', f'{label}-read-loaded', admission=False)
        cmd(f'/test locate {name}', f'{label}-locate', admission=False)
        for field in ('test', 'status', 'error_message'):
            cmd(f'/data get block {spot} {Y} 11 data.{field}', f'{label}-{field}', admission=False)
        cmd(f'/data get block {spot} {Y} 11', f'{label}-body', diagnostic=True)

    run('h275:never', 'never')
    run('h275:late', 'late-enabled')
    cmd('/datapack disable "file/h275-late"', 'disable-late', admission=False)
    cmd('/datapack list enabled', 'packs-late-off', admission=False)
    run('h275:late', 'late-disabled')
    cmd('/datapack enable "file/h275-late"', 'enable-late', admission=False)
    cmd('/datapack list enabled', 'packs-late-on', admission=False)
    run('h275:late', 'late-reenabled')
    run('h275:dup', 'dup')
    run('minecraft:always_pass', 'builtin-overridden')
    cmd('/datapack disable "file/h275-override"', 'disable-override', admission=False)
    cmd('/datapack list enabled', 'packs-override-off', admission=False)
    run('minecraft:always_pass', 'builtin-after-disable')
    return ops
