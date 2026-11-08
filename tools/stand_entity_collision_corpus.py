"""Armor stands moving into the entities that canBeCollidedWith them (#11877).

ArmorStand.travel moves with Entity.move, whose Entity.collide adds
EntityGetter.getEntityCollisions: a living shulker (Shulker.canBeCollidedWith)
and a boat (AbstractBoat.canBeCollidedWith) stop the stand as blocks do. One
stand falls onto a NoAI shulker, one onto a boat, one slides along the stone
into a shulker's side, and one falls onto the bare stone as the control. The
level is frozen and stepped, so each record is at a known tick.
"""
from command_exec_corpus import cmd, setup


def stand_entity_collisions():
    ops = [cmd('/tick freeze'), cmd('/forceload add 0 0 15 15'),
           {'op': 'settle', 'required_chunks': [[0, 0]]},
           {'op': 'wait_chunks', 'chunks': [[0, 0]]},
           *setup('/difficulty normal', '/gamerule spawn_mobs false'),
           cmd('/fill 0 199 0 15 199 15 stone', expect_result=256),
           cmd('/fill 0 200 0 15 210 15 air'),
           cmd('/execute if loaded 4 205 4', expect_result=1),
           cmd('/scoreboard objectives add stand_hit dummy', expect_result=1)]
    # The shulkers cling to the stone (AttachFace down) with their shells shut.
    for n, (x, z) in enumerate([(4.5, 4.5), (8.5, 10.5)], 1):
        ops += [cmd(f'/summon shulker {x} 200 {z} {{UUID:[I;11877,1,0,{n}],NoAI:1b,PersistenceRequired:1b,AttachFace:0b}}', expect_result=1),
                cmd(f'/execute if entity 00002e65-0000-0001-0000-00000000000{n}', expect_result=1)]
    ops += [cmd('/summon oak_boat 12.5 200 4.5 {UUID:[I;11877,2,0,1]}', expect_result=1),
            cmd('/execute if entity 00002e65-0000-0002-0000-000000000001', expect_result=1)]
    # (name, x, y, z, motion x): above the first shulker, above the boat, above
    # the stone, and on the stone heading east into the second shulker.
    stands = [('shulker', 4.5, 203, 4.5, 0), ('boat', 12.5, 203, 4.5, 0),
              ('stone', 8.5, 203, 4.5, 0), ('side', 5.5, 200, 10.5, 1)]
    uuids = []
    for n, (name, x, y, z, motion) in enumerate(stands, 1):
        uuid = f'00002e65-0000-0000-0000-00000000000{n}'
        uuids.append(uuid)
        ops += [cmd(f'/summon armor_stand {x} {y} {z} {{UUID:[I;11877,0,0,{n}],Tags:["stand_hit_{name}"],Motion:[{motion}d,0d,0d]}}', expect_result=1),
                cmd(f'/execute if data entity {uuid} {{Pos:[{x}d,{y}d,{z}d]}}', expect_result=1)]
    ops += setup('/execute store result score start stand_hit run time query gametime')
    # A fall from three blocks lands on the shulker (one block high) at tick 8,
    # on the boat at tick 9 and on the stone at tick 10; the side stand meets
    # the shulker at tick 3.
    for ticks, step in [(0, 0), (1, 1), (3, 2), (5, 2), (8, 3), (9, 1), (10, 1), (20, 10)]:
        if step:
            ops.append(cmd(f'/tick step {step}'))
            ops.append({'op': 'tick', 'n': step})
        ops += setup('/execute store result score elapsed stand_hit run time query gametime',
                     '/scoreboard players operation elapsed stand_hit -= start stand_hit')
        ops.append(cmd('/scoreboard players get elapsed stand_hit', expect_result=ticks))
        for uuid, (name, *_rest) in zip(uuids, stands):
            ops += [cmd(f'/execute as {uuid} if entity @s[type=minecraft:armor_stand,tag=stand_hit_{name}]', expect_result=1),
                    cmd(f'/data get entity {uuid} Pos', expect_result=3),
                    cmd(f'/data get entity {uuid} Motion', expect_result=3),
                    cmd(f'/data get entity {uuid} OnGround'),
                    cmd(f'/data get entity {uuid} fall_distance')]
    # The shulkers and the boat stayed where they were.
    for uuid in ['00002e65-0000-0001-0000-000000000001', '00002e65-0000-0001-0000-000000000002',
                 '00002e65-0000-0002-0000-000000000001']:
        ops.append(cmd(f'/data get entity {uuid} Pos', expect_result=3))
    return ops
