"""Vehicle command transitions compared against vanilla's integrated server."""


def passenger_teleports(cmd, setup):
    """#9335: nested passengers keep their offsets, turns and destination level."""
    vehicle = '@e[tag=tp_tree_vehicle,limit=1]'
    rider = '@e[tag=tp_tree_rider,limit=1]'
    child = '@e[tag=tp_tree_child,limit=1]'
    ops = setup('/forceload add 0 0', '/execute in minecraft:the_nether run tp @s 8.5 230 8.5')
    ops.append({'op': 'settle'})
    ops += setup('/forceload add 0 0', '/execute in minecraft:overworld run tp @s 8.5 230 8.5')
    ops.append({'op': 'settle'})
    for kind in ('horse', 'oak_boat', 'minecart'):
        ops += setup('/kill @e[type=!player]',
            f'/summon {kind} 2.5 210 2.5 '
            '{NoAI:1b,NoGravity:1b,Tags:["tp_tree_vehicle"],'
            'Passengers:[{id:"minecraft:pig",NoAI:1b,NoGravity:1b,Tags:["tp_tree_rider"],'
            'Passengers:[{id:"minecraft:chicken",NoAI:1b,NoGravity:1b,Tags:["tp_tree_child"]}]}]}',
            f'/rotate {vehicle} 10 20', f'/rotate {rider} 40 -30', f'/rotate {child} -20 50')
        for destination in ('8 215 8 90 80', '~1 ~1 ~1 ~90 ~100', '3 211 3'):
            ops.append(cmd(f'/tp {vehicle} {destination}'))
            for entity in (vehicle, rider, child):
                for field in ('Pos', 'Rotation'):
                    ops.append(cmd(f'/data get entity {entity} {field}', expect_result=3 if field == 'Pos' else 2))
        for dimension in ('the_nether', 'overworld'):
            ops.append(cmd(f'/execute in minecraft:{dimension} run tp {vehicle} 5 215 5 30 40'))
            for entity in (vehicle, rider, child):
                for field in ('Pos', 'Rotation'):
                    ops.append(cmd(f'/execute in minecraft:{dimension} run data get entity {entity} {field}',
                                   expect_result=3 if field == 'Pos' else 2))
            for entity in (rider, child):
                ops.append(cmd(f'/execute in minecraft:{dimension} as {entity} on vehicle run data get entity @s Pos', expect_result=3))
            other = 'overworld' if dimension == 'the_nether' else 'the_nether'
            # A bounded selector pins which level holds each entity: no ghosts
            # left behind and no missing descendants in the destination.
            for entity in (vehicle, rider, child):
                bounded = entity.replace('limit=1', 'limit=1,distance=..10000')
                ops.append(cmd(f'/execute in minecraft:{other} positioned 0 210 0 unless entity {bounded}', expect_result=1))
    ops += setup('/kill @e[type=!player]')
    ops += setup('/summon oak_boat 4.5 210 4.5 {NoGravity:1b,Tags:["tp_tree_vehicle"]}')
    ops.append(cmd(f'/ride @s mount {vehicle}', expect_result=1))
    ops.append(cmd(f'/execute in minecraft:the_nether run tp {vehicle} 6 215 6'))
    ops.append({'op': 'settle'})
    ops.append(cmd('/execute if dimension minecraft:the_nether', expect_result=1))
    ops.append(cmd('/execute on vehicle run data get entity @s Pos', expect_result=3))
    ops.append(cmd('/data get entity @s Pos', expect_result=3))
    ops += setup('/ride @s dismount', '/execute in minecraft:overworld run tp @s 8.5 230 8.5')
    ops.append({'op': 'settle'})
    ops += setup('/execute in minecraft:the_nether run kill @e[tag=tp_tree_vehicle]')
    return ops
