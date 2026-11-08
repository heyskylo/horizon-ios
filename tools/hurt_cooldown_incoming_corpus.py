"""A mob's hurt cooldown compares the incoming damage, before its reductions.

LivingEntity.hurtServer: inside the cooldown (invulnerableTime > 10) a hit
is taken only when its amount is above lastHurt, and actuallyHurt gets
amount - lastHurt; outside it, lastHurt = amount, invulnerableTime = 20 and
actuallyHurt gets the whole amount. actuallyHurt then applies armor
(getDamageAfterArmorAbsorb) and Resistance and protection
(getDamageAfterMagicAbsorb). So the comparison and lastHurt use the amount
before armor and Resistance.

Subjects: NoAI pigs with max_health 40. Full Resistance (amplifier 4), no
Resistance, partial Resistance (amplifier 0), armor 10, and the cooldown
expiry after exactly 9 and 10 ticks. Each hit records its result and success
stores, and each state read records Health and the raw HurtTime.
"""
from command_exec_corpus import cmd, setup


def hurt_cooldown_incoming():
    ops = [cmd('/gamerule spawn_mobs false'), cmd('/forceload add 0 0 31 31', expect_result=4), {'op': 'settle'}]
    for x, z in [(0, 0), (16, 0), (0, 16), (16, 16)]:
        ops.append(cmd(f'/execute if loaded {x} 200 {z}', expect_result=1))
    ops.extend(setup('/fill 0 199 0 31 208 31 air'))
    ops.extend([cmd('/fill 0 199 0 31 199 31 stone', expect_result=1024),
                cmd('/scoreboard objectives add h248 dummy', expect_result=1)])
    serial = 0

    def pig(resistance=None, armor=None):
        nonlocal serial
        serial += 1
        tag = f'h248_{serial}'
        uuid = f'00000000-0000-00f8-0000-0000000000{serial:02x}'
        x, z = 4.5 + 6 * ((serial - 1) % 5), 4.5 + 6 * ((serial - 1) // 5)
        nbt = (f'UUID:[I;0,248,0,{serial}],Tags:["h248","{tag}"],'
               'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,'
               'attributes:[{id:"minecraft:max_health",base:40.0d}],Health:40f')
        ops.extend([
            cmd(f'/summon minecraft:pig {x} 200 {z} {{{nbt}}}', expect_result=1),
            cmd(f'/execute if entity @e[type=minecraft:pig,tag={tag}]', expect_result=1),
            cmd(f'/execute as {uuid} if entity @s[tag={tag},nbt={{NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b}}]',
                expect_result=1),
            cmd(f'/data get entity {uuid} UUID', expect_result=4),
            cmd(f'/data get entity {uuid} Pos'),
            cmd(f'/attribute {uuid} minecraft:max_health base get', expect_result=40),
        ])
        if resistance is not None:
            ops.append(cmd(f'/effect give {uuid} minecraft:resistance 30 {resistance} true', expect_result=1))
            ops.append(cmd(f'/data get entity {uuid} active_effects[{{id:"minecraft:resistance"}}]'))
        if armor is not None:
            ops.append(cmd(f'/attribute {uuid} minecraft:armor base set {armor}'))
            ops.append(cmd(f'/attribute {uuid} minecraft:armor get'))
        read(uuid)
        return uuid

    def read(uuid):
        ops.extend([
            cmd(f'/execute if entity {uuid}', expect_result=1),
            cmd(f'/data get entity {uuid} Health'),
            cmd(f'/data get entity {uuid} Health 1000'),
            cmd(f'/data get entity {uuid} HurtTime'),
        ])

    def hit(uuid, amount, kind='minecraft:generic'):
        ops.extend([
            cmd('/scoreboard players set #r h248 31', expect_result=31),
            cmd('/scoreboard players set #s h248 17', expect_result=17),
            cmd(f'/execute store result score #r h248 store success score #s h248 run damage {uuid} {amount} {kind}'),
            cmd('/scoreboard players get #r h248'),
            cmd('/scoreboard players get #s h248'),
        ])
        read(uuid)

    def ticks(n, label):
        ops.append(cmd(f'/execute store result score #{label}0 h248 run time query gametime', record=False))
        ops.append({'op': 'tick', 'n': n})
        ops.append(cmd(f'/execute store result score #{label}1 h248 run time query gametime', record=False))
        ops.append(cmd(f'/scoreboard players operation #{label}1 h248 -= #{label}0 h248', record=False))
        ops.append(cmd(f'/scoreboard players get #{label}1 h248', expect_result=n))

    # Inside the cooldown: a stronger hit, an equal one, a weaker one, a stronger one.
    for resistance in [4, None, 0]:
        uuid = pig(resistance)
        for amount in [1, 2, 2, 1.5, 3]:
            hit(uuid, amount)
    # Armor 10 (no Resistance): the reduction is not linear in the amount.
    # Cactus damage, as generic is in #bypasses_armor.
    uuid = pig(armor=10)
    for amount in [4, 8, 8]:
        hit(uuid, amount, 'minecraft:cactus')
    # The cooldown's end: invulnerableTime 20 - 9 = 11 is inside, 20 - 10 = 10 is not.
    for resistance, wait, amounts in [(4, 9, [1, 2]), (4, 10, [1]), (None, 9, [1, 2]), (None, 10, [1])]:
        uuid = pig(resistance)
        hit(uuid, 1)
        ticks(wait, f't{serial}')
        read(uuid)
        for amount in amounts:
            hit(uuid, amount)
    ops.extend(setup('/kill @e[tag=h248]', '/forceload remove 0 0 31 31', '/scoreboard objectives remove h248'))
    return ops
