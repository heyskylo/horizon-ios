"""Armor stands moving into the entities that canBeCollidedWith them (#11877).

ArmorStand.travel moves with Entity.move, whose Entity.collide adds
EntityGetter.getEntityCollisions: a living shulker (Shulker.canBeCollidedWith)
and a boat (AbstractBoat.canBeCollidedWith) stop the stand as blocks do. One
stand falls onto a NoAI shulker, one onto a boat, one slides along the stone
into a shulker's side, and one falls onto the bare stone as the control. A
fifth stand rises with a shulker riding it: isPassengerOfSameVehicle leaves its
rider's box out. Then the first shulker's lid opens under its stand
(Shulker.onPeekAmountChange moves it up with MoverType.SHULKER, onto the box
the lid grew, Shulker.makeBoundingBox), opens fully and shuts again.

Then, in the next chunk east, lids on the other faces: a wall shulker's lid
pushes a stand sideways, a ceiling shulker's lid pushes a stand down into the
floor (the floor stops the push), and a floor shulker's lid lifts a stand into
a ceiling 0.025 above its head (makeBoundingBox's float height decides where it
stops). Vanilla ticks entities in the order they joined: each push comes once
with the stand joined after its shulker, so the push comes before the stand's
own travel, and once joined before it. A boat sliding on blue ice runs into a
wall shulker's open lid and picks the shulker up (AbstractBoat.tick): the box
query must reach the lid, one block out from the shulker's position. The level
is frozen and stepped, so each record is at a known tick.
"""
from command_exec_corpus import cmd, setup

SHULKER = '00002e65-0000-0001-0000-000000000001'


def stand_entity_collisions():
    ops = [cmd('/tick freeze'), cmd('/forceload add 0 0 31 15'),
           {'op': 'settle', 'required_chunks': [[0, 0], [1, 0]]},
           {'op': 'wait_chunks', 'chunks': [[0, 0], [1, 0]]},
           *setup('/difficulty normal', '/gamerule spawn_mobs false'),
           cmd('/fill 0 199 0 31 199 15 stone', expect_result=512),
           cmd('/fill 0 200 0 31 210 15 air'),
           cmd('/execute if loaded 4 205 4', expect_result=1),
           cmd('/execute if loaded 20 205 4', expect_result=1),
           cmd('/scoreboard objectives add stand_hit dummy', expect_result=1)]
    # The shulkers cling to the stone (AttachFace down) with their shells shut.
    for n, (x, z) in enumerate([(4.5, 4.5), (8.5, 10.5)], 1):
        ops += [cmd(f'/summon shulker {x} 200 {z} {{UUID:[I;11877,1,0,{n}],NoAI:1b,PersistenceRequired:1b,AttachFace:0b}}', expect_result=1),
                cmd(f'/execute if entity 00002e65-0000-0001-0000-00000000000{n}', expect_result=1)]
    ops += [cmd('/summon oak_boat 12.5 200 4.5 {UUID:[I;11877,2,0,1]}', expect_result=1),
            cmd('/execute if entity 00002e65-0000-0002-0000-000000000001', expect_result=1)]
    # (name, x, y, z, motion x, motion y): above the first shulker, above the
    # boat, above the stone, on the stone heading east into the second
    # shulker, and on the stone thrown up with a shulker on its head.
    stands = [('shulker', 4.5, 203, 4.5, 0, 0), ('boat', 12.5, 203, 4.5, 0, 0),
              ('stone', 8.5, 203, 4.5, 0, 0), ('side', 5.5, 200, 10.5, 1, 0),
              ('ridden', 12.5, 200, 10.5, 0, 0.6)]
    uuids = []
    for n, (name, x, y, z, motion_x, motion_y) in enumerate(stands, 1):
        uuid = f'00002e65-0000-0000-0000-00000000000{n}'
        uuids.append(uuid)
        ops += [cmd(f'/summon armor_stand {x} {y} {z} {{UUID:[I;11877,0,0,{n}],Tags:["stand_hit_{name}"],Motion:[{motion_x}d,{motion_y}d,0d]}}', expect_result=1),
                cmd(f'/execute if data entity {uuid} {{Pos:[{x}d,{y}d,{z}d]}}', expect_result=1)]
    # The rider sits on the stand's head (its passenger point, 1.975 up).
    rider = '00002e65-0000-0001-0000-000000000003'
    ops += [cmd('/summon shulker 12.5 201.975 10.5 {UUID:[I;11877,1,0,3],NoAI:1b,PersistenceRequired:1b}', expect_result=1),
            cmd(f'/ride {rider} mount {uuids[4]}', expect_result=1)]
    ops += setup('/execute store result score start stand_hit run time query gametime')

    def step(n):
        ops.append(cmd(f'/tick step {n}'))
        ops.append({'op': 'tick', 'n': n})

    def elapsed(ticks):
        ops.extend(setup('/execute store result score elapsed stand_hit run time query gametime',
                         '/scoreboard players operation elapsed stand_hit -= start stand_hit'))
        ops.append(cmd('/scoreboard players get elapsed stand_hit', expect_result=ticks))

    def motion(uuid, name):
        return [cmd(f'/execute as {uuid} if entity @s[type=minecraft:armor_stand,tag=stand_hit_{name}]', expect_result=1),
                cmd(f'/data get entity {uuid} Pos', expect_result=3),
                cmd(f'/data get entity {uuid} Motion', expect_result=3),
                cmd(f'/data get entity {uuid} OnGround')]

    # A fall from three blocks lands on the shulker (one block high) at tick 8,
    # on the boat at tick 9 and on the stone at tick 10; the side stand meets
    # the shulker at tick 3; the ridden stand rises until tick 5 or so.
    for ticks, n in [(0, 0), (1, 1), (3, 2), (5, 2), (8, 3), (9, 1), (10, 1), (20, 10)]:
        if n:
            step(n)
        elapsed(ticks)
        for uuid, (name, *_rest) in zip(uuids, stands):
            ops += motion(uuid, name)
            ops.append(cmd(f'/data get entity {uuid} fall_distance'))
        ops.append(cmd(f'/data get entity {rider} Pos', expect_result=3))
    # The shulkers and the boat stayed where they were.
    for uuid in [SHULKER, '00002e65-0000-0001-0000-000000000002',
                 '00002e65-0000-0002-0000-000000000001']:
        ops.append(cmd(f'/data get entity {uuid} Pos', expect_result=3))
    # The first shulker's lid opens to 30 (ShulkerPeekGoal's idle peek) at
    # 0.05 a tick, lifting its stand each tick, then all the way and shut
    # again: the stand rides the lid up and settles back on the shell. The
    # lid takes 20 ticks to shut. Horizon ticks armor stands before mobs, so
    # while it shuts the stand lands on the lid of a tick before; the records
    # wait until the lid is shut.
    ticks = 20
    for peek, steps in [(30, [1] * 8), (100, [1, 1, 2, 4, 12]), (0, [21, 4])]:
        ops.append(cmd(f'/data merge entity {SHULKER} {{Peek:{peek}b}}'))
        for n in steps:
            step(n)
            ticks += n
            elapsed(ticks)
            ops += motion(uuids[0], 'shulker')
            ops.append(cmd(f'/data get entity {SHULKER} Peek'))
    ops += other_faces()
    return ops


