"""/attribute on a zombie's minecraft:baby speed modifier.

Zombie.setBaby removes the minecraft:baby modifier and, for a baby, adds it
back as a transient modifier (+0.5 add_multiplied_base). /attribute
modifier add refuses an id the instance already holds (transient or not);
modifier remove takes it away whatever kind it is; the next setBaby
(an IsBaby merge) puts the transient one back for a baby.
"""
from command_exec_corpus import cmd

ZOMBIE = 'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b,Health:20f'
ATTR = 'minecraft:movement_speed'


def uuid(n):
    return f'[I;235,0,0,{n}]'


def baby_modifier(raw=False):
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/difficulty normal')
    run('/fill 0 199 0 15 199 15 minecraft:stone', 256)
    run('/execute if loaded 4 200 4', 1)
    run('/scoreboard objectives add h235 dummy', 1)
    run('/execute store result score #t0 h235 run time query gametime', record=False)

    def admit(tag):
        run(f'/execute if entity @e[tag={tag},type=minecraft:zombie]', 1)
        run(f'/data get entity @e[tag={tag},limit=1] UUID', 4)
        run(f'/data get entity @e[tag={tag},limit=1] IsBaby')
        run(f'/data get entity @e[tag={tag},limit=1] Health')
        run(f'/data get entity @e[tag={tag},limit=1] Pos')

    def read(tag):
        run(f'/attribute @e[tag={tag},limit=1] {ATTR} modifier value get minecraft:baby 1000')
        run(f'/attribute @e[tag={tag},limit=1] {ATTR} get 1000')
        run(f'/attribute @e[tag={tag},limit=1] {ATTR} base get 1000')
        run(f'/execute if data entity @e[tag={tag},limit=1] attributes[{{id:"{ATTR}"}}].modifiers[{{id:"minecraft:baby"}}]')
        run(f'/data get entity @e[tag={tag},limit=1] attributes[{{id:"{ATTR}"}}].modifiers')
        if raw:
            run(f'/data get entity @e[tag={tag},limit=1] attributes')
            run(f'/data get entity @e[tag={tag},limit=1]')

    def mod(tag, verb, extra=''):
        run(f'/attribute @e[tag={tag},limit=1] {ATTR} modifier {verb} minecraft:baby{extra}')

    actors = [('dup', 1), ('remove', 1), ('adult', 0), ('perm', 0)]
    for n, (name, baby) in enumerate(actors, 1):
        tag = f'h235_{name}'
        run(f'/summon minecraft:zombie {1.5 + 3 * n} 200 4.5 {{UUID:{uuid(n)},Tags:["{tag}"],{ZOMBIE},IsBaby:{baby}b}}', 1)
        admit(tag)
        read(tag)

    # Duplicate add on a baby: refused, the transient one stays.
    mod('h235_dup', 'add', ' 0.125 add_value')
    read('h235_dup')

    # Removal on a baby, again, an add after it, then setBaby(true) again.
    mod('h235_remove', 'remove')
    read('h235_remove')
    mod('h235_remove', 'remove')
    read('h235_remove')
    mod('h235_remove', 'add', ' 0.125 add_value')
    read('h235_remove')
    run('/data merge entity @e[tag=h235_remove,limit=1] {IsBaby:1b}')
    read('h235_remove')

    # An adult: nothing to remove; a permanent one can be added.
    mod('h235_adult', 'remove')
    mod('h235_adult', 'add', ' 0.125 add_value')
    read('h235_adult')

    # A permanent minecraft:baby, then the zombie becomes a baby and an adult again.
    mod('h235_perm', 'add', ' 0.125 add_value')
    read('h235_perm')
    run('/data merge entity @e[tag=h235_perm,limit=1] {IsBaby:1b}')
    read('h235_perm')
    run('/data merge entity @e[tag=h235_perm,limit=1] {IsBaby:0b}')
    read('h235_perm')

    for name, _ in actors:
        admit(f'h235_{name}')
    run('/execute store result score #t1 h235 run time query gametime', record=False)
    run('/scoreboard players operation #t1 h235 -= #t0 h235', record=False)
    run('/scoreboard players get #t1 h235')
    return ops
