"""Actual owner map creation and live frame Item rewrites; no packet/reopen claim."""

FRAMES = [
    ('item_frame', 1, [10, 101, 5], 5),
    ('glow_item_frame', 2, [-20, 101, -15], 1),
    ('item_frame', 3, [12, 101, 5], 5),
]


def frame_maps(cmd, setup):
    ops = []

    def check(command, result=1):
        ops.append(cmd(command, expect_result=result))

    def clock():
        ops.extend(setup('/execute store result score #now frame_maps run time query gametime'))
        check('/scoreboard players operation #now frame_maps -= #clock frame_maps', 0)

    def step():
        ops.extend(setup('/execute store result score #before frame_maps run time query gametime'))
        check('/tick step 20')
        ops.append({'op': 'tick', 'n': 20})
        ops.extend(setup('/execute store result score #clock frame_maps run time query gametime',
                         '/scoreboard players operation #elapsed frame_maps = #clock frame_maps'))
        check('/scoreboard players operation #elapsed frame_maps -= #before frame_maps', 20)

    def uuid(number):
        return f'0000013d-0000-0000-0000-{number:012x}'

    def actor(frame, item='filled_map', rotation=3, invisible=False):
        kind, number, pos, facing = frame
        fields = (f'UUID:[I;317,0,0,{number}],block_pos:[I;{pos[0]},{pos[1]},{pos[2]}],'
                  f'Facing:{facing}b,ItemRotation:{rotation}b')
        check(f'/execute as {uuid(number)} if entity @s[type={kind},tag=frame_map_{number},nbt={{{fields}}}]')
        ops.append(cmd(f'/data get entity {uuid(number)} Pos'))
        if item is None:
            check(f'/execute unless data entity {uuid(number)} Item')
        else:
            check(f'/execute if data entity {uuid(number)} '
                  f'{{Item:{{id:"minecraft:{item}",count:1,components:{{"minecraft:map_id":0}}}}}}')
        if invisible:
            check(f'/execute if data entity {uuid(number)} {{Invisible:1b}}')

    def read(frame, label, present, total):
        kind, number, pos, facing = frame
        ops.append({'op': 'frame_map_state', 'id': 0, 'uuid': uuid(number),
                    'kind': f'minecraft:{kind}', 'label': f'{label}_{number}',
                    'expect_observation': {
                        'id': 0, 'uuid': [317, 0, 0, number], 'kind': f'minecraft:{kind}',
                        'pos': pos, 'center': [0, 0], 'scale': 0, 'dimension': 'minecraft:overworld',
                        'saved': present, 'saved_pos': pos if present else None,
                        'saved_rotation': (270 if facing == 5 else -90) if present else None,
                        'decoration': present, 'frame_records': total, 'frame_decorations': total}})

    def write(command, accepted=True):
        ops.extend(setup('/scoreboard players set #r frame_maps 31',
                         '/scoreboard players set #s frame_maps 17'))
        check('/scoreboard players get #r frame_maps', 31)
        check('/scoreboard players get #s frame_maps', 17)
        operation = cmd('/execute store result score #r frame_maps store success score #s frame_maps '
                        f'run {command}')
        operation['expect_callback'] = [accepted, int(accepted)]
        ops.append(operation)
        check('/scoreboard players get #r frame_maps', int(accepted))
        check('/scoreboard players get #s frame_maps', int(accepted))
        clock()

    check('/tick freeze')
    check('/execute if dimension minecraft:overworld')
    ops.extend(setup('/forceload remove -32 -32 31 31'))
    check('/forceload add -32 -32 31 31', 16)
    check('/tp @s 4.5 100 4.5 0 0')
    ops.append({'op': 'settle', 'required_chunks': [[x, z] for x in range(-2, 2) for z in range(-2, 2)]})
    for x in range(-2, 2):
        for z in range(-2, 2):
            check(f'/execute if loaded {16*x} 100 {16*z}')
    for block in ['barrier', 'stone']:
        check(f'/fill -32 99 -32 31 99 31 {block}', 4096)
    for block in ['barrier', 'air']:
        check(f'/fill -32 100 -32 31 104 31 {block}', 20480)
    ops.extend(setup('/gamemode survival', '/clear @s'))
    check('/execute if data entity @s {playerGameType:0,Health:20f,Pos:[4.5d,100d,4.5d]}')
    check('/item replace entity @s weapon.mainhand with map')
    ops.append({'op': 'map_state', 'id': 0, 'expect_map': 'missing'})
    ops.append({'op': 'use_item'})
    check('/execute if items entity @s weapon.mainhand filled_map[map_id=0]')
    ops.append({'op': 'map_state', 'id': 0, 'expect_map': 'scale=0,locked=false,center=0,0'})
    check('/scoreboard objectives add frame_maps dummy')
    for x, y, z in [(9, 101, 5), (-20, 100, -15), (11, 101, 5)]:
        check(f'/setblock {x} {y} {z} stone')
        check(f'/execute if block {x} {y} {z} stone')
    for frame in FRAMES:
        kind, number, pos, facing = frame
        check(f'/summon {kind} {pos[0]} {pos[1]} {pos[2]} '
              f'{{UUID:[I;317,0,0,{number}],Tags:["frame_map_{number}"],'
              f'block_pos:[I;{pos[0]},{pos[1]},{pos[2]}],Facing:{facing}b,ItemRotation:3b,'
              'Item:{id:"minecraft:filled_map",count:1,components:{"minecraft:map_id":0}}}')
        actor(frame)
    step()
    for frame in FRAMES:
        actor(frame)
        read(frame, 'admitted', True, 3)
    a, b, control = FRAMES
    write(f'data merge entity {uuid(1)} {{Item:{{id:"minecraft:filled_map",count:1,components:{{"minecraft:map_id":0}}}}}}', False)
    read(a, 'unchanged', True, 3)
    write(f'data merge entity {uuid(1)} {{ItemRotation:4b}}')
    actor(a, rotation=4)
    read(a, 'rotated', True, 3)
    write(f'data merge entity {uuid(1)} {{Invisible:1b}}')
    actor(a, rotation=4, invisible=True)
    read(a, 'unrelated', True, 3)
    write(f'data merge entity {uuid(1)} {{Item:{{components:{{"minecraft:custom_data":{{audit:1}}}}}}}}')
    actor(a, rotation=4, invisible=True)
    read(a, 'component_changed', False, 2)
    read(control, 'component_control', True, 2)
    step()
    read(a, 'component_reattached', True, 3)
    write(f'data merge entity {uuid(2)} {{Item:{{id:"minecraft:stone",count:1}}}}')
    actor(b, item='stone')
    read(b, 'stone_retains_map_id', False, 2)
    read(control, 'stone_control', True, 2)
    step()
    actor(b, item='stone')
    read(b, 'stone_not_reattached', False, 2)
    for label, change in [('remove', f'data remove entity {uuid(2)} Item'),
                          ('empty', f'data modify entity {uuid(2)} Item set value {{}}')]:
        write(f'data modify entity {uuid(2)} Item set value '
              '{id:"minecraft:filled_map",count:1,components:{"minecraft:map_id":0}}')
        step()
        actor(b)
        read(b, f'{label}_admission', True, 3)
        write(change)
        actor(b, item=None)
        read(b, label, False, 2)
        read(control, f'{label}_control', True, 2)
    write(f'data modify entity {uuid(2)} Item set value '
          '{id:"minecraft:filled_map",count:1,components:{"minecraft:map_id":0}}')
    step()
    actor(b)
    read(b, 'kill_admission', True, 3)
    write(f'kill {uuid(2)}')
    check('/execute unless entity @e[tag=frame_map_2]')
    actor(control)
    read(control, 'kill_control', True, 2)
    clock()
    return ops + setup('/kill @e[tag=frame_map_1]', '/kill @e[tag=frame_map_3]',
                       '/scoreboard objectives remove frame_maps', '/clear @s', '/gamemode creative',
                       '/forceload remove -32 -32 31 31', '/tick unfreeze')


