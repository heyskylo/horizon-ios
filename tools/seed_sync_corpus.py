"""Fields an unseeded random decides, compared exactly after a sync step (docs/parity-workflow.md, "Randoms that repeat").

Each case syncs both engines' randoms (`sync_randoms`: the server thread's seed uniquifier, the entity-id counter, the
level's random) and then reads, before any tick unless it says otherwise, what the next randoms decided. Without the
sync (test_command_exec_parity.py --unsynced-randoms) every one of these differs run to run, in both engines.
"""


def sync(case):
    return {'op': 'sync_randoms', 'uniquifier': 12137 + case, 'entity_id': 100000 * (case + 1), 'level_seed': 4187 + case}


def cases(cmd, setup):
    def get(selector, *fields):
        return [cmd(f'/data get entity {selector} {field}') for field in fields]

    def near(kind, x, z):
        # Only this case's entity: chunks loading elsewhere can drop items too (a mineshaft's rails).
        return f'@e[type={kind},x={x}.5,y=201,z={z}.5,distance=..2,limit=1]'

    resting = (2, 5, 8, 11)
    return [
        *setup('/fill 0 200 0 15 200 15 minecraft:stone', '/kill @e[type=!player]'),
        # Block.popResource: the drop's position from the level's random. Its own random makes, in order, its UUID
        # (Mth.createInsecureUUID), bobOffs, its yaw and its motion.
        *setup('/setblock 4 201 4 minecraft:dirt'), sync(0), *setup('/setblock 4 201 4 minecraft:air destroy'),
        *get(near('item', 4, 4), 'Pos', 'UUID', 'Rotation', 'Motion'), *setup('/kill @e[type=item]'),
        # TntBlock.onPlace beside a signal: PrimedTnt's kick from the level's random, its UUID from its own.
        *setup('/setblock 8 201 4 minecraft:redstone_block'), sync(1), *setup('/setblock 9 201 4 minecraft:tnt'),
        *get(near('tnt', 9, 4), 'Motion', 'UUID'),
        *setup('/kill @e[type=tnt]', '/setblock 8 201 4 minecraft:air'),
        # DefaultDispenseItemBehavior.spawnItem, from the dispenser's own tick: the level's random again. The item's
        # own random makes only its UUID, bobOffs and yaw: the dispenser gives the motion.
        *setup('/setblock 4 201 10 minecraft:dispenser[facing=east]{Items:[{Slot:0b,id:"minecraft:stone",count:1}]}'),
        sync(2), {'op': 'crafter_tick', 'pos': [4, 201, 10]},
        *get(near('item', 5, 10), 'Pos', 'Motion', 'UUID'),
        *setup('/kill @e[type=item]', '/setblock 4 201 10 minecraft:air'),
        # ItemEntity.tick: a grounded, still item moves (and stops gravity piling up) one tick in four, by
        # (tickCount + id) % 4, so the entity ids decide which of four moves after a tick.
        sync(3), *setup(*[f'/summon item {x}.5 201 13.5 {{Item:{{id:"minecraft:stone",count:1}},OnGround:1b}}'
                          for x in resting]),
        {'op': 'tick'},
        *[c for x in resting for c in get(f'@e[type=item,x={x}.5,y=201,z=13.5,distance=..0.5,limit=1]', 'Motion')],
        *setup('/kill @e[type=item]'),
        # A summoned item without a UUID keeps its own random's; its bobOffs comes from that random too, so the
        # level's random is still where the sync left it when the TNT beside the signal is primed.
        *setup('/setblock 8 201 7 minecraft:redstone_block'), sync(4),
        *setup('/summon item 12.5 201 7.5 {Item:{id:"minecraft:stone",count:1}}', '/setblock 9 201 7 minecraft:tnt'),
        *get(near('item', 12, 7), 'UUID'), *get(near('tnt', 9, 7), 'Motion'),
        *setup('/kill @e[type=!player]', '/setblock 8 201 7 minecraft:air'),
        # SummonCommand.createEntity loads a mob summoned without NBT from `{id:..}`: yaw 0, with no draw from the
        # level's random. (What finalizeSpawn then draws from the level's random is #12965.)
        sync(5), *setup('/summon pig 12.5 201 1.5'),
        *get(near('pig', 12, 1), 'Rotation'),
        *setup('/kill @e[type=!player]'),
        # A dispensed spawn egg: EntityType.create snaps the mob to Mth.wrapDegrees(level.random.nextFloat() * 360),
        # so a draw of 180 or more reads back below zero. Two seeds, so that at least one draw is likely to wrap.
        *[c for case, z in ((6, 3), (7, 5)) for c in (
            *setup(f'/setblock 1 201 {z} minecraft:dispenser[facing=east]{{Items:[{{Slot:0b,id:"minecraft:pig_spawn_egg",count:1}}]}}'),
            sync(case), {'op': 'crafter_tick', 'pos': [1, 201, z]},
            *get(near('pig', 2, z), 'Rotation'),
            *setup('/kill @e[type=!player]', f'/setblock 1 201 {z} minecraft:air'),
        )],
        # Entity.random makes the UUID first; LivingEntity.<init> then draws the yaw (which the summon's load
        # replaces), and Chicken.<init> the egg timer.
        sync(8), *setup('/summon chicken 1.5 201 1.5 {NoAI:1b}'),
        *get(near('chicken', 1, 1), 'UUID', 'EggLayTime'),
        # Without NBT the summon finalizes the pig; its UUID is still its own random's first draws.
        sync(9), *setup('/summon pig 4.5 201 1.5'), *get(near('pig', 4, 1), 'UUID'),
        sync(10), *setup('/summon armor_stand 12.5 201 1.5'), *get(near('armor_stand', 12, 1), 'UUID'),
        # DispenserBlock with a spawn egg (EntityType.spawn) and with an armor stand: made by EntityType.create.
        *setup('/setblock 13 201 4 minecraft:dispenser[facing=up]{Items:[{Slot:0b,id:"minecraft:pig_spawn_egg",count:1}]}'),
        sync(11), {'op': 'crafter_tick', 'pos': [13, 201, 4]},
        *get('@e[type=pig,x=13.5,y=202,z=4.5,distance=..2,limit=1]', 'UUID'),
        *setup('/setblock 13 201 10 minecraft:dispenser[facing=up]{Items:[{Slot:0b,id:"minecraft:armor_stand",count:1}]}'),
        sync(12), {'op': 'crafter_tick', 'pos': [13, 201, 10]},
        *get('@e[type=armor_stand,x=13.5,y=202,z=10.5,distance=..2,limit=1]', 'UUID'),
        # A chicken due to lay lays on its first tick and draws the next egg time from its own random, which carries
        # on through the load of a summon's NBT.
        sync(13), *setup('/summon chicken 7.5 201 13.5 {NoAI:1b,EggLayTime:1}'), {'op': 'tick'},
        *get(near('chicken', 7, 13), 'EggLayTime'),
        *setup('/kill @e[type=!player]', '/fill 0 201 0 15 202 15 minecraft:air'),
        # Projectile.shoot spreads a dispensed shot with the projectile's own random, after its UUID's two draws.
        *[c for case, item, kind, z in ((14, 'arrow', 'arrow', 7), (15, 'snowball', 'snowball', 9)) for c in (
            *setup(f'/setblock 1 201 {z} minecraft:dispenser[facing=east]{{Items:[{{Slot:0b,id:"minecraft:{item}",count:1}}]}}'),
            sync(case), {'op': 'crafter_tick', 'pos': [1, 201, z]},
            *get(near(kind, 2, z), 'UUID', 'Motion'),
            *setup('/kill @e[type=!player]', f'/setblock 1 201 {z} minecraft:air'),
        )],
        # A summoned arrow without a UUID keeps the one its own random made.
        sync(16), *setup('/summon arrow 6.5 201 11.5 {NoGravity:1b}'), *get(near('arrow', 6, 11), 'UUID'),
        *setup('/kill @e[type=!player]'),
    ]
