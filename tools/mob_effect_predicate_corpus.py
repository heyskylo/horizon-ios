"""Named effect predicates over a real mob, with zero observation ticks."""

GROUP = 'mob effect predicate view'
ACTOR = '00000143-0000-0001-0000-000000000001'
UUID = '[I;323,1,0,1]'


def predicates():
    def entity(want):
        return {'type': 'minecraft:entity_properties', 'entity': 'this', 'predicate': want}
    def effects(want):
        return entity({'minecraft:effects': want})
    speed = 'minecraft:speed'
    fire = 'minecraft:fire_resistance'
    always = {'type': 'minecraft:all_of', 'terms': []}
    return {'effect_view:always': always,
            'effect_view:never': {'type': 'minecraft:inverted', 'term': always},
            'effect_view:zombie': entity({'minecraft:entity_type': 'minecraft:zombie'}),
            'effect_view:player': entity({'minecraft:entity_type': 'minecraft:player'}),
            'effect_view:speed_two': effects({speed: {'amplifier': 2}}),
            'effect_view:speed_five': effects({speed: {'amplifier': 5}}),
            'effect_view:fire_zero': effects({fire: {'amplifier': 0}}),
            'effect_view:both': effects({speed: {'amplifier': 2}, fire: {'amplifier': 0}}),
            'effect_view:finite_400': effects({speed: {'amplifier': 0, 'duration': 400}}),
            'effect_view:shorter': effects({speed: {'amplifier': 0, 'duration': {'max': 399}}})}


