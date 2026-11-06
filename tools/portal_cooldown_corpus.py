"""Observe live per-kind portal cooldowns through the production entity tick."""


def cases(cmd, setup):
    ops = setup('/forceload add 0 0',
                '/fill 2 209 2 14 214 14 air',
                '/fill 2 209 2 14 209 14 stone')
    actors = [('oak_boat', 'boat', 4), ('minecart', 'cart', 7),
              ('arrow', 'arrow', 10), ('pig', 'mob', 13)]
    for kind, tag, x in actors:
        ops += setup(f'/setblock {x} 210 4 nether_portal[axis=x]',
                     f'/summon {kind} {x}.5 210 4.5 {{NoGravity:1b,NoAI:1b,PortalCooldown:5,Tags:["portal_cooldown_actor","pc_{tag}"]}}')
    # While inside, setAsInsidePortal refreshes a positive cooldown to the
    # actual entity class delay on every tick; no destination is needed.
    for ticks in (1, 3):
        ops.append({'op': 'tick', 'n': ticks})
        for _, tag, _ in actors:
            ops.append(cmd(f'/data get entity @e[tag=pc_{tag},limit=1] PortalCooldown'))
    for _, tag, x in actors:
        ops += setup(f'/tp @e[tag=pc_{tag}] {x}.5 210 8.5')
    for ticks in (1, 1, 8):
        ops.append({'op': 'tick', 'n': ticks})
        for _, tag, _ in actors:
            ops.append(cmd(f'/data get entity @e[tag=pc_{tag},limit=1] PortalCooldown'))
    return ops + setup('/kill @e[tag=portal_cooldown_actor]',
                       '/fill 2 209 2 14 214 14 air')


def arrivals(cmd, setup):
    """Transferred counters survive the waiting arrival compound and live load."""
    end = '/execute in minecraft:the_end run'
    overworld = '/execute in minecraft:overworld run'
    ops = setup('/forceload add 0 0', '/setworldspawn 8 230 8',
                '/fill 6 229 6 10 229 10 stone',
                '/fill 6 230 6 10 233 10 air',
                f'{end} tp @s 8.5 230 8.5')
    ops += [{'op': 'settle'}]
    ops += setup(f'{end} forceload add 0 0',
                 f'{end} fill 2 209 2 12 214 12 air',
                 f'{end} fill 2 209 2 12 209 12 stone')
    for kind, tag, x in [('oak_boat', 'boat', 4), ('minecart', 'cart', 7),
                         ('arrow', 'arrow', 10)]:
        ops += setup(f'{end} setblock {x} 210 4 end_portal',
                     f'{end} summon {kind} {x}.5 210.1 4.5 '
                     '{NoGravity:1b,Tags:["portal_cooldown_arrival",'
                     f'"pca_{tag}"]' + '}')
    for ticks in (3, 1, 1, 8):
        ops.append({'op': 'tick', 'n': ticks})
        for tag in ('boat', 'cart', 'arrow'):
            target = f'@e[tag=pca_{tag},distance=..1000000,limit=1]'
            ops.append(cmd(f'{overworld} execute if entity {target}', expect_result=1))
            ops.append(cmd(f'{overworld} data get entity {target} PortalCooldown'))
    ops += setup('/kill @e[tag=portal_cooldown_arrival]',
                 f'{end} fill 2 209 2 12 214 12 air',
                 f'{overworld} tp @s 8.5 230 8.5')
    return ops + [{'op': 'settle'}]


def gateways(cmd, setup):
    """Gateway travellers share entity-specific counters, including live data loads."""
    ops = setup('/execute in minecraft:the_end run tp @s 8.5 230 8.5')
    ops += [{'op': 'settle'}]
    for index, kind in enumerate(('oak_boat', 'ender_pearl', 'item')):
        x = 2 + index * 2
        item = ',Item:{id:"minecraft:stone",count:1},PickupDelay:32767s' if kind == 'item' else ''
        ops += setup('/kill @e[tag=portal_cooldown_gateway]',
                     f'setblock {x} 200 2 end_gateway{{Age:200L,exit_portal:[I;8,210,8],ExactTeleport:1b}}',
                     f'summon {kind} {x}.5 200.2 2.5 '
                     '{NoGravity:1b,Tags:["portal_cooldown_gateway"]' + item + '}')
        ops += [{'op': 'tick', 'n': 3}]
        target = '@e[tag=portal_cooldown_gateway,limit=1]'
        ops.append(cmd('/execute if entity @e[tag=portal_cooldown_gateway,x=0,y=210,z=0,dx=16,dy=3,dz=16]', expect_result=1))
        ops.append(cmd(f'/data get entity {target} PortalCooldown'))
        # Horizon's projectile data mutation surface is separately incomplete.
        if kind != 'ender_pearl':
            ops += setup(f'/data merge entity {target} {{PortalCooldown:0}}')
            ops.append(cmd(f'/data get entity {target} PortalCooldown', expect_result=0))
            ops += [{'op': 'tick', 'n': 1}]
            ops.append(cmd(f'/data get entity {target} PortalCooldown', expect_result=0))
    ops += setup('/kill @e[tag=portal_cooldown_gateway]', 'fill 2 200 2 6 200 2 air',
                 '/execute in minecraft:overworld run tp @s 8.5 230 8.5')
    return ops + [{'op': 'settle'}]
