"""Live moving-piston reads, with guarded extension and retraction timelines.

The manually extended piston's later one-tick pulse is a drop diagnostic:
its final log stays at x=12. The independent normal sticky cycle pulls its
log from x=22 to x=21. Neither is a new simulation-policy contract.
"""


def moving_piston_data(cmd, setup):
    out = []
    elapsed = 0

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    def block(x, state, z=0):
        check(f'/execute if block {x} 100 {z} {state}')

    def step(n=1):
        nonlocal elapsed
        check(f'/tick step {n}')
        out.append({'op': 'tick', 'n': n})
        elapsed += n
        out.extend(setup('/execute store result score #clock piston_data run time query gametime'))
        check('/scoreboard players operation #clock piston_data -= #start piston_data', elapsed)

    def moving(x, progress, extending, source, z=0, carried=None):
        block(x, 'moving_piston[facing=east]', z)
        pos = f'{x} 100 {z}'
        fields = (f'id:"minecraft:piston",x:{x},y:100,z:{z},components:{{}},'
                  f'facing:5b,progress:{progress}f,extending:{extending}b,source:{source}b')
        check(f'/execute if data block {pos} {{{fields}}}')
        check(f'/data get block {pos}')
        for key, value in [('x', x), ('y', 100), ('z', z), ('facing', 5),
                           ('extending', extending), ('source', source), ('components', 0)]:
            check(f'/data get block {pos} {key}', value)
        check(f'/data get block {pos} progress', int(progress))
        out.append(cmd(f'/data get block {pos} blockState'))
        if carried:
            check(f'/execute if data block {pos} {{blockState:{carried}}}')
        out.extend(setup('/scoreboard players set #r piston_data 31',
                         '/scoreboard players set #s piston_data 17'))
        check('/scoreboard players get #r piston_data', 31)
        check('/scoreboard players get #s piston_data', 17)
        check('/execute store result score #r piston_data store success score #s piston_data '
              f'run data get block {pos} progress 1000', int(progress * 1000))
        check('/scoreboard players get #r piston_data', int(progress * 1000))
        check('/scoreboard players get #s piston_data', 1)

    def settled(x, state, z=0):
        block(x, state, z)
        # The positive material witness makes the absent block entity meaningful.
        out.append(cmd(f'/data get block {x} 100 {z}'))

    check('/tick freeze')
    check('/execute if dimension minecraft:overworld')
    out.extend(setup('/forceload remove -16 -16 31 16'))
    check('/forceload add -16 -16 31 16', 9)
    out.append({'op': 'settle'})
    for x in [-16, 0, 16]:
        for z in [-16, 0, 16]:
            check(f'/execute if loaded {x} 100 {z}')
    # Seed the air clear so its acknowledgement is independent of terrain.
    check('/fill -16 100 -16 31 104 16 barrier', 7920)
    check('/fill -16 100 -16 31 104 16 air', 7920)
    check('/fill -16 99 -16 31 99 16 stone', 1584)
    check('/scoreboard objectives add piston_data dummy')
    out.extend(setup('/execute store result score #start piston_data run time query gametime'))
    for x, z in [(0, 0), (1, 0), (10, 0), (11, 0), (12, 0), (20, 8), (21, 8), (22, 8)]:
        check(f'/execute if block {x} 99 {z} stone')
        block(x, 'air', z)
    for x, state, z in [(0, 'piston[facing=east]', 0), (1, 'oak_log[axis=z]', 0),
                         (10, 'sticky_piston[facing=east,extended=true]', 0),
                         (11, 'piston_head[facing=east,type=sticky]', 0),
                         (12, 'oak_log[axis=x]', 0), (20, 'sticky_piston[facing=east]', 8),
                         (21, 'oak_log[axis=y]', 8), (28, 'chest', 12)]:
        check(f'/setblock {x} 100 {z} {state}')
        block(x, state, z)
    check('/data get block 28 100 12')
    check('/execute if data block 28 100 12 {id:"minecraft:chest",Items:[],components:{}}')
    check('/data get block 28 100 12 components', 0)
    for x, z in [(0, 1), (10, 1), (20, 9)]:
        block(x, 'air', z)

    # The initial unpowered manual base retracts on this first warm-up tick.
    step()
    block(0, 'piston[facing=east,extended=false]')
    block(1, 'oak_log[axis=z]')
    moving(10, 0.0, 0, 1)
    moving(11, 0.0, 0, 0, carried='{id:"minecraft:oak_log",properties:{axis:"x"}}')
    block(12, 'air')
    check('/setblock 0 100 1 redstone_block')
    block(0, 'redstone_block', 1)
    step()
    moving(2, 0.0, 1, 0, carried='{id:"minecraft:oak_log",properties:{axis:"z"}}')
    moving(1, 0.0, 1, 1)
    moving(10, 0.5, 0, 1)
    moving(11, 0.5, 0, 0)
    step()
    moving(2, 0.5, 1, 0)
    moving(1, 0.5, 1, 1)
    settled(10, 'sticky_piston[facing=east,extended=false]')
    settled(11, 'oak_log[axis=x]')
    block(12, 'air')
    step()
    settled(2, 'oak_log[axis=z]')
    settled(1, 'piston_head[facing=east,type=normal]')
    block(0, 'piston[facing=east,extended=true]')
    step()

    # Preserve the original five-tick lead-in and one-tick powered pulse.
    check('/setblock 10 100 1 redstone_block')
    block(10, 'redstone_block', 1)
    step()
    block(11, 'moving_piston[facing=east]')
    block(12, 'moving_piston[facing=east]')
    check('/setblock 10 100 1 air')
    block(10, 'air', 1)
    step()
    out.append(cmd('/data get block 11 100 0'))
    moving(10, 0.0, 0, 1)
    step()
    out.append(cmd('/data get block 11 100 0'))
    step(2)
    out.append(cmd('/execute if block 11 100 0 oak_log[axis=x]'))
    block(11, 'air')
    block(12, 'oak_log[axis=x]')
    block(0, 'redstone_block', 1)
    block(2, 'oak_log[axis=z]')

    # A full powered interval gives the independent normal sticky pull control.
    check('/setblock 20 100 9 redstone_block')
    block(20, 'redstone_block', 9)
    step(4)
    block(20, 'sticky_piston[facing=east,extended=true]', 8)
    block(21, 'piston_head[facing=east,type=sticky]', 8)
    settled(22, 'oak_log[axis=y]', 8)
    check('/setblock 20 100 9 air')
    block(20, 'air', 9)
    step()
    moving(20, 0.0, 0, 1, 8)
    moving(21, 0.0, 0, 0, 8, carried='"minecraft:oak_log"')
    step()
    moving(20, 0.5, 0, 1, 8)
    moving(21, 0.5, 0, 0, 8, carried='"minecraft:oak_log"')
    step()
    settled(20, 'sticky_piston[facing=east,extended=false]', 8)
    settled(21, 'oak_log[axis=y]', 8)
    block(22, 'air', 8)
    out.extend(setup('/fill -16 100 -16 31 104 16 air',
                     '/scoreboard objectives remove piston_data',
                     '/forceload remove -16 -16 31 16', '/tick unfreeze'))
    return out