def cases(cmd, setup):
    ops = [cmd('/tick freeze'), cmd('/forceload add 0 0 15 15')]
    ops += setup('/fill 0 199 0 15 199 15 air')
    ops += [cmd('/fill 0 199 0 15 199 15 stone', expect_result=256),
            cmd('/execute if loaded 4 200 4', expect_result=1),
            cmd('/execute if block 4 199 4 stone', expect_result=1),
            cmd('/datapack list enabled'),
            cmd('/scoreboard objectives add effect_view dummy', expect_result=1)]
    ops += setup('/tp @s 8.5 200 8.5 0 0',
                 '/execute store result score start effect_view run time query gametime',
                 '/data modify storage effect_view:owner uuid set from entity @s UUID')
    ops += [cmd('/execute if predicate effect_view:always', expect_result=1),
            cmd('/execute unless predicate effect_view:never', expect_result=1),
            cmd('/execute if predicate effect_view:player', expect_result=1),
            cmd('/summon zombie 4.5 200 4.5 {UUID:' + UUID + ',Tags:["effect_view_subject"],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,IsBaby:0b,Health:20f}')]

    def body():
        return [cmd('/execute if entity @e[type=zombie,tag=effect_view_subject]', expect_result=1),
                cmd('/execute as ' + ACTOR + ' if entity @s[type=zombie,tag=effect_view_subject,nbt={UUID:' + UUID + ',Pos:[4.5d,200d,4.5d],Health:20f,NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,IsBaby:0b}]', expect_result=1),
                cmd('/data get entity ' + ACTOR + ' UUID'),
                cmd('/data get entity ' + ACTOR + ' Health'),
                cmd('/data get entity ' + ACTOR + ' Pos')]

    def probe(name, truth, subject=ACTOR, both=True):
        result = []
        for condition in (['if', 'unless'] if both else ['if']):
            succeeds = truth if condition == 'if' else not truth
            result += [cmd('/scoreboard players set hits effect_view 0', expect_result=0),
                       cmd('/scoreboard players set result effect_view 31', expect_result=31),
                       cmd('/scoreboard players set success effect_view 17', expect_result=17),
                       cmd('/execute store result score result effect_view store success score success effect_view as ' + subject + ' ' + condition + ' predicate effect_view:' + name + ' run scoreboard players add hits effect_view 1'),
                       cmd('/scoreboard players get hits effect_view', expect_result=int(succeeds)),
                       cmd('/scoreboard players get result effect_view', expect_result=1 if succeeds else 31),
                       cmd('/scoreboard players get success effect_view', expect_result=1 if succeeds else 17)]
        return result

    def effect_read(effect, amp, duration):
        entry = ' active_effects[{id:"minecraft:' + effect + '"}]'
        return [cmd('/execute if data entity ' + ACTOR + entry, expect_result=1),
                cmd('/data get entity ' + ACTOR + entry + '.amplifier', expect_result=amp if amp else None),
                cmd('/data get entity ' + ACTOR + entry + '.duration', expect_result=duration)]

    ops += body() + [cmd('/execute as ' + ACTOR + ' if predicate effect_view:zombie', expect_result=1),
                     cmd('/execute unless data entity ' + ACTOR + ' active_effects[]', expect_result=1)]
    ops += probe('speed_two', False)
    ops += [cmd('/effect give ' + ACTOR + ' minecraft:speed infinite 2 true', expect_result=1)]
    ops += body() + effect_read('speed', 2, -1)
    ops += probe('speed_two', True) + probe('speed_five', False) + probe('both', False, both=False)
    ops += [cmd('/effect give ' + ACTOR + ' minecraft:fire_resistance infinite 0 true', expect_result=1)]
    ops += effect_read('fire_resistance', 0, -1)
    ops += probe('fire_zero', True) + probe('both', True)
    ops += [cmd('/effect clear ' + ACTOR + ' minecraft:speed', expect_result=1)]
    ops += probe('speed_two', False, both=False) + probe('fire_zero', True, both=False) + probe('both', False, both=False)
    ops += [cmd('/effect clear ' + ACTOR, expect_result=1),
            cmd('/execute unless data entity ' + ACTOR + ' active_effects[]', expect_result=1)]
    ops += probe('fire_zero', False, both=False)
    # The wrong-amplifier predicate gets its own positive admission control.
    ops += [cmd('/effect give ' + ACTOR + ' minecraft:speed infinite 5 true', expect_result=1)]
    ops += effect_read('speed', 5, -1) + probe('speed_five', True, both=False)
    for seconds, duration in [(20, 400), (19, 380)]:
        ops += [cmd('/effect clear ' + ACTOR, expect_result=1),
                cmd('/effect give ' + ACTOR + ' minecraft:speed ' + str(seconds) + ' 0 true', expect_result=1)]
        ops += effect_read('speed', 0, duration)
        ops += probe('finite_400', duration == 400, both=False) + probe('shorter', duration <= 399, both=False)
    ops += [cmd('/effect clear ' + ACTOR, expect_result=1)]
    ops += probe('speed_two', False, both=False)
    # Existing connected-owner projection, not player simulation/timer parity.
    ops += [cmd('/effect give @s minecraft:speed infinite 2 true', expect_result=1)]
    ops += probe('speed_two', True, subject='@s')
    ops += [cmd('/effect clear @s minecraft:speed', expect_result=1)]
    ops += probe('speed_two', False, subject='@s', both=False) + body()
    ops += setup('/execute store result score elapsed effect_view run time query gametime',
                 '/scoreboard players operation elapsed effect_view -= start effect_view',
                 '/execute store success score owner_changed effect_view run data modify storage effect_view:owner uuid set from entity @s UUID')
    ops += [cmd('/scoreboard players get elapsed effect_view', expect_result=0),
            cmd('/scoreboard players get owner_changed effect_view', expect_result=0),
            cmd('/execute if entity @s[type=player,gamemode=creative,nbt={Pos:[8.5d,200d,8.5d],Health:20f}]', expect_result=1)]
    # Cleanup is outside the zero-tick observation interval.
    ops += setup('/kill @e[tag=effect_view_subject]', '/scoreboard objectives remove effect_view',
                 '/forceload remove 0 0 15 15', '/tp @s 8.5 230 8.5 0 0',
                 '/fill 0 199 0 15 199 15 air', '/tick unfreeze')
    return ops + [{'op': 'tick', 'n': 25}]
