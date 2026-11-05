"""Armor-stand side-slide through actual Game world ticks."""
from command_exec_corpus import cmd


def armor_stand_honey():
    ops = []

    def run(command, expected=None, label=None, record=True):
        op = cmd(command, expect_result=expected, record=record)
        if label:
            op['label'] = label
        ops.append(op)

    def step(n):
        run(f'/tick step {n}')
        ops.append({'op': 'tick', 'n': n})

    run('/tick freeze', 1)
    for c in ['/gamerule spawn_mobs false', '/gamerule advance_time false',
              '/gamerule advance_weather false', '/gamemode creative',
              '/tp @s 0.5 100 0.5 0 0']:
        run(c)
    run('/forceload add -16 -16 15 15', 4)
    ops.append({'op': 'settle', 'required_chunks': [[-1, -1], [-1, 0], [0, -1], [0, 0]]})
    for x in [-16, 0]:
        for z in [-16, 0]:
            run(f'/execute if loaded {x} 100 {z}', 1)
    run('/fill -8 99 -8 15 99 15 stone', 576)
    run('/fill -8 100 -8 15 130 15 air')
    run('/fill 6 100 4 6 120 4 honey_block', 21)
    run('/fill 6 100 10 6 120 10 stone', 21)
    for z, block in [(4, 'honey_block'), (10, 'stone')]:
        for y in [100, 110, 120]:
            run(f'/execute if block 6 {y} {z} {block}', 1)
        run(f'/execute if block 5 118 {z} air', 1)
        run(f'/execute if block 5 99 {z} stone', 1)
    run('/scoreboard objectives add honey_clock dummy', 1)
    step(20)
    run('/execute store result score start honey_clock run time query gametime', record=False)
    actors = []
    for i, (name, z) in enumerate([('honey', 4), ('stone', 10)], 1):
        uuid = f'0000013b-0000-0000-0000-{i:012x}'
        actors.append((name, z, i, uuid))
        run(f'/summon armor_stand 5.78 118 {z}.5 {{UUID:[I;315,0,0,{i}],Tags:["h315_{name}"]}}', 1)
        run(f'/execute if entity @e[type=armor_stand,tag=h315_{name},nbt={{UUID:[I;315,0,0,{i}],Health:20f,Pos:[5.78d,118d,{z}.5d]}}]', 1)
        for field in ['NoGravity', 'Marker']:
            run(f'/execute unless data entity {uuid} {field}', 1)
        run(f'/data get entity {uuid} Pos', label=name+'-initial-pos')
        run(f'/data get entity {uuid} Motion', label=name+'-initial-motion')
        run(f'/data get entity {uuid} Health', 20)
    for delta, elapsed in [(5, 5), (10, 15), (20, 35)]:
        step(delta)
        run('/execute store result score elapsed honey_clock run time query gametime', record=False)
        run('/scoreboard players operation elapsed honey_clock -= start honey_clock', record=False)
        run('/scoreboard players get elapsed honey_clock', elapsed)
        run('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[0.5d,100d,0.5d]}]', 1)
        for name, z, i, uuid in actors:
            label = f'{name}-{elapsed}'
            run(f'/execute if entity @e[type=armor_stand,tag=h315_{name},nbt={{UUID:[I;315,0,0,{i}],Health:20f}}]', 1)
            run(f'/data get entity {uuid} Pos', label=label+'-pos')
            run(f'/data get entity {uuid} Motion', label=label+'-motion')
            for field in ['Pos[1]', 'Motion[1]', 'OnGround', 'fall_distance']:
                run(f'/data get entity {uuid} {field}' + (' 1000' if '[' in field else ''), label=label+'-'+field)
            for field in ['NoGravity', 'Marker']:
                run(f'/execute unless data entity {uuid} {field}', 1)
            run(f'/execute if block 6 110 {z} ' + ('honey_block' if name == 'honey' else 'stone'), 1)
    # Separate no-physics controls, after the original two-column scene.
    for _, _, _, uuid in actors:
        run(f'/kill {uuid}', 1)
    for i, flag in [(3, 'NoGravity'), (4, 'Marker')]:
        run(f'/summon armor_stand 5.78 118 4.5 {{UUID:[I;315,0,0,{i}],Tags:["h315_{flag}"],{flag}:1b}}', 1)
        run(f'/execute if entity @e[type=armor_stand,tag=h315_{flag},nbt={{UUID:[I;315,0,0,{i}],Health:20f,Pos:[5.78d,118d,4.5d],{flag}:1b}}]', 1)
        run(f'/data get entity 0000013b-0000-0000-0000-{i:012x} Motion', label=flag+'-initial-motion')
    run('/execute store result score control_start honey_clock run time query gametime', record=False)
    step(5)
    run('/execute store result score control_elapsed honey_clock run time query gametime', record=False)
    run('/scoreboard players operation control_elapsed honey_clock -= control_start honey_clock', record=False)
    run('/scoreboard players get control_elapsed honey_clock', 5)
    for i, flag in [(3, 'NoGravity'), (4, 'Marker')]:
        uuid = f'0000013b-0000-0000-0000-{i:012x}'
        run(f'/execute if entity @e[type=armor_stand,tag=h315_{flag},nbt={{UUID:[I;315,0,0,{i}],Health:20f,Pos:[5.78d,118d,4.5d],{flag}:1b}}]', 1)
        run(f'/data get entity {uuid} Pos', label=flag+'-after-pos')
        run(f'/data get entity {uuid} Motion', label=flag+'-after-motion')
    return ops
