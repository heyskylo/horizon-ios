"""When an AI TraderLlama's despawn delay runs out, it is gone after that tick.

TraderLlama.aiStep -> maybeDespawn: an eligible llama counts DespawnDelay
down by one each tick and, at 0 or below, removes its leash and discards
itself in that same tick. A delay of 5 is gone after the fifth tick, and a
delay of 0 after the first. A tame llama and a persistent one are controls.
"""
from command_exec_corpus import cmd

LLAMAS = [(1, 'delay5', 'DespawnDelay:5'), (2, 'delay0', 'DespawnDelay:0'),
          (3, 'tame5', 'DespawnDelay:5,Tame:1b'), (4, 'persistent5', 'DespawnDelay:5,PersistenceRequired:1b')]


def uuid(n):
    return f'00000000-0000-00b6-0000-00000000000{n}'


def trader_llama_discard():
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    run('/gamerule spawn_mobs false')
    run('/forceload add 0 0 15 15', 1)
    run('/execute if loaded 8 200 8', 1)
    run('/fill 0 199 0 15 199 15 minecraft:stone', 256)
    run('/fill 0 200 0 15 202 15 minecraft:air')
    run('/scoreboard objectives add h182 dummy', 1)
    for n, tag, extra in LLAMAS:
        run(f'/summon minecraft:trader_llama {2.5 + 3 * n} 200 8.5 '
            f'{{UUID:[I;0,182,0,{n}],Tags:["h182","{tag}"],NoAI:0b,Silent:1b,{extra}}}', 1)

    def read(label):
        for n, tag, _ in LLAMAS:
            run(f'/execute if entity {uuid(n)}')
            run(f'/execute if entity @e[type=minecraft:trader_llama,tag={tag}]')
            run(f'/data get entity {uuid(n)} UUID')
            run(f'/data get entity {uuid(n)} DespawnDelay')
            run(f'/data get entity {uuid(n)} NoAI')
        run('/execute if entity @e[type=minecraft:trader_llama,tag=h182]')

    def ticks(k, label):
        run(f'/execute store result score #{label}0 h182 run time query gametime', record=False)
        ops.append({'op': 'tick', 'n': k})
        run(f'/execute store result score #{label}1 h182 run time query gametime', record=False)
        run(f'/scoreboard players operation #{label}1 h182 -= #{label}0 h182', record=False)
        run(f'/scoreboard players get #{label}1 h182', k)

    read('t0')
    for k, label in [(1, 't1'), (1, 't2'), (3, 't5'), (1, 't6')]:
        ticks(k, label)
        read(label)
    run('/kill @e[tag=h182]', record=False)
    run('/forceload remove 0 0 15 15', record=False)
    run('/scoreboard objectives remove h182', record=False)
    return ops
