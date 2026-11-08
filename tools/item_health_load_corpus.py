"""ItemEntity's saved short Health conversion, with complete body diagnostics."""
GROUP = 'item Health short load'
# Expected independently from pinned getShortOr / NumericTag.shortValue, then Java-validated.
CASES = [
    ('absent', None, 5), ('short', '5s', 5), ('int', '5', 5),
    ('long', '5L', 5), ('byte', '5b', 5),
    ('negative_float', '-1.5f', -2), ('negative_double', '-1.5d', -2),
    ('positive_float', '1.5f', 1), ('positive_double', '1.5d', 1),
    ('negative_outside_short', '-40000.5d', 25535),
    ('short_max', '32767', 32767), ('int_above_short', '32768', -32768),
    ('short_min', '-32768', -32768), ('int_below_short', '-32769', 32767),
    ('long_low_bits', '65535L', -1), ('long_wrap', '65536L', 0),
    ('negative_long_wrap', '-65537L', -1),
    ('long_max', '9223372036854775807L', -1),
    ('wrong_string', '"not_numeric"', 5), ('wrong_compound', '{}', 5),
]
DIAGNOSTIC_RECORDS = len(CASES)


def cases(cmd, setup):
    ops = [cmd('/tick freeze'), cmd('/forceload add 160 160 175 175'),
           {'op': 'wait_chunks', 'chunks': [[10, 10]]},
           cmd('/fill 160 199 160 175 199 175 stone', expect_result=256),
           cmd('/execute if loaded 164 200 164', expect_result=1),
           cmd('/scoreboard objectives add item_health dummy', expect_result=1)]
    ops += setup('/execute store result score start item_health run time query gametime')
    for n, (label, supplied, expected) in enumerate(CASES, 1):
        uuid = f'00000035-0000-0001-0000-{n:012x}'
        x = 163.5 + n * .25
        unique = f'item_health_{label}'
        extra = '' if supplied is None else ',Health:' + supplied
        identity = (f'UUID:[I;53,1,0,{n}],Pos:[{x}d,200d,164.5d],NoGravity:1b,'
                    'Item:{id:"minecraft:diamond",count:1}')
        ops += [cmd(f'/summon minecraft:item {x} 200 164.5 {{UUID:[I;53,1,0,{n}],Tags:["{unique}"],NoGravity:1b,Motion:[0d,0d,0d],Item:{{id:"minecraft:diamond",count:1}}{extra}}}', expect_result=1),
                cmd(f'/execute as {uuid} if entity @s[type=minecraft:item,tag={unique}]', expect_result=1),
                cmd(f'/execute if data entity {uuid} {{{identity}}}', expect_result=1),
                cmd(f'/data get entity {uuid} Health', expect_result=expected)]
        ops += setup('/scoreboard players set value item_health 317',
                     '/scoreboard players set success item_health 17')
        ops += [cmd(f'/execute store result score value item_health store success score success item_health run data get entity {uuid} Health', expect_result=expected),
                cmd('/scoreboard players get value item_health', expect_result=expected),
                cmd('/scoreboard players get success item_health', expect_result=1),
                cmd(f'/execute if data entity {uuid} {{Health:{expected}s,{identity}}}', expect_result=1)]
        full = cmd(f'/data get entity {uuid}', record=False)
        full['trace'] = True
        ops.append(full)
    ops += setup('/execute store result score elapsed item_health run time query gametime',
                 '/scoreboard players operation elapsed item_health -= start item_health')
    return ops + [cmd('/scoreboard players get elapsed item_health', expect_result=0)]
