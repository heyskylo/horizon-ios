"""Absorption on a mob: the max_absorption modifier and the amount it allows.

The effect adds 4 * (amplifier + 1) to max_absorption (onEffectAdded), then
onEffectStarted raises the amount to at least 4 * (amplifier + 1), clamped
to the maximum (setAbsorptionAmount). An upgrade swaps the modifier
(onEffectUpdated). When the effect goes, onAttributeUpdated(MAX_ABSORPTION)
clamps the amount to the new maximum: a mob whose base max_absorption is 20
keeps its amount, one with base 0 loses it.
"""
from command_exec_corpus import cmd

PIG = 'PersistenceRequired:1b,Silent:1b,NoAI:1b,Health:10f'


def uuid(n):
    return f'[I;203,0,0,{n}]'


def mob_absorption_max(raw=False):
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/fill 0 209 0 15 209 15 minecraft:stone', 256)
    run('/execute if loaded 4 210 4', 1)
    run('/scoreboard objectives add h203 dummy', 1)

    T = '@e[tag=T,limit=1]'
    # name, base max_absorption (None: type default), setup commands
    cases = [
        ('b0_add', None, [f'/effect give {T} minecraft:absorption 100 0']),
        ('b0_upgrade', None, [f'/effect give {T} minecraft:absorption 100 0', f'/effect give {T} minecraft:absorption 100 1']),
        ('b0_clear', None, [f'/effect give {T} minecraft:absorption 100 1']),
        ('b0_expiry', None, [f'/effect give {T} minecraft:absorption 1 0']),
        ('b20_add', 20, [f'/effect give {T} minecraft:absorption 100 0']),
        ('b20_upgrade', 20, [f'/effect give {T} minecraft:absorption 100 0', f'/effect give {T} minecraft:absorption 100 1']),
        ('b20_clear', 20, [f'/effect give {T} minecraft:absorption 100 1']),
        ('b20_expiry', 20, [f'/effect give {T} minecraft:absorption 1 0']),
        ('soak', None, [f'/effect give {T} minecraft:absorption 100 1', f'/damage {T} 3 minecraft:generic']),
        ('other', None, [f'/effect give {T} minecraft:speed 100 0']),
        ('modifier', None, [f'/attribute {T} minecraft:max_absorption modifier add h203:extra 6 add_value',
                            f'/effect give {T} minecraft:absorption 100 0']),
        # #13078: loading sets the amount as saved (internalSetAbsorptionAmount, unclamped), and the hit's
        # setAbsorptionAmount clamps what is left to max_absorption: 0 without the effect, 4 with Absorption I.
        ('data_soak', None, [f'/data merge entity {T} {{AbsorptionAmount:10.0f}}', f'/damage {T} 1 minecraft:generic']),
        ('data_soak_effect', None, [f'/effect give {T} minecraft:absorption 100 0',
                                    f'/data merge entity {T} {{AbsorptionAmount:10.0f}}',
                                    f'/damage {T} 1 minecraft:generic']),
    ]
    tags = []
    for i, (name, base, setup) in enumerate(cases):
        tag = f'h203_{name}'
        tags.append(tag)
        run(f'/summon minecraft:pig {1.5 + (i % 7) * 2} 210 {3.5 + (i // 7) * 4} {{UUID:{uuid(i + 1)},Tags:["{tag}"],{PIG}}}', 1)
        run(f'/execute if entity @e[tag={tag},type=minecraft:pig]', 1)
        run(f'/data get entity @e[tag={tag},limit=1] UUID', 4)
        if base is not None:
            run(f'/attribute @e[tag={tag},limit=1] minecraft:max_absorption base set {base}', 1)
        for c in setup:
            run(c.replace('T', tag))

    def read(tag):
        run(f'/data get entity @e[tag={tag},limit=1] AbsorptionAmount')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_absorption get')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_absorption base get')
        run(f'/data get entity @e[tag={tag},limit=1] Health')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_health get')
        run(f'/execute if entity @e[tag={tag},nbt={{active_effects:[{{id:"minecraft:absorption"}}]}}]')
        for field in ['id', 'duration', 'amplifier']:
            run(f'/data get entity @e[tag={tag},limit=1] active_effects[0].{field}')
        run(f'/data get entity @e[tag={tag},limit=1] Pos')
        if raw:
            run(f'/data get entity @e[tag={tag},limit=1]')

    def ticks(n, label):
        run(f'/execute store result score #{label}0 h203 run time query gametime', record=False)
        ops.append({'op': 'tick', 'n': n})
        run(f'/execute store result score #{label}1 h203 run time query gametime', record=False)
        run(f'/scoreboard players operation #{label}1 h203 -= #{label}0 h203', record=False)
        run(f'/scoreboard players get #{label}1 h203')

    for tag in tags:
        read(tag)
    for tag in ['h203_b0_clear', 'h203_b20_clear']:
        run(f'/effect clear @e[tag={tag},limit=1] minecraft:absorption')
    for tag in tags:
        read(tag)
    # The 1-second effects run out (20 ticks).
    ticks(25, 't')
    for tag in tags:
        read(tag)
    return ops


def mob_absorption_max_ops():
    """The effect's ADD_VALUE modifier among the other operations: summed
    into the base before ADD_MULTIPLIED_BASE and ADD_MULTIPLIED_TOTAL, then
    clamped to max_absorption's range (0..2048)."""
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/fill 0 209 0 15 209 15 minecraft:stone', 256)
    run('/execute if loaded 4 210 4', 1)
    T = '@e[tag=T,limit=1]'
    mod = f'/attribute {T} minecraft:max_absorption modifier add'
    # name, base, modifiers, amplifier
    cases = [
        ('op_base_mult', 20, [f'{mod} h203:half 0.5 add_multiplied_base'], 0),
        ('op_total_mult', 10, [f'{mod} h203:double 1 add_multiplied_total'], 0),
        ('op_all', 10, [f'{mod} h203:add 2 add_value', f'{mod} h203:half 0.5 add_multiplied_base',
                        f'{mod} h203:double 1 add_multiplied_total'], 1),
        ('op_cap_negative', 2048, [f'{mod} h203:minus -10 add_value'], 0),
        ('op_cap', 2046, [], 0),
    ]
    tags = []
    for i, (name, base, mods, amp) in enumerate(cases):
        tag = f'h203_{name}'
        tags.append(tag)
        run(f'/summon minecraft:pig {1.5 + 2 * i} 210 12.5 {{UUID:{uuid(31 + i)},Tags:["{tag}"],{PIG}}}', 1)
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_absorption base set {base}', 1)
        for c in mods:
            run(c.replace('T', tag))
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_absorption get')
        run(f'/effect give @e[tag={tag},limit=1] minecraft:absorption 100 {amp}')

    for tag in tags:
        run(f'/data get entity @e[tag={tag},limit=1] AbsorptionAmount')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_absorption get')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_absorption base get')
    for tag in tags:
        run(f'/effect clear @e[tag={tag},limit=1] minecraft:absorption')
        run(f'/data get entity @e[tag={tag},limit=1] AbsorptionAmount')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_absorption get')
    return ops
