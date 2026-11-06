"""Dropped-item motion through a same-level End gateway."""


def gateway_item_motion(cmd, setup):
    """#11319: gateway arrival retains the item's current velocity."""
    ops = setup('/execute in minecraft:the_end run tp @s 8.5 230 8.5')
    ops.append({'op': 'settle'})
    for index, motion in enumerate(('[0.125d,0.25d,-0.125d]', '[0.0d,0.0d,0.0d]')):
        x = 2 + index * 2
        ops += setup('/kill @e[type=!player]',
            f'setblock {x} 200 2 end_gateway{{Age:200L,exit_portal:[I;8,210,8],ExactTeleport:1b}}',
            f'summon item {x + 0.5} 200.2 2.5 '
            '{NoGravity:1b,PickupDelay:32767s,Tags:["gateway_item"],'
            f'Item:{{id:"minecraft:stone",count:1}},Motion:{motion}' + '}')
        ops.append({'op': 'tick', 'n': 3})
        ops.append(cmd('/execute if entity @e[tag=gateway_item,x=0,y=210,z=0,dx=16,dy=3,dz=16,limit=1]', expect_result=1))
        for axis in range(3):
            ops.append(cmd(f'/data get entity @e[tag=gateway_item,limit=1] Motion[{axis}] 1000'))
    ops += setup('/kill @e[type=!player]', 'fill 2 200 2 4 200 2 air',
                 '/execute in minecraft:overworld run tp @s 8.5 230 8.5')
    ops.append({'op': 'settle'})
    return ops
