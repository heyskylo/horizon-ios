"""Health Boost on a mob: its max_health modifier, healing up to it, and the
clamp when it goes.

Health Boost adds 4 * (amplifier + 1) to max_health (MobEffect
addAttributeModifier); the effect does not heal. Instant Health heals on the
mob's next tick up to the boosted maximum. When the effect is removed
(LivingEntity.onEffectsRemoved -> refreshDirtyAttributes -> onAttributeUpdated)
Health is clamped to the new maximum.
"""
from command_exec_corpus import cmd

PIG = 'PersistenceRequired:1b,Silent:1b,NoAI:1b'


def uuid(n):
    return f'[I;205,0,0,{n}]'


def mob_health_boost(raw=False):
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/fill 0 209 0 15 209 15 minecraft:stone', 256)
    run('/execute if loaded 4 210 4', 1)

    # name, Health, setup commands (effects, attribute bases), heal level or None
    cases = [
        ('boost', 10, ['/effect give @e[tag=T,limit=1] minecraft:health_boost 100 1'], 1),
        ('expiry', 10, ['/effect give @e[tag=T,limit=1] minecraft:health_boost 2 1'], 1),
        ('noboost', 10, [], 1),
        ('other', 10, ['/effect give @e[tag=T,limit=1] minecraft:speed 100 1'], 1),
        ('heal', 5, [], 0),
        ('base', 10, ['/attribute @e[tag=T,limit=1] minecraft:max_health base set 14'], 1),
        ('refused', 10, ['/effect give @e[tag=T,limit=1] minecraft:health_boost 100 1',
                         '/effect give @e[tag=T,limit=1] minecraft:health_boost 100 0'], None),
    ]
    tags = []
    for i, (name, health, setup, heal) in enumerate(cases):
        tag = f'h205_{name}'
        tags.append(tag)
        run(f'/summon minecraft:pig {2.5 + 2 * i} 210 4.5 {{UUID:{uuid(i + 1)},Tags:["{tag}"],{PIG},Health:{health}f}}', 1)
        run(f'/execute if entity @e[tag={tag},type=minecraft:pig]', 1)
        run(f'/data get entity @e[tag={tag},limit=1] UUID', 4)
        for c in setup:
            run(c.replace('T', tag))

    def read(tag):
        run(f'/data get entity @e[tag={tag},limit=1] Health')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_health get')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_health base get')
        run(f'/execute if entity @e[tag={tag},nbt={{active_effects:[{{id:"minecraft:health_boost"}}]}}]')
        for field in ['id', 'duration', 'amplifier']:
            run(f'/data get entity @e[tag={tag},limit=1] active_effects[0].{field}')
        run(f'/data get entity @e[tag={tag},limit=1] Pos')
        if raw:
            run(f'/data get entity @e[tag={tag},limit=1]')

    def clock(label):
        run(f'/execute store result score #{label} h205 run time query gametime', record=False)

    run('/scoreboard objectives add h205 dummy', 1)
    for tag in tags:
        read(tag)

    # Instant Health, applied on the mob's next tick.
    for (name, _, _, heal), tag in zip(cases, tags):
        if heal is not None:
            run(f'/effect give @e[tag={tag},limit=1] minecraft:instant_health 1 {heal}')
    clock('t0')
    ops.append({'op': 'tick', 'n': 1})
    clock('t1')
    run('/scoreboard players operation #t1 h205 -= #t0 h205', record=False)
    run('/scoreboard players get #t1 h205')
    for tag in tags:
        read(tag)

    # Removal: /effect clear of Health Boost clamps Health to the new maximum.
    for tag in ['h205_boost', 'h205_other']:
        run(f'/effect clear @e[tag={tag},limit=1] minecraft:health_boost')
    for tag in tags:
        read(tag)

    # Expiry: the 2-second boost runs out (40 ticks) and clamps the same way.
    clock('t2')
    ops.append({'op': 'tick', 'n': 40})
    clock('t3')
    run('/scoreboard players operation #t3 h205 -= #t2 h205', record=False)
    run('/scoreboard players get #t3 h205')
    for tag in tags:
        read(tag)
    return ops
