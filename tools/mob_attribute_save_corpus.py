"""Which attribute instances a living entity saves, before and after a tick.

AttributeMap.pack saves the instances that exist. AttributeMap.getInstance
creates one (computeIfAbsent); getValue of an absent one does not. A fresh
entity has none. Its first tick creates movement_speed (LivingEntity.aiStep
-> removeFrost -> getAttribute), and an /attribute query creates the one it
names.
"""
from command_exec_corpus import cmd

COMMON = 'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b'
KINDS = {'pig': COMMON, 'allay': COMMON, 'armor_stand': 'NoGravity:1b'}
IDS = ['movement_speed', 'max_health', 'follow_range', 'flying_speed', 'armor', 'scale']


def uuid(n):
    return f'[I;105,0,0,{n}]'


def mob_attribute_save(raw=False):
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/fill 0 199 0 15 199 15 minecraft:stone', 256)
    run('/execute if loaded 4 200 4', 1)
    run('/scoreboard objectives add h105 dummy', 1)

    def admit(tag, kind):
        # Routes that save or test the entity; none of them makes an attribute instance.
        run(f'/execute if entity @e[tag={tag},type=minecraft:{kind}]', 1)
        run(f'/data get entity @e[tag={tag},limit=1] UUID', 4)
        run(f'/data get entity @e[tag={tag},limit=1] Health')
        run(f'/data get entity @e[tag={tag},limit=1] Pos')
        run(f'/data get entity @e[tag={tag},limit=1] NoGravity')

    def attributes(tag):
        run(f'/execute store result score {tag} h105 run data get entity @e[tag={tag},limit=1] attributes', record=False)
        run(f'/scoreboard players get {tag} h105')
        for a in IDS:
            run(f'/execute if data entity @e[tag={tag},limit=1] attributes[{{id:"minecraft:{a}"}}]')
            run(f'/data get entity @e[tag={tag},limit=1] attributes[{{id:"minecraft:{a}"}}].base')
        run(f'/data get entity @e[tag={tag},limit=1] attributes[{{id:"minecraft:max_health"}}].modifiers')
        if raw:
            run(f'/data get entity @e[tag={tag},limit=1] attributes')
            run(f'/data get entity @e[tag={tag},limit=1]')

    actors = []
    n = 0
    for phase in ['pre', 'tick']:
        for kind, fields in KINDS.items():
            n += 1
            tag = f'h105_{phase}_{kind}'
            actors.append((phase, tag, kind))
            run(f'/summon minecraft:{kind} {1.5 + n} 200 3.5 {{UUID:{uuid(n)},Tags:["{tag}"],{fields}}}', 1)
    run(f'/summon minecraft:pig 2.5 200 8.5 {{UUID:{uuid(20)},Tags:["h105_read"],{COMMON}}}', 1)
    saved = ('attributes:[{id:"minecraft:max_health",base:20d,'
             'modifiers:[{id:"h105:extra",amount:2d,operation:"add_value"}]}]')
    run(f'/summon minecraft:pig 4.5 200 8.5 {{UUID:{uuid(21)},Tags:["h105_saved"],{COMMON},{saved}}}', 1)

    # Before any tick.
    run('/execute store result score #t0 h105 run time query gametime', record=False)
    for phase, tag, kind in actors:
        admit(tag, kind)
    admit('h105_read', 'pig')
    admit('h105_saved', 'pig')
    for phase, tag, kind in actors:
        if phase == 'pre':
            attributes(tag)
    attributes('h105_saved')
    # A query makes the instance it reads.
    run('/attribute @e[tag=h105_read,limit=1] minecraft:max_health get')
    attributes('h105_read')
    run('/execute store result score #t1 h105 run time query gametime', record=False)
    run('/scoreboard players operation #t1 h105 -= #t0 h105', record=False)
    run('/scoreboard players get #t1 h105')

    # Exactly one tick.
    ops.append({'op': 'tick', 'n': 1})
    run('/execute store result score #t2 h105 run time query gametime', record=False)
    run('/scoreboard players operation #t2 h105 -= #t0 h105', record=False)
    run('/scoreboard players get #t2 h105')
    for phase, tag, kind in actors:
        if phase == 'tick':
            attributes(tag)
    attributes('h105_saved')
    attributes('h105_read')
    return ops
