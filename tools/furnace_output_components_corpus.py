"""A real world-only smelting recipe, four cold furnaces and60 actual ticks."""
CASES = [('empty', None, False), ('plain', 1, False),
         ('capacity', 63, True), ('merge', 60, True)]
RECIPE_ID = 'h327:hot_iron'


def recipes():
    return {RECIPE_ID: {'type': 'minecraft:smelting', 'ingredient': 'minecraft:dirt',
                       'cookingtime': 10, 'result': {'id': 'minecraft:iron_ingot', 'count': 2,
                       'components': {'minecraft:custom_name': 'Hot'}}}}


def output_components(full=False):
    ops = []
    def cmd(text, label, result=None, admission=False, diagnostic=False):
        row = {'op':'cmd', 'cmd':text, 'label':label, 'admission':admission}
        if result is not None: row['expect_result'] = result
        if diagnostic and not full: row.update(record=False, trace=True)
        ops.append(row)
    cmd('/tick freeze', 'freeze', admission=True)
    for rule, value in [('spawn_mobs','false'),('advance_time','false'),('random_tick_speed','0')]:
        cmd(f'/gamerule {rule} {value}', 'rule-'+rule, admission=True)
    cmd('/forceload add 0 0', 'force', 1, True)
    ops.append({'op':'settle', 'required_chunks':[[0,0]]})
    cmd('/fill 0 99 0 15 99 15 stone', 'floor', 256, True)
    cmd('/fill 0 100 0 15 105 15 air', 'air', admission=True)
    cmd(f'/recipe give @s {RECIPE_ID}', 'recipe-id', 1, True)
    cmd('/scoreboard objectives add h327 dummy', 'objective', admission=True)
    cmd('/execute store result score start h327 run time query gametime', 'clock-start', diagnostic=True)
    for i, (name, count, named) in enumerate(CASES):
        x = 2+i*2
        for y, block in [(99,'stone'),(100,'air'),(101,'air')]:
            cmd(f'/execute if block {x} {y} 2 {block}', f'{name}-setup-{y}', 1, True)
        entries = ['{Slot:0b,id:"minecraft:dirt",count:3}', '{Slot:1b,id:"minecraft:coal",count:1}']
        if count is not None:
            component = ',components:{"minecraft:custom_name":"Hot"}' if named else ''
            entries.append(f'{{Slot:2b,id:"minecraft:iron_ingot",count:{count}{component}}}')
        cmd(f'/setblock {x} 100 2 minecraft:furnace{{Items:['+','.join(entries)+']}', name+'-place', 1, True)
    for elapsed in [0,60]:
        if elapsed:
            cmd('/tick step 60', 'step60', 1, True)
            ops.append({'op':'tick','n':60})
        cmd('/execute store result score now h327 run time query gametime', f'{elapsed}-clock-raw', diagnostic=True)
        cmd('/scoreboard players operation now h327 -= start h327', f'{elapsed}-clock-delta-op', elapsed, True)
        cmd('/scoreboard players get now h327', f'{elapsed}-clock-delta', elapsed, True)
        for i, (name, original, named) in enumerate(CASES):
            x=2+i*2;at=f'{x} 100 2';key=f'{name}-{elapsed}'
            control = elapsed==0 or name!='plain'
            output = original if elapsed==0 else {'empty':6,'plain':1,'capacity':63,'merge':64}[name]
            ingredient = 3 if elapsed==0 or name in {'plain','capacity'} else 0 if name=='empty' else 1
            fuel = elapsed==0 or name in {'plain','capacity'}
            lit = elapsed>0 and name in {'empty','merge'}
            cmd(f'/execute if loaded {at}', key+'-loaded', 1, True)
            cmd(f'/execute if block {x} 99 2 stone', key+'-floor', 1, True)
            cmd(f'/execute if data block {at} {{id:"minecraft:furnace",x:{x},y:100,z:2}}', key+'-identity', 1, True)
            cmd(f'/execute if block {at} furnace[lit={str(lit).lower()}]', key+'-lit', 1, control)
            cmd(f'/data get block {at} Items', key+'-slots', admission=control)
            if ingredient:
                cmd(f'/execute if data block {at} {{Items:[{{Slot:0b,id:"minecraft:dirt",count:{ingredient}}}]}}', key+'-input', 1, control)
            else:
                cmd(f'/execute unless data block {at} Items[{{Slot:0b}}]', key+'-no-input', 1, control)
            sign='if' if fuel else 'unless'
            path='{Items:[{Slot:1b,id:"minecraft:coal",count:1}]}' if fuel else 'Items[{Slot:1b}]'
            cmd(f'/execute {sign} data block {at} {path}', key+'-fuel', 1, control)
            if output is None:
                cmd(f'/execute unless data block {at} Items[{{Slot:2b}}]', key+'-no-output', 1, control)
            else:
                cmd(f'/execute if data block {at} {{Items:[{{Slot:2b,id:"minecraft:iron_ingot",count:{output}}}]}}', key+'-output', 1, control)
                path='Items[{Slot:2b}].components."minecraft:custom_name"'
                hot = named or name=='empty' and elapsed>0
                if hot:
                    cmd(f'/data get block {at} {path}', key+'-name', 3, control)
                else:
                    cmd(f'/execute unless data block {at} {path}', key+'-no-name', 1, control)
            if elapsed and name in {'empty','merge'}:
                amount=3 if name=='empty' else 2
                cmd(f'/data get block {at} RecipesUsed."{RECIPE_ID}"', key+'-actual-recipe', amount, True)
            else:
                cmd(f'/data get block {at} RecipesUsed', key+'-unused-recipe', 0, control)
            cmd(f'/data get block {at}', key+'-full-tag', diagnostic=True)
            for field in ['cooking_time_spent','cooking_total_time','lit_time_remaining','lit_total_time']:
                cmd(f'/data get block {at} {field}', key+'-'+field, diagnostic=True)
    return ops
