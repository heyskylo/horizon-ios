"""Saved custom-effect icon defaults through the actual chest item command route."""

GROUP = 'potion custom effect icons'


def cases(cmd, setup):
    ops = [cmd('/tick freeze'), cmd('/forceload add 160 160 175 175'),
           {'op': 'wait_chunks', 'chunks': [[10, 10]]},
           cmd('/fill 160 199 160 175 199 175 stone', expect_result=256),
           cmd('/execute if loaded 164 200 164', expect_result=1),
           cmd('/setblock 164 200 164 chest', expect_result=1),
           cmd('/execute if block 164 200 164 chest', expect_result=1),
           cmd('/data get block 164 200 164 id')]
    ops += setup('/scoreboard objectives add potion_icon_clock dummy',
                 '/execute store result score start potion_icon_clock run time query gametime')
    # An omitted icon follows show_particles; explicit icons are independent.
    for extra, icon in [('', 1), (',show_icon:0b', 0), (',show_icon:1b', 1),
                        (',show_particles:0b', 0),
                        (',show_particles:0b,show_icon:0b', 0),
                        (',show_particles:0b,show_icon:1b', 1)]:
        contents = '{potion:"minecraft:water",custom_color:123456,custom_effects:[' \
                   '{id:"minecraft:speed",duration:100,amplifier:2b' + extra + '}]}'
        ops += [cmd('/item replace block 164 200 164 container.0 with potion[potion_contents=' + contents + ']', expect_result=1),
                cmd('/execute if items block 164 200 164 container.0 potion', expect_result=1)]
        path = 'Items[0].components."minecraft:potion_contents"'
        ops += [cmd('/data get block 164 200 164 ' + path),
                cmd('/data get block 164 200 164 ' + path + '.custom_effects[0]'),
                cmd('/data get block 164 200 164 ' + path + '.custom_effects[0].show_icon', expect_result=icon),
                cmd('/execute if data block 164 200 164 ' + path + '.custom_effects[{show_icon:' + str(icon) + 'b}]', expect_result=1),
                cmd('/data get block 164 200 164 ' + path + '.custom_color', expect_result=123456),
                cmd('/data get block 164 200 164 ' + path + '.potion'),
                cmd('/data get block 164 200 164 ' + path + '.custom_effects[0].duration', expect_result=100),
                cmd('/data get block 164 200 164 ' + path + '.custom_effects[0].amplifier', expect_result=2)]
    # No custom-effect list is invented for an ordinary/base-only potion.
    for item in ['potion', 'potion[potion_contents={potion:"minecraft:water",custom_color:123456}]']:
        adopt = ('/data modify block 164 200 164 Items set value [{Slot:0b,id:"minecraft:potion",count:1}]'
                 if item == 'potion' else '/item replace block 164 200 164 container.0 with ' + item)
        ops += [cmd(adopt, expect_result=1),
                cmd('/execute if items block 164 200 164 container.0 potion', expect_result=1),
                cmd('/data get block 164 200 164 Items[0]'),
                cmd('/execute unless data block 164 200 164 Items[0].components."minecraft:potion_contents".custom_effects', expect_result=1)]
    ops += setup('/execute store result score elapsed potion_icon_clock run time query gametime',
                 '/scoreboard players operation elapsed potion_icon_clock -= start potion_icon_clock')
    return ops + [cmd('/scoreboard players get elapsed potion_icon_clock', expect_result=0)]
