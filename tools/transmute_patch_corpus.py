"""World-recipe result patches, observed through real powered crafters."""

GROUP = 'transmute result patches'


def recipes():
    def transmute(components):
        return {'type': 'minecraft:crafting_transmute', 'input': 'minecraft:wooden_sword',
                'material': 'minecraft:emerald', 'material_count': {'min': 1, 'max': 3},
                'result': {'id': 'minecraft:stone_sword', 'components': components}}
    return {'transmute_patch:remove': transmute({'minecraft:custom_name': 'Up', '!minecraft:damage': {}}),
            'transmute_patch:inherit': transmute({'minecraft:custom_name': 'Kept'}),
            'transmute_patch:zero': transmute({'minecraft:custom_name': 'Zero', 'minecraft:damage': 0})}


def cases(cmd, setup):
    ops = [cmd('/tick freeze'), cmd('/forceload add 0 0 15 15'),
           cmd('/fill 0 199 0 15 199 15 stone')]
    ops += setup('/tp @s 8.5 200 8.5 0 0', '/scoreboard objectives add transmute_clock dummy',
                 '/data modify storage transmute_patch:owner uuid set from entity @s UUID')
    ops += [cmd('/execute if loaded 4 200 4', expect_result=1),
            cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,200d,8.5d],Health:20f}]', expect_result=1)]
    for identifier in recipes():
        ops += [cmd('/recipe give @s ' + identifier, expect_result=3)]
    for label, emeralds, output in [('remove', 2, 'stone_sword'), ('inherit', 2, 'stone_sword'),
                                     ('zero', 2, 'stone_sword'), ('remove', 4, None),
                                     ('ordinary', 0, 'birch_planks')]:
        # Distinct materials avoid recipe-priority dependence.
        material = {'remove': 'emerald', 'inherit': 'diamond', 'zero': 'lapis_lazuli'}.get(label)
        ops += [cmd('/setblock 4 200 5 air'), cmd('/setblock 4 200 4 air'),
                cmd('/setblock 4 200 3 air'), cmd('/setblock 4 200 3 chest[facing=south]'),
                cmd('/setblock 4 200 4 crafter[orientation=north_up]')]
        item = 'birch_log' if label == 'ordinary' else 'wooden_sword[damage=5,lore=["Old"]]'
        ops += [cmd('/item replace block 4 200 4 container.0 with minecraft:' + item)]
        for slot in range(1, emeralds + 1):
            ops += [cmd(f'/item replace block 4 200 4 container.{slot} with minecraft:{material}')]
        ops += [cmd('/execute if block 4 199 4 stone', expect_result=1),
                cmd('/execute if block 4 200 4 crafter[orientation=north_up,triggered=false]', expect_result=1),
                cmd('/execute if block 4 200 3 chest[facing=south]', expect_result=1),
                cmd('/data get block 4 200 4 id'), cmd('/data get block 4 200 4 Items'),
                cmd('/data get block 4 200 4 disabled_slots'), cmd('/data get block 4 200 3 Items')]
        ops += setup('/execute store result score start transmute_clock run time query gametime')
        ops += [cmd('/setblock 4 200 5 redstone_block'), cmd('/tick step 10'), {'op': 'tick', 'n': 10},
                cmd('/data get block 4 200 4 Items'), cmd('/data get block 4 200 3 Items')]
        if output:
            count = 4 if label == 'ordinary' else 1
            ops += [cmd(f'/execute if items block 4 200 3 container.0 minecraft:{output}', expect_result=count),
                    cmd('/execute unless data block 4 200 4 Items[]', expect_result=1)]
        else:
            ops += [cmd('/execute unless data block 4 200 3 Items[]', expect_result=1),
                    cmd('/execute if items block 4 200 4 container.0 wooden_sword', expect_result=1)]
        if output == 'stone_sword':
            name = {'remove': 'Up', 'inherit': 'Kept', 'zero': 'Zero'}[label]
            component = '"minecraft:lore":["Old"],"minecraft:custom_name":"' + name + '"'
            if label == 'remove':
                component += ',"!minecraft:damage":{}'
            elif label == 'inherit':
                component += ',"minecraft:damage":5'
            ops += [cmd('/execute if data block 4 200 3 Items[{Slot:0b,count:1,components:{' + component + '}}]', expect_result=1)]
            if label != 'inherit':
                ops += [cmd('/execute unless data block 4 200 3 Items[].components."minecraft:damage"', expect_result=1)]
            if label != 'remove':
                ops += [cmd('/execute unless data block 4 200 3 Items[].components."!minecraft:damage"', expect_result=1)]
        ops += [cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,200d,8.5d],Health:20f}]', expect_result=1)]
        ops += setup('/execute store result score elapsed transmute_clock run time query gametime',
                     '/scoreboard players operation elapsed transmute_clock -= start transmute_clock')
        ops += [cmd('/scoreboard players get elapsed transmute_clock', expect_result=10)]
    ops += setup('/execute store success score changed transmute_clock run data modify storage transmute_patch:owner uuid set from entity @s UUID')
    ops += [cmd('/scoreboard players get changed transmute_clock', expect_result=0)]
    return ops + setup('/setblock 4 200 5 air', '/scoreboard objectives remove transmute_clock')


# Distinct materials make each result template unambiguous to world recipe lookup.
def fixture_recipes():
    result = recipes()
    result['transmute_patch:inherit']['material'] = 'minecraft:diamond'
    result['transmute_patch:zero']['material'] = 'minecraft:lapis_lazuli'
    return result