def treasure_map_copies(cmd, setup):
    """A same-id copy without components must not remove a treasure map's X."""
    ops = []
    def run(command, result=1):
        ops.append(cmd(command, expect_result=result))
    def step():
        ops.append({'op': 'map_carried'})
    def state():
        ops.append({'op': 'map_state', 'id': 0, 'red_x': True,
                    'expect_map': 'scale=0,locked=false,center=0,0,red_x=1'})
    run('/tick freeze')
    ops.extend(setup('/gamemode survival', '/clear @s', '/tp @s 4.5 100 4.5 0 0',
                     '/forceload add 0 0 15 15'))
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    run('/item replace entity @s weapon.mainhand with map')
    ops.append({'op': 'use_item'})
    run('/execute if items entity @s weapon.mainhand filled_map[map_id=0]')
    target = 'filled_map[map_id=0,map_decorations={"+":{type:"minecraft:red_x",x:20.0d,z:30.0d,rotation:180.0f}}]'
    run('/item replace entity @s weapon.mainhand with ' + target)
    step()
    state()
    run('/item replace entity @s hotbar.1 with filled_map[map_id=0]')
    step()
    state()
    # Retain the target even when only the undecorated copy remains.
    run('/item replace entity @s weapon.mainhand with air')
    step()
    state()
    return ops
