"""PlayerPredicate's food (FoodPredicate.matches) through named predicates and a tick advancement (#11314).

The player stands in survival on a platform and ticks only in player loops, so hunger drains it
and nothing else does: its food is
set by the saturation effect (FoodData.eat), drained by the hunger effect (exhaustion), and read
back with /data before every set of probes, so a predicate's result is compared on a known state.
The group leaves the player at 20 food and 20 saturation, so it runs last.
"""

GROUP = 'player food predicate'
NAMESPACE = 'h11314'
ZOMBIE = '00000143-0000-0001-0000-000000000002'


def _food(food):
    return {'type': 'minecraft:entity_properties', 'entity': 'this',
            'predicate': {'minecraft:type_specific/player': {'food': food}}}


def predicates():
    """Inclusive bounds on the level alone, the saturation alone, both, and neither."""
    cases = {
        'any': {},
        'level_20': {'level': {'min': 20}},
        'level_below_20': {'level': {'max': 19}},
        'level_18': {'level': {'min': 18}},
        'level_below_18': {'level': {'max': 17}},
        'level_exact_20': {'level': 20},
        'saturation_20': {'saturation': {'min': 20.0}},
        'saturation_below_20': {'saturation': {'max': 19.5}},
        'saturation_exact_20': {'saturation': 20.0},
        'saturation_none': {'saturation': {'max': 0.0}},
        'saturation_some': {'saturation': {'min': 0.5}},
        # The report's predicate: both bounds must hold.
        'well_fed': {'level': {'min': 18}, 'saturation': {'min': 3.0}},
        'fed_but_dry': {'level': {'min': 18}, 'saturation': {'max': 0.0}},
    }
    return {f'{NAMESPACE}:{name}': _food(food) for name, food in cases.items()}


def advancements():
    """A tick criterion whose player condition asks only for food."""
    return {f'{NAMESPACE}:well_fed': {'criteria': {'fed': {
        'trigger': 'minecraft:tick',
        'conditions': {'player': _food({'level': {'min': 18}, 'saturation': {'min': 3.0}})}}}}}


def cases(cmd, setup):
    ops = setup('/forceload add 0 0 15 15', '/fill 0 199 0 15 199 15 stone',
                '/tp @s 8.5 200 8.5 0 0', '/effect clear @s', '/gamemode survival',
                # Earlier groups may leave the player hurt: full health keeps regeneration out of the food.
                '/effect give @s minecraft:instant_health 1 10 true')

    def loop(label, n):
        # The player's own tick (ServerGamePacketListenerImpl.tickPlayer): its effects and FoodData.tick.
        return {'op': 'player_loop', 'label': label, 'n': n, 'expect_result': n}

    def state():
        return [cmd('/data get entity @s foodLevel'),
                cmd('/data get entity @s foodSaturationLevel 1000')]

    def probe(name, truth):
        out = []
        for condition in ('if', 'unless'):
            expected = truth if condition == 'if' else not truth
            out += [cmd(f'/execute {condition} predicate {NAMESPACE}:{name}',
                        expect_result=1 if expected else None)]
        return out

    def advancement(granted):
        word = 'if' if granted else 'unless'
        return [*setup(f'/advancement revoke @s only {NAMESPACE}:well_fed'),
                loop('well_fed criterion', 2),
                cmd(f'/execute {word} entity @s[advancements={{{NAMESPACE}:well_fed=true}}]',
                    expect_result=1)]

    def fill():
        # SaturationMobEffect: FoodData.eat(256, 1.0) each tick it lasts, clamped to 20 and 20.
        return [*setup('/effect give @s minecraft:saturation 1 255 true'),
                loop('saturation', 25),
                *setup('/effect clear @s minecraft:saturation')]

    # Full: 20 food, 20 saturation.
    ops += fill() + state()
    ops += [cmd('/execute if entity @s[type=player,gamemode=survival,nbt={Health:20f}]', expect_result=1)]
    for name, truth in [('any', True), ('level_20', True), ('level_below_20', False),
                        ('level_exact_20', True), ('saturation_20', True),
                        ('saturation_below_20', False), ('saturation_exact_20', True),
                        ('saturation_none', False), ('well_fed', True), ('fed_but_dry', False)]:
        ops += probe(name, truth)
    ops += advancement(True)

    # Not a ServerPlayer: every food predicate fails, even an unbounded one.
    # Through a selector: the source stays the player.
    zombie = f'@e[type=zombie,tag={NAMESPACE}_subject'
    ops += [cmd('/summon zombie 4.5 200 4.5 {UUID:[I;323,1,0,2],Tags:["' + NAMESPACE + '_subject"],'
                'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b}'),
            cmd(f'/execute if entity {zombie}]', expect_result=1),
            cmd(f'/execute if entity {zombie},predicate={NAMESPACE}:any]'),
            cmd(f'/execute unless entity {zombie},predicate={NAMESPACE}:any]', expect_result=1),
            cmd(f'/execute if entity @s[predicate={NAMESPACE}:any]', expect_result=1),
            *setup(f'/kill {ZOMBIE}')]

    # Drained by hunger 255 (1.28 exhaustion a tick): the saturation runs out, then the food drops.
    ops += [*setup('/effect give @s minecraft:hunger 30 255 true'),
            loop('hunger', 80),
            *setup('/effect clear @s minecraft:hunger')]
    ops += state()
    for name, truth in [('any', True), ('level_18', False), ('level_below_18', True),
                        ('saturation_none', True), ('saturation_some', False),
                        ('well_fed', False), ('fed_but_dry', False)]:
        ops += probe(name, truth)
    ops += advancement(False)

    # Fed again: the same predicates and the advancement pass.
    ops += fill() + state()
    ops += probe('well_fed', True) + probe('level_18', True) + advancement(True)

    ops += setup('/gamemode creative', '/effect clear @s',
                 f'/advancement revoke @s only {NAMESPACE}:well_fed',
                 '/tp @s 8.5 230 8.5 0 0', '/fill 0 199 0 15 199 15 air',
                 '/forceload remove 0 0 15 15')
    return ops
