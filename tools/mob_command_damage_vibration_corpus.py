"""Actual command damage -> existing sculk listener, before/after a same-tick teleport."""
from command_exec_corpus import cmd, setup


def raw(command, scope, callback=None):
    op = cmd(command)
    op['diagnostic'] = scope
    if callback is not None:
        op['expect_callback'] = callback
        op['canonical_callback'] = callback
    return op


def cases():
    origin, destination = '7 200 4', '39 200 4'
    ops = setup('/gamerule random_tick_speed 0', '/tick freeze',
                '/forceload add 0 0 47 15', '/scoreboard objectives add h113 dummy')
    ops += [{'op': 'settle'}]
    for index, (name, accepted, moved) in enumerate([
            ('stationary', True, False), ('moved', True, True),
            ('rejected_moved', False, True)]):
        if index:
            ops += setup(f'/setblock {origin} air strict', f'/setblock {destination} air strict',
                         '/kill @e[tag=h113_actor]', '/tick step 25')
            ops.append({'op': 'tick', 'n': 25})
        ops += setup('/fill 0 199 0 47 199 15 stone', '/fill 0 200 0 47 205 15 air')
        uuid = f'00000071-0000-0000-0000-{index + 1:012x}'
        fields = (f'UUID:[I;113,0,0,{index + 1}],Tags:["h113_actor","h113_{name}"],'
                  f'Health:10f,NoAI:1b,NoGravity:1b,Invulnerable:{0 if accepted else 1}b,Motion:[0d,0d,0d]')
        ops.append(cmd(f'/summon pig 4.5 200 4.5 {{{fields}}}', expect_result=1))
        ops += setup(f'/setblock {origin} sculk_sensor strict',
                     f'/setblock {destination} sculk_sensor strict', '/tick step 2')
        ops.append({'op': 'tick', 'n': 2})
        for x in [4, 7, 36, 39]:
            ops += [cmd(f'/execute if loaded {x} 200 4', expect_result=1),
                    cmd(f'/execute if block {x} 199 4 stone', expect_result=1)]

        def body(health, x):
            return cmd(f'/execute as {uuid} if entity @s[type=minecraft:pig,tag=h113_{name},'
                       f'nbt={{UUID:[I;113,0,0,{index + 1}],Health:{health}f,NoAI:1b,NoGravity:1b,'
                       f'Pos:[{x}d,200d,4.5d],Motion:[0d,0d,0d]}}]', expect_result=1)

        def clock(elapsed):
            return [*setup('/execute store result score after h113 run time query gametime',
                           '/scoreboard players operation elapsed h113 = after h113',
                           '/scoreboard players operation elapsed h113 -= before h113'),
                    cmd('/scoreboard players get elapsed h113', expect_result=elapsed)]

        def empty(sensor):
            return [cmd(f'/execute unless data block {sensor} listener.event', expect_result=1),
                    cmd(f'/execute unless data block {sensor} listener.selector.event', expect_result=1)]

        def point(sensor, path):
            return [cmd(f'/execute if data block {sensor} listener.{path}'
                        '{game_event:"minecraft:entity_damage",pos:[4.5d,200d,4.5d]}', expect_result=1),
                    cmd(f'/data get block {sensor} listener.{path}.pos', expect_result=3)]

        ops += [body(10, '4.5'), raw(f'/data get entity {uuid}', 'complete victim body')]
        for sensor in [origin, destination]:
            ops += [cmd(f'/execute if block {sensor} sculk_sensor[sculk_sensor_phase=inactive,power=0]', expect_result=1),
                    cmd(f'/execute if data block {sensor} listener', expect_result=1),
                    cmd(f'/data get block {sensor} last_vibration_frequency', expect_result=0), *empty(sensor)]
        ops += setup('/execute store result score before h113 run time query gametime',
                     '/scoreboard players set hit_result h113 31', '/scoreboard players set hit_success h113 17')
        ops.append(raw(f'/execute store result score hit_result h113 store success score hit_success h113 '
                       f'run damage {uuid} 2 minecraft:cactus', 'H191 complete damage feedback',
                       [accepted, 1 if accepted else 0]))
        ops += [cmd('/scoreboard players get hit_result h113', expect_result=1 if accepted else 0),
                cmd('/scoreboard players get hit_success h113', expect_result=1 if accepted else 0),
                body(8 if accepted else 10, '4.5')]
        ops += point(origin, 'selector.event') if accepted else empty(origin)
        ops += empty(destination) + clock(0)
        for sensor in [origin, destination]:
            ops.append(raw(f'/data get block {sensor}', 'H064/H191 complete listener body'))
        ops.append(raw(f'/data get entity {uuid}', 'complete damaged victim body'))
        if moved:
            ops.append(raw(f'/tp {uuid} 36.5 200 4.5', 'H191 complete teleport feedback', [True, 1]))
            ops += [body(8 if accepted else 10, '36.5')]
            ops += point(origin, 'selector.event') if accepted else empty(origin)
            ops += empty(destination) + clock(0)
            ops.append(raw(f'/data get entity {uuid}', 'complete teleported victim body'))
        previous = 0
        for tick in [1, 2, 8]:
            ops += setup(f'/tick step {tick - previous}')
            ops.append({'op': 'tick', 'n': tick - previous})
            previous = tick
            ops += clock(tick)
            ops += [body(8 if accepted else 10, '36.5' if moved else '4.5')]
            if tick <= 2:
                ops += point(origin, 'event') if accepted else empty(origin)
                ops += empty(destination)
            else:
                phase, power = ('active', 10) if accepted else ('inactive', 0)
                ops += [cmd(f'/execute if block {origin} sculk_sensor[sculk_sensor_phase={phase},power={power}]', expect_result=1),
                        cmd(f'/data get block {origin} last_vibration_frequency', expect_result=7 if accepted else 0),
                        cmd(f'/execute if block {destination} sculk_sensor[sculk_sensor_phase=inactive,power=0]', expect_result=1),
                        cmd(f'/data get block {destination} last_vibration_frequency', expect_result=0)]
            if tick in [1, 8]:
                for sensor in [origin, destination]:
                    ops.append(raw(f'/data get block {sensor}', 'H064/H191 complete listener body'))
        ops.append(raw(f'/data get entity {uuid}', 'complete final victim body'))
    return ops
