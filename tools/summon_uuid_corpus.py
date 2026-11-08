"""/summon trees whose UUIDs conflict with a live entity or with each other.

ServerLevel.tryAddFreshEntityWithPassengers rejects the whole tree when any
UUID in it is already loaded (commands.summon.failed.uuid), before anything
joins. Then it adds the root and each passenger in turn; addNewEntity skips
one whose UUID an earlier entity of the same tree took, but that entity still
rides its vehicle and saves under its Passengers.
"""
from command_exec_corpus import cmd

PIG = 'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b'


def uuid(n):
    return f'[I;160,0,0,{n}]'


def pig(tag, n, extra=''):
    return f'{{id:"minecraft:pig",UUID:{uuid(n)},Tags:["{tag}"],{PIG}{extra}}}'


INTERNAL = ['same_uuid', 'sibling', 'leaf_root']


def summon_uuid(raw=False, internal=True):
    """Live-UUID conflicts with their controls, and trees with duplicate UUIDs
    inside them (#11807)."""
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/tick freeze', 1)
    run('/scoreboard objectives add h160 dummy', 1)
    run('/fill 0 209 0 15 209 15 minecraft:stone', 256)
    run('/execute if loaded 4 210 4', 1)

    # The live entities later trees conflict with.
    run(f'/summon minecraft:pig 1.5 210 1.5 {{UUID:{uuid(100)},Tags:["h160_existing"],{PIG}}}', 1)
    run(f'/summon minecraft:armor_stand 1.5 210 3.5 {{UUID:{uuid(150)},Tags:["h160_stand"],NoGravity:1b}}', 1)
    for tag in ['h160_existing', 'h160_stand']:
        run(f'/execute if entity @e[tag={tag}]', 1)
        run(f'/data get entity @e[tag={tag},limit=1] UUID', 4)
        run(f'/data get entity @e[tag={tag},limit=1] Pos', 3)

    # Each member is (tag, uuid, children); its children ride it.
    def tree(name, root, children):
        return name, (f'h160_{name}', root, children)

    cases = [
        tree('unique', 120, [('h160_unique_child', 121, [('h160_unique_leaf', 122, [])])]),
        tree('existing_root', 100, []),
        tree('existing_stand_root', 150, []),
        tree('existing_child', 101, [('h160_existing_child_child', 100, [])]),
        tree('existing_leaf', 110, [('h160_existing_leaf_child', 111, [('h160_existing_leaf_leaf', 100, [])])]),
        tree('same_uuid', 102, [('h160_same_uuid_child', 102, [])]),
        tree('sibling', 103, [('h160_sibling_a', 104, []), ('h160_sibling_b', 104, [])]),
        tree('leaf_root', 130, [('h160_leaf_root_child', 131, [('h160_leaf_root_leaf', 130, [])])]),
        tree('unique_after', 140, [('h160_unique_after_child', 141, [])]),
    ]
    if not internal:
        cases = [case for case in cases if case[0] not in INTERNAL]

    def nbt(member):
        tag, n, children = member
        riders = ''
        if children:
            riders = ',Passengers:[' + ','.join(nbt(c) for c in children) + ']'
        return pig(tag, n, riders)

    def members(member, parent=None):
        tag, _, children = member
        yield tag, parent, children
        for c in children:
            yield from members(c, tag)

    for i, (name, root) in enumerate(cases):
        x = 3.5 + i
        body = nbt(root)[len('{id:"minecraft:pig",'):]
        run(f'/summon minecraft:pig {x} 210 6.5 {{{body}')
        for tag, parent, children in members(root):
            run(f'/execute if entity @e[tag={tag}]')
            run(f'/data get entity @e[tag={tag},limit=1] UUID')
            run(f'/data get entity @e[tag={tag},limit=1] Pos')
            if parent:
                run(f'/execute as @e[tag={tag}] on vehicle if entity @s[tag={parent}]')
            run(f'/execute store result score {tag} h160 run data get entity @e[tag={tag},limit=1] Passengers', record=False)
            run(f'/scoreboard players get {tag} h160')
            for n in range(len(children)):
                run(f'/data get entity @e[tag={tag},limit=1] Passengers[{n}].Tags[0]')
                run(f'/data get entity @e[tag={tag},limit=1] Passengers[{n}].UUID')
            if raw:
                run(f'/data get entity @e[tag={tag},limit=1]')
        run('/execute if entity @e[type=minecraft:pig]')
        run('/execute if entity @e[type=minecraft:armor_stand]')

    # A malformed UUID is not read (Entity.load keeps the random one): admitted.
    for n, value in enumerate(['[I;1,2,3]', '[I;1,2,3,4,5]', '"not-a-uuid"', '[L;1L,2L]']):
        tag = f'h160_malformed_{n}'
        run(f'/summon minecraft:pig {3.5 + n} 210 9.5 {{UUID:{value},Tags:["{tag}"],{PIG}}}')
        run(f'/execute if entity @e[tag={tag}]')
    run(f'/summon minecraft:pig 8.5 210 9.5 {{UUID:{uuid(160)},Tags:["h160_malformed_root"],{PIG},'
        f'Passengers:[{{id:"minecraft:pig",UUID:"bad",Tags:["h160_malformed_child"],{PIG}}}]}}')
    for tag in ['h160_malformed_root', 'h160_malformed_child']:
        run(f'/execute if entity @e[tag={tag}]')
    run('/execute as @e[tag=h160_malformed_child] on vehicle if entity @s[tag=h160_malformed_root]')

    # The entities the conflicts named are still the same ones.
    for tag in ['h160_existing', 'h160_stand']:
        run(f'/execute if entity @e[tag={tag}]', 1)
        run(f'/data get entity @e[tag={tag},limit=1] UUID', 4)
        run(f'/data get entity @e[tag={tag},limit=1] Pos', 3)
    run('/execute if entity @e[type=minecraft:pig]')
    run('/execute if entity @e[type=minecraft:armor_stand]', 1)
    return ops
