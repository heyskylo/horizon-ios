"""Independent zero-tick PrimedTnt numeric fuse loads; no live data writes."""
from command_exec_corpus import cmd, setup


def cases():
    inputs = [(None, 80), ('80s', 80), ('2s', 2), ('-1s', -1),
              ('-2b', -2), ('32768', -32768), ('65535L', -1),
              ('9223372036854775807L', -1), ('-1.5f', -2),
              ('-1.5d', -2), ('1.5d', 1), ('32768.75d', -32768),
              ('"wrong_type"', 80)]
    ops = setup('/forceload add 0 0 15 15', '/fill 0 199 0 15 199 15 stone',
                '/fill 0 200 0 15 205 15 air', '/scoreboard objectives add h090 dummy')
    ops += [{'op': 'settle'}]
    ops += [cmd('/execute if loaded 4 200 4', expect_result=1),
            cmd('/execute if block 4 199 4 stone', expect_result=1)]
    ops += setup('/execute store result score before h090 run time query gametime')
    for i, (value, expected) in enumerate(inputs):
        uuid = f'0000005a-0000-0000-0000-{i + 1:012x}'
        fields = (f'UUID:[I;90,0,0,{i + 1}],Tags:["h090_{i}"],'
                  'NoGravity:1b,Motion:[0d,0d,0d],'
                  'block_state:"minecraft:stone",explosion_power:2.5f')
        if value is not None:
            fields += f',fuse:{value}'
        ops.append(cmd(f'/summon tnt 4.5 200 4.5 {{{fields}}}', expect_result=1))
        guard = (f'@s[type=minecraft:tnt,tag=h090_{i},nbt={{UUID:[I;90,0,0,{i + 1}],'
                 'Pos:[4.5d,200d,4.5d],Motion:[0d,0d,0d],NoGravity:1b}]')
        ops += [cmd(f'/execute as {uuid} if entity {guard}', expect_result=1),
                cmd(f'/data get entity {uuid} Pos'),
                cmd(f'/data get entity {uuid} Motion'),
                cmd(f'/execute if data entity {uuid} fuse', expect_result=1),
                cmd(f'/data get entity {uuid} fuse', expect_result=expected),
                cmd(f'/execute as {uuid} if entity @s[nbt={{fuse:{expected}s}}]', expect_result=1),
                cmd(f'/data get entity {uuid} block_state'),
                cmd(f'/data get entity {uuid} explosion_power')]
        ops += setup('/scoreboard players set result h090 31',
                     '/scoreboard players set success h090 17')
        ops += [cmd(f'/execute store result score result h090 store success score success h090 run data get entity {uuid} fuse', expect_result=expected),
                cmd('/scoreboard players get result h090', expect_result=expected),
                cmd('/scoreboard players get success h090', expect_result=1)]
        raw = cmd(f'/data get entity {uuid}')
        raw['diagnostic'] = 'complete TNT body'
        ops.append(raw)
    ops += setup('/execute store result score after h090 run time query gametime')
    ops.append(cmd('/execute if score before h090 = after h090', expect_result=1))
    return ops