def moving_piston_presentation(cmd, setup):
    """The same tag value from live writers versus ordinary storage/entity data."""
    uid = '00000135-0000-0000-0000-000200000001'
    fake = ('{id:"minecraft:piston",blockState:{id:"minecraft:piston_head",'
            'properties:{facing:"east",short:"false",type:"normal"}},components:{},'
            'x:5,y:100,z:4,facing:5b,progress:0.0f,source:1b,extending:1b}')
    out = []

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    check('/tick freeze')
    out.extend(setup('/forceload remove 0 0'))
    check('/forceload add 0 0')
    out.append({'op': 'settle'})
    check('/execute if loaded 5 100 4')
    check('/fill 0 100 0 15 104 15 barrier', 1280)
    check('/fill 0 100 0 15 104 15 air', 1280)
    # Core may have already placed this floor; clear it before the positive fill.
    out.extend(setup('/fill 0 99 0 15 99 15 air'))
    check('/fill 0 99 0 15 99 15 stone', 256)
    check('/setblock 8 100 8 chest')
    check('/execute if block 8 100 8 chest')
    check('/data get block 8 100 8')
    check('/data get block 8 100 8 components', 0)
    out.extend(setup('/data remove storage command_piston:order id'))
    check('/data merge storage command_piston:order ' + fake)
    check('/data get storage command_piston:order')
    check('/data get storage command_piston:order blockState', 2)
    check('/data get storage command_piston:order blockState.properties', 3)
    check('/summon marker 10.5 100 10.5 {UUID:[I;309,0,2,1],'
          'Tags:["command_piston_order"],data:' + fake + '}')
    check(f'/execute as {uid} if entity @s[type=marker,tag=command_piston_order]')
    check(f'/data get entity {uid} UUID', 4)
    check(f'/execute if entity @e[type=marker,tag=command_piston_order,nbt={{Pos:[10.5d,100d,10.5d]}}]')
    check(f'/data get entity {uid} data', 10)
    check(f'/data get entity {uid} data.blockState.properties', 3)
    check('/scoreboard objectives add piston_order dummy')
    out.extend(setup('/execute store result score #start piston_order run time query gametime'))
    check('/setblock 4 100 4 piston[facing=east]')
    check('/setblock 5 100 4 oak_log[axis=z]')
    check('/tick step 1')
    out.append({'op': 'tick', 'n': 1})
    check('/setblock 4 100 5 redstone_block')
    check('/execute if block 4 100 5 redstone_block')
    check('/tick step 1')
    out.append({'op': 'tick', 'n': 1})
    out.extend(setup('/execute store result score #clock piston_order run time query gametime'))
    check('/scoreboard players operation #clock piston_order -= #start piston_order', 2)
    check('/execute if block 5 100 4 moving_piston[facing=east]')
    check('/execute if data block 5 100 4 {source:1b,progress:0.0f,components:{}}')
    check('/data get block 5 100 4')
    for path, result in [('blockState', 2), ('blockState.properties', 3),
                         ('"blockState"."properties"', 3),
                         ('{source:1b}.blockState.properties', 3),
                         ('blockState{id:"minecraft:piston_head"}.properties', 3),
                         ('{source:1b}', 10), ('blockState.properties.short', 5)]:
        check(f'/data get block 5 100 4 {path}', result)
    # Query equal-valued ordinary data again after the live context was used.
    check('/data get storage command_piston:order')
    check(f'/data get entity {uid} data.blockState.properties', 3)
    check('/data get block 8 100 8')
    out.extend(setup(f'/kill {uid}', '/fill 0 100 0 15 104 15 air',
                     '/scoreboard objectives remove piston_order',
                     '/forceload remove 0 0', '/tick unfreeze'))
    return out
