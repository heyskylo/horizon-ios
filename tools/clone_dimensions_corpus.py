"""`/clone from <sourceDimension>` and `to <targetDimension>` (H236).

`CloneCommands` resolves each dimension argument while it reads the
position in it (`DimensionArgument.getDimension`, then
`BlockPosArgument.getLoadedBlockPos` in that level): begin and end in the
source level, the destination in the target level, in that order. An
unknown name fails with `argument.dimension.invalid` before anything is
cloned. Each scene clones the chest named "h236" at (0, 100, 0) of the
overworld onto a destination held by a gold block, which must stay when the
command fails. The cross-dimension scene's nether area is loaded and checked
through `execute in`; those rows are recorded, not native admissions.
"""

Y = 100
SCENES = [
    # name, command arguments after "clone", destination x, destination level
    ('unknown_source', 'from h236:none 0 {Y} 0 0 {Y} 0 4 {Y} 0', 4, None),
    ('unknown_target', '0 {Y} 0 0 {Y} 0 to h236:none 6 {Y} 0', 6, None),
    ('both_unknown', 'from h236:first 0 {Y} 0 0 {Y} 0 to h236:second 8 {Y} 0', 8, None),
    ('implicit', '0 {Y} 0 0 {Y} 0 10 {Y} 0', 10, None),
    ('explicit_overworld',
     'from minecraft:overworld 0 {Y} 0 0 {Y} 0 to minecraft:overworld 12 {Y} 0', 12, None),
    ('to_nether', 'from minecraft:overworld 0 {Y} 0 0 {Y} 0 to minecraft:the_nether 14 {Y} 0', 14,
     'minecraft:the_nether'),
]
NAME = '{"text":"h236"}'


def clone_dimensions(full=False):
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
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/execute if loaded 8 {Y} 8', 'loaded', 1)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd(f'/fill 0 {Y} 0 15 {Y + 1} 15 air', 'clear', admission=False)
    cmd(f'/setblock 0 {Y} 0 chest{{CustomName:\'{NAME}\'}}', 'source-chest', 1)
    cmd(f'/execute if block 0 {Y} 0 chest', 'source-is-chest', 1)
    # The nether destination of the cross-dimension scene.
    cmd('/execute in minecraft:the_nether run forceload add 0 0', 'nether-force', admission=False)
    cmd(f'/execute in minecraft:the_nether if loaded 8 {Y} 8', 'nether-loaded', admission=False)
    cmd(f'/execute in minecraft:the_nether run fill 0 {Y - 1} 0 15 {Y + 1} 15 air', 'nether-clear',
        admission=False)
    cmd(f'/execute in minecraft:the_nether run setblock 14 {Y} 0 gold_block', 'nether-guard', admission=False)
    cmd('/scoreboard objectives add h236 dummy', 'objective')
    cmd('/execute store result score start h236 run time query gametime', 'clock-start', diagnostic=True)
    for name, args, dx, level in SCENES:
        if not level:
            cmd(f'/setblock {dx} {Y} 0 gold_block', f'{name}-guard', 1)
            cmd(f'/execute if block {dx} {Y} 0 gold_block', f'{name}-guard-placed', 1)
        else:
            # The nether area, checked again just before its clone.
            cmd(f'/execute in {level} if loaded {dx} {Y} 0', f'{name}-target-loaded', admission=False)
            cmd(f'/execute in {level} if block {dx} {Y} 0 gold_block', f'{name}-guard-placed', admission=False)
        command = f'clone {args.format(Y=Y)}'
        cmd(f'/{command}', f'{name}-clone', admission=False)
        cmd(f'/execute store success score {name} h236 run {command}', f'{name}-store-op', admission=False)
        cmd(f'/scoreboard players get {name} h236', f'{name}-store', admission=False)
        within = f'in {level} ' if level else ''
        for block in ('gold_block', 'chest'):
            cmd(f'/execute {within}if block {dx} {Y} 0 {block}', f'{name}-destination-{block}', admission=False)
        cmd(f'/execute {within}run data get block {dx} {Y} 0 CustomName', f'{name}-destination-name',
            admission=False)
        cmd(f'/execute if block {dx} {Y} 0 chest', f'{name}-overworld-at-destination', admission=False)
        cmd(f'/data get block 0 {Y} 0 CustomName', f'{name}-source-name', admission=False)
    cmd('/execute store result score now h236 run time query gametime', 'clock-now', diagnostic=True)
    cmd('/scoreboard players operation now h236 -= start h236', 'clock-op', 0)
    cmd('/scoreboard players get now h236', 'clock', 0)
    return ops
