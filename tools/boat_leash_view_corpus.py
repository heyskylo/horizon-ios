"""Live boat leash command reads after real connected-owner interactions."""


def cases(cmd, setup):
    boat = '00000146-0000-0001-0000-000000000001'
    pending = '00000146-0000-0001-0000-000000000002'
    unknown = '[I;111,222,333,444]'
    ops = [cmd('/tick freeze'), cmd('/forceload add 0 0 31 31'),
           {'op': 'wait_chunks', 'chunks': [[0, 0], [1, 0], [0, 1], [1, 1]]}]
    for x, z in [(0, 0), (16, 0), (0, 16), (16, 16)]:
        ops.append(cmd(f'/execute if loaded {x} 100 {z}', expect_result=1))
    ops += [cmd('/fill 0 99 0 31 99 31 stone', expect_result=1024),
            cmd('/fill 0 100 0 31 104 31 air'),
            cmd('/gamemode survival @s'), cmd('/tp @s 4.5 100 4.5 0 0'),
            cmd('/clear @s'), cmd('/scoreboard objectives add boat_view dummy'),
            cmd('/item replace entity @s hotbar.0 with lead 2', expect_result=1)]
    ops += setup('/execute store result score #start boat_view run time query gametime',
                 '/data modify storage boat_view:guard owner set from entity @s UUID')

    def owner():
        return [cmd('/execute if entity @s[type=player,gamemode=survival,nbt={Pos:[4.5d,100d,4.5d],Health:20f}]', expect_result=1),
                cmd('/data get entity @s UUID'), cmd('/data get entity @s Inventory')]

    def actor(target, x, z, tag):
        return [
            cmd(f'/execute if entity @e[type=oak_boat,tag={tag},nbt={{UUID:[I;326,1,0,{int(target[-8:], 16)}],Pos:[{x}d,100d,{z}d],NoGravity:1b}}]', expect_result=1),
            cmd(f'/data get entity {target} UUID', expect_result=4),
            cmd(f'/data get entity {target} Pos', expect_result=3),
            cmd(f'/execute if block {int(x)} 99 {int(z)} stone', expect_result=1)]

    def leash(target, present):
        rows = [cmd(f'/data get entity {target} leash'),
                cmd(f'/data get entity {target} leash.UUID', expect_result=4 if present else None)]
        rows += setup('/scoreboard players set #r boat_view 31',
                      '/scoreboard players set #s boat_view 17')
        rows += [cmd('/scoreboard players get #r boat_view', expect_result=31),
                 cmd('/scoreboard players get #s boat_view', expect_result=17),
                 cmd('/execute store result score #r boat_view store success score #s boat_view '
                     f'run data get entity {target} leash.UUID'),
                 cmd('/scoreboard players get #r boat_view', expect_result=4 if present else None),
                 cmd('/scoreboard players get #s boat_view', expect_result=int(present))]
        return rows

    ops += owner()
    ops += [cmd('/summon oak_boat 6.5 100 4.5 {UUID:[I;326,1,0,1],Tags:["boat_view"],NoGravity:1b,Motion:[0d,0d,0d]}')]
    ops += actor(boat, 6.5, 4.5, 'boat_view') + leash(boat, False)
    # Attach, detach and attach again. Each click traverses the real owner packet path.
    for attached, lead_count in [(True, 1), (False, 1), (True, 0)]:
        ops.append({'op': 'boat_interact', 'uuid': boat})
        ops += owner() + actor(boat, 6.5, 4.5, 'boat_view') + leash(boat, attached)
        ops.append(cmd('/clear @s lead 0', expect_result=lead_count if lead_count else None))
        if not lead_count:
            ops.append(cmd('/execute unless items entity @s hotbar.0 lead', expect_result=1))
        if attached:
            ops += setup('/data modify storage boat_view:guard owner set from entity @s UUID')
            ops.append(cmd('/execute store success score #different boat_view run data modify '
                           f'storage boat_view:guard owner set from entity {boat} leash.UUID'))
            ops.append(cmd('/scoreboard players get #different boat_view', expect_result=0))
            ops += leash(boat, True)  # Reads themselves do not change the live association.
    ops += [cmd(f'/summon oak_boat 10.5 100 4.5 {{UUID:[I;326,1,0,2],Tags:["boat_pending"],NoGravity:1b,leash:{{UUID:{unknown}}}}}')]
    ops += actor(pending, 10.5, 4.5, 'boat_pending') + leash(pending, True)
    ops.append(cmd('/execute unless entity 0000006f-0000-00de-0000-014d000001bc', expect_result=1))
    ops += [cmd(f'/execute if entity {pending}'),
            cmd('/data get storage boat_view:guard owner')]
    ops += setup('/execute store result score #elapsed boat_view run time query gametime',
                 '/scoreboard players operation #elapsed boat_view -= #start boat_view')
    ops += [cmd('/scoreboard players get #elapsed boat_view', expect_result=0)] + owner()
    # Independently measured loaded-link control: after resolution and a real
    # detach, stale input NBT must not override the now-absent live association.
    # This is the pinned harness owner's UUID, positively checked before use.
    holder = '[I;-1608632253,-450807400,-1786055591,-2146546415]'
    loaded = '00000146-0000-0001-0000-000000000003'
    ops += [cmd(f'/execute if entity @s[nbt={{UUID:{holder}}}]', expect_result=1),
            cmd('/tp @s 4.5 100 8.5 0 0'),
            cmd('/execute if entity @s[type=player,gamemode=survival,nbt={Pos:[4.5d,100d,8.5d],Health:20f}]', expect_result=1),
            cmd(f'/summon oak_boat 6.5 100 8.5 {{UUID:[I;326,1,0,3],Tags:["boat_loaded"],NoGravity:1b,Motion:[0d,0d,0d],leash:{{UUID:{holder}}}}}')]
    ops += actor(loaded, 6.5, 8.5, 'boat_loaded') + leash(loaded, True)
    ops.append(cmd(f'/execute as {loaded} on leasher if entity @s[type=player]'))
    ops += setup('/execute store result score #loaded_start boat_view run time query gametime')
    ops += [cmd('/tick step 1', expect_result=1), {'op': 'tick', 'n': 1}]
    ops += actor(loaded, 6.5, 8.5, 'boat_loaded') + leash(loaded, True)
    ops += [cmd(f'/execute as {loaded} on leasher if entity @s[nbt={{UUID:{holder}}}]', expect_result=1),
            cmd('/item replace entity @s hotbar.0 with lead', expect_result=1),
            {'op': 'boat_interact', 'uuid': loaded}]
    ops += actor(loaded, 6.5, 8.5, 'boat_loaded') + leash(loaded, False)
    ops += [cmd(f'/execute as {loaded} on leasher if entity @s[type=player]'),
            cmd('/clear @s lead 0', expect_result=1),
            cmd('/execute if entity @s[type=player,gamemode=survival,nbt={Pos:[4.5d,100d,8.5d],Health:20f}]', expect_result=1)]
    ops += setup('/execute store result score #loaded_elapsed boat_view run time query gametime',
                 '/scoreboard players operation #loaded_elapsed boat_view -= #loaded_start boat_view')
    ops += [cmd('/scoreboard players get #loaded_elapsed boat_view', expect_result=1)]
    return ops
