"""Bounded live Enderman placement: stable outcomes, separate random-state trace."""
from command_exec_corpus import cmd


def enderman_placement(selector_position_diagnostic=False, property_control=False):
    ops = []
    def run(c, expected=None, record=True, trace=False, label=None):
        op = cmd(c, record, expected)
        if trace:
            op['trace'] = True
        if label:
            op['label'] = label
        ops.append(op)
    def silent(c):
        run(c, record=False)
    def step(n):
        silent(f'/tick step {n}')
        ops.append({'op': 'tick', 'n': n})
    run('/tick freeze', 1)
    for c in ['/gamemode creative', '/gamerule spawn_mobs false',
              '/gamerule advance_time false', '/gamerule advance_weather false',
              '/gamerule random_tick_speed 0', '/gamerule mob_griefing true',
              '/time set midnight', '/weather clear']:
        run(c)
    run('/forceload add 0 0 47 47', 9)
    ops.append({'op': 'settle', 'required_chunks': [[x,z] for x in range(3) for z in range(3)]})
    run('/setblock 32 199 32 stone')
    run('/tp @s 32.5 200 32.5 0 0')
    run('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[32.5d,200d,32.5d]}]', 1)
    run('/data get entity @s UUID', record=False, trace=True)
    run('/scoreboard objectives add h325 dummy', 1)
    for x in range(3):
        for z in range(3):
            run(f'/execute if loaded {16*x} 200 {16*z}', 1)
    for rule in ['spawn_mobs', 'advance_time', 'random_tick_speed', 'mob_griefing']:
        run(f'/gamerule {rule}')
    cases = [('glass','stone',48), ('torch','stone',48), ('poppy','stone',48), ('glass','farmland',12)]
    if property_control:
        cases = [('note_block','stone',48)]
    for index, (block, floor, batches) in enumerate(cases, 1):
        prefix = f'{block}-{floor}'
        uuid = f'00000145-0000-0000-0000-{index:012x}'
        silent('/kill @e[type=!player]')
        step(25)
        silent('/kill @e[type=!player]')
        run('/fill 0 199 0 6 204 6 glass')
        run('/fill 1 200 1 5 203 5 air', 100)
        run(f'/fill 1 199 1 5 199 5 {floor}', 25)
        # Complete pen geometry, including the support and every enclosed cell.
        for x in range(7):
            for y in range(199,205):
                for z in range(7):
                    interior = 1 <= x <= 5 and 1 <= z <= 5
                    expected = floor if interior and y == 199 else ('air' if interior and 200 <= y <= 203 else 'glass')
                    run(f'/execute if block {x} {y} {z} {expected}', 1)
        run(f'/summon enderman 3.5 200 3.5 {{UUID:[I;325,0,0,{index}],Tags:["h325_{index}"],PersistenceRequired:1b,carriedBlockState:"minecraft:{block}"}}', 1)
        identity = f'@e[type=enderman,tag=h325_{index},nbt={{UUID:[I;325,0,0,{index}],Health:40f,PersistenceRequired:1b}}]'
        run(f'/execute if entity {identity}', 1)
        run(f'/execute if data entity {uuid} {{Pos:[3.5d,200d,3.5d],carriedBlockState:"minecraft:{block}"}}', 1)
        run(f'/execute unless data entity {uuid} {{NoAI:1b}}', 1)
        run(f'/data get entity {uuid} Health', 40)
        run(f'/data get entity {uuid} carriedBlockState')
        silent('/execute store result score start h325 run time query gametime')
        silent('/scoreboard players set expected h325 0')
        for batch in range(batches):
            if floor == 'farmland':
                silent('/scoreboard players add expected h325 1000')
                silent('/tick step 1000')
            else:
                silent(f'/execute if data entity {uuid} carriedBlockState run scoreboard players add expected h325 1000')
                silent(f'/execute if data entity {uuid} carriedBlockState run tick step 1000')
            ops.append({'op':'tick','n':1000})
            for field in ['Pos','Motion','carriedBlockState']:
                run(f'/data get entity {uuid} {field}', record=False, trace=True, label=f'{prefix}-{batch}-{field}')
        silent('/execute store result score elapsed h325 run time query gametime')
        silent('/scoreboard players operation elapsed h325 -= start h325')
        run('/execute if score elapsed h325 = expected h325', 1)
        run('/execute if score elapsed h325 matches 12000' if floor == 'farmland' else '/execute if score elapsed h325 matches 1000..48000', 1)
        run('/scoreboard players get elapsed h325', record=False, trace=True, label=prefix+'-elapsed')
        run(f'/execute if entity {identity}', 1)
        run(f'/data get entity {uuid} Health', 40)
        run(f'/execute unless data entity {uuid} {{NoAI:1b}}', 1)
        # Farm support is 15/16 high: include the verified floor layer, not only y>=200.
        y, dy = (200, 3) if selector_position_diagnostic else (199, 4)
        run(f'/execute if entity @e[type=enderman,tag=h325_{index},x=1,y={y},z=1,dx=4,dy={dy},dz=4]', 1)
        run(f'/execute {"if" if floor == "farmland" else "unless"} data entity {uuid} carriedBlockState', 1)
        if floor == 'farmland':
            run(f'/data get entity {uuid} carriedBlockState')
        for field in ['Pos','Motion']:
            run(f'/data get entity {uuid} {field}', record=False, trace=True, label=prefix+'-final-'+field)
        silent('/scoreboard players set matching h325 0')
        silent('/scoreboard players set occupied h325 0')
        for x in range(1,6):
            for y in range(200,204):
                for z in range(1,6):
                    silent(f'/execute if block {x} {y} {z} {block} run scoreboard players add matching h325 1')
                    silent(f'/execute unless block {x} {y} {z} air run scoreboard players add occupied h325 1')
                    run(f'/data get block {x} {y} {z}', record=False, trace=True, label=prefix+'-block-data')
                    run(f'/execute if block {x} {y} {z} {block}', record=False, trace=True, label=prefix+'-block')
        wanted = 1 if floor == 'stone' and block != 'poppy' else 0
        run('/scoreboard players get matching h325', wanted, label=prefix+'-matching')
        run('/scoreboard players get occupied h325', wanted, label=prefix+'-occupied')
        if property_control:
            silent('/scoreboard players set instrument h325 0')
            for x in range(1,6):
                for y in range(200,204):
                    for z in range(1,6):
                        silent(f'/execute if block {x} {y} {z} note_block[instrument=basedrum] run scoreboard players add instrument h325 1')
                        run(f'/execute if block {x} {y} {z} note_block[instrument=basedrum]', record=False, trace=True)
            run('/scoreboard players get instrument h325', 1)
        run('/time query time', 18000)
        run('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[32.5d,200d,32.5d]}]', 1)
    return ops
