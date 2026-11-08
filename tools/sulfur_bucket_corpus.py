"""A live sulfur cube picked up with an empty bucket (H245).

`SulfurCube.mobInteract` ends with `Bucketable.bucketMobPickup`, and its
`canBePickedUpWithBucket` takes an empty bucket (not a water bucket). An
adult, unprimed cube is removed (`discard`), and the held stack becomes
`ItemUtils.createFilledResult(held, player, sulfur_cube_bucket)`. The bucket
carries `saveToBucketTag`: the default bucket data, the body block
(`sulfur_cube_content`) and the cube's `age` and `age_locked`.

Every click is the connected player's real Interact packet (`mob_interact`)
on a live cube with a fixed UUID. Before each click the player, its held
stack and the cube are admitted; after it, the cube's UUID, the held stack
and the whole inventory are read.

Label classes, declared before any native run: `target-` rows are the H245
pickup (the cube's removal, the held stack and the inventory after a
sulfur pickup); `item-` rows are the filled bucket's components (the
bucket data codec, reported before any change beyond the pickup);
`diag-` rows are recorded diagnostics; every other row is a setup
admission or a control that must hold.
"""

Y = 100
Z = 4
# name, UUID tail, x, summon data, held item, held count, mode, eligible
SCENES = [
    ('empty', 1, 2, '', 'bucket', 2, 'survival', True),
    ('tnt', 2, 8, ',equipment:{body:{id:"minecraft:tnt"}}', 'bucket', 1, 'survival', True),
    ('water', 3, 14, '', 'water_bucket', 1, 'survival', False),
    ('baby', 4, 20, ',Age:-24000', 'bucket', 1, 'survival', False),
    ('primed', 5, 26, ',fuse:80,equipment:{body:{id:"minecraft:tnt"}}', 'bucket', 1, 'survival', False),
    ('creative', 6, 32, '', 'bucket', 1, 'creative', True),
]
COD = ('cod', 7, 38)


def uuid(n):
    return f'00000245-0000-0000-0000-00000000000{n}'


def sulfur_bucket(full=False):
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
    cmd('/forceload add 0 0 47 15', 'force', 3)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0], [1, 0], [2, 0]]})
    for cx in range(3):
        cmd(f'/execute if loaded {cx * 16 + 8} {Y} {Z}', f'loaded-{cx}', 1)
    cmd(f'/fill 0 {Y - 1} 0 47 {Y - 1} 15 stone', 'floor', 768)
    cmd(f'/fill 0 {Y} 0 47 {Y + 3} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    tags = 'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Rotation:[0f,0f],Motion:[0d,0d,0d]'
    for name, n, x, extra, *_ in SCENES:
        cmd(f'/summon sulfur_cube {x + 0.5} {Y} {Z + 0.5} {{{tags},UUID:[I;581,0,0,{n}]{extra}}}', f'{name}-summon',
            admission=False)
    name, n, x = COD
    cmd(f'/summon cod {x + 0.5} {Y} {Z + 0.5} {{{tags},UUID:[I;581,0,0,{n}]}}', 'cod-summon', admission=False)
    for name, n, x, *_ in SCENES + [COD]:
        kind = 'cod' if name == 'cod' else 'sulfur_cube'
        cmd(f'/execute if entity @e[type={kind},limit=1,x={x + 0.5},y={Y},z={Z + 0.5},distance=..0.01]',
            f'{name}-at', 1)
        cmd(f'/execute as {uuid(n)} if entity @s[type={kind}]', f'{name}-type', 1)
        cmd(f'/execute if block {x} {Y - 1} {Z} stone', f'{name}-floor', 1)
        cmd(f'/data get entity {uuid(n)} Health', f'{name}-health', admission=False)
        cmd(f'/data get entity {uuid(n)} Age', f'{name}-age', admission=False)
        cmd(f'/data get entity {uuid(n)} equipment', f'{name}-equipment', admission=False)
        cmd(f'/data get entity {uuid(n)} fuse', f'diag-{name}-fuse', admission=False)
        cmd(f'/data get entity {uuid(n)} Size', f'diag-{name}-size', admission=False)
    cmd('/scoreboard objectives add h245 dummy', 'objective')
    cmd('/execute store result score start h245 run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)

    def click(name, n, x, item, count, mode, eligible, kind):
        cmd(f'/gamemode {mode} @p', f'{name}-mode', admission=False)
        cmd(f'/execute if entity @p[gamemode={mode}]', f'{name}-mode-is', 1)
        cmd('/clear @p', f'{name}-clear', admission=False)
        cmd(f'/item replace entity @p hotbar.0 with {item} {count}', f'{name}-held', admission=False)
        cmd('/tp @p {} {} {} -90 0'.format(x - 1.5, Y, Z + 0.5), f'{name}-tp', admission=False)
        cmd('/data get entity @p SelectedItem', f'{name}-held-before', admission=True)
        cmd('/data get entity @p Inventory', f'{name}-inventory-before', admission=True)
        cmd('/data get entity @p Health', f'{name}-player-health', admission=True)
        cmd('/data get entity @p Pos', f'{name}-player-pos', admission=True)
        cmd(f'/execute if entity {uuid(n)}', f'{name}-alive-before', 1)
        ops.append({'op': 'mob_interact', 'uuid': uuid(n)})
        prefix = 'target-' if kind == 'sulfur_cube' and eligible else ''
        cmd(f'/execute if entity {uuid(n)}', f'{prefix}{name}-alive-after', admission=False)
        cmd(f'/execute if entity @e[type={kind},x={x + 0.5},y={Y},z={Z + 0.5},distance=..2]',
            f'{prefix}{name}-any-after', admission=False)
        cmd('/data get entity @p SelectedItem.id', f'{prefix}{name}-held-id', admission=False)
        cmd('/data get entity @p SelectedItem.count', f'{prefix}{name}-held-count', admission=False)
        cmd('/execute store result score items h245 run data get entity @p Inventory',
            f'{prefix}{name}-inventory-store', admission=False, diagnostic=True)
        cmd('/scoreboard players get items h245', f'{prefix}{name}-inventory-slots', admission=False)
        cmd(f'/execute if items entity @p container.* minecraft:sulfur_cube_bucket',
            f'{prefix}{name}-has-sulfur-bucket', admission=False)
        cmd(f'/execute if items entity @p container.* minecraft:bucket',
            f'{prefix}{name}-has-bucket', admission=False)
        # The filled bucket's data, read wherever it is.
        item = 'item-' if kind == 'sulfur_cube' else 'diag-'
        cmd('/data get entity @p SelectedItem.components', f'{item}{name}-held-components', admission=False)
        cmd('/data get entity @p Inventory', f'{item}{name}-inventory-after', admission=False)

    for name, n, x, extra, item, count, mode, eligible in SCENES:
        click(name, n, x, item, count, mode, eligible, 'sulfur_cube')
    name, n, x = COD
    click(name, n, x, 'water_bucket', 1, 'survival', True, 'cod')
    cmd('/execute store result score now h245 run time query gametime', 'diag-clock-now',
        admission=False, diagnostic=True)
    cmd('/scoreboard players operation now h245 -= start h245', 'clock', 0)
    return ops
