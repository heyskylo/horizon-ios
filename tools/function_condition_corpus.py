"""Real function execution, independent counters, and continuation stores (H154)."""

ACTOR = '0000009a-0000-0000-0000-000000000001'


def functions():
    counter = 'scoreboard players add #calls h154 1\n'
    return {
        'h154:counter': counter,
        'h154:say_counter': 'say H154 fallthrough\n' + counter,
        'h154:zero': counter + 'return 0\n',
        'h154:five': counter + 'return 5\n',
        'h154:fail': counter + 'return fail\n',
    }


def conditions():
    try:
        from .command_exec_corpus import cmd
    except ImportError:
        from command_exec_corpus import cmd
    ops = [cmd('/tick freeze'), cmd('/datapack list enabled'),
           cmd('/scoreboard objectives add h154 dummy', expect_result=1),
           cmd('/forceload add 0 0 15 15'), {'op': 'settle'},
           cmd('/execute if loaded 4 200 4', expect_result=1),
           cmd('/setblock 4 199 4 stone'),
           cmd('/execute if block 4 199 4 stone', expect_result=1),
           cmd('/summon pig 4.5 200 4.5 {UUID:[I;154,0,0,1],Tags:["h154"],'
               'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:10f}'),
           cmd(f'/execute as {ACTOR} if entity @s[type=pig,tag=h154,nbt={{Health:10f,NoAI:1b,NoGravity:1b,Pos:[4.5d,200d,4.5d]}}]', expect_result=1),
           cmd('/execute store result score #before h154 run time query gametime', False)]
    for name in functions():
        name = name.split(':')[1]
        # Each admitted function executes its counter exactly once, even with
        # no return; its direct-call feedback remains a separate raw record.
        ops += [cmd('/scoreboard players set #calls h154 0', expect_result=0),
                cmd(f'/execute as {ACTOR} run function h154:{name}'),
                cmd('/scoreboard players get #calls h154', expect_result=1)]
        for sign in ('if', 'unless'):
            keep = (name == 'five') == (sign == 'if')
            ops += [cmd('/scoreboard players set #calls h154 0', expect_result=0),
                    cmd('/scoreboard players set #out h154 99', expect_result=99),
                    cmd('/scoreboard players set #result h154 31', expect_result=31),
                    cmd('/scoreboard players set #success h154 17', expect_result=17),
                    cmd(f'/execute as {ACTOR} store result score #result h154 '
                        f'store success score #success h154 {sign} function h154:{name} '
                        'run scoreboard players set #out h154 7'),
                    cmd('/scoreboard players get #calls h154', expect_result=1),
                    cmd('/scoreboard players get #out h154', expect_result=7 if keep else 99),
                    cmd('/scoreboard players get #result h154'),
                    cmd('/scoreboard players get #success h154')]
    ops += [cmd(f'/execute as {ACTOR} if entity @s[type=pig,tag=h154,nbt={{Health:10f,NoAI:1b,NoGravity:1b,Pos:[4.5d,200d,4.5d]}}]', expect_result=1),
            cmd('/execute store result score #after h154 run time query gametime', False),
            cmd('/execute if score #before h154 = #after h154', expect_result=1),
            cmd(f'/kill {ACTOR}'), cmd('/scoreboard objectives remove h154'),
            cmd('/tick unfreeze')]
    return ops
