"""A mob's Absorption effect once its absorption is used up.

AbsorptionMobEffect.applyEffectTick answers false when the absorption amount
is 0, on every tick (shouldApplyEffectTickThisTick is always true), so
MobEffectInstance.tickServer returns false before the duration counts down
and LivingEntity.tickEffects removes the effect on the mob's next tick.
"""
from command_exec_corpus import cmd

PIG = 'PersistenceRequired:1b,Silent:1b,Health:10f'


def uuid(n):
    return f'[I;204,0,0,{n}]'


def mob_absorption(raw=False):
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/scoreboard objectives add h204 dummy', 1)
    run('/fill 0 209 0 15 209 15 minecraft:stone', 256)
    run('/execute if loaded 4 210 4', 1)

    # name, extra NBT, give the effect, damage amount (None: no hit)
    cases = [
        ('depleted', 'NoAI:1b', True, 4),
        ('kept', 'NoAI:1b', True, None),
        ('partial', 'NoAI:1b', True, 2),
        ('none', 'NoAI:1b', False, 4),
        ('refused', 'NoAI:1b,Invulnerable:1b', True, 4),
        ('ai', '', True, 4),
    ]
    tags = []
    for i, (name, extra, give, hit) in enumerate(cases):
        tag = f'h204_{name}'
        tags.append((tag, name))
        fields = ','.join(f for f in [f'UUID:{uuid(i + 1)}', f'Tags:["{tag}"]', PIG, extra] if f)
        run(f'/summon minecraft:pig {2.5 + 2 * i} 210 4.5 {{{fields}}}', 1)
        run(f'/execute if entity @e[tag={tag},type=minecraft:pig]', 1)
        run(f'/data get entity @e[tag={tag},limit=1] UUID', 4)
        if give:
            run(f'/effect give @e[tag={tag},limit=1] minecraft:absorption 100 0', 1)
    still = [tag for tag, name in tags if name != 'ai']

    def read(tag, positions=True):
        run(f'/data get entity @e[tag={tag},limit=1] Health')
        run(f'/data get entity @e[tag={tag},limit=1] AbsorptionAmount')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_health get')
        run(f'/attribute @e[tag={tag},limit=1] minecraft:max_absorption get')
        run(f'/execute if entity @e[tag={tag},nbt={{active_effects:[{{id:"minecraft:absorption"}}]}}]')
        run(f'/execute store result score {tag} h204 run data get entity @e[tag={tag},limit=1] active_effects', record=False)
        run(f'/scoreboard players get {tag} h204')
        for field in ['id', 'duration', 'amplifier', 'show_particles', 'show_icon', 'ambient']:
            run(f'/data get entity @e[tag={tag},limit=1] active_effects[0].{field}')
        if positions:
            run(f'/data get entity @e[tag={tag},limit=1] Pos')
        if raw:
            run(f'/data get entity @e[tag={tag},limit=1]')

    # The hits, each a separate accepted (or refused) damage.
    for i, (name, extra, give, hit) in enumerate(cases):
        if hit is not None:
            run(f'/damage @e[tag=h204_{name},limit=1] {hit} minecraft:generic')
    for tag, name in tags:
        read(tag, name != 'ai')

    run('/execute store result score #t0 h204 run time query gametime', record=False)
    ops.append({'op': 'tick', 'n': 1})
    run('/execute store result score #t1 h204 run time query gametime', record=False)
    run('/scoreboard players operation #t1 h204 -= #t0 h204', record=False)
    run('/scoreboard players get #t1 h204')
    for tag, name in tags:
        read(tag, name != 'ai')

    ops.append({'op': 'tick', 'n': 1})
    for tag, name in tags:
        run(f'/execute if entity @e[tag={tag},nbt={{active_effects:[{{id:"minecraft:absorption"}}]}}]')
        run(f'/data get entity @e[tag={tag},limit=1] AbsorptionAmount')
    for tag in still:
        run(f'/data get entity @e[tag={tag},limit=1] Pos')
    run('/execute if entity @e[type=minecraft:pig,tag=!h204_none]')
    return ops


def mob_absorption_infinite():
    """Infinite Absorption: tickServer asks applyEffectTick on every tick
    (by the mob's tickCount), so a used-up infinite one goes too; an unused
    one stays infinite, and another infinite effect is untouched."""
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/fill 0 209 0 15 209 15 minecraft:stone', 256)
    run('/execute if loaded 4 210 4', 1)
    # name, infinite effects given, hit
    cases = [
        ('inf_depleted', ['absorption'], 4),
        ('inf_kept', ['absorption'], None),
        ('inf_other', ['speed'], 4),
        ('inf_both', ['speed', 'absorption'], 4),
    ]
    for i, (name, effects, hit) in enumerate(cases):
        tag = f'h204_{name}'
        run(f'/summon minecraft:pig {2.5 + 2 * i} 210 8.5 {{UUID:{uuid(11 + i)},Tags:["{tag}"],{PIG},NoAI:1b}}', 1)
        run(f'/execute if entity @e[tag={tag},type=minecraft:pig]', 1)
        for effect in effects:
            run(f'/effect give @e[tag={tag},limit=1] minecraft:{effect} infinite 0', 1)
    for name, effects, hit in cases:
        if hit is not None:
            run(f'/damage @e[tag=h204_{name},limit=1] {hit} minecraft:generic')

    def read(tag):
        run(f'/data get entity @e[tag={tag},limit=1] Health')
        run(f'/data get entity @e[tag={tag},limit=1] AbsorptionAmount')
        for effect in ['absorption', 'speed']:
            run(f'/execute if entity @e[tag={tag},nbt={{active_effects:[{{id:"minecraft:{effect}",duration:-1}}]}}]')
        run(f'/data get entity @e[tag={tag},limit=1] active_effects')

    for name, _, _ in cases:
        read(f'h204_{name}')
    for _ in range(2):
        ops.append({'op': 'tick', 'n': 1})
        for name, _, _ in cases:
            read(f'h204_{name}')
    return ops
