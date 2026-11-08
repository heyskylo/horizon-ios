"""Live mob custom-name components; saved names only admit the actors."""
import json

GROUP = 'mob custom name components'


def predicates():
    values = {'bob': 'Bob', 'object': {'text': 'Bob'}, 'wrong': 'Alice',
              'colon': 'minecraft:Bob',
              'styled': {'text': 'Bob', 'color': 'red', 'bold': True},
              'split': {'text': 'B', 'extra': [{'text': 'ob'}]},
              'false_style': {'text': 'Bob', 'bold': False}}
    always = {'type': 'minecraft:all_of', 'terms': []}
    out = {'name_view:always': always,
           'name_view:never': {'type': 'minecraft:inverted', 'term': always}}
    for prefix, key in [('bare', 'components'), ('full', 'minecraft:components')]:
        for name, value in values.items():
            out[f'name_view:{prefix}_{name}'] = {
                'type': 'minecraft:entity_properties', 'entity': 'this',
                'predicate': {key: {'minecraft:custom_name': value}}}
        out[f'name_view:{prefix}_variant'] = {
            'type': 'minecraft:entity_properties', 'entity': 'this',
            'predicate': {key: {'minecraft:pig/variant': 'minecraft:temperate'}}}
    return out


def cases(cmd, setup, presentation_diagnostic=False):
    ops = [cmd('/tick freeze'), cmd('/forceload add 0 0 15 15'),
           cmd('/execute if loaded 4 200 4', expect_result=1)]
    ops += setup('/fill 0 199 0 15 199 15 air')
    ops += [cmd('/fill 0 199 0 15 199 15 stone', expect_result=256),
            cmd('/execute if block 4 199 4 stone', expect_result=1),
            cmd('/datapack list enabled'),
            cmd('/scoreboard objectives add name_view dummy', expect_result=1),
            cmd('/execute if predicate name_view:always', expect_result=1),
            cmd('/execute unless predicate name_view:never', expect_result=1)]
    ops += setup('/execute store result score start name_view run time query gametime')
    actors = [('pig', 'Bob'), ('pig', None), ('cow', 'Bob'),
              ('pig', 'minecraft:Bob'),
              ('pig', {'text': 'Bob', 'color': 'red', 'bold': True}),
              ('pig', {'text': 'B', 'extra': [{'text': 'ob'}]})]
    ids = []
    for i, (kind, name) in enumerate(actors, 1):
        # These ten whole replies have separate name-display / saved-text
        # presentation differences. Keep them executable in the full diagnostic;
        # the typed actor/body admission remains strict in both contracts.
        record_presentation = i < 5 or presentation_diagnostic
        uuid = f'0000007d-0000-0000-0000-{i:012x}'
        ids.append(uuid)
        fields = (f'UUID:[I;125,0,0,{i}],Tags:["name_view_subject","name_view_{i}"],'
                  'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:10f')
        if name is not None:
            fields += ',CustomName:' + json.dumps(name, separators=(',', ':'))
        ops += [cmd(f'/summon {kind} {i + .5} 200 4.5 {{{fields}}}', record=record_presentation, expect_result=1),
                cmd(f'/execute as {uuid} if entity @s[type={kind},tag=name_view_{i},nbt={{UUID:[I;125,0,0,{i}],Health:10f,Pos:[{i + .5}d,200d,4.5d],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b}}]', expect_result=1),
                cmd(f'/data get entity {uuid} UUID', record=record_presentation),
                cmd(f'/data get entity {uuid} Health', record=record_presentation, expect_result=10),
                cmd(f'/data get entity {uuid} Pos', record=record_presentation)]
        ops += [cmd(f'/data get entity {uuid} CustomName', record=record_presentation)] if name is not None else [
            cmd(f'/execute unless data entity {uuid} CustomName', expect_result=1)]

    def probe(actor, name, truth, both=True):
        out = []
        for prefix in ('bare', 'full'):
            for condition in (('if', 'unless') if both else ('if',)):
                succeeds = truth if condition == 'if' else not truth
                out += [cmd('/scoreboard players set hits name_view 0', expect_result=0),
                        cmd('/scoreboard players set result name_view 31', expect_result=31),
                        cmd('/scoreboard players set success name_view 17', expect_result=17),
                        cmd(f'/execute store result score result name_view store success score success name_view as {ids[actor - 1]} {condition} predicate name_view:{prefix}_{name} run scoreboard players add hits name_view 1'),
                        cmd('/scoreboard players get hits name_view', expect_result=int(succeeds)),
                        cmd('/scoreboard players get result name_view', expect_result=1 if succeeds else 31),
                        cmd('/scoreboard players get success name_view', expect_result=1 if succeeds else 17)]
        return out

    for actor, name, truth, both in [
            (1, 'bob', True, True), (1, 'object', True, False),
            (1, 'wrong', False, False), (1, 'colon', False, False),
            (1, 'false_style', False, False), (1, 'variant', True, False),
            (2, 'bob', False, True), (3, 'bob', True, False),
            (4, 'colon', True, True), (4, 'bob', False, False),
            (5, 'styled', True, True), (5, 'bob', False, False),
            (6, 'split', True, True), (6, 'bob', False, False)]:
        ops += probe(actor, name, truth, both)
    ops += setup('/execute store result score elapsed name_view run time query gametime',
                 '/scoreboard players operation elapsed name_view -= start name_view')
    ops += [cmd('/scoreboard players get elapsed name_view', expect_result=0)]
    # Cleanup ticks happen after the guarded zero-tick live observation interval.
    ops += setup('/kill @e[tag=name_view_subject]', '/scoreboard objectives remove name_view',
                 '/forceload remove 0 0 15 15', '/fill 0 199 0 15 199 15 air',
                 '/tick unfreeze')
    return ops + [{'op': 'tick', 'n': 25}]
