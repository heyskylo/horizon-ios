"""Malformed inline features are refused while `/place feature` is parsed (H171).

`ResourceOrIdArgument` decodes an inline value with the registry's codec
while the command is parsed: an inline feature that the
`ConfiguredFeature` codec refuses fails the parse
(`argument.resource_or_id.failed_to_parse`), so the command never runs and
an `execute store` around it stores nothing. A feature that decodes but
does not place fails when it runs (`commands.place.feature.failed`), and
the store writes 0.

The world is frozen and no tick runs. Each case first runs the plain
command (its full message), then the same command under fresh `execute
store result` and `store success` scores set to -1, and reads the target
block before and after.

Label classes, declared before any native run: `target-` rows are the two
malformed inline features; `diag-` rows are recorded diagnostics; every
other row is a setup admission or a control (a valid named feature, a valid
inline feature, and a valid inline feature that does not place).
"""

Y = 100
CASES = [
    # name, feature argument, target x
    ('named', 'minecraft:pumpkin', 1),
    ('inline', '{type:"minecraft:simple_block",to_place:{id:"minecraft:stone"}}', 3),
    # Decodes, but a dandelion cannot stand on stone (SimpleBlockFeature).
    ('no-place', '{type:"minecraft:simple_block",to_place:{id:"minecraft:dandelion"}}', 5),
    ('target-no-to-place', '{type:"minecraft:simple_block"}', 7),
    ('target-unknown-type', '{type:"minecraft:h171_unknown"}', 9),
    # The same two, written with an extra `config` wrapper (diagnostics).
    ('diag-config-no-to-place', '{type:"minecraft:simple_block",config:{}}', 11),
    ('diag-config-unknown-type', '{type:"minecraft:h171_unknown",config:{}}', 13),
]


def inline_feature_parse(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/execute if loaded 8 {Y} 8', 'loaded', 1)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd(f'/fill 0 {Y} 0 15 {Y + 6} 15 air', 'clear', admission=False)
    cmd('/scoreboard objectives add h171 dummy', 'objective')
    cmd('/execute store result score start h171 run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)
    for name, feature, x in CASES:
        cmd(f'/execute if block {x} {Y} 8 air', f'{name}-air-before', 1)
        cmd(f'/place feature {feature} {x} {Y} 8', f'{name}-plain', admission=False)
        cmd(f'/data get block {x} {Y} 8', f'diag-{name}-block-data', admission=False)
        cmd(f'/execute if block {x} {Y} 8 air', f'{name}-air-after', admission=False)
        cmd(f'/setblock {x} {Y} 8 air', f'{name}-reset', admission=False)
        cmd('/scoreboard players set r h171 -1', f'{name}-r-reset', admission=False)
        cmd('/scoreboard players set s h171 -1', f'{name}-s-reset', admission=False)
        cmd(f'/execute store result score r h171 store success score s h171 run place feature {feature} '
            f'{x} {Y} 8', f'{name}-stored', admission=False)
        cmd('/scoreboard players get r h171', f'{name}-result', admission=False)
        cmd('/scoreboard players get s h171', f'{name}-success', admission=False)
        cmd(f'/execute if block {x} {Y} 8 air', f'{name}-air-after-stored', admission=False)
    cmd('/execute store result score now h171 run time query gametime', 'diag-clock-now',
        admission=False, diagnostic=True)
    cmd('/scoreboard players operation now h171 -= start h171', 'clock', 0)
    return ops
