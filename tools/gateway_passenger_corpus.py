"""End gateway vehicle transitions compared with vanilla."""


def gateway_passengers(cmd, setup):
    """#9335: a same-level gateway moves a live vehicle and all its riders."""
    prefix = '/execute in minecraft:the_end run '
    ops = setup(prefix + 'tp @s 8.5 230 8.5')
    ops.append({'op': 'settle'})
    for index, kind in enumerate(('horse', 'oak_boat', 'minecart')):
        x = 2 + index * 2
        ops += setup('/kill @e[type=!player]',
            f'setblock {x} 200 2 end_gateway{{Age:200L,exit_portal:[I;8,210,8],ExactTeleport:1b}}',
            f'summon {kind} {x + 0.5} 200 2.5 '
            '{NoGravity:1b,PersistenceRequired:1b,Tags:["gateway_vehicle"],'
            'Passengers:[{id:"minecraft:pig",NoAI:1b,NoGravity:1b,Tags:["gateway_rider"]}]}')
        if kind == 'horse':
            ops += setup('/attribute @e[tag=gateway_vehicle,limit=1] movement_speed base set 0')
        for _ in range(5):
            ops.append({'op': 'tick', 'n': 1})
            for tag in ('gateway_vehicle', 'gateway_rider'):
                ops.append(cmd(f'/data get entity @e[tag={tag},limit=1] Pos', expect_result=3))
        ops.append(cmd('/execute as @e[tag=gateway_rider,limit=1] on vehicle run data get entity @s Pos', expect_result=3))
    ops += setup('/kill @e[type=!player]',
                 'setblock 8 200 2 end_gateway{Age:200L,exit_portal:[I;8,210,8],ExactTeleport:1b}',
                 'summon oak_boat 8.5 200 2.5 {NoGravity:1b,Tags:["gateway_vehicle"]}',
                 '/advancement revoke @s only minecraft:end/enter_end_gateway')
    ops.append(cmd('/ride @s mount @e[tag=gateway_vehicle,limit=1]', expect_result=1))
    ops.append(cmd('/execute unless entity @s[advancements={minecraft:end/enter_end_gateway=true}]', expect_result=1))
    for step in range(5):
        # Unlike a world-only tick, this also runs the standalone Java player's
        # network-listener tick. Its enter_block trigger must run as Horizon's does.
        ops.append({'op': 'player_loop', 'n': 1, 'label': f'gateway passenger {step}'})
        ops.append(cmd('/execute if entity @s[advancements={minecraft:end/enter_end_gateway=true}]', expect_result=1))
        ops.append(cmd('/execute on vehicle run data get entity @s Pos', expect_result=3))
        ops.append(cmd('/data get entity @s Pos', expect_result=3))
    ops += setup('/ride @s dismount', '/kill @e[type=!player]', 'fill 2 200 2 8 200 2 air',
                 '/execute in minecraft:overworld run tp @s 8.5 230 8.5')
    ops.append({'op': 'settle'})
    return ops