def uuid(kind, n):
    return f'00002e65-0000-{kind:04x}-0000-{n:012x}'


def other_faces():
    """The lids on the other faces, and the boat that runs into an open one."""
    ops = []

    def shulker(n, x, y, z, face, peek=0):
        return [cmd(f'/summon shulker {x} {y} {z} {{UUID:[I;11877,3,0,{n}],NoAI:1b,PersistenceRequired:1b,AttachFace:{face}b,Peek:{peek}b}}', expect_result=1),
                cmd(f'/execute if entity {uuid(3, n)}', expect_result=1)]

    def stand(n, x, y, z):
        return [cmd(f'/summon armor_stand {x} {y} {z} {{UUID:[I;11877,4,0,{n}]}}', expect_result=1),
                cmd(f'/execute if entity {uuid(4, n)}', expect_result=1)]

    # (stand, shulker): a west wall shulker opening east, then the stand
    # beside it, and the same with the stand first; a ceiling shulker opening
    # down onto a stand on the floor, both ways; a stand on a floor shulker
    # 0.025 under a stone ceiling, the stand first (the order Horizon ticks).
    ops += [cmd('/setblock 19 200 1 stone'), *shulker(1, 20.5, 200, 1.5, 4), *stand(1, 21.25, 200, 1.5),
            *stand(2, 21.25, 200, 4.5), cmd('/setblock 19 200 4 stone'), *shulker(2, 20.5, 200, 4.5, 4),
            cmd('/setblock 24 203 1 stone'), *shulker(3, 24.5, 202, 1.5, 1), *stand(3, 24.5, 200, 1.5),
            *stand(4, 24.5, 200, 4.5), cmd('/setblock 24 203 4 stone'), *shulker(4, 24.5, 202, 4.5, 1),
            *stand(5, 28.5, 201, 1.5), *shulker(5, 28.5, 200, 1.5, 0), cmd('/setblock 28 203 1 stone')]
    # The boat's lane: blue ice up to a west wall shulker, already open.
    ops += [cmd('/fill 20 199 10 31 199 14 blue_ice', expect_result=60),
            cmd('/setblock 19 200 12 stone'), *shulker(6, 20.5, 200, 12.5, 4, 100)]
    stands = [uuid(4, n) for n in range(1, 6)]
    boat = uuid(5, 1)
    rider = uuid(3, 6)

    def step(n):
        ops.extend([cmd(f'/tick step {n}'), {'op': 'tick', 'n': n}])

    def probe(with_boat):
        for u in stands:
            ops.extend([cmd(f'/data get entity {u} Pos', expect_result=3),
                        cmd(f'/data get entity {u} Motion', expect_result=3),
                        cmd(f'/data get entity {u} OnGround')])
        if with_boat:
            ops.extend([cmd(f'/data get entity {boat} Pos', expect_result=3),
                        cmd(f'/execute as {rider} on vehicle if entity @s[type=minecraft:oak_boat]')])

    # The stands settle and the boat lane's lid opens (20 ticks).
    step(25)
    probe(False)
    ops.append(cmd('/summon oak_boat 26.5 200 12.5 {UUID:[I;11877,5,0,1],Motion:[-0.6d,0d,0d]}', expect_result=1))
    for peek, steps in [(30, [1] * 8), (100, [1, 1, 2, 4, 12])]:
        for n in range(1, 6):
            ops.append(cmd(f'/data merge entity {uuid(3, n)} {{Peek:{peek}b}}'))
        for n in steps:
            step(n)
            probe(True)
    # The shulkers kept their faces, and the boat's shulker rides it.
    for n in range(1, 7):
        ops.append(cmd(f'/data get entity {uuid(3, n)} AttachFace'))
    ops.append(cmd(f'/data get entity {rider} Pos', expect_result=3))
    return ops
