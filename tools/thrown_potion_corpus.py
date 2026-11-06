"""Thrown potion components and grounded arrow contents through runtime paths."""


def thrown_contents(cmd, setup):
    out = setup('/kill @e[type=!player]', '/fill 0 199 0 15 200 15 stone')
    # Real thrown entities collide with the floor beside stationary subjects.
    for index, (kind, contents, scale) in enumerate([
        ('splash_potion', 'custom_effects:[{id:"minecraft:glowing",duration:800}]', '1'),
        ('splash_potion', 'potion:"minecraft:swiftness",custom_effects:[{id:"minecraft:glowing",duration:800}]', '0.125'),
        ('lingering_potion', 'custom_color:123456,custom_effects:[{id:"minecraft:glowing",duration:800}]', '0.5'),
        ('splash_potion', 'custom_effects:[{id:"minecraft:glowing",duration:800,ambient:true,show_particles:false}]', '1'),
    ]):
        out += setup('/kill @e[type=!player]')
        out += [{'op': 'tick', 'n': 25}]
        out += setup('/summon pig 4.5 201 4.5 {Tags:["subject"],NoAI:1b,NoGravity:1b,Silent:1b}',
                     f'/summon {kind} 4.5 202 4.5 {{Item:{{id:"minecraft:{kind}",count:1,components:{{'
                     f'"minecraft:potion_contents":{{{contents}}},"minecraft:potion_duration_scale":{scale}f}}}}}}')
        out += [{'op': 'tick', 'n': 30}]
        out += [cmd('/execute if entity @e[tag=subject,nbt={active_effects:[{id:"minecraft:glowing"}]}]')]
        if index == 3:
            out += [cmd('/execute if entity @e[tag=subject,nbt={active_effects:[{id:"minecraft:glowing",ambient:1b,show_particles:0b}]}]')]
        if index == 1:
            out += setup('/scoreboard objectives add potion_audit dummy',
                         '/execute store result score duration potion_audit run data get entity @e[tag=subject,limit=1] active_effects[{id:"minecraft:glowing"}].duration')
            out += [cmd('/execute if score duration potion_audit matches 50..100')]
            out += [cmd('/execute if entity @e[tag=subject,nbt={active_effects:[{id:"minecraft:speed"}]}]')]
        if kind == 'lingering_potion':
            for field in ('potion_duration_scale', 'potion_contents.custom_color'):
                out += [cmd(f'/data get entity @e[type=area_effect_cloud,limit=1] {field}')]
    # Exercise actual collision/instant effects on both server players and
    # mobs. High amplifiers use Java's masked int shift count, not a clamp.
    out += setup('/gamerule natural_regeneration false')
    for effect in ('instant_health', 'instant_damage'):
        for amplifier in (31, 32, 33, 255):
            out += setup('/gamemode creative', '/kill @e[type=!player]',
                         '/effect clear @s', '/effect give @s instant_health 1 4',
                         '/tp @s 4.5 201 4.5', '/gamemode survival')
            if effect == 'instant_health':
                out += setup('/damage @s 10 minecraft:generic')
            out += [{'op': 'tick', 'n': 25}]
            health = 5 if effect == 'instant_health' else 20
            out += setup(f'/summon pig 4.5 201 4.5 {{Tags:["extreme_subject"],Health:{health}f,NoAI:1b,NoGravity:1b,Silent:1b,attributes:[{{id:"minecraft:max_health",base:40d}}]}}',
                         '/summon splash_potion 4.5 202 4.5 {Item:{id:"minecraft:splash_potion",count:1,components:{'
                         '"minecraft:potion_contents":{custom_effects:[{'
                         f'id:"minecraft:{effect}",amplifier:{amplifier}' + '}]}}}}')
            out += [{'op': 'tick', 'n': 30}, cmd('/data get entity @s Health'),
                    cmd('/data get entity @e[tag=extreme_subject,limit=1] Health')]
    out += setup('/gamemode creative', '/effect give @s instant_health 1 4',
                 '/gamerule natural_regeneration true')
    # Water with an added effect does not satisfy PotionContents.is(tag).
    for custom in (False, True):
        out += setup('/kill @e[type=!player]', '/setblock 4 201 4 campfire[lit=true]')
        extra = ',custom_effects:[{id:"minecraft:glowing",duration:800}]' if custom else ''
        out += setup('/summon splash_potion 4.5 203 4.5 {Item:{id:"minecraft:splash_potion",count:1,'
                     'components:{"minecraft:potion_contents":{potion:"minecraft:water"' + extra + '}}}}')
        out += [{'op': 'tick', 'n': 30}, cmd('/execute if block 4 201 4 campfire[lit=true]')]
    out += setup('/kill @e[type=!player]', '/setblock 8 201 8 stone')
    for index, contents in enumerate(('', 'custom_color:123456',
                                      'custom_effects:[{id:"minecraft:glowing",duration:800}]',
                                      'potion:"minecraft:swiftness"')):
        out += setup('/summon arrow 8.5 201.5 8.5 '
                     f'{{Tags:["arrow_{index}"],inGround:1b,inBlockState:{{Name:"minecraft:stone"}},'
                     'item:{id:"minecraft:tipped_arrow",count:1,components:{'
                     f'"minecraft:potion_contents":{{{contents}}}}}}}}}')
    out += [{'op': 'tick', 'n': 599}]
    for index in range(4):
        out += [cmd(f'/execute if entity @e[tag=arrow_{index},nbt={{item:{{id:"minecraft:tipped_arrow"}}}}]')]
    out += [{'op': 'tick', 'n': 1}]
    for index in range(4):
        item = 'tipped_arrow' if index == 0 else 'arrow'
        out += [cmd(f'/execute if entity @e[tag=arrow_{index},nbt={{item:{{id:"minecraft:{item}"}}}}]')]
    return out + setup('/kill @e[type=!player]', '/scoreboard objectives remove potion_audit')


if __name__ == '__main__':
    import command_exec_corpus as corpus
    import test_command_exec_parity as runner
    runner.corpus_module.groups = lambda: [('setup', corpus.group_setup()),
                                          ('thrown potion contents', thrown_contents(corpus.cmd, corpus.setup))]
    runner.main()
