"""Real connected guest chest close -> existing sculk listener.

Whole listener compounds remain executable H064/H191 diagnostics. The strict
contract covers exact partial event/position reads, callbacks, state and clocks.
"""
try:
    from .command_exec_corpus import cmd
except ImportError:
    from command_exec_corpus import cmd


def cases():
    out = []
    def add(command, result=None, admission=False, diagnostic=None):
        op = cmd(command) if result is None else cmd(command, expect_result=result)
        if admission: op['h112_admission'] = True
        if command.startswith('/execute store result score #now h112 run time query gametime') or command.startswith('/execute store result score #start h112 run time query gametime'):
            diagnostic = 'actual absolute scene clock; relative clock contract stays strict'
        if diagnostic: op['diagnostic'] = diagnostic
        out.append(op)
    def clock(elapsed):
        add('/execute store result score #now h112 run time query gametime')
        add('/scoreboard players operation #now h112 -= #start h112')
        add('/scoreboard players get #now h112', elapsed, True)
    def step(n):
        add(f'/tick step {n}', 1, True)
        out.append({'op': 'tick', 'n': n})
    def sensor():
        add('/setblock 9 100 4 sculk_sensor strict', 1, True)
        step(2)
        add('/execute if block 9 100 4 sculk_sensor[sculk_sensor_phase=inactive,power=0]', 1, True)
        add('/execute if data block 9 100 4 listener', 1, True)
        add('/execute unless data block 9 100 4 listener.event', 1, True)
        add('/execute unless data block 9 100 4 listener.selector.event', 1, True)
    def body():
        add('/execute as @s if entity @s[type=minecraft:player,nbt={Health:20f,Pos:[4.5d,100d,4.5d],playerGameType:0}]', 1, True)
        add('/data get entity @s UUID', 4, True)
        add('/execute if loaded 6 100 4', 1, True)
        add('/execute if block 6 99 4 stone', 1, True)
        add('/execute if block 6 100 4 chest', 1, True)
    def point(event, path='selector.event', admission=False):
        add('/execute if data block 9 100 4 listener.' + path +
            '{game_event:"minecraft:' + event + '",pos:[6.5d,100.5d,4.5d]}', 1, admission)
        add('/data get block 9 100 4 listener.' + path + '.pos', 3, admission)

    add('/tick freeze', admission=True)
    add('/gamerule minecraft:spawn_mobs false', admission=True)
    add('/gamerule minecraft:random_tick_speed 0', admission=True)
    add('/gamemode survival', admission=True)
    add('/scoreboard objectives add h112 dummy', 1, True)
    add('/forceload add 0 0 15 15', 1, True)
    out.append({'op': 'settle'})
    add('/execute if loaded 6 100 4', 1, True)
    add('/fill 0 99 0 15 105 15 barrier', 1792, True)
    add('/fill 0 100 0 15 105 15 air', 1536, True)
    add('/fill 0 99 0 15 99 15 stone', 256, True)
    add('/tp @s 4.5 100 4.5', 1, True)
    add('/clear @s', admission=True)
    add('/setblock 6 100 4 chest', 1, True)
    body(); sensor()
    add('/execute store result score #start h112 run time query gametime')
    out.append({'op': 'use_block', 'pos': [6, 100, 4], 'face': 1, 'sequence': 1})
    out.append({'op': 'menu_state', 'label': 'ordinary-open',
                'expect_observation': {'own_slots': 27}, 'h112_admission': True})
    point('container_open', admission=True); body(); clock(0)
    add('/data get block 9 100 4', diagnostic='H064/H191 complete first-open listener')
    add('/setblock 9 100 4 air strict', 1, True)
    sensor(); body(); clock(2)
    add('/scoreboard players set #r h112 31', 31, True)
    add('/scoreboard players set #s h112 17', 17, True)
    out.append({'op': 'close_menu'})
    out.append({'op': 'menu_state', 'label': 'ordinary-closed',
                'expect_observation': {'own_slots': 0}})
    add('/execute store result score #r h112 store success score #s h112 run execute if data block 9 100 4 listener.selector.event{game_event:"minecraft:container_close",pos:[6.5d,100.5d,4.5d]}', 1)
    add('/scoreboard players get #r h112', 1)
    add('/scoreboard players get #s h112', 1)
    point('container_close'); body(); clock(2)
    add('/data get block 9 100 4', diagnostic='H064/H191 complete zero-tick close listener')
    step(1); point('container_close', 'event'); clock(3)
    add('/data get block 9 100 4', diagnostic='H064/H191 complete travelling close listener')
    step(4); body(); clock(7)
    add('/data get block 9 100 4 last_vibration_frequency')
    add('/data get block 9 100 4', diagnostic='H064/H191 complete delivered close listener')
    add('/setblock 9 100 4 air strict', 1, True)
    sensor(); clock(9)
    out.append({'op': 'close_menu'})
    out.append({'op': 'menu_state', 'label': 'already-closed', 'expect_observation': {'own_slots': 0}})
    add('/execute unless data block 9 100 4 listener.event', 1)
    add('/execute unless data block 9 100 4 listener.selector.event', 1)
    body(); clock(9)
    add('/forceload remove 0 0 15 15', 1)
    add('/scoreboard objectives remove h112')
    add('/tick unfreeze')
    return out
