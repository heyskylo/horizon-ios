"""`/place feature` with inline and named features on loaded chunks (H170).

`PlaceCommand.placeFeature` checks the 3x3 chunks around the position, then
places the feature value: an inline one (`ResourceOrHolderArgument`) reports
`commands.place.feature.success.inline`, a named one
`commands.place.feature.success`. Each case has its own column over a stone
floor; the block before and after, the command's result and a fresh
`execute store success` are recorded. The named and unknown cases are
controls for the registry side of the same command.
"""

Y = 100
# label, x, z, feature argument, block expected to be placed (None: no change)
CASES = [
    ('inline_stone', 1, 1, '{type:"minecraft:simple_block",to_place:{id:"minecraft:stone"}}', 'minecraft:stone'),
    ('inline_air', 5, 1, '{type:"minecraft:simple_block",to_place:{id:"minecraft:air"}}', 'minecraft:air'),
    ('inline_state', 9, 1,
     '{type:"minecraft:simple_block",to_place:{id:"minecraft:oak_log",properties:{axis:"x"}}}',
     'minecraft:oak_log[axis=x]'),
    ('inline_provider', 13, 1,
     '{type:"minecraft:simple_block",to_place:{type:"minecraft:weighted",entries:[{data:"minecraft:diamond_block",weight:1}]}}',
     'minecraft:diamond_block'),
    ('inline_cannot_survive', 1, 6,
     '{type:"minecraft:simple_block",to_place:{id:"minecraft:dandelion"}}', None),
    ('named_success', 5, 6, 'minecraft:patch_fire', 'minecraft:fire'),
    ('named_failure', 9, 6, 'minecraft:sunflower', None),
    ('unknown_name', 13, 6, 'minecraft:h170_missing_feature', None),
]


def place_feature_inline(full=False):
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
    for rule, value in [('spawn_mobs', 'false'), ('advance_time', 'false'), ('random_tick_speed', '0'),
                        ('fire_spread_radius_around_player', '0'), ('advance_weather', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    # The cases' chunk, and the 3x3 chunks placeFeature checks around it.
    cmd('/forceload add -16 -16 31 31', 'force', 9)
    ops.append({'op': 'settle', 'required_chunks': [[cx, cz] for cx in (-1, 0, 1) for cz in (-1, 0, 1)]})
    for cx in (-1, 0, 1):
        for cz in (-1, 0, 1):
            cmd(f'/execute if loaded {cx * 16 + 8} {Y} {cz * 16 + 8}', f'loaded-{cx}-{cz}', 1)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd(f'/fill 0 {Y} 0 15 {Y + 3} 15 air', 'clear', admission=False)
    cmd(f'/tp @s 8.5 {Y + 10} 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h170 dummy', 'objective')
    cmd('/execute store result score start h170 run time query gametime', 'clock-start', diagnostic=True)
    for label, x, z, feature, placed in CASES:
        if label == 'inline_air':
            # Air is placed over a block that is there first.
            cmd(f'/setblock {x} {Y} {z} glass', label + '-setup', 1)
        cmd(f'/execute if block {x} {Y - 1} {z} stone', label + '-floor', 1)
        cmd(f'/data get block {x} {Y} {z}', label + '-before-entity', diagnostic=True)
        before = 'minecraft:glass' if label == 'inline_air' else 'minecraft:air'
        cmd(f'/execute if block {x} {Y} {z} {before}', label + '-before', 1)
        cmd(f'/place feature {feature} {x} {Y} {z}', label + '-place', admission=False)
        if placed:
            cmd(f'/execute if block {x} {Y} {z} {placed}', label + '-placed', admission=False)
        cmd(f'/execute if block {x} {Y} {z} {before}', label + '-unchanged', admission=False)
        cmd(f'/execute store success score {label} h170 run place feature {feature} {x + 2} {Y} {z + 2}',
            label + '-store-place', admission=False)
        cmd(f'/scoreboard players get {label} h170', label + '-store', admission=False)
        cmd(f'/execute if block {x + 2} {Y} {z + 2} {placed or "minecraft:air"}', label + '-store-block',
            admission=False)
        for dy in (1, 2):
            cmd(f'/execute if block {x} {Y + dy} {z} air', f'{label}-above-{dy}', admission=False)
    cmd('/execute store result score now h170 run time query gametime', 'clock-now', diagnostic=True)
    cmd('/scoreboard players operation now h170 -= start h170', 'clock-delta-op', 0)
    cmd('/scoreboard players get now h170', 'clock-delta', 0)
    return ops
