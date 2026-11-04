"""Command scripts for tools/test_command_exec_parity.py.

Each group is a list of operations: `cmd` runs a command as the player and
records its messages and results (`record: false` runs setup silently),
`tick` advances the game, `settle` loads the terrain and entities around the
player, `portal` sends the player through the nether portal it stands in,
and `falling` counts physical falling blocks by dimension, state and gravity.
Everything happens high above the terrain (y 200 and up, where both worlds
are air) after the world's entities are removed, so the results do not
depend on world generation.
"""


def cmd(command, record=True, expect_result=None, comparison=None):
    op = {'op': 'cmd', 'cmd': command}
    if not record:
        op['record'] = False
    if expect_result is not None:
        op['expect_result'] = expect_result
    if comparison is not None:
        op['comparison'] = comparison
    return op


def setup(*commands):
    return [cmd(c, False) for c in commands]


def weather_predicate_cases():
    return [('any', None, None), ('rain', True, None), ('dry', False, None),
            ('thunder', None, True), ('quiet', None, False),
            ('rain_thunder', True, True), ('rain_quiet', True, False),
            ('dry_thunder', False, True), ('dry_quiet', False, False)]

def inline_predicate_constants():
    always = {'type': 'minecraft:all_of', 'terms': []}
    never = {'type': 'minecraft:inverted', 'term': always}
    empty_any = {'type': 'minecraft:any_of', 'terms': []}
    return [
        ('always', always, True), ('never', never, False),
        ('empty_any', empty_any, False),
        ('nested_all', {'type': 'minecraft:all_of', 'terms': [
            always, {'type': 'minecraft:inverted', 'term': empty_any}]}, True),
        ('nested_any', {'type': 'minecraft:any_of', 'terms': [never, always]}, True),
        ('nested_inverted', {'type': 'minecraft:inverted', 'term': {
            'type': 'minecraft:any_of', 'terms': [always]}}, False),
    ]


def predicate_selector_fixtures(group_names):
    fixtures = {}
    if 'selector missing predicates' in group_names:
        sheep = {'type': 'minecraft:entity_properties', 'entity': 'this',
                 'predicate': {'minecraft:entity_type': 'minecraft:sheep'}}
        fixtures.update({'h124_selector:sheep': sheep, 'minecraft:h124_selector_sheep': sheep})
    if 'named weather predicates' in group_names:
        for name, rain, thunder in weather_predicate_cases():
            condition = {'type': 'minecraft:weather_check'}
            if rain is not None:
                condition['raining'] = rain
            if thunder is not None:
                condition['thundering'] = thunder
            fixtures['command_weather:' + name] = condition
        always = {'type': 'minecraft:all_of', 'terms': []}
        fixtures['command_weather:always'] = always
        fixtures['command_weather:never'] = {'type': 'minecraft:inverted', 'term': always}

    if 'inline execute predicates' in group_names:
        fixtures.update({f'h286_inline:{name}': value
                         for name, value, _ in inline_predicate_constants()})
    return fixtures


BASE = (0, 200, 0)


def group_setup():
    return [
        *setup('/gamerule spawn_mobs false', '/gamerule mob_drops false', '/gamerule entity_drops false',
               '/gamerule advance_time false', '/gamerule advance_weather false', '/gamerule spawn_wandering_traders false',
               '/gamerule spawn_patrols false', '/gamerule spawn_phantoms false', '/gamerule spawn_wardens false',
               '/gamerule show_advancement_messages false', '/gamemode creative', '/tp @s 8.5 230 8.5 0 0'),
        {'op': 'settle'},
        *setup('/kill @e[type=!player]'),
        {'op': 'tick', 'n': 25},
        *setup('/kill @e[type=!player]'),
        {'op': 'tick', 'n': 25},
        *setup('/fill 0 200 0 15 228 15 air', '/time set 1000', '/weather clear', '/difficulty normal', '/clear @s'),
        # Before the gamerules above, while the oracle waits for its entities to load (a number of ticks that
        # depends on the machine), mobs spawn and some die with their drops on: a bat's death rolled
        # minecraft:entities/bat on 1 vanilla run in 24, one more sequence for the ticking group's
        # /random reset * to count. Every run starts the script without them.
        *setup('/random reset *'),
    ]


def allay_pocket_inventory():
    """The one-slot pocket stores full stacks independently of the held filter."""
    stone = '{id:"minecraft:stone",count:2}'
    diamond = '{id:"minecraft:diamond",count:1}'
    named = '{id:"minecraft:stone",count:2,components:{"minecraft:custom_name":"Pocket"}}'
    cases = [
        ('bare', [stone], None, stone),
        ('different', [stone], diamond, stone),
        ('filter_only', [], diamond, None),
        ('same', [stone], '{id:"minecraft:stone",count:1}', stone),
        ('merge', [stone, '{id:"minecraft:stone",count:3}'], None,
         '{id:"minecraft:stone",count:5}'),
        ('occupied', [stone, diamond], None, stone),
        ('empty', [], None, None),
        ('components', [named], '{id:"minecraft:stone",count:1,components:{"minecraft:custom_name":"Filter"}}', named),
        ('distinct', [named, stone], None, named),
        ('component_merge', [named, named], None,
         '{id:"minecraft:stone",count:4,components:{"minecraft:custom_name":"Pocket"}}'),
        ('stack_limit', ['{id:"minecraft:stone",count:99}'], None,
         '{id:"minecraft:stone",count:64}'),
        ('tool_limit', ['{id:"minecraft:diamond_sword",count:99}'], None,
         '{id:"minecraft:diamond_sword",count:1}'),
        ('invalid_count', ['{id:"minecraft:stone",count:100}'], None,
         '{id:"minecraft:stone",count:1}'),
        ('override', ['{id:"minecraft:stone",count:12,components:{"minecraft:max_stack_size":10}}'], None,
         '{id:"minecraft:stone",count:10,components:{"minecraft:max_stack_size":10}}'),
    ]
    ops = setup('/fill 0 199 0 15 199 15 stone')
    for i, (name, pocket, hand, expected) in enumerate(cases):
        uuid = f'[I;139,0,0,{i + 1}]'
        fields = f'UUID:{uuid},Tags:["h139_{name}"],NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Health:14f,Inventory:[{",".join(pocket)}]'
        if hand:
            fields += f',equipment:{{mainhand:{hand}}}'
        ops += setup(f'/summon allay 10.5 200 10.5 {{{fields}}}')
    for tick in range(2):
        if tick:
            ops.append({'op': 'tick', 'n': 1})
        for i, (name, pocket, hand, expected) in enumerate(cases):
            target = f'@e[tag=h139_{name},limit=1]'
            guard = f'@e[tag=h139_{name},nbt={{UUID:[I;139,0,0,{i + 1}],Health:14f,NoAI:1b}},limit=1]'
            ops.append(cmd(f'/execute if entity {guard}', expect_result=1))
            inv = f'[{expected}]' if expected else '[]'
            # The full getter also detects extra slots: list predicates alone do not.
            ops.append(cmd(f'/data get entity {target} Inventory'))
            ops.append(cmd(f'/execute if entity @e[tag=h139_{name},nbt={{Inventory:{inv}}},limit=1]', expect_result=1))
            if hand:
                ops.append(cmd(f'/execute if entity @e[tag=h139_{name},nbt={{equipment:{{mainhand:{hand}}}}},limit=1]', expect_result=1))
            else:
                ops.append(cmd(f'/execute unless data entity {target} equipment.mainhand', expect_result=1))
    return ops


def named_weather_predicates():
    out = []

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    check('/tick freeze')
    check('/gamerule minecraft:advance_weather', 0)
    check('/execute if dimension minecraft:overworld')
    out.extend(setup('/forceload remove 0 0'))
    check('/forceload add 0 0')
    out.append({'op': 'settle'})
    check('/execute if loaded 8 200 8')
    check('/weather clear 100t', 100)
    check('/scoreboard objectives add weather_cmd dummy')
    out.extend(setup('/execute store result score #start weather_cmd run time query gametime'))

    def clock(ticks):
        out.extend(setup('/execute store result score #clock weather_cmd run time query gametime'))
        check('/scoreboard players operation #clock weather_cmd -= #start weather_cmd', ticks)

    def snapshot(name, ticks, rain, thunder, saved):
        out.append({'op': 'weather_state', 'label': name, 'expect_state': saved})
        clock(ticks)
        expected = [(name, (r is None or r == rain) and (t is None or t == thunder))
                    for name, r, t in weather_predicate_cases()]
        for predicate, passes in expected + [('always', True), ('never', False)]:
            out.extend(setup('/scoreboard players set #r weather_cmd 31',
                             '/scoreboard players set #s weather_cmd 17'))
            check('/scoreboard players get #r weather_cmd', 31)
            check('/scoreboard players get #s weather_cmd', 17)
            out.append(cmd('/execute store result score #r weather_cmd store success score #s weather_cmd '
                           'run execute if predicate command_weather:' + predicate))
            check('/scoreboard players get #r weather_cmd', int(passes))
            check('/scoreboard players get #s weather_cmd', int(passes))

    dry = [0, 0, 100, 0, 0]
    storm = [1, 1, 0, 100, 100]
    snapshot('clear_initial', 0, False, False, dry)
    out.append(cmd('/execute if predicate command_weather:missing'))
    check('/weather thunder 100t', 100)
    snapshot('storm_0', 0, False, False, storm)
    # Fresh Java 26.3 measurements: both levels rise from zero. Thunder
    # uses their product, rather than the saved flags or raw thunder level.
    elapsed = 0
    for at, rain, thunder in [(19, False, False), (20, True, False), (21, True, False),
                              (90, True, False), (91, True, False), (94, True, False),
                              (95, True, True), (100, True, True)]:
        n = at - elapsed
        check(f'/tick step {n}')
        out.append({'op': 'tick', 'n': n})
        snapshot(f'storm_{at}', at, rain, thunder, storm)
        elapsed = at
    check('/weather clear 100t', 100)
    snapshot('fade_0', 100, True, True, dry)
    elapsed = 0
    for at, rain, thunder in [(1, True, True), (5, True, True), (6, True, False),
                              (79, True, False), (80, True, False), (81, False, False),
                              (100, False, False)]:
        n = at - elapsed
        check(f'/tick step {n}')
        out.append({'op': 'tick', 'n': n})
        snapshot(f'fade_{at}', 100 + at, rain, thunder, dry)
        elapsed = at
    clock(200)
    out.extend(setup('/scoreboard objectives remove weather_cmd', '/forceload remove 0 0',
                     '/tick unfreeze'))
    return out


def client_commands():
    return [
        cmd('/version'),
        cmd('/spectate'),
        cmd('/spectate @s'),
        cmd('/gamemode spectator'),
        cmd('/spectate'),
        cmd('/spectate @s'),
        cmd('/spectate @s @s'),
        cmd('/gamemode creative'),
        cmd('/posteffect list @s'),
        cmd('/posteffect remove @s minecraft:blur'),
        cmd('/posteffect clear @s'),
        cmd('/posteffect add @s minecraft:blur'),
        cmd('/posteffect add @s minecraft:blur'),
        cmd('/posteffect add @a minecraft:invert'),
        cmd('/posteffect list @s'),
        cmd('/posteffect remove @s minecraft:blur'),
        cmd('/posteffect remove @p minecraft:blur'),
        cmd('/posteffect list @p'),
        cmd('/posteffect clear @s'),
        cmd('/posteffect clear @s'),
        cmd('/waypoint list'),
        cmd('/waypoint modify @s color red'),
        cmd('/waypoint modify @s color dark_aqua'),
        cmd('/waypoint modify @s color hex 12ab3f'),
        cmd('/waypoint modify @s color hex F0A'),
        cmd('/waypoint list'),
        cmd('/waypoint modify @s color reset'),
        cmd('/waypoint modify @s style set minecraft:bowtie'),
        cmd('/waypoint modify @s style reset'),
        cmd('/waypoint list'),
        cmd('/dialog show @s minecraft:server_links'),
        cmd('/dialog show @a minecraft:quick_actions'),
        cmd('/dialog show @s {type:"minecraft:notice",title:"Hello"}'),
        cmd('/dialog clear @s'),
        cmd('/dialog clear @a'),
        cmd('/kick @s'),
        cmd('/kick @s Go away'),
        cmd('/kick Player'),
        cmd('/fetchprofile name Notch'),
        {'op': 'tick', 'n': 5, 'record': True},
        cmd('/fetchprofile id 069a79f4-44e9-4726-a5be-fca90e38aaf5'),
        {'op': 'tick', 'n': 5, 'record': True},
        cmd('/unpublish'),
        cmd('/debug stop'),
        cmd('/debug start'),
        cmd('/debug start'),
        cmd('/debug stop'),
        cmd('/debug stop'),
        cmd('/jfr stop'),
    ]


def game_state():
    return [
        cmd('/gamemode survival'),
        cmd('/gamemode survival'),
        cmd('/gamemode adventure @s'),
        cmd('/gamemode creative @a'),
        cmd('/defaultgamemode survival'),
        cmd('/defaultgamemode creative'),
        cmd('/difficulty'),
        cmd('/difficulty hard'),
        cmd('/difficulty hard'),
        cmd('/difficulty normal'),
        cmd('/seed'),
        cmd('/time set 1000'),
        cmd('/time query daytime'),
        cmd('/time set noon'),
        cmd('/time set night'),
        cmd('/time add 100'),
        cmd('/time set 1000'),
        cmd('/weather rain'),
        cmd('/weather thunder 600'),
        cmd('/weather clear 1d'),
        cmd('/gamerule keep_inventory'),
        cmd('/gamerule keep_inventory true'),
        cmd('/gamerule keep_inventory'),
        cmd('/gamerule keep_inventory false'),
        cmd('/gamerule random_tick_speed 5'),
        cmd('/gamerule random_tick_speed'),
        cmd('/gamerule random_tick_speed 3'),
        cmd('/list'),
        cmd('/say Hello world'),
        cmd('/me waves'),
        cmd('/msg @s secret'),
        cmd('/tellraw @s {"text":"raw","color":"gold","bold":true}'),
        cmd('/tellraw @s ["a",{"text":"b","italic":true},{"translate":"block.minecraft.stone"}]'),
        cmd('/title @s title "Hi"'),
        cmd('/title @s subtitle "Sub"'),
        cmd('/title @s actionbar "Bar"'),
        cmd('/title @s times 10 20 30'),
        cmd('/title @s clear'),
        cmd('/title @s reset'),
        cmd('/help gamemode'),
        cmd('/random value 5..5'),
        cmd('/random roll 7..7'),
        cmd('/tick query'),
        cmd('/stopwatch create harness:a'),
        cmd('/stopwatch create harness:a'),
        cmd('/stopwatch query harness:b'),
    ]


def player_state():
    return [
        *setup('/gamemode survival'),
        cmd('/experience add @s 10'),
        cmd('/experience add @s 3 levels'),
        cmd('/experience query @s levels'),
        cmd('/experience query @s points'),
        cmd('/experience set @s 0 levels'),
        cmd('/xp set @s 5 points'),
        cmd('/experience query @s points'),
        cmd('/effect give @s minecraft:speed 30 1'),
        cmd('/effect give @s minecraft:speed 10 0'),
        cmd('/effect give @s minecraft:night_vision infinite 0 true'),
        cmd('/effect clear @s minecraft:speed'),
        cmd('/effect clear @s minecraft:speed'),
        cmd('/effect clear @s'),
        cmd('/effect clear @s'),
        cmd('/give @s minecraft:diamond 3'),
        cmd('/give @s minecraft:diamond_sword[minecraft:custom_name="Blade"]'),
        cmd('/give @s minecraft:diamond[item_name={text:"ItemName",color:"red"}]'),
        cmd('/give @s minecraft:stone 130'),
        cmd('/clear @s minecraft:diamond 0'),
        cmd('/clear @s minecraft:stone 0'),
        cmd('/clear @s minecraft:stone 64'),
        cmd('/clear @s minecraft:stone 0'),
        cmd('/item replace entity @s weapon.mainhand with minecraft:iron_pickaxe'),
        cmd('/enchant @s minecraft:efficiency 3'),
        cmd('/enchant @s minecraft:sharpness'),
        cmd('/enchant @s minecraft:efficiency 9'),
        cmd('/item replace entity @s armor.head with minecraft:golden_helmet'),
        cmd('/clear @s minecraft:golden_helmet 0'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_count",count:1}'),
        cmd('/clear @s'),
        cmd('/clear @s'),
        # Inventory.clearOrCountMatchingItems visits the off hand as well as
        # the main inventory, and consumes limited clears in inventory-slot order.
        cmd('/item replace entity @s weapon.offhand with minecraft:shield'),
        cmd('/clear @s minecraft:shield 0'),
        cmd('/clear @s minecraft:shield 1'),
        cmd('/clear @s minecraft:shield 0'),
        cmd('/item replace entity @s weapon.offhand with minecraft:stone 5'),
        cmd('/clear @s minecraft:stone 2'),
        cmd('/clear @s minecraft:stone 0'),
        cmd('/clear @s'),
        cmd('/clear @s minecraft:stone 0'),
        cmd('/item replace entity @s armor.head with minecraft:iron_helmet'),
        cmd('/item replace entity @s armor.feet with minecraft:iron_boots'),
        cmd('/item replace entity @s armor.chest with minecraft:stone'),
        cmd('/item replace entity @s armor.legs with minecraft:iron_boots'),
        cmd('/item replace entity @s armor.feet with minecraft:stone'),
        cmd('/clear @s * 1'),
        cmd('/clear @s minecraft:iron_boots 0'),
        cmd('/clear @s minecraft:iron_helmet 0'),
        cmd('/clear @s'),
        cmd('/attribute @s minecraft:max_health get'),
        cmd('/attribute @s minecraft:max_health base get'),
        cmd('/attribute @s minecraft:max_health base set 30'),
        cmd('/attribute @s minecraft:max_health modifier add harness:extra 4 add_value'),
        cmd('/attribute @s minecraft:max_health modifier value get harness:extra'),
        cmd('/attribute @s minecraft:max_health get'),
        cmd('/attribute @s minecraft:max_health modifier remove harness:extra'),
        cmd('/attribute @s minecraft:max_health base reset'),
        cmd('/spawnpoint @s 8 230 8'),
        cmd('/setworldspawn 8 230 8'),
        cmd('/tp @s 8.5 230 8.5'),
        cmd('/tp @s ~1 ~ ~'),
        cmd('/teleport @s 8.5 230 8.5 90 0'),
        cmd('/rotate @s 45 10'),
        cmd('/damage @s 2'),
        cmd('/damage @s 1 minecraft:magic'),
        *setup('/gamemode creative'),
    ]


def world_blocks():
    return [
        cmd('/setblock 2 205 2 minecraft:stone'),
        cmd('/setblock 2 205 2 minecraft:stone'),
        cmd('/setblock 2 205 2 minecraft:oak_stairs[facing=east]'),
        cmd('/execute if block 2 205 2 minecraft:oak_stairs[facing=east]'),
        cmd('/execute if block 2 205 2 minecraft:stone'),
        cmd('/execute unless block 2 205 2 minecraft:stone'),
        cmd('/fill 0 201 0 4 203 4 minecraft:glass'),
        cmd('/fill 0 201 0 4 203 4 minecraft:glass'),
        cmd('/fill 0 201 0 4 203 4 minecraft:stone hollow'),
        cmd('/fill 0 201 0 4 203 4 minecraft:dirt replace minecraft:stone'),
        cmd('/fill 0 201 0 4 203 4 minecraft:air destroy'),
        cmd('/execute if blocks 0 201 0 4 203 4 0 210 0 all'),
        cmd('/setblock 3 206 3 minecraft:chest'),
        cmd('/item replace block 3 206 3 container.0 with minecraft:apple 5'),
        cmd('/data get block 3 206 3 Items[0].count'),
        cmd('/data merge block 3 206 3 {CustomName:"Box"}'),
        cmd('/data get block 3 206 3 CustomName'),
        cmd('/data merge block 3 206 3 {Items:[]}'),
        cmd('/data get block 3 206 3 CustomName'),
        cmd('/clone 0 205 0 4 207 4 6 205 6'),
        cmd('/clone 0 205 0 4 207 4 6 205 6 masked'),
        cmd('/execute if blocks 0 205 0 4 207 4 6 205 6 all'),
        cmd('/data get block 9 206 9 CustomName'),
        cmd('/setblock 3 206 3 minecraft:air'),
        # A container keeps its loot table and seed until its items change
        # (RandomizableContainer.tryLoadLootTable / trySaveLootTable).
        cmd('/setblock 7 206 7 minecraft:chest{LootTable:"minecraft:chests/simple_dungeon",LootTableSeed:42L}'),
        cmd('/data get block 7 206 7'),
        cmd('/data get block 7 206 7 LootTableSeed'),
        cmd('/setblock 7 206 7 minecraft:air'),
        # BaseContainerBlockEntity.loadAdditional reads the tag's CustomName.
        cmd('/setblock 7 206 7 minecraft:barrel{CustomName:"Loot"}'),
        cmd('/data get block 7 206 7 CustomName'),
        cmd('/setblock 7 206 7 minecraft:air'),
        # A hopper placed into power turns `enabled=false` and keeps its block entity.
        cmd('/setblock 5 205 5 minecraft:redstone_block'),
        cmd('/setblock 5 206 5 minecraft:hopper'),
        cmd('/execute if block 5 206 5 minecraft:hopper[enabled=false]'),
        cmd('/item replace block 5 206 5 container.0 with minecraft:diamond 3'),
        cmd('/data get block 5 206 5 Items[0].count'),
        cmd('/fill 5 205 4 5 205 4 minecraft:hopper'),
        cmd('/data get block 5 205 4 Items'),
        cmd('/fill 0 200 0 15 228 15 air'),
        cmd('/execute if block 0 300 0 air'),
        cmd('/setblock 0 400 0 stone'),
        cmd('/worldborder get'),
        cmd('/worldborder set 1000'),
        cmd('/worldborder set 1000'),
        cmd('/worldborder center 10 20'),
        cmd('/worldborder center 10 20'),
        cmd('/worldborder damage amount 0.5'),
        cmd('/worldborder damage buffer 3'),
        cmd('/worldborder warning distance 10'),
        cmd('/worldborder warning time 20'),
        cmd('/worldborder add 100'),
        cmd('/worldborder get'),
        cmd('/worldborder set 59999968'),
        cmd('/worldborder center 0 0'),
    ]


def entities():
    return [
        cmd('/summon minecraft:pig 4.5 210 4.5 {NoAI:1b,NoGravity:1b,Tags:["a","h"]}'),
        cmd('/summon minecraft:cow 6.5 210 4.5 {NoAI:1b,NoGravity:1b,CustomName:"Daisy",Tags:["h"]}'),
        cmd('/summon minecraft:armor_stand 8.5 210 4.5 {NoGravity:1b}'),
        cmd('/execute if entity @e[tag=h,type=minecraft:pig]'),
        cmd('/execute if entity @e[tag=h,type=minecraft:sheep]'),
        cmd('/tag @e[tag=h,type=minecraft:pig] list'),
        cmd('/tag @e[tag=h,type=minecraft:cow] add b'),
        cmd('/tag @e[tag=h,type=minecraft:cow] add b'),
        cmd('/tag @e[tag=b] list'),
        cmd('/tag @e[tag=b] remove b'),
        cmd('/data get entity @e[tag=h,type=minecraft:pig,limit=1] Tags'),
        cmd('/data get entity @e[tag=h,type=minecraft:cow,limit=1] CustomName'),
        cmd('/data merge entity @e[tag=h,type=minecraft:pig,limit=1] {Tags:["a","c"]}'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed base get'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed base set 0.5'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed get'),
        cmd('/effect give @e[tag=h,type=minecraft:pig,limit=1] minecraft:speed 10 1'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed base get'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed get'),
        cmd('/effect clear @e[tag=h,type=minecraft:pig,limit=1] minecraft:speed'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed base reset'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed base get'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed base set 0.5'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed modifier add harness:m 0.5 add_value'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed modifier add harness:m 0.5 add_value'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed modifier value get harness:m'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed get'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed modifier remove harness:m'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed modifier remove harness:m'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed get'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:movement_speed base reset'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:follow_range base set 16'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:follow_range modifier add harness:m 8 add_value'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:follow_range modifier add harness:m 8 add_value'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:follow_range modifier value get harness:m'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:follow_range get'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:follow_range modifier remove harness:m'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:follow_range modifier remove harness:m'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:follow_range get'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:follow_range base reset'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:knockback_resistance base set 0.25'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:knockback_resistance modifier add harness:m 0.25 add_value'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:knockback_resistance modifier add harness:m 0.25 add_value'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:knockback_resistance modifier value get harness:m'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:knockback_resistance get'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:knockback_resistance modifier remove harness:m'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:knockback_resistance modifier remove harness:m'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:knockback_resistance get'),
        cmd('/attribute @e[tag=h,type=minecraft:pig,limit=1] minecraft:knockback_resistance base reset'),
        cmd('/effect give @e[tag=h] minecraft:glowing 10'),
        cmd('/damage @e[tag=h,type=minecraft:cow,limit=1] 1'),
        cmd('/ride @s mount @e[tag=h,type=minecraft:pig,limit=1]'),
        cmd('/ride @s dismount'),
        cmd('/ride @s dismount'),
        cmd('/spectate @e[tag=h,type=minecraft:pig,limit=1]'),
        cmd('/gamemode spectator'),
        cmd('/spectate @e[tag=h,type=minecraft:pig,limit=1]'),
        cmd('/spectate'),
        cmd('/gamemode creative'),
        cmd('/waypoint modify @e[tag=h,type=minecraft:pig,limit=1] color red'),
        cmd('/waypoint modify @e[type=minecraft:armor_stand,limit=1] color red'),
        cmd('/fetchprofile entity @e[tag=h,type=minecraft:pig,limit=1]'),
        cmd('/kill @e[tag=h,type=minecraft:pig]'),
        cmd('/kill @e[tag=h,type=minecraft:pig]'),
        cmd('/execute as @e[tag=h] run say mob'),
        cmd('/execute store result score @s harness_score run data get entity @s SelectedItemSlot'),
        cmd('/kill @e[tag=h]'),
        # A live rider travels with its vehicle in `/data get`, the way `saveWithoutId`
        # writes it (#1519): the pig is mounted, so its health is the nested value.
        cmd('/summon minecraft:cow 4.5 210 8.5 {Tags:["root"],NoAI:1b,Passengers:[{id:"minecraft:pig",NoAI:1b}]}'),
        cmd('/data get entity @e[tag=root,limit=1] Passengers[0].Health'),
        cmd('/data get entity @e[tag=root,limit=1] Passengers[0].id'),
        cmd('/kill @e[tag=root]'),
        # Saved attribute bases are read for every mob, then Health or the max
        # health when there is none (LivingEntity.readAdditionalSaveData, #1483).
        cmd('/summon minecraft:cow 4.5 210 8.5 {Tags:["attr"],NoAI:1b,NoGravity:1b,'
            'attributes:[{id:"minecraft:max_health",base:40.0d},{id:"minecraft:movement_speed",base:0.5d}]}'),
        cmd('/data get entity @e[tag=attr,limit=1] Health'),
        cmd('/attribute @e[tag=attr,limit=1] minecraft:max_health base get'),
        cmd('/attribute @e[tag=attr,limit=1] minecraft:movement_speed base get'),
        cmd('/kill @e[tag=attr]'),
        cmd('/summon minecraft:zombie 4.5 210 8.5 {Tags:["attr"],NoAI:1b,NoGravity:1b,Health:7.0f,'
            'attributes:[{id:"minecraft:attack_damage",base:9.0d}]}'),
        cmd('/data get entity @e[tag=attr,limit=1] Health'),
        cmd('/attribute @e[tag=attr,limit=1] minecraft:attack_damage base get'),
        cmd('/kill @e[tag=attr]'),
        # A summoned mob is not persistent: vanilla never calls
        # `Mob.setPersistenceRequired` for a mob it spawns (#2787).
        cmd('/summon minecraft:zombie 4.5 210 12.5'),
        cmd('/execute if entity @e[type=minecraft:zombie,nbt={PersistenceRequired:1b}]'),
        cmd('/kill @e[type=minecraft:zombie]'),
        # An End crystal is an entity of its own: a type selector finds it,
        # reads it and kills it (#1531).
        cmd('/summon minecraft:end_crystal 4.5 210 8.5'),
        cmd('/execute if entity @e[type=minecraft:end_crystal]'),
        cmd('/data get entity @e[type=minecraft:end_crystal,limit=1] Pos'),
        cmd('/kill @e[type=minecraft:end_crystal]'),
        cmd('/execute if entity @e[type=minecraft:end_crystal]'),
        cmd('/summon minecraft:ender_dragon 4.5 210 8.5'),
        cmd('/execute if entity @e[type=minecraft:ender_dragon]'),
        cmd('/data get entity @e[type=minecraft:ender_dragon,limit=1] Pos'),
        cmd('/kill @e[type=minecraft:ender_dragon]'),
        # A primed TNT is an entity too: a type selector finds, tags, reads,
        # moves and kills it.
        cmd('/summon minecraft:tnt 4.5 210 8.5 {fuse:2000s,Tags:["t"]}'),
        cmd('/execute if entity @e[type=minecraft:tnt]'),
        cmd('/tag @e[type=minecraft:tnt,limit=1] list'),
        cmd('/execute if entity @e[type=minecraft:tnt,tag=t]'),
        cmd('/data get entity @e[type=minecraft:tnt,limit=1] Tags'),
        cmd('/tp @e[type=minecraft:tnt] 4.5 215 8.5'),
        cmd('/kill @e[type=minecraft:tnt]'),
        cmd('/execute if entity @e[type=minecraft:tnt]'),
        # So is a falling block, named after its block (`getTypeName`).
        cmd('/summon minecraft:falling_block 4.5 210 8.5 {BlockState:{Name:"minecraft:sand"},NoGravity:1b,Tags:["f"]}'),
        cmd('/execute if entity @e[type=minecraft:falling_block,tag=f]'),
        cmd('/tag @e[type=minecraft:falling_block,limit=1] list'),
        cmd('/data get entity @e[type=minecraft:falling_block,limit=1] BlockState.Name'),
        cmd('/tp @e[type=minecraft:falling_block] 4.5 215 8.5'),
        cmd('/kill @e[type=minecraft:falling_block]'),
        cmd('/execute if entity @e[type=minecraft:falling_block]'),
        # A firework rocket, with what `addAdditionalSaveData` writes.
        cmd('/summon minecraft:firework_rocket 4.5 210 8.5 {LifeTime:2000,NoGravity:1b,Tags:["r"]}'),
        cmd('/execute if entity @e[type=minecraft:firework_rocket,tag=r]'),
        cmd('/tag @e[type=minecraft:firework_rocket,limit=1] list'),
        cmd('/data get entity @e[type=minecraft:firework_rocket,limit=1] LifeTime'),
        cmd('/data get entity @e[type=minecraft:firework_rocket,limit=1] FireworksItem.id'),
        cmd('/data get entity @e[type=minecraft:firework_rocket,limit=1] ShotAtAngle'),
        cmd('/tp @e[type=minecraft:firework_rocket] 4.5 215 8.5'),
        cmd('/kill @e[type=minecraft:firework_rocket]'),
        cmd('/execute if entity @e[type=minecraft:firework_rocket]'),
        # An ominous item spawner, with an item or empty (`ItemStack.EMPTY`,
        # which saves no `item` and spawns nothing when its time comes).
        cmd('/summon minecraft:ominous_item_spawner 4.5 210 8.5 {spawn_item_after_ticks:2000L,item:{id:"minecraft:diamond",count:2},Tags:["o"]}'),
        cmd('/execute if entity @e[type=minecraft:ominous_item_spawner,tag=o]'),
        cmd('/tag @e[type=minecraft:ominous_item_spawner,limit=1] list'),
        cmd('/data get entity @e[type=minecraft:ominous_item_spawner,limit=1] item'),
        cmd('/data get entity @e[type=minecraft:ominous_item_spawner,limit=1] spawn_item_after_ticks'),
        cmd('/tp @e[type=minecraft:ominous_item_spawner] 4.5 215 8.5'),
        cmd('/kill @e[type=minecraft:ominous_item_spawner]'),
        cmd('/summon minecraft:ominous_item_spawner 4.5 210 8.5'),
        cmd('/data get entity @e[type=minecraft:ominous_item_spawner,limit=1] item'),
        cmd('/kill @e[type=minecraft:ominous_item_spawner]'),
        cmd('/summon minecraft:ominous_item_spawner 4.5 210 8.5 {spawn_item_after_ticks:5L}'),
        {'op': 'tick', 'n': 10},
        cmd('/execute if entity @e[type=minecraft:ominous_item_spawner]'),
        cmd('/execute if entity @e[type=minecraft:item,distance=..3]'),
        # And the thrown items.
        *[c for kind in ('snowball', 'egg', 'ender_pearl') for c in (
            cmd(f'/summon minecraft:{kind} 4.5 210 8.5 {{NoGravity:1b}}'),
            cmd(f'/execute if entity @e[type=minecraft:{kind}]'),
            cmd(f'/data get entity @e[type=minecraft:{kind},limit=1] Item.id'),
            cmd(f'/kill @e[type=minecraft:{kind}]'),
        )],
        *setup('/kill @e[type=minecraft:armor_stand]'),
        {'op': 'tick', 'n': 25},
        cmd('/execute if entity @e[tag=h]'),
    ]


def wither_spawn():
    """A wither charging up (`Invul`) neither moves toward its target nor is hurt by
    the blast that ends the charge (#3322): `WitherBoss.customServerAiStep` sets no
    alternative target while invulnerable, and `ServerLevel.explode`'s entity query
    skips the source."""
    wither = '@e[type=minecraft:wither,limit=1]'
    return [
        *setup('/fill 0 209 4 9 209 20 minecraft:stone',
               '/summon minecraft:pig 4.5 210 18.5 {NoAI:1b,Tags:["wither_prey"]}',
               '/summon minecraft:wither 4.5 210 8.5 {Invul:20}'),
        {'op': 'tick', 'n': 10},
        cmd(f'/data get entity {wither} Pos'),
        cmd(f'/data get entity {wither} Invul'),
        {'op': 'tick', 'n': 12},
        cmd(f'/data get entity {wither} Invul'),
        cmd(f'/data get entity {wither} Health'),
        *setup('/kill @e[type=minecraft:wither]', '/kill @e[tag=wither_prey]', '/fill 0 200 0 15 228 20 air'),
        {'op': 'tick', 'n': 5},
    ]


def riding_teleport():
    """Player teleports remove the vehicle, leave it behind and keep the destination (#1473)."""
    horse = '@e[tag=tp_horse,limit=1]'
    operations = setup(
        '/summon minecraft:horse 4.5 220 4.5 {Tame:1b,NoAI:1b,NoGravity:1b,Tags:["tp_horse"]}',
        '/summon minecraft:marker 8.5 220 0.5 {Tags:["tp_destination"]}',
    )
    for teleport in ('/tp @s 8.5 220 0.5', '/teleport @s ~4 220 ~-4',
                     '/tp @s @e[tag=tp_destination,limit=1]'):
        operations += [
            *setup('/tp @s 4.5 220 4.5'),
            cmd(f'/ride @s mount {horse}'),
            cmd('/execute on vehicle run say mounted'),
            # An invalid destination must not dismount the player.
            cmd('/tp @s 30000000 220 0'),
            cmd('/execute on vehicle run say still mounted'),
            cmd(teleport),
            cmd('/execute on vehicle run say still riding after teleport'),
            cmd('/data get entity @s Pos'),
            cmd(f'/data get entity {horse} Pos'),
            {'op': 'tick', 'n': 60},
            cmd('/execute on vehicle run say still riding after ticks'),
            cmd('/data get entity @s Pos'),
            cmd(f'/data get entity {horse} Pos'),
            cmd('/ride @s dismount'),
        ]
    return operations + setup('/kill @e[tag=tp_horse]', '/kill @e[tag=tp_destination]',
                              '/tp @s 8.5 230 8.5 0 0')


def mob_riding():
    """Mobs mount and dismount mobs with /ride, NBT passengers included (#1520)."""
    rider, vehicle = '@e[tag=ride_r,limit=1]', '@e[tag=ride_v,limit=1]'
    return [
        *setup('/summon minecraft:cow 4.5 220 12.5 {NoAI:1b,NoGravity:1b,Tags:["ride_v"]}',
               '/summon minecraft:pig 6.5 220 12.5 {NoAI:1b,NoGravity:1b,Tags:["ride_r"]}'),
        cmd(f'/ride {rider} dismount'),
        cmd(f'/ride {rider} mount @s'),
        cmd(f'/ride {rider} mount {rider}'),
        cmd(f'/ride {rider} mount {vehicle}'),
        cmd(f'/ride {rider} mount {vehicle}'),
        cmd(f'/ride {vehicle} mount {rider}'),
        cmd('/execute as @e[tag=ride_r] on vehicle run say carrying'),
        cmd(f'/ride {rider} dismount'),
        cmd(f'/data get entity {rider} Pos'),
        cmd('/execute as @e[tag=ride_r] on vehicle run say still carrying'),
        *setup('/kill @e[tag=ride_r]', '/kill @e[tag=ride_v]'),
        *setup('/summon minecraft:cow 4.5 220 12.5 {NoAI:1b,NoGravity:1b,Tags:["ride_v"],'
               'Passengers:[{id:"minecraft:pig",NoAI:1b,NoGravity:1b,Tags:["ride_r"]}]}'),
        cmd(f'/ride {rider} dismount'),
        cmd(f'/data get entity {rider} Pos'),
        *setup('/kill @e[tag=ride_r]', '/kill @e[tag=ride_v]'),
    ]


def camel_riders():
    """A husk riding a camel (#3199): the camel and its rider never push each other
    (Entity.push skips passengers of the same vehicle), so the camel stays near
    where it was summoned instead of sliding off backwards, and it never sits down
    with a controlling passenger (CamelAi.RandomSitting needs none)."""
    camel = '@e[tag=camel_v,limit=1]'
    return [
        # A walled pen, and a camel that cannot walk: only a push would move it.
        *setup('/fill 0 200 0 15 203 15 minecraft:glass hollow', '/fill 1 203 1 14 203 14 minecraft:air',
               '/scoreboard objectives add camel_pose dummy',
               '/summon minecraft:camel 8.0 201 8.0 {Tags:["camel_v"],'
               'Passengers:[{id:"minecraft:husk",Tags:["camel_r"]}]}',
               f'/attribute {camel} minecraft:movement_speed base set 0'),
        {'op': 'tick', 'n': 100},
        cmd('/execute as @e[tag=camel_r] on vehicle run say carrying'),
        cmd('/execute if entity @e[tag=camel_v,x=8,y=201,z=8,distance=..4]'),
        {'op': 'tick', 'n': 500},
        *setup(f'/execute store result score camel camel_pose run data get entity {camel} LastPoseTick'),
        # Negative while sitting.
        cmd('/execute if score camel camel_pose matches ..-1'),
        cmd('/execute as @e[tag=camel_r] on vehicle run say still carrying'),
        *setup('/kill @e[tag=camel_r]', '/kill @e[tag=camel_v]', '/scoreboard objectives remove camel_pose',
               '/fill 0 200 0 15 203 15 minecraft:air'),
    ]


def scores():
    return [
        cmd('/scoreboard objectives add harness dummy'),
        cmd('/scoreboard objectives add harness dummy'),
        cmd('/scoreboard objectives add harness_h health "Health"'),
        cmd('/scoreboard objectives list'),
        cmd('/scoreboard players set @s harness 5'),
        cmd('/scoreboard players add @s harness 3'),
        cmd('/scoreboard players remove @s harness 1'),
        cmd('/scoreboard players get @s harness'),
        cmd('/scoreboard players set fake harness 9'),
        cmd('/scoreboard players operation @s harness += fake harness'),
        cmd('/scoreboard players get @s harness'),
        cmd('/scoreboard players list'),
        cmd('/scoreboard players list @s'),
        cmd('/scoreboard players reset fake harness'),
        cmd('/scoreboard players get fake harness'),
        cmd('/scoreboard objectives setdisplay list harness'),
        cmd('/scoreboard objectives setdisplay sidebar harness'),
        cmd('/scoreboard objectives modify harness displayname "Shown"'),
        cmd('/scoreboard objectives modify harness rendertype hearts'),
        cmd('/execute store result score @s harness run scoreboard players get @s harness'),
        cmd('/execute if score @s harness matches 15'),
        cmd('/team add red_team'),
        cmd('/team add red_team'),
        cmd('/team modify red_team color red'),
        cmd('/team modify red_team prefix "[R] "'),
        cmd('/team join red_team @s'),
        cmd('/team list'),
        cmd('/team list red_team'),
        cmd('/waypoint list'),
        cmd('/team leave @s'),
        cmd('/team remove red_team'),
        cmd('/scoreboard objectives remove harness'),
        cmd('/scoreboard objectives remove harness_h'),
        cmd('/bossbar add harness:bar "Bar"'),
        cmd('/bossbar add harness:bar "Bar"'),
        cmd('/bossbar set harness:bar value 30'),
        cmd('/bossbar set harness:bar max 60'),
        cmd('/bossbar set harness:bar color red'),
        cmd('/bossbar set harness:bar style notched_10'),
        cmd('/bossbar set harness:bar players @s'),
        cmd('/bossbar get harness:bar value'),
        cmd('/bossbar get harness:bar players'),
        cmd('/bossbar list'),
        cmd('/bossbar remove harness:bar'),
        cmd('/bossbar remove harness:bar'),
    ]


def selector_missing_predicates():
    """Missing named predicates reject both signs before negation (#7794)."""
    missing = 'h124_missing_873b04b1:absent'
    missing_default = 'h124_missing_default_873b04b1'
    defined = ('h124_selector:sheep', 'minecraft:h124_selector_sheep', 'h124_selector_sheep')
    ops = setup('/tick freeze')
    for i, kind in enumerate(('sheep', 'cow', 'pig', 'chicken')):
        custom = ',CustomName:{text:"Named pig"}' if kind == 'pig' else ''
        ops += setup(f'/summon minecraft:{kind} {6 + i * 2}.5 220 4.5 '
                     f'{{Tags:["h124","h124_{kind}"],NoAI:1b,NoGravity:1b,'
                     f'PersistenceRequired:1b{custom}}}')
    for kind in ('sheep', 'cow', 'pig', 'chicken'):
        base = f'@e[tag=h124_{kind}'
        ops.append(cmd(f'/execute if entity {base}]', expect_result=1))
        for predicate in (missing, missing_default):
            for sign in ('', '!'):
                ops.append(cmd(f'/execute if entity {base},predicate={sign}{predicate}]'))
        for predicate in defined:
            ops += [cmd(f'/execute if entity {base},predicate={predicate}]',
                        expect_result=1 if kind == 'sheep' else None),
                    cmd(f'/execute if entity {base},predicate=!{predicate}]',
                        expect_result=1 if kind != 'sheep' else None)]
        ops += [cmd(f'/execute if entity {base},predicate={defined[0]},predicate=!{missing}]'),
                cmd(f'/execute if entity {base},predicate=!{missing},predicate={defined[0]}]')]
    ops += [cmd(f'/execute if entity @e[tag=h124,predicate={defined[0]}]', expect_result=1),
            cmd(f'/execute if entity @e[tag=h124,predicate=!{defined[0]}]', expect_result=3),
            cmd(f'/execute if entity @e[tag=h124_unrelated,predicate=!{missing}]')]
    return ops + setup('/kill @e[tag=h124]', '/tick unfreeze')



def item_command_damage():
    """DamageCommand -> live ItemEntity health, resistance and destruction."""
    ops = setup('/forceload add 0 0 31 31', '/scoreboard objectives add item_hurt dummy')
    ops += [{'op': 'settle'}]
    ops += [cmd(f'/execute if loaded {x} 100 {z}', expect_result=1)
            for x, z in ((0, 0), (16, 0), (0, 16), (16, 16))]
    ops += setup('/tick freeze', '/fill 12 99 0 15 103 4 air')
    witness = '@e[tag=item_hurt_witness,limit=1]'
    ops += setup('/summon item 14.5 100 3.5 {Tags:["item_hurt_actor","item_hurt_witness"],Item:{id:"minecraft:stone",count:1},CustomName:"witness",PickupDelay:32767,NoGravity:1b,Motion:[0d,0d,0d]}')
    ops += [cmd(f'/execute if entity {witness}', expect_result=1)]
    cases = (
        ('ordinary', 'stone', '', ((3, '', 2, True), (1, 'in_fire', 1, True), (1, '', None, True))),
        ('zero', 'stone', '', ((0, '', 5, True),)),
        ('fractional', 'stone', '', ((0.25, '', 4, True),)),
        ('netherite', 'netherite_ingot', '', ((3, 'lava', 5, False), (1, '', 4, True))),
        ('star', 'nether_star', '', ((3, 'explosion', 5, False), (1, '', 4, True))),
        ('component', 'stone', ',components:{"minecraft:damage_resistant":{types:"#minecraft:is_fire"}}',
         ((3, 'in_fire', 5, False), (1, '', 4, True))),
    )
    for label, item, components, hits in cases:
        target = f'@e[tag=item_hurt_{label},limit=1]'
        ops += setup(f'/summon item 14.5 100 0.5 {{Tags:["item_hurt_actor","item_hurt_{label}"],Item:{{id:"minecraft:{item}",count:1{components}}},CustomName:"hit_{label}",PickupDelay:32767,NoGravity:1b,Motion:[0d,0d,0d]}}')
        ops += [cmd(f'/execute if entity {target}', expect_result=1),
                cmd(f'/data get entity {target} Health', expect_result=5)]
        for amount, source, health, accepted in hits:
            damage_type = f' minecraft:{source}' if source else ''
            ops += setup('/scoreboard players set #result item_hurt -1', '/scoreboard players set #success item_hurt -1')
            ops += [cmd(f'/execute store result score #result item_hurt store success score #success item_hurt run damage {target} {amount}{damage_type}'),
                    cmd('/scoreboard players get #result item_hurt', expect_result=int(accepted)),
                    cmd('/scoreboard players get #success item_hurt', expect_result=int(accepted))]
            if health is None:
                ops += [cmd(f'/execute unless entity {target}', expect_result=1),
                        cmd(f'/data get entity {witness} Health', expect_result=5)]
            else:
                ops += [cmd(f'/execute if entity {target}', expect_result=1),
                        cmd(f'/data get entity {target} Health', expect_result=health)]
    mob = '@e[tag=item_hurt_mob,limit=1]'
    ops += setup('/summon pig 12.5 100 0.5 {Tags:["item_hurt_actor","item_hurt_mob"],NoAI:1b,NoGravity:1b,CustomName:"mob_control"}')
    ops += [cmd(f'/execute if entity {mob}', expect_result=1),
            cmd(f'/data get entity {mob} Health', expect_result=10),
            cmd(f'/damage {mob} 3', expect_result=1),
            cmd(f'/data get entity {mob} Health', expect_result=7)]
    return ops + setup('/kill @e[tag=item_hurt_actor]', '/scoreboard objectives remove item_hurt', '/tick unfreeze')


def execute():
    return [
        *setup('/scoreboard objectives add exec dummy', '/scoreboard players set @s exec 4', '/scoreboard players set fake exec 9'),
        cmd('/execute as @s run say hi'),
        cmd('/execute as @s at @s run tp @s ~ ~ ~'),
        cmd('/execute positioned 4 210 4 run setblock ~ ~ ~ minecraft:stone'),
        cmd('/execute positioned 4 210 4 if block ~ ~ ~ minecraft:stone'),
        cmd('/execute positioned as @s run tp @s ~ ~ ~'),
        cmd('/execute align xyz run summon minecraft:armor_stand ~ ~-20 ~ {NoGravity:1b,Tags:["ex"],UUID:[I;1,2,3,4]}'),
        cmd('/summon minecraft:armor_stand 3 210 3 {NoGravity:1b,Tags:["implicit_tp"]}'),
        cmd('/execute as @e[tag=implicit_tp,limit=1] run tp 4 210 4'),
        cmd('/data get entity @e[tag=implicit_tp,limit=1] Pos'),
        cmd('/kill @e[tag=implicit_tp]'),
        cmd('/execute if entity @e[tag=ex]'),
        # A UUID names any entity (ServerLevel.getEntity), not only mobs (#4966).
        cmd('/execute if entity 00000001-0000-0002-0000-000300000004'),
        cmd('/data get entity 00000001-0000-0002-0000-000300000004 Tags'),
        cmd('/summon minecraft:item_display 2.5 210 2.5 {UUID:[I;5,6,7,8],Tags:["uuid_display"]}'),
        cmd('/data get entity 00000005-0000-0006-0000-000700000008 Tags'),
        cmd('/tp 00000005-0000-0006-0000-000700000008 2.5 215 2.5'),
        cmd('/data get entity 00000005-0000-0006-0000-000700000008 Pos'),
        cmd('/kill 00000005-0000-0006-0000-000700000008'),
        # The killed id stays in the UUID map. A re-summon of that UUID must
        # still resolve to the entity the level holds.
        cmd('/summon minecraft:item_display 2.5 210 2.5 {UUID:[I;5,6,7,8],Tags:["uuid_display_2"]}'),
        cmd('/data get entity 00000005-0000-0006-0000-000700000008 Tags'),
        cmd('/kill 00000005-0000-0006-0000-000700000008'),
        cmd('/summon minecraft:marker 2.5 210 8.5 {UUID:[I;13,14,15,16],Tags:["uuid_marker"]}'),
        cmd('/execute as 0000000d-0000-000e-0000-000f00000010 run data get entity @s Tags'),
        cmd('/kill 0000000d-0000-000e-0000-000f00000010'),
        cmd('/data get entity 00000015-0000-0016-0000-001700000018'),
        cmd('/summon minecraft:pig 2.5 210 6.5 {NoAI:1b,NoGravity:1b,UUID:[I;9,10,11,12]}'),
        cmd('/kill 00000009-0000-000a-0000-000b0000000c'),
        # OwnableEntity.getOwner → EntityReference.getLivingEntity: an armor
        # stand is an owner, a marker is not.
        cmd('/summon minecraft:wolf 2.5 210 14.5 {NoAI:1b,NoGravity:1b,Sitting:1b,Tags:["uuid_pet"],Owner:[I;1,2,3,4]}'),
        cmd('/execute as @e[tag=uuid_pet,limit=1] on owner run data get entity @s Tags'),
        cmd('/summon minecraft:marker 4.5 210 14.5 {UUID:[I;17,18,19,20]}'),
        cmd('/summon minecraft:wolf 6.5 210 14.5 {NoAI:1b,NoGravity:1b,Sitting:1b,Tags:["uuid_pet_marker"],Owner:[I;17,18,19,20]}'),
        cmd('/execute as @e[tag=uuid_pet_marker,limit=1] on owner run say reached'),
        cmd('/kill @e[tag=uuid_pet]'),
        cmd('/kill @e[tag=uuid_pet_marker]'),
        cmd('/kill 00000011-0000-0012-0000-001300000014'),
        cmd('/execute as @e[tag=ex] run data get entity @s Pos'),
        cmd('/data get entity @e[tag=ex,limit=1]'),
        cmd('/data get entity @e[tag=ex,limit=1] Pose'),
        cmd('/execute anchored eyes run tp @s ^ ^ ^'),
        cmd('/execute rotated 90 0 run tp @s ~ ~ ~ ~ ~'),
        cmd('/execute rotated as @e[tag=ex,limit=1] run tp @s ~ ~ ~ ~ ~'),
        cmd('/execute facing 0 230 0 run tp @s ~ ~ ~ ~ ~'),
        cmd('/execute facing entity @e[tag=ex,limit=1] feet run tp @s ~ ~ ~ ~ ~'),
        cmd('/execute in minecraft:the_nether run setblock 0 200 0 minecraft:stone'),
        cmd('/execute in minecraft:the_end if block 0 64 0 minecraft:air'),
        cmd('/execute in minecraft:the_end run say elsewhere'),
        cmd('/execute in minecraft:nowhere run say nowhere'),
        cmd('/execute in minecraft:overworld run setblock 5 210 5 minecraft:dirt'),
        cmd('/execute if dimension minecraft:overworld'),
        cmd('/execute if dimension minecraft:the_end'),
        cmd('/execute if loaded 8 230 8'),
        cmd('/execute if loaded 100000 230 100000'),
        cmd('/execute if score @s exec matches 4'),
        cmd('/execute if score @s exec < fake exec'),
        cmd('/execute if score @s exec = fake exec'),
        cmd('/execute unless score @s exec matches 1..3'),
        cmd('/execute if score nobody exec matches 1'),
        cmd('/execute if data entity @s Inventory'),
        cmd('/execute if data entity @e[tag=ex,limit=1] Tags[0]'),
        cmd('/execute if data block 4 210 4 Items'),
        cmd('/execute if data storage harness:s value'),
        cmd('/execute if predicate minecraft:tool/can_shear'),
        cmd('/execute if predicate {condition:"minecraft:random_chance",chance:1.0}'),
        cmd('/execute if predicate {condition:"minecraft:entity_properties",entity:"this",predicate:{type:"minecraft:player"}}'),
        cmd('/execute if items entity @s weapon.mainhand minecraft:air'),
        cmd('/execute if items entity @s container.* *'),
        cmd('/execute store result score @s exec run data get entity @s SelectedItemSlot'),
        cmd('/execute store success score @s exec run setblock 4 210 4 minecraft:stone'),
        cmd('/scoreboard players get @s exec'),
        cmd('/execute store result storage harness:s value int 2 run scoreboard players get fake exec'),
        cmd('/data get storage harness:s value'),
        cmd('/execute store result entity @e[tag=ex,limit=1] Rotation[0] float 1 run scoreboard players get fake exec'),
        cmd('/data get entity @e[tag=ex,limit=1] Rotation'),
        cmd('/execute store result block 4 210 4 x int 1 run scoreboard players get fake exec'),
        cmd('/execute summon minecraft:armor_stand run tag @s add summoned'),
        cmd('/execute if entity @e[tag=summoned]'),
        cmd('/execute as @e[tag=ex] on passengers run say passenger'),
        cmd('/execute on vehicle run say vehicle'),
        cmd('/execute as @e[tag=ex] run return 3'),
        cmd('/execute if entity @s run return fail'),
        cmd('/return 7'),
        cmd('/return fail'),
        cmd('/return run scoreboard players get fake exec'),
        cmd('/execute if biome 8 230 8 #minecraft:is_overworld'),
        cmd('/execute if block 8 0 8 #minecraft:base_stone_overworld'),
        cmd('/execute as @a run tellraw @s "each"'),
        *setup('/kill @e[type=minecraft:armor_stand]', '/setblock 4 210 4 minecraft:air', '/setblock 5 210 5 minecraft:air',
               '/scoreboard objectives remove exec', '/tp @s 8.5 230 8.5 0 0'),
        {'op': 'tick', 'n': 25},
        *setup('/gamemode spectator'),
        cmd('/execute in minecraft:the_nether if dimension minecraft:the_nether run say source dimension'),
        cmd('/execute in minecraft:the_nether at @s if dimension minecraft:overworld run say entity dimension'),
        cmd('/execute in minecraft:the_nether positioned as @s if dimension minecraft:the_nether run say position only'),
        cmd('/execute in minecraft:the_nether if loaded 0 100 0'),
        cmd('/execute in minecraft:the_nether run tp @s 0 100 0'),
        {'op': 'settle'},
        cmd('/execute if dimension minecraft:the_nether run say dimension switched'),
        cmd('/data get entity @s Pos'),
        # Back to the terrain loaded since the start (x and z scaled by 8: 12 230 4). The fresh
        # ocean chunks at 644 100 -316 broke some of their kelp and seagrass while they loaded,
        # on some runs only (vanilla loads chunks asynchronously): that rolled those blocks' loot
        # and added their random sequences to the count of /random reset *.
        cmd('/execute in minecraft:the_nether positioned 1 230 0 in minecraft:overworld run tp @s ~ ~ ~'),
        {'op': 'settle'},
        cmd('/execute if dimension minecraft:overworld run say relative dimension'),
        cmd('/data get entity @s Pos'),
        cmd('/execute in minecraft:overworld positioned 80 100 -40 in minecraft:the_nether run tp @s ~ ~ ~ 45 30'),
        {'op': 'settle'},
        cmd('/data get entity @s Pos'),
        cmd('/data get entity @s Rotation'),
        cmd('/execute in minecraft:overworld run tp @s 8.5 230 8.5'),
        {'op': 'settle'},
        cmd('/execute if dimension minecraft:overworld run say dimension restored'),
        cmd('/data get entity @s Pos'),
        *setup('/gamemode creative'),
    ]



def projectile_origin():
    """TraceableEntity.getOwner resolves any loaded entity, not only a mob."""
    ops = setup('/forceload add 0 0 31 31', '/scoreboard objectives add origin_probe dummy')
    ops += [{'op': 'settle'}]
    ops += [cmd(f'/execute if loaded {x} 100 {z}', expect_result=1)
            for x, z in ((0, 0), (16, 0), (0, 16), (16, 16))]
    ops += setup('/tick freeze', '/fill 2 99 2 10 103 10 air',
                 '/summon armor_stand 3.5 100 4.5 {Tags:["origin_actor","origin_stand"],CustomName:"owner_stand",UUID:[I;208,1,2,3],NoGravity:1b}',
                 '/summon pig 4 100 4 {Tags:["origin_actor","origin_pig"],CustomName:"owner_pig",UUID:[I;208,11,12,13],NoAI:1b,NoGravity:1b}')
    for tag, words in (('origin_stand', '208,1,2,3'), ('origin_pig', '208,11,12,13')):
        target = f'@e[tag={tag},limit=1]'
        ops += [cmd(f'/execute if entity @e[tag={tag},nbt={{UUID:[I;{words}]}}]', expect_result=1),
                cmd(f'/data get entity {target} UUID'),
                cmd(f'/data get entity {target} CustomName')]
    for name, owner, count in (('stand', '208,1,2,3', 1), ('pig', '208,11,12,13', 1),
                               ('unknown', '208,15,16,17', 0), ('absent', None, 0)):
        tag = f'origin_arrow_{name}'
        target = f'@e[tag={tag},limit=1]'
        owner_nbt = f',Owner:[I;{owner}]' if owner else ''
        ops += setup(f'/summon arrow 8 100 8 {{Tags:["origin_actor","{tag}"],NoGravity:1b{owner_nbt}}}')
        ops += [cmd(f'/execute if entity {target}', expect_result=1)]
        # Saved Owner reads are a separate command-getter regression; the
        # input UUID is retained by arrow_from_nbt independently of Who.
        if owner is None:
            ops += [cmd(f'/execute unless data entity {target} Owner', expect_result=1)]
        ops += [cmd(f'/execute as {target} on origin run data get entity @s CustomName')]
        ops += setup('/scoreboard players set #origin origin_probe 0')
        ops += [cmd(f'/execute as {target} on origin run scoreboard players add #origin origin_probe 1'),
                cmd('/scoreboard players get #origin origin_probe', expect_result=count)]
    return ops + setup('/kill @e[tag=origin_actor]', '/scoreboard objectives remove origin_probe', '/tick unfreeze')


def levels():
    """A nether portal round trip: on the way back the Overworld's entities (the
    pig left by `entities`) are found and a summon names what it summoned. An
    unbounded selector also reaches the level the player is not in, while a
    bounded one does not."""
    return [
        *setup('/fill 2 231 12 5 236 12 minecraft:obsidian', '/fill 3 232 12 4 235 12 minecraft:nether_portal[axis=x]',
               '/tp @s 3.5 232 12.5 0 0',
               # A cow in the Overworld, in a chunk that stays loaded while the
               # player is in the nether.
               '/forceload add 0 0',
               '/summon minecraft:cow 10.5 210 4.5 {NoAI:1b,NoGravity:1b,Tags:["remote_cow"]}'),
        {'op': 'portal'},
        {'op': 'settle'},
        cmd('/execute if dimension minecraft:the_nether run say through the portal'),
        cmd('/data get entity @e[tag=remote_cow,limit=1] Pos'),
        cmd('/data get entity @e[tag=remote_cow,distance=..128,limit=1] Pos'),
        # The unbounded selector reaches the Overworld cow while the player is
        # in the Nether: /attribute acts on the entity in its own level, as
        # AttributeCommand does on the entity it was given.
        cmd('/attribute @e[tag=remote_cow,limit=1] minecraft:max_health get'),
        cmd('/attribute @e[tag=remote_cow,limit=1] minecraft:follow_range base set 21'),
        cmd('/attribute @e[tag=remote_cow,limit=1] minecraft:follow_range base get'),
        cmd('/attribute @e[tag=remote_cow,limit=1] minecraft:follow_range base reset'),
        cmd('/attribute @e[tag=remote_cow,limit=1] minecraft:scale modifier add harness:xdim 0.5 add_value'),
        cmd('/attribute @e[tag=remote_cow,limit=1] minecraft:scale get'),
        cmd('/attribute @e[tag=remote_cow,limit=1] minecraft:scale modifier remove harness:xdim'),
        cmd('/attribute @e[tag=remote_cow,limit=1] minecraft:scale get'),
        {'op': 'portal'},
        {'op': 'settle'},
        cmd('/execute if dimension minecraft:overworld run say back through the portal'),
        cmd('/data get entity @s Pos'),
        cmd('/data get entity @s Rotation'),
        cmd('/execute if entity @e[type=minecraft:pig]'),
        cmd('/summon minecraft:armor_stand 4.5 210 4.5 {NoGravity:1b,Tags:["lv"],CustomName:"Back"}'),
        *setup('/kill @e[tag=lv]', '/fill 2 231 12 5 236 12 minecraft:air', '/tp @s 8.5 230 8.5 0 0'),
        {'op': 'settle'},
    ]


def end_portal_entities():
    """Non-player exit travel with two ticking levels, and a falling block
    crossing the portal's inside shape on the tick it reaches the floor."""
    end = '/execute in minecraft:the_end run'
    local = 'distance=..1000000'
    return [
        *setup('/forceload add 0 0', '/setblock 8 229 8 minecraft:stone', '/setworldspawn 8 230 8',
               f'{end} tp @s 8.5 230 8.5'),
        {'op': 'settle'},
        *setup(f'{end} forceload add 0 0',
               f'{end} fill 2 209 2 6 209 6 minecraft:stone',
               f'{end} fill 2 210 2 6 215 6 minecraft:air',
               f'{end} setblock 4 210 4 minecraft:end_portal',
               f'{end} summon minecraft:pig 4.5 210.5 4.5 {{NoAI:1b,NoGravity:1b,Tags:["end_portal_mob"]}}'),
        {'op': 'tick', 'n': 10},
        cmd(f'{end} execute if entity @e[tag=end_portal_mob,{local}]'),
        cmd(f'/execute in minecraft:overworld if entity @e[tag=end_portal_mob,{local}]'),
        *setup(f'{end} summon minecraft:falling_block 4.5 213 4.5 {{BlockState:{{id:"minecraft:sand"}},Time:1,Motion:[0.0d,-2.0d,0.0d],Tags:["end_portal_falling"]}}'),
        {'op': 'tick', 'n': 10},
        cmd('/execute in minecraft:overworld if block 8 230 8 minecraft:sand'),
        cmd(f'{end} execute if block 4 210 4 minecraft:end_portal'),
        *setup('/execute in minecraft:overworld run setblock 8 230 8 minecraft:air',
               f'{end} summon minecraft:falling_block 4.5 210.5 4.5 {{BlockState:{{id:"minecraft:gravel"}},Time:1,NoGravity:1b,Tags:["end_portal_float"]}}'),
        {'op': 'tick', 'n': 5},
        {'op': 'falling', 'dimension': 'minecraft:the_end', 'block': 'minecraft:gravel', 'no_gravity': True},
        {'op': 'falling', 'dimension': 'minecraft:overworld', 'block': 'minecraft:gravel', 'no_gravity': True, 'discard': True},
        # FallingBlock.isFree(end_portal) is false: a placed egg above it
        # stays a block in vanilla, despite the portal's empty collision shape.
        *setup(f'{end} setblock 4 211 4 minecraft:dragon_egg'),
        {'op': 'tick', 'n': 5},
        cmd(f'{end} execute if block 4 211 4 minecraft:dragon_egg'),
        *setup('/kill @e[tag=end_portal_mob]',
               f'{end} fill 2 209 2 6 215 6 minecraft:air',
               f'{end} forceload remove 0 0',
               '/execute in minecraft:overworld run fill 8 229 8 8 231 8 minecraft:air',
               '/execute in minecraft:overworld run tp @s 8.5 230 8.5'),
        {'op': 'settle'},
    ]


def loot_set_damage():
    """SetItemDamageFunction in /loot's items engine, including its RNG tail."""
    import json
    sequence = 'harness:loot_damage'
    uniform = {'type': 'minecraft:uniform', 'min': 0.2, 'max': 0.8}
    cases = [('minecraft:iron_sword', {'type': 'minecraft:set_damage', 'damage': n})
             for n in (0, 0.5, -1, 1, 2)]
    cases += [
        ('minecraft:stone', {'type': 'minecraft:set_damage', 'damage': 0.5}),
        ('minecraft:iron_sword', [
            {'type': 'minecraft:set_damage', 'damage': 0.5},
            {'type': 'minecraft:set_damage', 'damage': 0.25, 'add': True}]),
        ('minecraft:iron_sword', {'type': 'minecraft:set_damage', 'damage': uniform}),
        # An ineligible item must leave the provider's random draw untouched.
        ('minecraft:stone', {'type': 'minecraft:set_damage', 'damage': uniform}),
    ]
    ops = setup('/setblock 2 220 2 minecraft:chest')
    # Keep the inline fallback without a named sequence covered as well.
    cases.append(('minecraft:iron_sword', {'type': 'minecraft:set_damage', 'damage': 0.5}))
    for index, (item, modifier) in enumerate(cases):
        table = {'random_sequence': sequence, 'pools': [{'rolls': 1, 'entries': [
            {'type': 'minecraft:item', 'name': item, 'modifier': modifier}]}]}
        if index == len(cases) - 1:
            del table['random_sequence']
        ops += setup(f'/random reset {sequence} 42 false true')
        ops += [
            cmd('/loot replace block 2 220 2 container.0 loot ' + json.dumps(table)),
            cmd('/data get block 2 220 2 Items'),
            cmd(f'/random value 0..1000000 {sequence}'),
        ]
    return ops + setup('/setblock 2 220 2 minecraft:air')


def items_and_loot():
    return [
        *setup('/clear @s', '/gamemode survival'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_count",count:3}'),
        cmd('/item replace entity @s weapon.mainhand with minecraft:stone 10'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_count",count:3}'),
        cmd('/clear @s minecraft:stone 0'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_count",count:5,add:true}'),
        cmd('/clear @s minecraft:stone 0'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:limit_count",limit:{max:4}}'),
        cmd('/clear @s minecraft:stone 0'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_name",name:"Rock"}'),
        cmd('/data get entity @s SelectedItem'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_name",name:{text:"Item",color:"red"},target:"item_name"}'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_lore",lore:["one","two"],mode:"replace_all"}'),
        cmd('/item modify entity @s weapon.mainhand [{type:"minecraft:set_count",count:2},{type:"minecraft:set_custom_data",tag:{a:1b}}]'),
        cmd('/data get entity @s SelectedItem'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_components",components:{"minecraft:max_stack_size":16}}'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:filtered",item_filter:{items:"minecraft:dirt"},on_pass:{type:"minecraft:set_count",count:1}}'),
        cmd('/clear @s minecraft:stone 0'),
        cmd('/item replace entity @s weapon.mainhand with minecraft:diamond_sword'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_damage",damage:0.5}'),
        cmd('/data get entity @s SelectedItem.components'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_enchantments",enchantments:{"minecraft:sharpness":3}}'),
        cmd('/data get entity @s SelectedItem.components."minecraft:enchantments"'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:set_enchantments",enchantments:{"minecraft:sharpness":1},add:true}'),
        cmd('/data get entity @s SelectedItem.components."minecraft:enchantments"'),
        cmd('/item modify entity @s weapon.mainhand {type:"minecraft:toggle_tooltips",toggles:{"minecraft:enchantments":false}}'),
        cmd('/item modify entity @s armor.head {type:"minecraft:set_count",count:1}'),
        cmd('/item modify entity @s weapon.offhand {type:"minecraft:set_count",count:1}'),
        cmd('/item modify entity @s hotbar.* {type:"minecraft:set_count",count:1}'),
        cmd('/item modify entity @s weapon.mainhand minecraft:nonexistent'),
        cmd('/item replace entity @s hotbar.1 from entity @s weapon.mainhand'),
        cmd('/item replace entity @s hotbar.2 from entity @s weapon.mainhand {type:"minecraft:set_count",count:1}'),
        cmd('/clear @s minecraft:diamond_sword 0'),
        cmd('/setblock 3 206 3 minecraft:barrel'),
        cmd('/item replace block 3 206 3 container.4 with minecraft:emerald 7'),
        cmd('/item modify block 3 206 3 container.4 {type:"minecraft:set_count",count:2}'),
        cmd('/item modify block 3 206 3 container.5 {type:"minecraft:set_count",count:2}'),
        cmd('/item modify block 3 206 3 container.99 {type:"minecraft:set_count",count:2}'),
        cmd('/item modify block 3 207 3 container.0 {type:"minecraft:set_count",count:2}'),
        cmd('/data get block 3 206 3 Items'),
        cmd('/data get block 3 206 3'),
        cmd('/item replace entity @s hotbar.3 from block 3 206 3 container.4'),
        cmd('/item replace block 3 206 3 container.0 from entity @s hotbar.3'),
        cmd('/data get block 3 206 3 Items'),
        cmd('/loot give @s loot minecraft:blocks/stone'),
        cmd('/loot give @s loot minecraft:blocks/dirt'),
        cmd('/loot insert 3 206 3 loot minecraft:blocks/oak_planks'),
        cmd('/loot replace block 3 206 3 container.8 loot minecraft:blocks/cobblestone'),
        cmd('/loot replace entity @s hotbar.5 loot minecraft:blocks/glass'),
        cmd('/loot spawn 3 210 3 loot minecraft:blocks/stone'),
        # NestedLootTable: the reward tables pick another table and roll it.
        cmd('/loot spawn 3 210 3 loot minecraft:chests/trial_chambers/reward'),
        cmd('/loot give @s loot minecraft:chests/trial_chambers/reward_ominous'),
        cmd('/loot give @s loot {pools:[{rolls:1,entries:[{type:"minecraft:item",name:"minecraft:apple",functions:[{type:"minecraft:set_count",count:4}]}]}]}'),
        cmd('/setblock 2 205 2 minecraft:stone'),
        cmd('/loot give @s mine 2 205 2'),
        cmd('/loot give @s mine 2 205 2 mainhand'),
        cmd('/loot give @s mine 2 205 2 minecraft:diamond_pickaxe[minecraft:enchantments={"minecraft:silk_touch":1}]'),
        cmd('/data get block 3 206 3 Items'),
        cmd('/clear @s minecraft:cobblestone 0'),
        cmd('/clear @s minecraft:apple 0'),
        cmd('/give @s minecraft:written_book[minecraft:written_book_content={title:"T",author:"A",pages:["p"]}]'),
        cmd('/give @s minecraft:potion[minecraft:potion_contents={potion:"minecraft:swiftness"}] 2'),
        cmd('/give @s minecraft:air'),
        cmd('/give @s minecraft:stone 6401'),
        cmd('/clear @s #minecraft:logs'),
        cmd('/clear @s *[minecraft:custom_data]'),
        cmd('/clear @s'),
        # A chest or hopper minecart's slots (ContainerEntity.getChestVehicleSlot): container.N below its size.
        *setup('/summon minecraft:chest_minecart 4.5 210 4.5 {NoGravity:1b,Tags:["c"]}',
               '/summon minecraft:hopper_minecart 6.5 210 4.5 {NoGravity:1b,Tags:["h"]}'),
        cmd('/item replace entity @e[tag=c,limit=1] container.0 with minecraft:diamond 3'),
        cmd('/item replace entity @e[tag=c,limit=1] container.26 with minecraft:emerald 2'),
        cmd('/item replace entity @e[tag=c,limit=1] container.27 with minecraft:emerald 2'),
        cmd('/item replace entity @e[tag=h,limit=1] container.4 with minecraft:gold_ingot 5'),
        cmd('/item replace entity @e[tag=h,limit=1] container.5 with minecraft:gold_ingot 5'),
        cmd('/item modify entity @e[tag=c,limit=1] container.0 {type:"minecraft:set_count",count:2}'),
        cmd('/item replace entity @s hotbar.0 from entity @e[tag=c,limit=1] container.26'),
        cmd('/data get entity @e[tag=c,limit=1] Items'),
        cmd('/execute if items entity @e[tag=h,limit=1] container.* minecraft:gold_ingot'),
        cmd('/clear @s'),
        *setup('/kill @e[tag=c]', '/kill @e[tag=h]'),
        *setup('/setblock 3 206 3 minecraft:air', '/setblock 2 205 2 minecraft:air', '/kill @e[type=minecraft:item]', '/gamemode creative'),
    ]


def aquatic_from_bucket():
    """Saved bucket flags default false, except the always-true tadpole (#7763)."""
    ops = []
    case = 0
    for kind in ('cod', 'salmon', 'pufferfish', 'tropical_fish', 'axolotl', 'tadpole'):
        for value in (None, '0b', '1b'):
            case += 1
            tag = f'h108_{case}'
            target = f'@e[type=minecraft:{kind},tag={tag},limit=1]'
            bucket = '' if value is None else ',FromBucket:' + value
            ops += setup(f'/summon minecraft:{kind} 10.5 220 10.5 '
                         f'{{Tags:["{tag}"],NoAI:1b,NoGravity:1b,Silent:1b,'
                         f'PersistenceRequired:1b{bucket}}}')
            # Advance an entity tick before reading the saved field and
            # checking that this isolated subject still exists.
            ops += [{'op': 'tick', 'n': 1},
                    cmd(f'/execute if entity {target}'),
                    cmd(f'/data get entity {target} FromBucket')]
            ops += setup(f'/kill {target}')
    return ops




def summoned_fangs_lifetime():
    ops = setup('/fill 0 199 0 15 199 15 stone',
                '/fill 0 200 0 15 206 15 air')
    target = '@e[type=minecraft:evoker_fangs,limit=1]'
    for i, warmup in enumerate((None, 0, 5, -2)):
        extra = '' if warmup is None else f',Warmup:{warmup}'
        ops += [cmd('/summon evoker_fangs 10.5 200 10.5 '
                    f'{{UUID:[I;0,0,1,{32+i}],NoGravity:1b,Silent:1b{extra}}}'),
                cmd(f'/data get entity {target} Pos')]
        previous = 0
        for tick in (0, 1, 7, 22, 23, 27, 28):
            if tick > previous:
                ops += [{'op': 'tick', 'n': tick - previous}]
            # Record both: missing entity and missing Warmup are different
            # command failures and must never be collapsed by the verifier.
            ops += [cmd(f'/execute if entity {target}'),
                    cmd(f'/data get entity {target} Warmup')]
            previous = tick
        ops += setup('/kill @e[type=minecraft:evoker_fangs]')
        ops += [{'op': 'tick', 'n': 1}]
    return ops


def summoned_fangs_edits():
    target = '@e[type=evoker_fangs,tag=fang_control,limit=1]'
    return [
        cmd('/summon evoker_fangs 10.5 200 10.5 '
            '{UUID:[I;0,0,1,99],Tags:["fang_control"],NoGravity:1b,Silent:1b,Warmup:50}'),
        cmd('/execute if entity 00000000-0000-0000-0000-000100000063'),
        cmd(f'/data get entity {target} UUID'),
        cmd(f'/tag {target} add fang_edited'),
        cmd('/execute if entity @e[type=evoker_fangs,tag=fang_edited]'),
        cmd(f'/data modify entity {target} Warmup set value 0'),
        {'op': 'tick', 'n': 1},
        cmd(f'/data get entity {target} Warmup'),
        cmd(f'/data remove entity {target} Warmup'),
        {'op': 'tick', 'n': 1},
        cmd(f'/data get entity {target} Warmup'),
        # Java int predecrement wraps; loading this value must not panic.
        cmd(f'/data modify entity {target} Warmup set value -2147483648'),
        {'op': 'tick', 'n': 1},
        cmd(f'/data get entity {target} Warmup'),
        cmd(f'/kill {target}'),
        cmd('/execute if entity @e[type=evoker_fangs]'),
    ]


def summoned_fangs_levels():
    # Distance limits these selectors to the source level. An already
    # loaded fang must stay in the Overworld across a command-level swap.
    box = 'x=10,y=200,z=10,distance=..3'
    return [
        cmd('/summon evoker_fangs 10.5 200 10.5 {Tags:["fang_overworld"],Warmup:1000,Silent:1b}'),
        cmd(f'/execute in minecraft:the_nether if entity @e[tag=fang_overworld,{box}]'),
        cmd(f'/execute in minecraft:overworld if entity @e[tag=fang_overworld,{box}]'),
        *setup('/kill @e[type=evoker_fangs]'),
    ]


def entity_landing_sensors():
    """Entity.checkFallDamage -> real Game dispatch -> a fresh sculk listener."""
    ops = setup('/gamerule random_tick_speed 0', '/gamerule entity_drops false',
                '/tp @s 6.5 200 7.5', '/forceload add 0 0 31 31',
                '/scoreboard objectives add landing_probe dummy')
    ops += [{'op': 'settle'}]
    ops += [cmd(f'/execute if loaded {x} 200 {z}', expect_result=1)
            for x, z in ((0, 0), (16, 0), (0, 16), (16, 16))]
    ops += setup('/tick freeze')
    for kind, landing_tick in (('item', 13), ('armor_stand', 10)):
        for no_gravity in (0, 1):
            tag = f'landing_{kind}_{no_gravity}'
            target = f'@e[tag={tag},limit=1]'
            ops += setup('/setblock 9 200 10 air strict', '/kill @e[tag=landing_actor]',
                         '/tick step 25')
            ops += [{'op': 'tick', 'n': 25}]
            ops += setup('/fill 0 200 0 31 206 31 air',
                         '/fill 0 199 0 31 199 31 stone',
                         '/tp @s 6.5 200 7.5',
                         '/setblock 9 200 10 sculk_sensor strict', '/tick step 2')
            ops += [{'op': 'tick', 'n': 2}]
            ops += [cmd('/execute if block 9 200 10 sculk_sensor[sculk_sensor_phase=inactive,power=0]', expect_result=1),
                    cmd('/data get block 9 200 10 last_vibration_frequency', expect_result=0),
                    cmd('/execute unless data block 9 200 10 listener.event', expect_result=1),
                    cmd('/execute unless data block 9 200 10 listener.selector.event', expect_result=1),
                    cmd('/execute if entity @s[nbt={Pos:[6.5d,200d,7.5d]}]', expect_result=1)]
            item = ',Item:{id:"minecraft:stone",count:1},PickupDelay:32767' if kind == 'item' else ''
            ops += setup(f'/summon {kind} 6.5 203 10.5 '
                         f'{{Tags:["landing_actor","{tag}"],NoGravity:{no_gravity}b{item}}}')
            ops += [cmd(f'/execute if entity {target}', expect_result=1),
                    cmd(f'/data get entity {target} Pos'),
                    cmd(f'/execute unless entity {target[:-1]},nbt={{OnGround:1b}}]', expect_result=1)]
            for tick in range(1, 21):
                ground = not no_gravity and tick >= landing_tick
                active = not no_gravity and tick >= landing_tick + 3
                phase, power = ('active', 10) if active else ('inactive', 0)
                sign = 'if' if ground else 'unless'
                ops += setup('/tick step 1')
                ops += [{'op': 'tick', 'n': 1}]
                # Flight height at milliblock precision: landing timing and
                # final coordinates are exact controls; float-drag parity is separate.
                ops += setup(f'/execute store result score landing_y landing_probe run data get entity {target} Pos[1] 1000')
                ops += [cmd(f'/execute if entity {target}', expect_result=1),
                        cmd('/scoreboard players get landing_y landing_probe'),
                        cmd(f'/execute {sign} entity {target[:-1]},nbt={{OnGround:1b}}]', expect_result=1),
                        cmd(f'/execute if block 9 200 10 sculk_sensor[sculk_sensor_phase={phase},power={power}]', expect_result=1)]
            y = 203 if no_gravity else 200
            ops += [cmd(f'/execute if entity {target[:-1]},nbt={{Pos:[6.5d,{y}d,10.5d]}}]', expect_result=1),
                    cmd('/data get block 9 200 10 last_vibration_frequency', expect_result=0 if no_gravity else 2)]
    return ops + setup('/setblock 9 200 10 air strict', '/kill @e[tag=landing_actor]',
                       '/scoreboard objectives remove landing_probe',
                       '/tick unfreeze', '/gamerule random_tick_speed 3')


def bird_flap_sensors():
    # Real mob movement -> Game event dispatch -> sculk sensor. Silent birds
    # still emit vibrations; NoAI controls must keep the same positions too.
    ops = setup('/gamerule random_tick_speed 0', '/tp @s 1.5 200 1.5')
    target = '@e[tag=flap_bird,limit=1]'
    for species in ('chicken', 'parrot'):
        for no_ai in (0, 1):
            ops += setup('/kill @e[tag=flap_bird]',
                         '/fill 0 200 0 15 206 15 air',
                         '/fill 0 199 0 15 199 15 stone')
            ops += [{'op': 'tick', 'n': 45}]
            ops += setup('/setblock 9 200 10 sculk_sensor strict')
            ops += [{'op': 'tick', 'n': 2}]
            ops += [cmd('/execute if block 9 200 10 sculk_sensor[sculk_sensor_phase=inactive,power=0]')]
            ops += setup(f'/summon {species} 6.5 205 10.5 '
                         f'{{Tags:["flap_bird"],NoAI:{no_ai}b,Silent:1b,NoGravity:1b,'
                         'Motion:[0d,-1d,0d],attributes:['
                         '{id:"minecraft:movement_speed",base:0d},'
                         '{id:"minecraft:flying_speed",base:0d}]}')
            ops += [cmd(f'/execute if entity {target}'),
                    cmd(f'/data get entity {target} Pos')]
            for _ in range(30):
                ops += [{'op': 'tick', 'n': 1},
                        cmd(f'/data get entity {target} Pos'),
                        cmd('/execute if block 9 200 10 sculk_sensor[sculk_sensor_phase=active,power=8]')]
            ops += [cmd('/data get block 9 200 10 last_vibration_frequency')]
    return ops + setup('/kill @e[tag=flap_bird]', '/setblock 9 200 10 air strict',
                       '/gamerule random_tick_speed 3')


def item_nbt_count_bounds():
    """ItemStack's NBT count codec defaults invalid counts to one (#7724)."""
    stand = '@e[type=minecraft:armor_stand,tag=h085,limit=1]'
    chest = '12 220 4'
    ops = setup('/tick freeze', '/kill @e[tag=h085]',
                f'/setblock {chest} minecraft:air strict',
                f'/setblock {chest} minecraft:chest strict',
                '/summon minecraft:armor_stand 8 220 4 {Tags:["h085"],NoGravity:1b}')
    ops += [cmd(f'/execute if entity {stand}'),
            cmd(f'/execute if block {chest} minecraft:chest')]
    for item in ('minecraft:stone', 'minecraft:diamond_sword'):
        for count in (None, '-1', '0', '1', '2', '64', '99', '100', '255', '256',
                      '2147483647', '1b', '1.5f', '"1"'):
            nbt = '{id:"' + item + '"' + ('' if count is None else ',count:' + count) + '}'
            ops += setup(f'/item replace entity {stand} weapon.mainhand with minecraft:air')
            ops += [
                cmd(f'/data modify entity {stand} equipment.mainhand set value {nbt}'),
                cmd(f'/data get entity {stand} equipment.mainhand.id'),
                cmd(f'/data get entity {stand} equipment.mainhand.count'),
            ]
            entry = '{Slot:0b,' + nbt[1:]
            ops += setup(f'/data modify block {chest} Items set value []')
            ops += [
                cmd(f'/data modify block {chest} Items set value [{entry}]'),
                cmd(f'/data get block {chest} Items[0].id'),
                cmd(f'/data get block {chest} Items[0].count'),
            ]
    return ops + setup('/kill @e[tag=h085]', f'/setblock {chest} minecraft:air strict',
                       '/tick unfreeze')


def item_block_containers():
    at = '12 222 8'
    chest = '12 222 10'
    ops = setup('/tick freeze', f'/setblock {chest} minecraft:air strict', f'/setblock {chest} minecraft:chest strict')
    for block, size in (('minecraft:furnace', 3), ('minecraft:blast_furnace', 3), ('minecraft:smoker', 3),
                        ('minecraft:brewing_stand', 5), ('minecraft:crafter', 9),
                        ('minecraft:chiseled_bookshelf', 6), ('minecraft:oak_shelf', 3), ('minecraft:jukebox', 1)):
        ops += setup(f'/setblock {at} minecraft:air strict', f'/setblock {at} {block} strict')
        ops += [
            cmd(f'/item replace block {at} container.0 with minecraft:book 2'),
            cmd(f'/item replace block {at} container.{size - 1} with minecraft:enchanted_book'),
            cmd(f'/item replace block {at} container.{size} with minecraft:book'),
            cmd(f'/execute if items block {at} container.* minecraft:book'),
            cmd(f'/execute if items block {at} container.* *'),
            cmd(f'/item modify block {at} container.0 {{type:"minecraft:set_count",count:1}}'),
            cmd(f'/item replace block {chest} container.0 from block {at} container.0'),
            cmd(f'/data get block {chest} Items[0]'),
            cmd(f'/data get block {at} Items'),
            cmd(f'/item replace block {at} container.0 with minecraft:air'),
            cmd(f'/execute if items block {at} container.* *'),
        ]
    for block in ('minecraft:furnace', 'minecraft:blast_furnace', 'minecraft:smoker'):
        ops += setup(f'/setblock {at} minecraft:air strict', f'/setblock {at} {block} strict')
        ops += [
            cmd(f'/item replace block {at} container.0 with minecraft:iron_ore'),
            cmd(f'/data get block {at} cooking_total_time'),
        ]
        ops += setup(f'/data merge block {at} {{cooking_time_spent:50}}')
        ops += [
            cmd(f'/item replace block {at} container.0 with minecraft:iron_ore 3'),
            cmd(f'/data get block {at} cooking_time_spent'),
            cmd(f'/item replace block {at} container.1 with minecraft:coal'),
            cmd(f'/data get block {at} cooking_time_spent'),
            cmd(f'/item replace block {at} container.0 with minecraft:beef'),
            cmd(f'/data get block {at} cooking_time_spent'),
            cmd(f'/data get block {at} cooking_total_time'),
        ]
    ops += setup(f'/setblock {at} minecraft:air strict', f'/setblock {at} minecraft:crafter strict',
                 f'/data merge block {at} {{disabled_slots:[I;0,4]}}')
    ops += [
        cmd(f'/item replace block {at} container.0 with minecraft:stone'),
        cmd(f'/data get block {at} disabled_slots'),
        cmd(f'/item replace block {at} container.4 with minecraft:air'),
        cmd(f'/data get block {at} disabled_slots'),
    ]
    ops += setup(f'/setblock {at} minecraft:air strict', f'/setblock {at} minecraft:chiseled_bookshelf strict')
    ops += [
        cmd(f'/item replace block {at} container.2 with minecraft:stone'),
        cmd(f'/execute if items block {at} container.* *'),
        cmd(f'/item replace block {at} container.2 with minecraft:writable_book'),
        cmd(f'/execute if block {at} minecraft:chiseled_bookshelf[slot_2_occupied=true]'),
        cmd(f'/data get block {at} last_interacted_slot'),
        cmd(f'/item replace block {at} container.2 with minecraft:air'),
        cmd(f'/execute if block {at} minecraft:chiseled_bookshelf[slot_2_occupied=false]'),
    ]
    ops += setup(f'/setblock {at} minecraft:air strict', f'/setblock {at} minecraft:jukebox strict')
    ops += [
        cmd(f'/item replace block {at} container.0 with minecraft:music_disc_cat'),
        cmd(f'/execute if block {at} minecraft:jukebox[has_record=true]'),
        cmd(f'/data get block {at} RecordItem'),
        cmd(f'/item replace block {at} container.0 with minecraft:stone'),
        cmd(f'/execute if block {at} minecraft:jukebox[has_record=true]'),
        cmd(f'/item replace block {at} container.0 with minecraft:air'),
        cmd(f'/execute if block {at} minecraft:jukebox[has_record=false]'),
    ]
    ops += setup(f'/setblock {at} minecraft:air strict', f'/setblock {at} minecraft:furnace strict')
    ops += [
        cmd(f'/loot insert {at} loot minecraft:blocks/cobblestone'),
        cmd(f'/loot insert {at} loot minecraft:blocks/coal_block'),
        cmd(f'/loot insert {at} loot minecraft:blocks/stone'),
        cmd(f'/loot insert {at} loot minecraft:blocks/oak_planks'),
        cmd(f'/data get block {at} Items'),
        cmd(f'/loot replace block {at} container.2 loot minecraft:blocks/dirt'),
        cmd(f'/loot replace block {at} container.1 loot minecraft:blocks/dirt'),
        cmd(f'/loot replace block {at} container.0 loot minecraft:blocks/dirt'),
        cmd(f'/data get block {at} Items'),
    ]
    ops += setup(f'/setblock {at} minecraft:air strict', f'/setblock {at} minecraft:chiseled_bookshelf strict')
    ops += [
        cmd(f'/loot insert {at} loot minecraft:blocks/dirt'),
        cmd(f'/loot replace block {at} container.3 loot minecraft:blocks/dirt'),
        cmd(f'/execute if items block {at} container.* *'),
    ]
    for block in ('minecraft:oak_sign', 'minecraft:lectern', 'minecraft:campfire', 'minecraft:beacon',
                  'minecraft:stone'):
        ops += setup(f'/setblock {at} minecraft:air strict', f'/setblock {at} {block} strict')
        ops += [
            cmd(f'/loot insert {at} loot minecraft:blocks/dirt'),
            cmd(f'/item replace block {at} container.0 with minecraft:book'),
            cmd(f'/item modify block {at} container.0 {{type:"minecraft:set_count",count:1}}'),
            cmd(f'/item replace block {chest} container.0 from block {at} container.0'),
            cmd(f'/execute if items block {at} container.* *'),
        ]
    return ops + setup(f'/setblock {at} minecraft:air strict', f'/setblock {chest} minecraft:air strict',
                       '/tick unfreeze')


def world_misc():
    return [
        # BlockInput.place calculates neighbour shapes before setting the block,
        # then reapplies only properties explicitly supplied by the command.
        cmd('/setblock 4 219 4 minecraft:stone'),
        cmd('/setblock 4 220 4 minecraft:note_block'),
        cmd('/execute if block 4 220 4 minecraft:note_block[instrument=basedrum]'),
        cmd('/execute if block 4 220 4 minecraft:note_block[instrument=harp]'),
        cmd('/setblock 4 219 4 minecraft:gold_block'),
        cmd('/execute if block 4 220 4 minecraft:note_block[instrument=bell]'),
        cmd('/setblock 4 219 4 minecraft:stone'),
        cmd('/execute if block 4 220 4 minecraft:note_block[instrument=basedrum]'),
        cmd('/setblock 4 220 4 minecraft:air'),
        cmd('/setblock 4 220 4 minecraft:note_block[instrument=flute,note=7]'),
        cmd('/execute if block 4 220 4 minecraft:note_block[instrument=flute,note=7]'),
        cmd('/setblock 4 220 4 minecraft:air'),
        cmd('/setblock 4 220 4 minecraft:note_block[note=7]'),
        cmd('/execute if block 4 220 4 minecraft:note_block[instrument=basedrum,note=7]'),
        cmd('/setblock 4 220 4 minecraft:air'),
        cmd('/setblock 4 220 4 minecraft:note_block strict'),
        cmd('/execute if block 4 220 4 minecraft:note_block[instrument=harp]'),
        # Even an unchanged input state must be normalized before comparison.
        cmd('/setblock 4 220 4 minecraft:note_block'),
        cmd('/execute if block 4 220 4 minecraft:note_block[instrument=basedrum]'),
        # An unsupported shape falling back to air retains the requested block.
        cmd('/setblock 6 220 4 minecraft:poppy'),
        cmd('/execute if block 6 220 4 minecraft:poppy'),
        cmd('/setblock 1 210 1 minecraft:stone keep'),
        cmd('/setblock 1 210 1 minecraft:dirt keep'),
        cmd('/setblock 1 210 1 minecraft:dirt destroy'),
        cmd('/setblock 1 210 1 minecraft:sand strict'),
        cmd('/setblock 1 210 1 minecraft:air replace'),
        cmd('/fill 0 210 0 2 212 2 minecraft:stone outline'),
        cmd('/fill 0 210 0 2 212 2 minecraft:glass keep'),
        cmd('/fill 0 210 0 2 212 2 minecraft:dirt replace #minecraft:base_stone_overworld'),
        cmd('/fill 0 210 0 2 212 2 minecraft:air strict'),
        # A connected server loads more terrain than the oracle's 5x5 core.
        # Keep the far corner unambiguously unloaded in both executions.
        cmd('/fill 0 210 0 1000 300 1000 minecraft:stone'),
        cmd('/setblock 0 211 0 minecraft:oak_sign{front_text:{messages:["a","b","c","d"]}}'),
        cmd('/data get block 0 211 0 front_text.messages'),
        cmd('/clone 0 211 0 0 211 0 1 211 1 replace move'),
        cmd('/clone 1 211 1 1 211 1 0 211 0 filtered minecraft:oak_sign'),
        cmd('/clone 0 211 0 0 211 0 0 211 0'),
        cmd('/clone 0 211 0 0 211 0 0 212 0 replace force'),
        cmd('/clone from minecraft:overworld 0 211 0 0 211 0 to minecraft:overworld 2 211 2'),
        cmd('/fill 0 210 0 2 212 2 minecraft:air'),
        cmd('/fillbiome 0 200 0 15 215 15 minecraft:desert'),
        cmd('/execute if biome 4 205 4 minecraft:desert'),
        cmd('/fillbiome 0 200 0 15 215 15 minecraft:plains replace minecraft:desert'),
        cmd('/fillbiome 0 200 0 15 215 15 minecraft:plains replace minecraft:desert'),
        cmd('/fillbiome 0 0 0 1000 300 1000 minecraft:plains'),
        cmd('/forceload query'),
        cmd('/forceload add 0 0'),
        cmd('/forceload add 0 0'),
        cmd('/forceload query 0 0'),
        cmd('/forceload query 40 40'),
        cmd('/forceload add 32 32 47 47'),
        cmd('/forceload query'),
        cmd('/forceload remove 32 32'),
        cmd('/forceload remove all'),
        cmd('/forceload query'),
        cmd('/forceload add 0 0 1000 1000'),
        cmd('/particle minecraft:flame 4 210 4'),
        cmd('/particle minecraft:flame 4 300 4'),
        cmd('/particle minecraft:flame 4 300 4 0 0 0 0 1 force'),
        cmd('/particle minecraft:flame 4 300 4 0 0 0 0 1 force @e[type=minecraft:pig]'),
        cmd('/particle minecraft:dust{color:[1.0,0.0,0.0],scale:1.0} 4 210 4 0 0 0 0 5 force @s'),
        cmd('/particle minecraft:block{block_state:"minecraft:stone"} 4 210 4 1 1 1 0.1 10 normal @a'),
        cmd('/playsound minecraft:block.note_block.harp master @s'),
        cmd('/playsound minecraft:block.note_block.harp record @s 4 210 4 1 2 0'),
        cmd('/playsound minecraft:block.note_block.harp master @a 1000 210 1000'),
        cmd('/stopsound @s'),
        cmd('/stopsound @s master'),
        cmd('/stopsound @s * minecraft:block.note_block.harp'),
        cmd('/stopsound @s music minecraft:music.game'),
        cmd('/swing @s'),
        cmd('/swing @s offhand'),
        cmd('/swing @e[type=minecraft:pig]'),
        cmd('/summon minecraft:armor_stand 4.5 210 4.5 {NoGravity:1b,Tags:["w"],CustomName:{text:"Stand",color:"blue"}}'),
        cmd('/swing @e[tag=w]'),
        cmd('/summon minecraft:lightning_bolt 4.5 300 4.5'),
        # Primed TNT is no command target in Horizon: still "Summoned new
        # Primed TNT" (it explodes high in the air, far from the rest).
        cmd('/summon minecraft:tnt 60.5 300 60.5 {fuse:1s}'),
        cmd('/summon minecraft:pig 4.5 210 4.5 {NoAI:1b,Invulnerable:1b,Tags:["w"],Passengers:[{id:"minecraft:chicken",NoAI:1b,Tags:["w"]}]}'),
        cmd('/summon minecraft:pig 4.5 400 4.5'),
        cmd('/summon minecraft:pig 30000000 210 4.5'),
        cmd('/summon minecraft:player'),
        cmd('/summon minecraft:marker 4.5 210 4.5 {Tags:["w","mk"]}'),
        cmd('/gamemode spectator'),
        cmd('/spectate @e[tag=mk,limit=1]'),
        cmd('/spectate @e[type=minecraft:armor_stand,tag=w,limit=1]'),
        cmd('/spectate @e[type=minecraft:pig,tag=w,limit=1] @s'),
        cmd('/spectate @e[type=minecraft:chicken,tag=w,limit=1] Player'),
        cmd('/spectate'),
        cmd('/spectate @e[tag=nothing,limit=1]'),
        cmd('/gamemode creative'),
        # The waypoint manager is a HashSet of entities (identity hash order),
        # so at most one transmitter at a time. Range changes take effect when
        # the entity ticks.
        cmd('/attribute @s minecraft:waypoint_transmit_range base set 0'),
        cmd('/waypoint list'),
        {'op': 'tick', 'n': 1},
        cmd('/waypoint list'),
        cmd('/attribute @e[type=minecraft:pig,tag=w,limit=1] minecraft:waypoint_transmit_range base set 100'),
        cmd('/attribute @e[type=minecraft:pig,tag=w,limit=1] minecraft:waypoint_transmit_range get'),
        cmd('/waypoint list'),
        {'op': 'tick', 'n': 1},
        cmd('/waypoint list'),
        cmd('/waypoint modify @e[type=minecraft:pig,tag=w,limit=1] color gold'),
        cmd('/waypoint modify @e[tag=mk,limit=1] color gold'),
        cmd('/waypoint list'),
        cmd('/waypoint modify @e[tag=w] color gold'),
        cmd('/attribute @e[type=minecraft:armor_stand,tag=w,limit=1] minecraft:max_health get'),
        cmd('/attribute @e[type=minecraft:armor_stand,tag=w,limit=1] minecraft:scale modifier add harness:big 1 add_multiplied_base'),
        cmd('/attribute @e[type=minecraft:armor_stand,tag=w,limit=1] minecraft:scale get'),
        cmd('/attribute @e[type=minecraft:armor_stand,tag=w,limit=1] minecraft:scale modifier add harness:big 1 add_value'),
        cmd('/attribute @e[type=minecraft:armor_stand,tag=w,limit=1] minecraft:scale modifier value get harness:big'),
        cmd('/attribute @e[type=minecraft:armor_stand,tag=w,limit=1] minecraft:scale modifier remove harness:big'),
        cmd('/attribute @e[type=minecraft:armor_stand,tag=w,limit=1] minecraft:scale modifier remove harness:big'),
        cmd('/attribute @e[type=minecraft:armor_stand,tag=w,limit=1] minecraft:attack_damage get'),
        cmd('/attribute @e[tag=mk,limit=1] minecraft:scale get'),
        cmd('/attribute @e[type=minecraft:pig,tag=w,limit=1] minecraft:waypoint_transmit_range base set 0'),
        {'op': 'tick', 'n': 1},
        cmd('/waypoint list'),
        cmd('/waypoint modify @e[type=minecraft:pig,tag=w,limit=1] style set minecraft:bowtie'),
        cmd('/waypoint list'),
        {'op': 'tick', 'n': 1},
        cmd('/waypoint list'),
        cmd('/kill @e[type=minecraft:pig,tag=w]'),
        {'op': 'tick', 'n': 1},
        cmd('/waypoint list'),
        {'op': 'tick', 'n': 25},
        cmd('/waypoint list'),
        cmd('/attribute @s minecraft:waypoint_transmit_range base reset'),
        {'op': 'tick', 'n': 1},
        cmd('/waypoint list'),
        cmd('/dialog show @s minecraft:custom_options'),
        cmd('/dialog show @s minecraft:nonexistent'),
        cmd('/dialog show @s {type:"minecraft:confirmation",title:"Sure?",yes:{label:"Yes"},no:{label:"No"}}'),
        cmd('/dialog show @s {type:"minecraft:notice"}'),
        cmd('/dialog show @e[tag=w] minecraft:server_links'),
        cmd('/dialog clear @e[tag=w]'),
        cmd('/dialog clear @s'),
        cmd('/spreadplayers 8 8 0 1 false @s'),
        cmd('/spreadplayers 8 8 5 1 false @s'),
        cmd('/spreadplayers 8 8 0 1 under 100 false @s'),
        cmd('/kill @e[tag=w]'),
        cmd('/tp @s 8.5 230 8.5 0 0'),
        cmd('/teleport @s 8.5 230 8.5 facing 0 230 0'),
        cmd('/teleport @s 8.5 230 8.5 facing entity @s'),
        cmd('/tp @s 30000000 230 0'),
        cmd('/tp 8.5 230 8.5'),
        cmd('/rotate @s facing 0 230 0'),
        cmd('/rotate @s ~10 ~'),
        cmd('/tp @s 8.5 230 8.5 0 0'),
        cmd('/locate biome minecraft:plains'),
        cmd('/locate biome #minecraft:is_ocean'),
        cmd('/locate structure minecraft:village_plains'),
        cmd('/locate structure #minecraft:village'),
        cmd('/locate structure minecraft:end_city'),
        cmd('/locate poi minecraft:meeting'),
        cmd('/place template minecraft:nonexistent 4 210 4'),
        cmd('/place feature minecraft:oak 4 210 4'),
        cmd('/place jigsaw minecraft:village/plains/town_centers minecraft:bottom 1 4 210 4'),
        cmd('/place structure minecraft:nonexistent'),
        *setup('/kill @e[type=!minecraft:player]', '/fill 0 200 0 15 228 15 air', '/forceload remove all', '/tp @s 8.5 230 8.5 0 0'),
        {'op': 'tick', 'n': 25},
    ]


def progression():
    return [
        *setup('/gamemode survival', '/experience set @s 0 levels', '/experience set @s 0 points'),
        cmd('/advancement grant @s only minecraft:story/mine_stone'),
        cmd('/advancement grant @s only minecraft:story/mine_stone'),
        cmd('/advancement revoke @s only minecraft:story/mine_stone'),
        cmd('/advancement revoke @s only minecraft:story/mine_stone'),
        cmd('/advancement grant @s only minecraft:story/upgrade_tools minecraft:stone_pickaxe'),
        cmd('/advancement grant @s only minecraft:story/upgrade_tools minecraft:stone_pickaxe'),
        cmd('/advancement grant @s only minecraft:story/upgrade_tools minecraft:nonexistent'),
        cmd('/advancement revoke @s only minecraft:story/upgrade_tools minecraft:stone_pickaxe'),
        cmd('/advancement grant @s until minecraft:story/smelt_iron'),
        cmd('/advancement grant @s from minecraft:story/root'),
        cmd('/advancement revoke @s through minecraft:story/iron_tools'),
        cmd('/advancement grant @s everything'),
        cmd('/advancement grant @s everything'),
        cmd('/experience query @s levels'),
        cmd('/experience query @s points'),
        cmd('/advancement revoke @s everything'),
        cmd('/advancement revoke @s everything'),
        cmd('/advancement grant @s only minecraft:nonexistent'),
        # The advancement subtree a `from` grants, one criterion of an
        # advancement that needs hundreds (`adventuring_time` wants every
        # biome), and a `through` that keeps what is below the named one.
        cmd('/advancement revoke @s everything'),
        cmd('/advancement grant @s from minecraft:story/mine_stone'),
        cmd('/advancement grant @s only minecraft:adventure/adventuring_time minecraft:plains'),
        cmd('/advancement grant @s only minecraft:adventure/adventuring_time'),
        cmd('/advancement revoke @s only minecraft:adventure/adventuring_time minecraft:plains'),
        cmd('/advancement grant @s through minecraft:story/upgrade_tools'),
        cmd('/advancement grant @s until minecraft:story/mine_stone'),
        cmd('/advancement revoke @s everything'),
        cmd('/advancement grant @e[type=!minecraft:player] everything'),
        cmd('/experience query @s levels'),
        cmd('/recipe give @s minecraft:oak_planks'),
        cmd('/recipe give @s minecraft:oak_planks'),
        cmd('/recipe take @s minecraft:oak_planks'),
        cmd('/recipe take @s minecraft:oak_planks'),
        cmd('/recipe give @s *'),
        cmd('/recipe give @s *'),
        cmd('/recipe take @s *'),
        cmd('/recipe take @s *'),
        cmd('/experience add @s -5 levels'),
        cmd('/experience add @s 1000'),
        cmd('/experience query @s levels'),
        cmd('/experience set @s 100 points'),
        cmd('/experience set @s 1000 points'),
        cmd('/experience add @e[type=!minecraft:player] 1'),
        *setup('/gamemode creative'),
    ]


def advancement_reward_fixtures(group_names):
    if 'advancement reward functions' not in group_names:
        return {}
    def advancement(function=None):
        value = {'criteria': {'never': {'trigger': 'minecraft:impossible'}}}
        if function is not None:
            value['rewards'] = {'function': function}
        return value
    return {'h291:reward': advancement('h291:reward'),
            'h291:no_function': advancement(),
            'h291:missing_function': advancement('h291:missing'),
            'h291:alias': advancement('h291_alias')}


def function_return_fixtures(group_names):
    functions = {}
    if 'function return feedback' in group_names:
        functions.update({'parity:fail': 'return fail\n', 'parity:zero': 'return 0\n',
                          'parity:value': 'return 7\n', 'parity:empty': '# no commands\n',
                          'parity:add': 'scoreboard players add #count audit 1\n'})
    if 'advancement reward functions' in group_names:
        # Explicit return must not replace the enclosing advancement callback.
        functions.update({'h291:reward': 'scoreboard players add @s reward_hits 1\n'
                          'tag @s add h291_rewarded\n'
                          'execute if block ~ ~-1 ~ stone run scoreboard players add @s reward_origin 1\n'
                          'function h291:nested\nreturn 9\n',
                          'h291:nested': 'scoreboard players add @s reward_nested 1\n',
                          'minecraft:h291_alias': 'scoreboard players add @s reward_alias 1\n'})
    return functions


def function_return_feedback():
    # No ticks between calls: distinguish failure from a successful zero,
    # and both explicit returns from functions that leave callbacks untouched.
    lines = [cmd('/datapack list enabled'),
             cmd('/scoreboard objectives add audit dummy', expect_result=1),
             cmd('/scoreboard players set #count audit 0', expect_result=0),
             cmd('/execute store result score #clock audit run time query gametime', False)]
    count = 0
    for name, result, success in [('fail', 0, 0), ('zero', 0, 1),
                                  ('value', 7, 1), ('empty', 31, 17),
                                  ('add', 31, 17)]:
        lines.append(cmd(f'/function parity:{name}'))
        count += name == 'add'
        lines.extend([
            cmd(f'/execute if score #count audit matches {count}', expect_result=1),
            cmd('/scoreboard players set #r audit 31', expect_result=31),
            cmd('/scoreboard players set #s audit 17', expect_result=17),
            cmd(f'/execute store result score #r audit store success score #s audit run function parity:{name}'),
            cmd('/scoreboard players get #r audit', expect_result=result),
            cmd('/scoreboard players get #s audit', expect_result=success),
            cmd(f'/execute if score #r audit matches {result}', expect_result=1),
            cmd(f'/execute if score #s audit matches {success}', expect_result=1),
        ])
        count += name == 'add'
        lines.append(cmd(f'/execute if score #count audit matches {count}', expect_result=1))
    lines.extend([cmd('/execute store result score #after audit run time query gametime', False),
                  cmd('/execute if score #clock audit = #after audit', expect_result=1),
                  cmd('/scoreboard objectives remove audit')])
    return lines


def functions_and_packs():
    return [
        cmd('/function minecraft:nonexistent'),
        cmd('/function #minecraft:load'),
        cmd('/function #minecraft:tick'),
        cmd('/debug function minecraft:nonexistent'),
        cmd('/schedule function minecraft:nonexistent 10'),
        cmd('/schedule clear minecraft:nonexistent'),
        cmd('/datapack list'),
        cmd('/datapack list available'),
        cmd('/datapack list enabled'),
        cmd('/datapack enable vanilla'),
        # Disabling vanilla reloads without its data; enable it again before
        # anything observes the empty registries.
        cmd('/datapack disable vanilla'),
        {'op': 'tick', 'n': 20},
        cmd('/datapack list enabled'),
        cmd('/datapack enable vanilla'),
        {'op': 'tick', 'n': 20},
        cmd('/datapack list'),
        cmd('/datapack enable "file/missing"'),
        cmd('/datapack enable trade_rebalance'),
        cmd('/datapack disable trade_rebalance'),
        cmd('/reload'),
        {'op': 'tick', 'n': 5, 'record': True},
    ]


def ticking():
    return [
        # Isolate named-stream commands from earlier loot rolls, including
        # ocean vegetation updated while asynchronous terrain loads settle.
        *setup('/random reset *'),
        cmd('/tick query'),
        cmd('/tick rate 40'),
        cmd('/tick query'),
        cmd('/tick rate 20'),
        cmd('/tick freeze'),
        cmd('/tick query'),
        cmd('/tick freeze'),
        cmd('/tick step'),
        cmd('/tick step 5'),
        cmd('/tick step stop'),
        cmd('/tick step stop'),
        cmd('/tick unfreeze'),
        cmd('/tick unfreeze'),
        cmd('/tick step'),
        cmd('/tick sprint stop'),
        cmd('/tick rate 1'),
        cmd('/tick rate 20'),
        cmd('/stopwatch create harness:t'),
        cmd('/stopwatch restart harness:t'),
        cmd('/stopwatch restart harness:missing'),
        cmd('/stopwatch remove harness:t'),
        cmd('/stopwatch remove harness:t'),
        cmd('/stopwatch remove harness:a'),
        cmd('/random value 1..100 harness:seq'),
        cmd('/random value 1..100 harness:seq'),
        cmd('/random roll 1..6 harness:dice'),
        cmd('/random reset harness:seq'),
        cmd('/random value 1..100 harness:seq'),
        cmd('/random reset harness:seq 42'),
        cmd('/random value 1..100 harness:seq'),
        cmd('/random reset harness:seq 42 false false'),
        cmd('/random value 1..100 harness:seq'),
        cmd('/random reset *'),
        cmd('/random value 1..100 harness:seq'),
        cmd('/random roll 1..2147483647 harness:big'),
        cmd('/random value -2147483648..2147483647 harness:big'),
        cmd('/time query day'),
        cmd('/time set 0'),
        cmd('/time set day'),
        cmd('/time set midnight'),
        cmd('/time set sunset'),
        cmd('/time add -500'),
        cmd('/time set 1000'),
    ]


def compute():
    return [
        cmd('/compute default integer 5'),
        cmd('/compute default integer {type:"minecraft:add",left:2,right:3}'),
        cmd('/compute default integer {type:"minecraft:add",inputs:[2,3,4]}'),
        cmd('/compute default integer {type:"minecraft:add",inputs:[2147483647,1]}'),
        cmd('/compute default integer {type:"minecraft:mul",inputs:[6,7]}'),
        cmd('/compute default integer {type:"minecraft:avg",inputs:[1,2,4]}'),
        cmd('/compute default integer {type:"minecraft:div",left:7,right:2}'),
        cmd('/compute default integer {type:"minecraft:div",left:7,right:0}'),
        cmd('/compute default integer {type:"minecraft:mod",left:-7,right:3}'),
        cmd('/compute default integer {type:"minecraft:floor_mod",left:-7,right:3}'),
        cmd('/compute default integer {type:"minecraft:floor_div",left:-7,right:2}'),
        cmd('/compute default integer {type:"minecraft:sub",left:-2147483648,right:1}'),
        cmd('/compute default integer {type:"minecraft:pow",left:2,right:10}'),
        cmd('/compute default integer {type:"minecraft:pow",base:2,exponent:10}'),
        cmd('/compute default integer {type:"minecraft:pow",base:0,exponent:0}'),
        cmd('/compute default integer {type:"minecraft:pow",base:2,exponent:40}'),
        cmd('/compute default integer {type:"minecraft:max",values:[1,9,4]}'),
        cmd('/compute default integer {type:"minecraft:max",inputs:[1,9,4]}'),
        cmd('/compute default integer {type:"minecraft:min",inputs:[1,9,4]}'),
        cmd('/compute default integer {type:"minecraft:abs",input:-4}'),
        cmd('/compute default integer {type:"minecraft:abs",input:-2147483648}'),
        cmd('/compute default integer {type:"minecraft:negate",input:4}'),
        cmd('/compute default integer {type:"minecraft:from_float",input:2.7}'),
        cmd('/compute default integer {type:"minecraft:from_float",input:-2.7}'),
        cmd('/compute default integer {type:"minecraft:from_float",input:{type:"minecraft:div",left:1,right:0}}'),
        cmd('/compute default integer {type:"minecraft:uniform",min:3,max:3}'),
        cmd('/compute default integer {type:"minecraft:conditional",condition:{type:"minecraft:inverted",term:{type:"minecraft:match_block",blocks:"minecraft:stone"}},on_true:5}'),
        cmd('/compute default integer {type:"minecraft:nope"}'),
        cmd('/compute default integer minecraft:brewing/uses_default'),
        cmd('/compute default integer minecraft:cooking/time_coal'),
        cmd('/compute default integer minecraft:cooking/fast_burn_time_reduction_factor'),
        cmd('/compute default integer minecraft:nonexistent'),
        cmd('/compute default float 2.5'),
        cmd('/compute default float 2.5 2'),
        cmd('/compute default float 3'),
        cmd('/compute default float -0.5'),
        cmd('/compute default float 1e30'),
        cmd('/compute default float {type:"minecraft:div",left:1,right:0}'),
        cmd('/compute default float {type:"minecraft:mod",left:1,right:0}'),
        cmd('/compute default float {type:"minecraft:sqrt",value:2}'),
        cmd('/compute default float {type:"minecraft:sqrt",input:2}'),
        cmd('/compute default float {type:"minecraft:sqrt",input:2} 1000'),
        cmd('/compute default float {type:"minecraft:sqrt",input:-1}'),
        cmd('/compute default float {type:"minecraft:mul",left:1.5,right:4}'),
        cmd('/compute default float {type:"minecraft:mul",inputs:[1.5,4]}'),
        cmd('/compute default float {type:"minecraft:sub",left:1.5,right:4}'),
        cmd('/compute default float {type:"minecraft:floor",input:-1.5}'),
        cmd('/compute default float {type:"minecraft:ceil",input:-1.5}'),
        cmd('/compute default float {type:"minecraft:round",input:2.5}'),
        cmd('/compute default float {type:"minecraft:round",input:-2.5}'),
        cmd('/compute default float {type:"minecraft:truncate",input:-2.7}'),
        cmd('/compute default float {type:"minecraft:from_int",input:3}'),
        cmd('/compute default float {type:"minecraft:avg",inputs:[1,2]}'),
        cmd('/compute default float {type:"minecraft:length",inputs:[3,4]}'),
        cmd('/compute default float {type:"minecraft:pow",base:2,exponent:0.5} 100'),
        cmd('/compute default float {type:"minecraft:sin",input:1} 1000'),
        cmd('/compute default float {type:"minecraft:cos",input:1} 1000'),
        cmd('/compute default float {type:"minecraft:abs",input:-3.25}'),
        cmd('/compute default float {type:"minecraft:negate",input:3.25}'),
        cmd('/compute default float {type:"minecraft:max",inputs:[]}'),
        cmd('/compute default float minecraft:brewing/speed_default'),
        cmd('/compute default float minecraft:cooking/speed_default 100'),
        cmd('/compute block 8 229 8 float minecraft:cooking/speed_default'),
        *setup('/setblock 8 229 8 minecraft:smoker'),
        cmd('/compute block 8 229 8 float minecraft:cooking/speed_default'),
        cmd('/compute block 8 229 8 integer minecraft:cooking/time_coal'),
        *setup('/setblock 8 229 8 minecraft:composter[level=0]'),
        cmd('/compute block 8 229 8 integer minecraft:compostable/low'),
        *setup('/setblock 8 229 8 minecraft:air'),
        cmd('/compute entity @s integer {type:"minecraft:add",inputs:[1,1]}'),
        cmd('/compute entity @e[type=minecraft:pig,limit=1] integer 1'),
        cmd('/compute block 8 400 8 integer 1'),
        cmd('/compute block 100000 64 100000 integer 1'),
        cmd('/execute store result score @s cmp run compute default integer 5'),
    ]


def scoreboard_remainder_boundaries():
    operations = setup('/scoreboard objectives add boundary dummy')
    for dividend, divisor in [(2147483646, 2147483647), (7, -3), (-2147483648, -1)]:
        operations += setup(f'/scoreboard players set left boundary {dividend}',
                            f'/scoreboard players set right boundary {divisor}')
        operations += [cmd('/scoreboard players operation left boundary %= right boundary'),
                       cmd('/scoreboard players get left boundary'),
                       cmd('/scoreboard players get right boundary')]
    operations += setup('/scoreboard objectives remove boundary')
    return operations


def social():
    return [
        *scoreboard_remainder_boundaries(),
        *setup('/scoreboard objectives add trig trigger', '/scoreboard objectives add plain dummy'),
        cmd('/trigger trig'),
        cmd('/scoreboard players enable @s trig'),
        cmd('/scoreboard players enable @s trig'),
        cmd('/scoreboard players enable @s plain'),
        cmd('/trigger trig'),
        cmd('/trigger trig'),
        cmd('/scoreboard players enable @s trig'),
        cmd('/trigger trig add 5'),
        cmd('/scoreboard players enable @s trig'),
        cmd('/trigger trig set 3'),
        cmd('/scoreboard players get @s trig'),
        cmd('/trigger plain'),
        cmd('/scoreboard players display name @s trig "Shown"'),
        cmd('/scoreboard players display numberformat @s trig styled {color:"red"}'),
        cmd('/scoreboard players display numberformat @s trig fixed "X"'),
        cmd('/scoreboard players display numberformat @s trig blank'),
        cmd('/scoreboard players display numberformat @s trig'),
        cmd('/scoreboard objectives modify trig numberformat fixed "Y"'),
        cmd('/scoreboard objectives modify trig displayautoupdate true'),
        cmd('/scoreboard objectives setdisplay sidebar'),
        cmd('/scoreboard objectives setdisplay sidebar.team.red trig'),
        cmd('/scoreboard players operation @s trig = @s trig'),
        cmd('/scoreboard players operation @s trig /= @s plain'),
        cmd('/scoreboard players operation @s trig >< @s plain'),
        cmd('/scoreboard players list'),
        cmd('/scoreboard players reset @s'),
        cmd('/scoreboard players list @s'),
        cmd('/teammsg hello'),
        cmd('/team add blue "Blue Team"'),
        cmd('/team join blue'),
        cmd('/teammsg hello team'),
        cmd('/tm short'),
        cmd('/team modify blue friendlyFire false'),
        cmd('/team modify blue friendlyFire false'),
        cmd('/team modify blue seeFriendlyInvisibles false'),
        cmd('/team modify blue nametagVisibility hideForOtherTeams'),
        cmd('/team modify blue deathMessageVisibility never'),
        cmd('/team modify blue collisionRule pushOwnTeam'),
        cmd('/team modify blue suffix " [B]"'),
        cmd('/team modify blue displayName "Blues"'),
        cmd('/team modify blue color reset'),
        cmd('/team list blue'),
        cmd('/team empty blue'),
        cmd('/team empty blue'),
        cmd('/team join blue fake1 fake2'),
        cmd('/team list blue'),
        cmd('/team leave fake1'),
        cmd('/team remove blue'),
        cmd('/team list'),
        cmd('/msg @s'),
        cmd('/tell Player hi'),
        cmd('/w @a hi all'),
        cmd('/me'),
        cmd('/say @s says'),
        cmd('/tellraw @s {"selector":"@s"}'),
        cmd('/tellraw @s {"score":{"name":"@s","objective":"trig"}}'),
        cmd('/tellraw @s {"nbt":"SelectedItemSlot","entity":"@s"}'),
        cmd('/tellraw @s {"nbt":"Inventory","entity":"@s","plain":true}'),
        cmd('/tellraw @s {"nbt":"value","storage":"harness:s","color":"red"}'),
        cmd('/tellraw @s {"nbt":"nothing","entity":"@s"}'),
        cmd('/tellraw @s {"text":"click","click_event":{"action":"run_command","command":"/say x"},"underlined":true}'),
        cmd('/title @s title {"selector":"@s"}'),
        cmd('/list uuids'),
        cmd('/help'),
        cmd('/help execute'),
        cmd('/help scoreboard players'),
        cmd('/help nonexistent'),
        *setup('/scoreboard objectives remove trig', '/scoreboard objectives remove plain'),
    ]


def test_command():
    # `/test run|runmultiple` of an existing instance start a real test (a
    # tick-based run, not compared here); `verify`, `create`, `locate` and
    # `pos` with test instance blocks around are not ported.
    return [
        cmd('/test clearall'),
        cmd('/test stop'),
        cmd('/test pos'),
        cmd('/test locate minecraft:always_pass'),
        cmd('/test runclosest'),
        cmd('/test resetclosest'),
        cmd('/test runfailed'),
        cmd('/test clearthese'),
        cmd('/test runthese'),
        cmd('/test runthat 2 true'),
        cmd('/test runfailed true'),
        cmd('/test resetthese'),
        cmd('/test resetthat'),
        cmd('/test clearthat'),
        cmd('/test clearall 10'),
        cmd('/test pos myPos'),
        cmd('/test locate *'),
        cmd('/test run minecraft:nothing*'),
        cmd('/test locate nothing'),
        *setup('/summon minecraft:marker 8 230 8 {Tags:["tp"]}'),
        cmd('/execute as @e[tag=tp] run test pos'),
        cmd('/execute positioned 0 230 0 run test pos'),
        cmd('/execute as @e[tag=tp] run kill @e[tag=nothing]'),
        cmd('/execute as @e[tag=tp] at @s run kill @e[tag=nothing]'),
        cmd('/execute positioned 1 2 3 run kill @e[tag=nothing]'),
        *setup('/kill @e[tag=tp]'),
    ]


def inline_execute_predicates():
    """ResourceOrId command admission, using only already-supported constant conditions."""
    import json
    ops = setup('/forceload add 0 0 15 15',
                '/scoreboard objectives add inline_pred dummy',
                '/execute store result score before inline_pred run time query gametime')
    ops += [cmd('/execute if loaded 4 200 4', expect_result=1),
            cmd('/execute if predicate h286_inline:always', expect_result=1),
            cmd('/execute unless predicate h286_inline:never', expect_result=1)]
    cases = []
    for name, value, truth in inline_predicate_constants():
        for argument in (f'h286_inline:{name}', json.dumps(value, separators=(',', ':'))):
            for sign in ('if', 'unless'):
                cases.append((f'execute {sign} predicate {argument}', int(truth == (sign == 'if'))))
    for value in ('h286_inline:missing', '{}', '{type:"minecraft:all_of"}',
                  '{type:"minecraft:inverted"}', '{type:"h286_inline:missing"}',
                  '{type:"minecraft:all_of",terms:[{}]}',
                  '{type:"minecraft:inverted",term:{}}'):
        for sign in ('if', 'unless'):
            # Observe parse failure stores independently; do not assume callbacks fire.
            cases.append((f'execute {sign} predicate {value}', -99))
    for command, expected in cases:
        ops += setup('/scoreboard players set result inline_pred -99',
                     '/scoreboard players set success inline_pred -99')
        ops += [cmd('/scoreboard players get result inline_pred', expect_result=-99),
                cmd('/scoreboard players get success inline_pred', expect_result=-99),
                cmd('/execute store result score result inline_pred '
                    f'store success score success inline_pred run {command}'),
                cmd('/scoreboard players get result inline_pred', expect_result=expected),
                cmd('/scoreboard players get success inline_pred', expect_result=expected)]
    # Functional conditional execution, beyond the terminal Test passed/failed reply.
    ops += setup('/scoreboard players set hits inline_pred 0')
    for value, sign in (('always', 'if'), ('never', 'if'), ('always', 'unless'), ('never', 'unless')):
        predicate = next(v for name, v, _ in inline_predicate_constants() if name == value)
        ops.append(cmd(f'/execute {sign} predicate {json.dumps(predicate, separators=(",", ":"))} '
                       'run scoreboard players add hits inline_pred 1'))
    ops.append(cmd('/scoreboard players get hits inline_pred', expect_result=2))
    ops += setup('/execute store result score elapsed inline_pred run time query gametime',
                 '/scoreboard players operation elapsed inline_pred -= before inline_pred')
    return ops + [cmd('/scoreboard players get elapsed inline_pred', expect_result=0)] + setup(
        '/scoreboard objectives remove inline_pred')


def end_clock_commands():
    """No observation ticks: independent built-in clocks and command callbacks."""
    ops = [*setup('/tick freeze', '/gamerule advance_time false',
                  '/scoreboard objectives add audit_time dummy',
                  '/execute store result score before audit_time run time query gametime')]
    for dimension in ('overworld', 'the_nether', 'the_end'):
        ops += [cmd(f'/execute in minecraft:{dimension} if dimension minecraft:{dimension}', expect_result=1)]
    ops += [cmd('/time of minecraft:the_end set 333', expect_result=333),
            cmd('/time of minecraft:overworld set 30000', expect_result=30000),
            cmd('/time of minecraft:the_end query time', expect_result=333),
            cmd('/time of minecraft:overworld query time', expect_result=30000),
            cmd('/execute in minecraft:the_end run time query time', expect_result=333),
            cmd('/execute in minecraft:the_nether run time query time'),
            cmd('/execute in minecraft:the_nether run time of minecraft:overworld query time', expect_result=30000)]
    commands = [
        '/time of minecraft:overworld query time', '/time of minecraft:the_end query time',
        '/time of minecraft:the_end set 900', '/time of minecraft:the_end set 900',
        '/time of minecraft:the_end add -1000', '/time of minecraft:the_end set day',
        '/time of minecraft:the_end pause', '/time of minecraft:the_end pause',
        '/time of minecraft:the_end resume', '/time of minecraft:the_end resume',
        '/time of minecraft:the_end rate 2', '/time of minecraft:the_end rate 2',
        '/time of minecraft:the_end rate 1', '/execute in minecraft:the_nether run time query time',
        '/execute in minecraft:the_end run time query time',
        '/execute in minecraft:the_nether run time of minecraft:overworld query time',
        '/time query time', '/time of minecraft:the_end set 777']
    expected = [(30000, 1, 333), (333, 1, 333), (900, 1, 900), (0, 0, 900),
                (0, 1, 0), (0, 0, 0), (1, 1, 0), (0, 0, 0), (1, 1, 0),
                (0, 0, 0), (1, 1, 0), (0, 0, 0), (1, 1, 0), (0, 0, 0),
                (0, 1, 0), (30000, 1, 0), (30000, 1, 0), (777, 1, 777)]
    for command, (result, success, end_time) in zip(commands, expected):
        ops += [*setup('/scoreboard players set result audit_time -99',
                       '/scoreboard players set success audit_time -99'),
                cmd('/execute store result score result audit_time store success score success audit_time run '+command.lstrip('/')),
                cmd('/scoreboard players get result audit_time', expect_result=result),
                cmd('/scoreboard players get success audit_time', expect_result=success),
                cmd('/time of minecraft:overworld query time', expect_result=30000),
                cmd('/time of minecraft:the_end query time', expect_result=end_time)]
        if command == '/time of minecraft:the_end pause' and success:
            ops += [cmd('/time of minecraft:overworld pause', expect_result=1),
                    cmd('/time of minecraft:overworld resume', expect_result=1)]
        if command == '/time of minecraft:the_end rate 2' and success:
            ops += [cmd('/time of minecraft:overworld rate 2', expect_result=1),
                    cmd('/time of minecraft:overworld rate 1', expect_result=1)]
    ops += [cmd('/time of minecraft:the_end query minecraft:day'),
            cmd('/time of minecraft:the_moon query time'),
            # End pause/rate operations must leave the Overworld state alone.
            cmd('/time pause'), cmd('/time pause'), cmd('/time resume'),
            cmd('/time rate 2'), cmd('/time rate 2'), cmd('/time rate 1'),
            cmd('/time set day'), cmd('/time query minecraft:day'),
            *setup('/execute store result score elapsed audit_time run time query gametime',
                   '/scoreboard players operation elapsed audit_time -= before audit_time'),
            cmd('/scoreboard players get elapsed audit_time', expect_result=0),
            *setup('/scoreboard objectives remove audit_time', '/tick unfreeze')]
    return ops


def server_clock():
    """Only explicit tick operations advance the script's authoritative clock."""
    return [
        *setup('/scoreboard objectives add harness_clock dummy',
               '/execute store result score before harness_clock run time query gametime'),
        {'op': 'tick', 'n': 3},
        *setup('/execute store result score elapsed harness_clock run time query gametime',
               '/scoreboard players operation elapsed harness_clock -= before harness_clock'),
        cmd('/scoreboard players get elapsed harness_clock'),
        # The connected owner is the only player; @s and @a use the same body.
        cmd('/execute as @a run say connected owner'),
        *setup('/scoreboard objectives remove harness_clock'),
    ]


def wither_breaks():
    """WitherBoss.customServerAiStep 20 ticks after it is hurt: Level.destroyBlock around it, which leaves
    each cell's fluid (FluidState.createLegacyBlock), so water stays and a waterlogged block turns to water."""
    return [
        *setup('/fill 4 206 4 12 217 12 minecraft:glass hollow', '/fill 5 207 5 11 216 11 minecraft:water',
               '/setblock 7 210 7 minecraft:stone', '/setblock 9 211 9 minecraft:oak_stairs[waterlogged=true]',
               '/setblock 7 212 9 minecraft:oak_slab[waterlogged=false]'),
        {'op': 'tick', 'n': 10},
        *setup('/summon minecraft:wither 8.5 210 8.5 {NoGravity:1b,Tags:["wb"]}'),
        cmd('/damage @e[tag=wb,limit=1] 1'),
        # WaterAvoidingRandomFlyingGoal may set off on a stroll (the wither's random is unseeded in vanilla),
        # which moves the box destroyBlocksTick breaks. Hold it in place each tick until then: customServerAiStep
        # runs before travel, so the break sees the teleported position.
        *[op for _ in range(25) for op in (*setup('/tp @e[tag=wb] 8.5 210 8.5'), {'op': 'tick', 'n': 1})],
        {'op': 'tick', 'n': 15},
        cmd('/execute if block 8 212 8 minecraft:water'),
        cmd('/execute if block 9 211 9 minecraft:water'),
        cmd('/execute if block 9 211 9 minecraft:oak_stairs'),
        cmd('/execute if block 7 210 7 minecraft:stone'),
        cmd('/execute if block 7 212 9 minecraft:oak_slab'),
        # The broken cells' neighbours were updated (setBlock flag 3): the pool flows back in.
        cmd('/execute if block 7 210 7 minecraft:water'),
        cmd('/execute if block 7 212 9 minecraft:water'),
        cmd('/execute if block 6 210 6 minecraft:water'),
        *setup('/kill @e[tag=wb]', '/fill 4 206 4 12 217 12 minecraft:air'),
        {'op': 'tick', 'n': 25},
    ]


def tnt_minecart_rails():
    """A primed TNT minecart (MinecartTNT.isPrimed) on a powered activator rail inside an obsidian box: its
    blast neither resists on nor breaks a rail or the block under one (getBlockExplosionResistance and
    shouldBlockExplode, #4992), but breaks the stone beside them; the obsidian holds."""
    return [
        *setup('/fill 3 203 3 7 207 7 minecraft:obsidian', '/fill 4 204 4 6 206 6 minecraft:air',
               '/setblock 5 204 5 minecraft:redstone_block', '/setblock 5 205 5 minecraft:activator_rail',
               '/setblock 4 204 5 minecraft:stone', '/setblock 2 205 5 minecraft:stone'),
        cmd('/summon minecraft:tnt_minecart 5.5 205.0625 5.5'),
        {'op': 'tick', 'n': 5},
        cmd('/execute if entity @e[type=minecraft:tnt_minecart]'),
        {'op': 'tick', 'n': 100},
        cmd('/execute if entity @e[type=minecraft:tnt_minecart]'),
        cmd('/execute if block 5 205 5 minecraft:activator_rail'),
        cmd('/execute if block 5 204 5 minecraft:redstone_block'),
        cmd('/execute if block 4 204 5 minecraft:stone'),
        cmd('/execute if block 3 205 5 minecraft:obsidian'),
        cmd('/execute if block 2 205 5 minecraft:stone'),
        *setup('/kill @e[type=!player]', '/fill 2 203 3 7 207 7 minecraft:air'),
        {'op': 'tick', 'n': 5},
    ]


def minecart_drag():
    """AbstractMinecart's drags are floats widened to double: getAirDrag (0.95F) off the rails, and
    applyNaturalSlowdown's 0.95F in water. Positions and motions are compared to the last bit."""
    return [
        # A cart sent back by the stone at the track's end leaves the rails at x 0 and flies on.
        *setup('/fill 0 209 0 12 209 0 minecraft:redstone_block', '/fill 0 210 0 9 210 0 minecraft:powered_rail',
               '/setblock 10 210 0 minecraft:stone'),
        cmd('/summon minecraft:minecart 1.5 210.0625 0.5 {Motion:[0.1d,0.0d,0.0d],Tags:["drag"]}'),
        {'op': 'tick', 'n': 60},
        cmd('/data get entity @e[tag=drag,limit=1] Pos'),
        cmd('/data get entity @e[tag=drag,limit=1] Motion'),
        *setup('/kill @e[type=!player]', '/fill 0 205 0 15 215 1 minecraft:air'),
        # Waterlogged rails: the cart slows by 0.95F a tick more.
        *setup('/fill 0 209 4 12 209 4 minecraft:stone', '/fill 0 210 4 12 210 4 minecraft:rail[waterlogged=true]'),
        cmd('/summon minecraft:minecart 1.5 210.0625 4.5 {Motion:[0.3d,0.0d,0.0d],Tags:["wet"]}'),
        {'op': 'tick', 'n': 20},
        cmd('/data get entity @e[tag=wet,limit=1] Pos'),
        cmd('/data get entity @e[tag=wet,limit=1] Motion'),
        *setup('/kill @e[type=!player]', '/fill 0 205 3 15 215 5 minecraft:air'),
        {'op': 'tick', 'n': 5},
    ]


def ender_dragon():
    """`/kill` on the fight's dragon ends the fight (#3319): `EnderDragon.kill` removes it and calls
    `EnderDragonFight.setDragonKilled`, so no new dragon takes its place, the exit portal opens and the
    first dragon leaves its egg. The player stays over the exit portal, in the fight (`validPlayer`)."""
    end = '/execute in minecraft:the_end run'
    return [
        *setup('/gamemode creative', f'{end} tp @s 0.5 100 0.5 0 0'),
        {'op': 'settle'},
        # updatePlayers, the arena scan and createNewDragon once the 17 by 17 arena chunks are block ticking
        # (EnderDragonFight.isArenaLoaded). The oracle generates them on worker threads: a tick op with a
        # `record` key gives it a moment of wall time before its ticks.
        *[{'op': 'tick', 'n': 20, 'record': False}] * 10,
        cmd('/execute if entity @e[type=minecraft:ender_dragon]'),
        cmd('/kill @e[type=minecraft:ender_dragon]'),
        {'op': 'tick', 'n': 40},
        cmd('/execute if entity @e[type=minecraft:ender_dragon]'),
        cmd(f'{end} fill -3 0 -3 3 120 3 minecraft:air replace minecraft:end_portal'),
        cmd(f'{end} fill -3 0 -3 3 120 3 minecraft:air replace minecraft:dragon_egg'),
        *setup('/execute in minecraft:overworld run tp @s 8.5 230 8.5 0 0'),
        {'op': 'settle'},
    ]


def place_structure():
    """`/place structure` (PlaceCommand.placeStructure): the id is resolved, a
    `StructureStart` is generated for the position's chunk with vanilla's
    `biome -> true` predicate, and its `placeInChunk` runs for every chunk of the
    start's box in the live level. Buried treasure's one piece digs down from the
    ocean floor and leaves a single chest, so a chest count in its chunk compares
    the generated start and its placement without knowing the terrain height."""
    return [
        *setup('/gamemode creative', '/tp @s 8.5 230 8.5 0 0'),
        {'op': 'settle'},
        *setup('/kill @e[type=!minecraft:player]',
               '/fill 0 0 0 15 120 15 minecraft:air replace minecraft:chest'),
        cmd('/place structure minecraft:buried_treasure 4 210 4'),
        # One chest between y 0 and 120 of chunk (0, 0): the piece's column is
        # inside that chunk whatever height its dig reached.
        cmd('/fill 0 0 0 15 120 15 minecraft:air replace minecraft:chest'),
    ]


def tame_wolf_kills():
    # LivingEntity.resolvePlayerResponsibleForDamage: a tame wolf's hit is its
    # owner's (the harness player, OfflinePlayer:Player), so the kill drops
    # experience and ServerPlayer.awardKillScore counts it (mob_kills,
    # totalKillCount); the killed statistic stays the wolf's
    # (Player.killedEntity). A wild wolf's kill is nobody's.
    owner = '[I;-1608632253,-450807400,-1786055591,-2146546415]'
    return [
        *setup('/gamerule mob_drops true', '/scoreboard objectives add wolf_kills minecraft.killed:minecraft.zombie',
               '/scoreboard objectives add wolf_mob_kills minecraft.custom:minecraft.mob_kills',
               '/scoreboard objectives add wolf_total totalKillCount'),
        cmd('/summon minecraft:wolf 4.5 210 4.5 {NoAI:1b,NoGravity:1b,Tags:["wild"]}'),
        cmd('/summon minecraft:zombie 6.5 210 4.5 {NoAI:1b,NoGravity:1b,Tags:["prey"]}'),
        cmd('/damage @e[tag=prey,limit=1] 100 minecraft:mob_attack by @e[tag=wild,limit=1]'),
        {'op': 'tick', 'n': 25},
        cmd('/execute if entity @e[type=minecraft:experience_orb,limit=1]'),
        cmd('/scoreboard players get @s wolf_kills'),
        cmd('/scoreboard players get @s wolf_mob_kills'),
        cmd('/scoreboard players get @s wolf_total'),
        cmd(f'/summon minecraft:wolf 4.5 210 8.5 {{NoAI:1b,NoGravity:1b,Owner:{owner},Tags:["pet"]}}'),
        cmd('/summon minecraft:zombie 6.5 210 8.5 {NoAI:1b,NoGravity:1b,Tags:["prey"]}'),
        cmd('/damage @e[tag=prey,limit=1] 100 minecraft:mob_attack by @e[tag=pet,limit=1]'),
        {'op': 'tick', 'n': 25},
        cmd('/execute if entity @e[type=minecraft:experience_orb,limit=1]'),
        cmd('/scoreboard players get @s wolf_kills'),
        cmd('/scoreboard players get @s wolf_mob_kills'),
        cmd('/scoreboard players get @s wolf_total'),
        *setup('/kill @e[type=!player]', '/scoreboard objectives remove wolf_kills',
               '/scoreboard objectives remove wolf_mob_kills', '/scoreboard objectives remove wolf_total',
               '/gamerule mob_drops false'),
    ]


def experience_orb_fall():
    """`ExperienceOrb.tick`'s fall and landing (the dragon-death XP rain, #4781):
    gravity 0.03 (skipped while the orb is inside blocks), one drag of
    `getAirDrag()` times the ground's friction scaled over every axis, and a
    landing bounce of `-fallSpeed * 0.4` gated on `fallSpeed < -getGravity()` —
    not a fixed restitution. The orb spawns 16 blocks from the player, out of
    the 8 block follow radius, 45 blocks above a stone platform; the track
    draws no random, so both games compare to the digit."""
    return [
        *setup('/fill 20 214 20 28 214 28 minecraft:stone'),
        cmd('/summon minecraft:experience_orb 24.5 260 24.5 {Value:1s}'),
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Pos'),
        {'op': 'tick', 'n': 1},
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Pos'),
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Motion'),
        {'op': 'tick', 'n': 64},
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Pos'),
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Motion'),
        {'op': 'tick', 'n': 20},
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Pos'),
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Motion'),
        {'op': 'tick', 'n': 35},
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Pos'),
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Motion'),
        *setup('/kill @e[type=minecraft:experience_orb]', '/fill 20 214 20 28 214 28 minecraft:air'),
        {'op': 'tick', 'n': 5},
    ]


def creative_shulker_drops():
    """ShulkerBoxBlock.playerWillDestroy: Creative keeps a filled box as one item,
    even with block_drops off. Empty boxes and survival use the ordinary drop rules."""
    colors = ('white orange magenta light_blue yellow lime pink gray light_gray cyan '
              'purple blue brown green red black').split()
    blocks = ['shulker_box', *(f'{color}_shulker_box' for color in colors)]
    item = '@e[type=minecraft:item,limit=1]'
    contents = ('Items:[{Slot:0b,id:"minecraft:diamond_sword",count:1,components:{'
                '"minecraft:custom_name":"Blade","minecraft:enchantments":{"minecraft:sharpness":5}}},'
                '{Slot:26b,id:"minecraft:apple",count:16}],')
    out = []
    for block in blocks:
        for mode, filled, drops in [('creative', True, True), ('creative', False, True),
                                    ('creative', True, False), ('survival', True, True),
                                    ('survival', False, True), ('survival', True, False)]:
            out += setup('/kill @e[type=minecraft:item]', f'/gamemode {mode}',
                         f'/gamerule block_drops {str(drops).lower()}',
                         f'/setblock 4 210 4 minecraft:{block}'
                         '{' + (contents if filled else '') + 'TestOnly:7}')
            out += [{'op': 'break', 'pos': [4, 210, 4]},
                    cmd('/execute if block 4 210 4 minecraft:air'),
                    cmd('/execute if entity @e[type=minecraft:item]')]
            if (mode == 'creative' and filled) or (mode == 'survival' and drops):
                out += [cmd(f'/data get entity {item} Item'),
                        cmd(f'/execute if data entity {item} Item.components."minecraft:block_entity_data"')]
                if mode == 'creative':
                    out += [cmd(f'/data get entity {item} Pos'), cmd(f'/data get entity {item} PickupDelay')]
    out += setup('/kill @e[type=minecraft:item]', '/gamemode creative', '/gamerule block_drops true')
    return out


def piglin_pickups():
    """PiglinAi.pickUpItem/stopHoldingOffHandItem: babies keep admired gold, adults barter it."""
    ops = [*setup('/fill 2 209 2 6 212 6 minecraft:glass hollow',
                  '/fill 3 209 3 5 209 5 minecraft:stone')]
    for baby, held in ((False, "crossbow"), (True, None), (True, "golden_sword"),
                       (True, "crossbow")):
        # NoAI would skip admiration expiry. A small pen keeps both the piglin
        # and its item together without suppressing the server's normal ticks.
        # Finish the previous corpse's death ticks before clearing its drops.
        ops += [
            *setup('/kill @e[type=!player]'),
            {'op': 'tick', 'n': 25},
            *setup('/kill @e[type=minecraft:item]',
                   '/summon minecraft:piglin 4.5 210 4.5 '
                   '{Tags:["barter"],IsImmuneToZombification:1b,PersistenceRequired:1b,drop_chances:{mainhand:0.0f},IsBaby:'
                   + ('1b' if baby else '0b')
                   + (',equipment:{mainhand:{id:"minecraft:' + held + '",count:1}}' if held else '') + '}'),
            cmd('/execute if items entity @e[tag=barter,limit=1] weapon.mainhand *'),
            *setup('/summon minecraft:item 4.5 210 4.5 '
                   '{PickupDelay:0s,Item:{id:"minecraft:gold_ingot",count:1}}'),
            {'op': 'tick', 'n': 40},
            cmd('/execute if items entity @e[tag=barter,limit=1] weapon.offhand minecraft:gold_ingot'),
            cmd('/execute if entity @e[type=minecraft:item]'),
            {'op': 'tick', 'n': 140},
            cmd('/execute if items entity @e[tag=barter,limit=1] weapon.offhand *'),
            cmd('/execute if items entity @e[tag=barter,limit=1] weapon.mainhand minecraft:gold_ingot'),
            # A baby drops nothing. What an adult's barter throws comes from the level's random (vanilla's is
            # unseeded), and the piglin picks some of it back up, so only the gold's fate is fixed: none of it lies
            # on the ground.
            cmd('/execute if entity @e[type=minecraft:item]' if baby
                else '/execute if items entity @e[type=minecraft:item] contents minecraft:gold_ingot'),
        ]
        if baby:
            ops.append(cmd('/execute if data entity @e[tag=barter,limit=1] Inventory[{id:"minecraft:golden_sword"}]'))
    # IGNORED_BY_PIGLIN_BABIES contains leather: adults take it, babies leave it.
    for baby in (False, True):
        ops += [
            *setup('/kill @e[type=!player]'),
            {'op': 'tick', 'n': 25},
            *setup('/kill @e[type=minecraft:item]',
                   '/summon minecraft:piglin 4.5 210 4.5 '
                   '{Tags:["barter"],IsImmuneToZombification:1b,IsBaby:' + ('1b' if baby else '0b') + '}',
                   '/summon minecraft:item 4.5 210 4.5 '
                   '{PickupDelay:0s,Item:{id:"minecraft:leather",count:1}}'),
            {'op': 'tick', 'n': 40},
            cmd('/execute if entity @e[type=minecraft:item]'),
            cmd('/execute if items entity @e[tag=barter,limit=1] weapon.mainhand minecraft:leather'),
        ]
    ops += [*setup('/kill @e[type=!player]'), {'op': 'tick', 'n': 25},
            *setup('/kill @e[type=minecraft:item]', '/fill 2 209 2 6 212 6 minecraft:air')]
    return ops


def experience_orb_stuck():
    """`ExperienceOrb.tick`'s push out of a block, the one path of the tick that
    draws a random: a 3x3 block placed over a settled orb leaves it colliding,
    and with no player within the 8 block follow radius the server nudges it
    towards the nearest free side (`Entity.moveTowardsClosestSpace`),
    `nextFloat() * 0.2 + 0.1` at a time, gravity skipped while its box is inside
    blocks. The orb's random is unseeded in vanilla (`RandomSource.create()`),
    so the outcome must not depend on the draws: a ring of stone one block
    higher keeps the top the only free side all the way out. Without it, once
    the orb's middle rose past y 216 with its box still in the block, north
    tied with south, west and east, won as the first, and some draws slid the
    orb off the block (at z 23.25, 22.x or down to y 215 on the floor).
    An orb that is never pushed stays boxed in at y 215 instead of resting in
    the ring at 216. 25 blocks from the player, out of the follow radius."""
    return [
        *setup('/fill 30 214 20 36 214 26 minecraft:stone'),
        cmd('/summon minecraft:experience_orb 33.5 215 23.5 {Value:1s}'),
        {'op': 'tick', 'n': 2},
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Pos'),
        *setup('/fill 32 215 22 34 216 24 minecraft:stone', '/setblock 33 216 23 minecraft:air'),
        {'op': 'tick', 'n': 60},
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Pos'),
        cmd('/data get entity @e[type=minecraft:experience_orb,limit=1] Motion'),
        *setup('/kill @e[type=minecraft:experience_orb]', '/fill 30 214 20 36 216 26 minecraft:air'),
        {'op': 'tick', 'n': 5},
    ]


def summoned_potions():
    """`ThrowableItemProjectile.getItem`/`readAdditionalSaveData` and
    `AbstractThrownPotion.getDefaultGravity`: a summoned splash or lingering
    potion is the projectile its type is, not an entity nothing ticks. It
    flies with the gravity 0.05 of its type (a bare one as a splash potion
    with no effects), `Motion` is in its compound, and what it splashes on
    landing are the effects of its `Item`'s `potion_contents` (#6642)."""
    ops = [*setup('/kill @e[type=minecraft:splash_potion]',
                  '/kill @e[type=minecraft:lingering_potion]',
                  '/fill 0 196 0 20 200 20 minecraft:stone')]
    # A bare summon flies as the default splash potion: no effects, and the
    # tick's gravity and drag move it off where it was placed.
    ops += [
        cmd('/summon minecraft:splash_potion 4.5 206 4.5'),
        cmd('/data get entity @e[type=minecraft:splash_potion,limit=1] Item'),
        cmd('/data get entity @e[type=minecraft:splash_potion,limit=1] Motion'),
        cmd('/data get entity @e[type=minecraft:splash_potion,limit=1] Pos'),
        {'op': 'tick', 'n': 3},
        cmd('/data get entity @e[type=minecraft:splash_potion,limit=1] Pos'),
        cmd('/data get entity @e[type=minecraft:splash_potion,limit=1] Motion'),
        {'op': 'tick', 'n': 30},
        cmd('/execute if entity @e[type=minecraft:splash_potion]'),
    ]
    # A harming potion landing on a pig hurts it for the potion's 6, and the
    # potion is gone: ThrownSplashPotion.onHitAsPotion over the stack. The pig
    # stands on the stone at y 201 (a summon at y 200 is inside it).
    ops += [
        *setup('/kill @e[type=!player]'),
        {'op': 'tick', 'n': 25},
        *setup('/summon minecraft:pig 4.5 201 4.5 {NoAI:1b,Silent:1b,PersistenceRequired:1b}'),
        {'op': 'tick', 'n': 5},
        cmd('/data get entity @e[type=minecraft:pig,limit=1] Health'),
        *setup('/summon minecraft:splash_potion 4.5 206 4.5 '
               '{Item:{id:"minecraft:splash_potion",count:1,components:'
               '{"minecraft:potion_contents":{potion:"minecraft:harming"}}}}'),
        {'op': 'tick', 'n': 40},
        cmd('/data get entity @e[type=minecraft:pig,limit=1] Health'),
        cmd('/execute if entity @e[type=minecraft:splash_potion]'),
    ]
    # A lingering potion leaves its cloud where it lands, and the cloud puts
    # the potion's poison on the pig standing in it
    # (ThrownLingeringPotion.onHitAsPotion). The duration is left out of the
    # check: how the cloud's age and the mob's effect tick interleave is not
    # what this covers.
    ops += [
        *setup('/kill @e[type=!player]'),
        {'op': 'tick', 'n': 25},
        *setup('/summon minecraft:pig 8.5 201 8.5 '
               '{Tags:["poisoned"],NoAI:1b,Silent:1b,PersistenceRequired:1b}',
               '/summon minecraft:lingering_potion 8.5 206 8.5 '
               '{Item:{id:"minecraft:lingering_potion",count:1,components:'
               '{"minecraft:potion_contents":{potion:"minecraft:poison"}}}}'),
        cmd('/data get entity @e[type=minecraft:lingering_potion,limit=1] Motion'),
        {'op': 'tick', 'n': 40},
        cmd('/execute if entity @e[type=minecraft:lingering_potion]'),
        cmd('/execute if entity @e[tag=poisoned,limit=1,nbt='
            '{active_effects:[{id:"minecraft:poison"}]}]'),
        # A bare lingering potion has empty contents, so its cloud, ten blocks
        # from the poison one, puts nothing on its own pig.
        *setup('/summon minecraft:pig 18.5 201 18.5 '
               '{Tags:["bare"],NoAI:1b,Silent:1b,PersistenceRequired:1b}',
               '/summon minecraft:lingering_potion 18.5 206 18.5'),
        {'op': 'tick', 'n': 40},
        cmd('/execute if entity @e[tag=poisoned,limit=1,nbt='
            '{active_effects:[{id:"minecraft:poison"}]}]'),
        cmd('/execute if entity @e[tag=bare,limit=1,nbt='
            '{active_effects:[{id:"minecraft:poison"}]}]'),
    ]
    ops += [*setup('/kill @e[type=!player]'), {'op': 'tick', 'n': 25}]
    return ops


def trial_spawner_passengers():
    """Configured passengers must exist as entities with a vehicle (#7254)."""
    ops = [*setup('/fill 1 219 1 13 219 8 stone',
                  '/fill 1 220 1 13 224 8 air', '/gamerule spawn_mobs true')]
    # Exercise namespace and position independently, plus the explicit control.
    for i, (kind, pos) in enumerate([
        ('chicken', ''), ('minecraft:chicken', ''),
        ('chicken', ',Pos:[4.5d,220.0d,6.5d]'),
        ('minecraft:chicken', ',Pos:[4.5d,220.0d,6.5d]'),
    ]):
        parent, rider = f'trial_parent_{i}', f'trial_rider_{i}'
        ops += setup(
            '/setblock 4 220 4 trial_spawner[trial_spawner_state=active]'
            '{normal_config:{spawn_range:3,total_mobs:1.0f,simultaneous_mobs:1.0f,'
            'total_mobs_added_per_player:0.0f,simultaneous_mobs_added_per_player:0.0f,'
            'ticks_between_spawn:1000},spawn_data:{entity:{id:"zombie",'
            f'Tags:["{parent}"],Pos:[4.5d,220.0d,6.5d],NoAI:1b,'
            f'Passengers:[{{id:"{kind}",NoAI:1b,Tags:["{rider}"]{pos}}}]'
            '}},next_mob_spawns_at:0L}',
            '/data modify block 4 220 4 registered_players append from entity @s UUID')
        ops += [
            {'op': 'tick', 'n': 5},
            cmd(f'/execute if entity @e[tag={parent}]'),
            cmd(f'/execute if entity @e[tag={rider}]'),
            cmd(f'/execute as @e[tag={rider}] on vehicle if entity @s[tag={parent}]'),
            cmd(f'/data get entity @e[tag={parent},limit=1] Passengers[0].Tags'),
            *setup(f'/kill @e[tag={parent}]', f'/kill @e[tag={rider}]',
                   '/setblock 4 220 4 air'),
            # The killed pair lies dying for 20 ticks, and a dying mob still
            # obstructs the next spawn (`Mob.checkSpawnObstruction`).
            {'op': 'tick', 'n': 25},
        ]
    return ops + setup('/fill 1 219 1 13 224 8 air', '/gamerule spawn_mobs false')


def villager_command_names():
    ops = setup('/forceload add 0 0')
    case = 0
    for profession in ('none', 'nitwit', 'cleric', 'farmer', 'librarian'):
        for named in (False, True):
            case += 1
            target = f'00000000-0000-0000-0000-0030{case:08x}'
            custom = ',CustomName:{text:"Ada"}' if named else ''
            ops += setup(f'/summon villager 4.5 220 4.5 {{UUID:[I;0,0,48,{case}],'
                         'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,'
                         f'VillagerData:{{profession:"minecraft:{profession}",type:"minecraft:plains",level:1}}{custom}}}')
            for path in ('VillagerData', 'Pos', 'Rotation'):
                ops.append(cmd(f'/data get entity {target} {path}'))
            changed = 'cleric' if profession == 'farmer' else 'farmer'
            ops.append(cmd(f'/data merge entity {target} '
                           f'{{VillagerData:{{profession:"minecraft:{changed}",type:"minecraft:plains",level:1}}}}'))
            ops.append(cmd(f'/data get entity {target} VillagerData'))
            ops += setup(f'/kill {target}')
    for kind in ('zombie_villager', 'wandering_trader'):
        case += 1
        target = f'00000000-0000-0000-0000-0030{case:08x}'
        ops += setup(f'/summon {kind} 4.5 220 4.5 {{UUID:[I;0,0,48,{case}],'
                     'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,'
                     'VillagerData:{profession:"minecraft:cleric",type:"minecraft:plains",level:1}}')
        for path in ('Pos', 'Rotation'):
            ops.append(cmd(f'/data get entity {target} {path}'))
        ops += setup(f'/kill {target}')
    ops.append({'op': 'tick', 'n': 25})
    return ops + setup('/forceload remove 0 0')


def animal_signed_in_love():
    ops = setup('/forceload add 0 0')
    for case, value in enumerate(('-32769', '-32768', '-129', '-128', '-1',
                                  '-1b', '-128b', '0', '1', '37'), 1):
        target = f'00000000-0000-0000-0000-0038{case:08x}'
        ops += setup(f'/summon pig 4 200 4 {{UUID:[I;0,0,56,{case}],'
                     f'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,InLove:{value}}}')
        ops.append(cmd(f'/data get entity {target} InLove'))
        ops.append(cmd(f'/data remove entity {target} InLove'))
        ops.append(cmd(f'/data get entity {target} InLove'))
        ops += setup(f'/kill {target}')
    return ops + setup('/forceload remove 0 0')


def mob_fire_resistance_damage():
    ops = setup('/forceload add 0 0')
    case = 0
    fire_types = ('campfire', 'fireball', 'hot_floor', 'in_fire', 'lava',
                  'on_fire', 'sulfur_cube_hot', 'unattributed_fireball')
    cases = [(damage, variant) for damage in fire_types
             for variant in ('normal', 'fire_resistance', 'resistance', 'invulnerable')]
    cases += [('cactus', 'fire_resistance'), ('generic', 'fire_resistance')]
    for damage, variant in cases:
        case += 1
        # Killed entities remain registered until removal: never reuse a UUID.
        target = f'00000000-0000-0000-0000-002c{case:08x}'
        invulnerable = ',Invulnerable:1b' if variant == 'invulnerable' else ''
        ops += setup(f'/summon pig 4.5 220 4.5 {{UUID:[I;0,0,44,{case}],'
                     f'NoAI:1b,NoGravity:1b,PersistenceRequired:1b{invulnerable}}}')
        if variant in ('fire_resistance', 'resistance'):
            ops.append(cmd(f'/effect give {target} minecraft:{variant} infinite 0 true'))
        ops.append(cmd(f'/data get entity {target} Health 1000'))
        for amount in (1, 1, 2):
            ops.append(cmd(f'/damage {target} {amount} minecraft:{damage}'))
            ops.append(cmd(f'/data get entity {target} Health 1000'))
        ops += setup(f'/kill {target}')
    ops.append({'op': 'tick', 'n': 25})
    return ops + setup('/forceload remove 0 0')


def item_lifetime_counters():
    target = '00000000-0000-0000-0000-000000000001'
    ops = setup('/forceload add 0 0', f'/kill {target}')
    for age in (-32768, -1, 0, 5998, 5999, 6000, 32767):
        for delay in (-1, 0, 1, 2, 32767):
            ops.append(cmd('/summon item 4.5 210 4.5 '
                           '{UUID:[I;0,0,0,1],NoGravity:1b,Item:{id:"minecraft:stone",count:1}}'))
            ops.append({'op': 'tick', 'n': 3})
            ops.append(cmd(f'/data get entity {target} Age'))
            ops += setup(f'/data merge entity {target} {{Age:{age}s,PickupDelay:{delay}s}}')
            for step in range(4):
                if step:
                    ops.append({'op': 'tick', 'n': 1})
                ops.append(cmd(f'/execute if entity {target}'))
                ops.append(cmd(f'/data get entity {target} Age'))
                ops.append(cmd(f'/data get entity {target} PickupDelay'))
            ops += setup(f'/kill {target}')
    return ops + setup('/forceload remove 0 0')


def armor_stand_climbing():
    ops = setup('/gamerule random_tick_speed 0', '/forceload add 0 0 31 31',
                '/fill 0 198 0 31 199 31 stone',
                '/fill 0 200 0 31 214 31 air', '/fill 10 200 9 10 212 11 stone')
    for block, below in [
            ('air', False), ('ladder[facing=west]', False),
            ('vine[east=true]', False), ('twisting_vines_plant', False),
            ('weeping_vines_plant', False),
            ('oak_trapdoor[open=true,facing=west]', True),
            ('oak_trapdoor[open=true,facing=east]', True)]:
        ops += setup('/fill 9 200 10 9 213 10 air',
                     f'/fill 9 200 10 9 210 10 {block}')
        if block == 'weeping_vines_plant':
            ops += setup('/fill 9 200 10 9 212 10 weeping_vines_plant',
                         '/setblock 9 213 10 stone')
        if below:
            ops += setup('/setblock 9 209 10 ladder[facing=west]')
        for x, motion in [(9.5, '[0.0d,0.0d,0.0d]'), (8.5, '[1.0d,0.0d,0.0d]'),
                          (8.5, '[2.0d,0.0d,0.0d]')]:
            y = 210 if x == 9.5 else 200
            ops += setup(f'/summon armor_stand {x} {y} 10.5 {{Silent:1b,Tags:["climb"],Motion:{motion}}}')
            previous = 0
            for tick in (1, 2, 3, 5, 10, 20, 30, 40):
                ops.append({'op': 'tick', 'n': tick - previous})
                previous = tick
                for path in ('Pos', 'Motion', 'OnGround', 'Health'):
                    ops.append(cmd(f'/data get entity @e[tag=climb,limit=1] {path}'))
            ops += setup('/kill @e[tag=climb]')
    ops += setup('/fill 0 198 0 31 214 31 air', '/forceload remove 0 0 31 31',
                 '/gamerule random_tick_speed 3')
    return ops



def creeper_cloud_selectors():
    """Creeper clouds are command targets for their actual lifetime (#7438)."""
    generated = '@e[type=minecraft:area_effect_cloud,tag=!direct]'
    direct = '@e[type=minecraft:area_effect_cloud,tag=direct]'
    creeper = ('/summon minecraft:creeper 4.5 220 4.5 '
               '{NoAI:1b,NoGravity:1b,ignited:1b,Tags:["source"],'
               'active_effects:[{id:"minecraft:speed",duration:1000,amplifier:1b}]}')
    ops = [*setup('/gamerule mob_griefing false', creeper,
                  '/summon minecraft:area_effect_cloud 14.5 220 4.5 '
                  '{Tags:["direct"],Duration:300,Radius:2.5f}'),
           cmd('/execute if entity @e[type=minecraft:creeper,tag=source]')]
    # The default fuse is intentional: custom Fuse is a separate regression.
    for ticks in (30, 10):
        ops += [{'op': 'tick', 'n': ticks},
                cmd(f'/execute if entity {generated}'),
                cmd(f'/execute if entity {direct}')]
    cloud = '@e[type=minecraft:area_effect_cloud,tag=!direct,limit=1]'
    ops += [
        cmd(f'/data get entity {cloud} Pos'),
        cmd(f'/data get entity {cloud} Duration'),
        cmd(f'/data get entity {cloud} WaitTime'),
        cmd(f'/tag {generated} add generated'),
        cmd('/execute if entity @e[type=minecraft:area_effect_cloud,tag=generated]'),
        cmd('/execute if entity @e[type=minecraft:area_effect_cloud,nbt={Tags:["generated"],Duration:300}]'),
        cmd('/scoreboard objectives add cloud_seen dummy'),
        cmd(f'/execute as {generated} run scoreboard players set @s cloud_seen 7'),
        cmd('/execute if entity @e[type=minecraft:area_effect_cloud,scores={cloud_seen=7}]'),
        # Past its maximum lifetime, the generated cloud must be gone.
        {'op': 'tick', 'n': 330},
        cmd(f'/execute if entity {generated}'),
        # A second generated cloud exercises explicit removal independently.
        *setup(creeper),
        {'op': 'tick', 'n': 30},
        cmd(f'/execute if entity {generated}'),
        cmd(f'/kill {generated}'),
        cmd(f'/execute if entity {generated}'),
        *setup('/scoreboard objectives remove cloud_seen', '/kill @e[tag=direct]',
               '/gamerule mob_griefing true'),
    ]
    return ops


def mob_armor_damage():
    """Attribute commands must change runtime mob damage, not only getters (#7494)."""
    cases = [(armor, toughness, amount, 'cactus')
             for armor in (0, 0.5, 1, 20) for toughness in (0, 8) for amount in (0.25, 4, 100)]
    cases += [(20, 8, 4, kind) for kind in ('arrow', 'generic', 'falling_anvil', 'sonic_boom')]
    ops = []
    for index, (armor, toughness, amount, kind) in enumerate(cases):
        tag = f'audit_armor_{index}'
        target = f'@e[tag={tag},limit=1]'
        ops += setup('/summon minecraft:pig 4.5 220 4.5 '
                     f'{{NoAI:1b,NoGravity:1b,Tags:["{tag}"]}}',
                     f'/attribute {target} minecraft:max_health base set 1000',
                     f'/data merge entity {target} {{Health:1000f}}',
                     f'/attribute {target} minecraft:armor base set {armor}',
                     f'/attribute {target} minecraft:armor_toughness base set {toughness}')
        ops += [cmd(f'/attribute {target} minecraft:armor get 1000'),
                cmd(f'/attribute {target} minecraft:armor_toughness get 1000'),
                cmd(f'/data get entity {target} Health 1000'),
                cmd(f'/damage {target} {amount} minecraft:{kind}'),
                cmd(f'/data get entity {target} Health 1000'),
                *setup(f'/kill {target}')]
    return ops + [{'op': 'tick', 'n': 25}]



def bossbar_list_order():
    """CustomBossEvents lists Java HashMap values, with no sorting (#7469)."""
    ops = [
        cmd('/bossbar add audit:test "Title"'),
        cmd('/bossbar set audit:test name "Other"'),
        cmd('/bossbar add audit:second "Second"'),
        cmd('/bossbar list'),
    ]
    for i in range(11):
        ops.append(cmd(f'/bossbar add audit:grow{i} "Extra {i}"'))
    for i in range(11):
        ops.append(cmd(f'/bossbar remove audit:grow{i}'))
    ops += [
        cmd('/bossbar list'),
        cmd('/bossbar remove audit:test'),
        cmd('/bossbar list'),
        cmd('/bossbar add audit:test "Again"'),
        cmd('/bossbar list'),
        cmd('/bossbar remove audit:test'),
        cmd('/bossbar remove audit:second'),
        cmd('/bossbar list'),
    ]
    return ops


def experience_level_boundaries():
    """Exact zero retains progress/total; a negative result resets all three (#7515)."""
    def observe():
        return [cmd('/experience query @s levels'),
                cmd('/experience query @s points'),
                cmd('/data get entity @s XpLevel'),
                cmd('/data get entity @s XpP 1000000'),
                cmd('/data get entity @s XpTotal')]
    ops = []
    for start, delta in [(1, -1), (1, -2), (2, -1), (0, -1)]:
        ops += setup('/experience set @s 0 levels', '/experience set @s 0 points',
                     '/experience add @s -2147483647 points', '/experience add @s 10 points',
                     '/experience set @s 1 levels', '/experience set @s 3 points',
                     f'/experience set @s {start} levels')
        ops += observe()
        ops.append(cmd(f'/experience add @s {delta} levels'))
        ops += observe()
    return ops + setup('/experience set @s 0 levels', '/experience set @s 0 points',
                       '/experience add @s -2147483647 points')


def mob_max_health_bounds():
    """Raw bases survive effective clamping without changing Health (#7413)."""
    pig = '00000000-0000-0000-0000-000000000001'
    attribute = f'/attribute {pig} minecraft:max_health'
    ops = [*setup('/summon minecraft:pig 4 200 4 '
                  '{UUID:[I;0,0,0,1],NoAI:1b,NoGravity:1b,PersistenceRequired:1b}')]
    for base in (-1.75, 0, 20, 1025):
        ops += [
            cmd(f'{attribute} base set {base}'),
            cmd(f'/data get entity {pig} Health'),
            cmd(f'{attribute} get'),
            cmd(f'{attribute} base get'),
            cmd(f'{attribute} get 100'),
            cmd(f'{attribute} base get 100'),
        ]
    ops += [
        {'op': 'tick', 'n': 25},
        cmd(f'/execute if entity {pig}'),
        cmd(f'/data get entity {pig} Health'),
        cmd(f'{attribute} base reset'),
        cmd(f'{attribute} get'),
        cmd(f'{attribute} base get'),
        cmd(f'/data get entity {pig} Health'),
        *setup(f'/kill {pig}'),
    ]
    return ops

def setblock_destroy_air():
    """Destroy-to-air skips placement, including already-air targets (#7691)."""
    pos = '24 220 10'
    # Keep this section nonempty so strict placement can retain air variants.
    out = setup('/tick freeze', '/gamerule block_drops true',
                '/scoreboard objectives add destroy_air dummy',
                '/setblock 24 219 11 minecraft:stone strict')
    for before, after, mode in [
        ('stone', 'air', 'destroy'),
        ('air', 'air', 'destroy'),
        ('stone', 'cave_air', 'destroy'),
        ('cave_air', 'air', 'destroy'),
        ('cave_air', 'cave_air', 'destroy'),
        ('stone', 'void_air', 'destroy'),
        ('void_air', 'air', 'destroy'),
        ('water', 'air', 'destroy'),
        ('stone', 'dirt', 'destroy'),
        ('stone', 'stone', 'destroy'),
        ('air', 'dirt', 'destroy'),
        ('air', 'air', 'replace'),
        ('stone', 'air', 'replace'),
    ]:
        out += setup('/kill @e[type=minecraft:item]',
                     f'/setblock {pos} minecraft:{before} strict',
                     '/scoreboard players set result destroy_air -99',
                     '/scoreboard players set success destroy_air -99')
        out += [
            cmd(f'/execute if block {pos} minecraft:{before}'),
            cmd('/execute store success score success destroy_air '
                'store result score result destroy_air run '
                f'setblock {pos} minecraft:{after} {mode}'),
            cmd('/scoreboard players get success destroy_air'),
            cmd('/scoreboard players get result destroy_air'),
            *[cmd(f'/execute if block {pos} minecraft:{air}')
              for air in ('air', 'cave_air', 'void_air')],
            cmd('/execute if entity @e[type=minecraft:item]'),
            cmd('/execute as @e[type=minecraft:item] run data get entity @s Item'),
        ]
    # Skipping BlockInput.place must still notify neighbours. A strict write
    # leaves this unsupported power value intact until a neighbour updates it.
    out += setup(f'/setblock {pos} minecraft:air strict',
                 '/setblock 23 219 10 minecraft:stone strict',
                 '/setblock 23 220 10 minecraft:redstone_wire[power=15] strict')
    out += [cmd('/execute if block 23 220 10 minecraft:redstone_wire[power=15]'),
            cmd(f'/setblock {pos} minecraft:air destroy'),
            cmd('/execute if block 23 220 10 minecraft:redstone_wire[power=0]'),
            cmd(f'/execute if block {pos} minecraft:air')]
    out += setup('/setblock 23 220 10 minecraft:redstone_wire[power=15] strict')
    out += [cmd(f'/setblock {pos} minecraft:air replace'),
            cmd('/execute if block 23 220 10 minecraft:redstone_wire[power=15]')]
    out += setup('/kill @e[type=minecraft:item]', f'/setblock {pos} minecraft:air strict',
                 '/setblock 23 220 10 minecraft:air strict',
                 '/setblock 23 219 10 minecraft:air strict',
                 '/setblock 24 219 11 minecraft:air strict',
                 '/scoreboard objectives remove destroy_air', '/tick unfreeze')
    return out


def forceload_query_order():
    """ForceLoadCommand sorts signed packed chunk IDs, not coordinate tuples (#7451)."""
    return [
        *setup('/forceload remove all'),
        cmd('/forceload query'),
        cmd('/forceload query 32 0'),
        cmd('/forceload add 32 0'),
        cmd('/forceload query'),
        cmd('/forceload query 32 0'),
        cmd('/forceload add -16 0'),
        cmd('/forceload add 0 -16'),
        # Java 26.3: [0,-1], [2,0], [-1,0]. These replies say nothing
        # about whether the force-loaded chunks have finished loading.
        cmd('/forceload query'),
        cmd('/forceload remove 32 0'),
        cmd('/forceload query 32 0'),
        cmd('/forceload remove all'),
        cmd('/forceload query'),
    ]


def creeper_custom_fuse():
    """Observe timed entity removal, rather than just echoed raw Fuse NBT (#7435)."""
    ops = setup('/gamerule mob_griefing false')
    for x, tag, fuse in [(4.5, 'fuse_short', ',Fuse:1s'),
                         (14.5, 'fuse_default', ''), (24.5, 'fuse_long', ',Fuse:40s')]:
        ops += setup(f'/summon minecraft:creeper {x} 220 4.5 '
                     f'{{Tags:["{tag}"],NoAI:1b,NoGravity:1b,ignited:1b{fuse}}}')
        ops.append(cmd(f'/data get entity @e[tag={tag},limit=1] Fuse'))
    elapsed = 0
    for tick in (1, 29, 30, 39, 40):
        ops.append({'op': 'tick', 'n': tick - elapsed})
        for tag in ('fuse_short', 'fuse_default', 'fuse_long'):
            ops.append(cmd(f'/execute if entity @e[tag={tag}]'))
        elapsed = tick
    return ops + setup('/gamerule mob_griefing true')


def marker_data_mutation():
    """Marker data write-back preserves identity and modeled-entity controls (#8929)."""
    marker = '00000000-0000-0000-0000-000000000001'
    pig = '00000000-0000-0000-0000-000000000002'
    objective = 'marker_result'
    ops = setup('/forceload add 0 0 15 15', f'/scoreboard objectives add {objective} dummy')
    ops += [cmd('/execute if loaded 0 200 0', expect_result=1),
            cmd('/summon marker 4.5 200 4.5 {UUID:[I;0,0,0,1]}', expect_result=1),
            cmd(f'/execute if entity {marker}', expect_result=1),
            cmd(f'/data get entity {marker} Pos', expect_result=3),
            cmd(f'/data get entity {marker} UUID', expect_result=4)]

    def stored(command, result, success=True):
        return setup(f'/scoreboard players set result {objective} -99',
                     f'/scoreboard players set success {objective} -99') + [
            cmd(f'/execute store result score result {objective} '
                f'store success score success {objective} run {command}'),
            cmd(f'/scoreboard players get result {objective}', expect_result=result),
            cmd(f'/scoreboard players get success {objective}', expect_result=int(success))]

    def names(target):
        return (stored(f'data modify entity {target} CustomName set value "original"', 1)
                + stored(f'data modify entity {target} CustomName set value "hello"', 1)
                + stored(f'data get entity {target} CustomName', 5)
                + stored(f'data modify entity {target} CustomName set value "hello"', 0, False)
                + stored(f'data remove entity {target} CustomName', 1)
                + stored(f'data get entity {target} CustomName', 0, False)
                + [cmd(f'/execute if entity {target}', expect_result=1)])

    ops += names(marker)
    ops += stored(f'data modify entity {marker} Pos set value [6.25d,202d,7.75d]', 1)
    ops += stored(f'data get entity {marker} Pos', 3)
    ops += [cmd('/execute positioned 6.25 202 7.75 if entity '
                '@e[type=marker,nbt={UUID:[I;0,0,0,1]},distance=..0.01]', expect_result=1)]
    ops += stored(f'data modify entity {marker} Rotation set value [45f,-30f]', 1)
    ops += stored(f'data get entity {marker} Rotation', 2)
    ops += [cmd('/execute if entity @e[type=marker,nbt={UUID:[I;0,0,0,1]},'
                'y_rotation=45,x_rotation=-30]', expect_result=1)]
    ops += stored(f'data merge entity {marker} {{data:{{value:7,label:"plain"}}}}', 1)
    ops += stored(f'data get entity {marker} data', 2)
    ops += stored(f'data modify entity {marker} data.value set value 9', 1)
    ops += stored(f'data get entity {marker} data.value', 9)
    ops += stored(f'data remove entity {marker} data.label', 1)
    ops += stored(f'data get entity {marker} data', 1)
    # /tag already has a working raw-storage path; /data must join it.
    ops += [cmd(f'/tag {marker} add control', expect_result=1)]
    ops += stored(f'data modify entity {marker} Tags append value "mutated"', 1)
    ops += [cmd('/execute if entity @e[type=marker,tag=mutated]', expect_result=1),
            cmd(f'/tag {marker} list', expect_result=2)]
    ops += stored(f'data modify entity {marker} UUID set value [I;0,0,0,99]', 1)
    ops += stored(f'data get entity {marker} UUID', 4)
    ops += [cmd(f'/execute if entity {marker}', expect_result=1)]
    ops += stored('execute if entity @e[nbt={UUID:[I;0,0,0,99]}]', 0, False)
    ops += stored(f'data modify entity {marker} id set value "minecraft:pig"', 1)
    ops += [cmd('/execute if entity @e[type=marker,nbt={UUID:[I;0,0,0,1]}]', expect_result=1)]
    # Earlier groups can leave pigs loaded; the count below is about this group's pig only.
    ops += setup('/kill @e[type=minecraft:pig]')
    ops += [cmd('/summon pig 10.5 200 10.5 {UUID:[I;0,0,0,2],NoAI:1b,NoGravity:1b}', expect_result=1)]
    ops += names(pig)
    ops += stored(f'data modify entity {pig} Pos set value [10.25d,201d,11.75d]', 1)
    ops += stored(f'data get entity {pig} Pos', 3)
    ops += [cmd('/execute if entity @e[type=pig]', expect_result=1),
            cmd(f'/kill {marker}', expect_result=1), cmd(f'/kill {pig}', expect_result=1)]
    return ops + setup(f'/scoreboard objectives remove {objective}', '/forceload remove 0 0')
def recipe_special_arguments():
    """Recipe commands reject specials before book mutation; displays still count (#8883)."""
    objective = 'recipe_result'
    ops = setup(f'/scoreboard objectives add {objective} dummy', '/recipe take @s *')
    ops.append(cmd('/execute if entity @s[type=player]', expect_result=1))

    def recipe(verb, recipe_id, expected):
        return setup(f'/scoreboard players set result {objective} -99',
                     f'/scoreboard players set success {objective} -99') + [
            cmd(f'/execute store result score result {objective} '
                f'store success score success {objective} run recipe {verb} @s {recipe_id}'),
            cmd(f'/scoreboard players get result {objective}', expect_result=expected),
            cmd(f'/scoreboard players get success {objective}', expect_result=int(expected > 0))]

    for recipe_id, displays in [('minecraft:crafting_table', 1),
                                ('minecraft:leather_helmet_dyed', 1),
                                ('minecraft:tipped_arrow', 1), ('minecraft:map_cloning', 8)]:
        for verb, expected in [('give', displays), ('give', 0), ('take', displays), ('take', 0)]:
            ops += recipe(verb, recipe_id, expected)
    special = ['black_banner_duplicate', 'book_cloning', 'decorated_pot',
               'firework_rocket', 'repair_item', 'shield_decoration']
    invalid = special + ['h206_no_such_recipe', 'lingering_potion_awkward_blaze_powder']
    for recipe_id in invalid:
        for verb in ('give', 'take'):
            ops += recipe(verb, 'minecraft:' + recipe_id, 0)
    ops += recipe('give', '*', 1746)
    ops += recipe('give', '*', 0)
    # Explicit argument rejection is independent of whether the book is empty or full.
    for recipe_id in invalid:
        for verb in ('give', 'take'):
            ops += recipe(verb, 'minecraft:' + recipe_id, 0)
    ops += recipe('take', '*', 1746)
    ops += recipe('take', '*', 0)
    return ops + setup(f'/scoreboard objectives remove {objective}')
def mob_effect_update_result():
    """EffectCommands counts active/visibility updates, without advancing effect clocks (#8862)."""
    objective = 'effect_changed'
    ops = setup(f'/scoreboard objectives add {objective} dummy')
    pairs = [('equal_short', '10', 1, '5', 1, 'true', 0),
             ('equal_long', '5', 1, '10', 1, 'true', 1),
             ('weaker_short', '10', 2, '5', 0, 'true', 0),
             ('weaker_long', '5', 2, '10', 0, 'true', 0),
             ('stronger_short', '10', 0, '2', 2, 'true', 1),
             ('stronger_long', '2', 0, '10', 2, 'true', 1),
             ('infinite_finite', 'infinite', 1, '10', 1, 'true', 0),
             ('finite_infinite', '10', 1, 'infinite', 1, 'true', 1),
             ('equal_visible', '10', 1, '5', 1, 'false', 1),
             ('weaker_visible', '10', 2, '5', 0, 'false', 1)]

    def apply(target, seconds, amplifier, hide, expected):
        return setup(f'/scoreboard players set result {objective} -99',
                     f'/scoreboard players set success {objective} -99') + [
            cmd(f'/execute store result score result {objective} '
                f'store success score success {objective} run effect give '
                f'{target} speed {seconds} {amplifier} {hide}'),
            cmd(f'/scoreboard players get result {objective}', expect_result=expected),
            cmd(f'/scoreboard players get success {objective}', expect_result=int(expected > 0))]

    def fields(target):
        return [cmd(f'/data get entity {target} '
                    f'active_effects[{{id:"minecraft:speed"}}].{field}')
                for field in ('duration', 'amplifier', 'show_particles', 'show_icon')]

    for name, first, amp, second, second_amp, hide, changed in pairs:
        tag = f'effect_update_{name}'
        target = f'@e[tag={tag},limit=1]'
        ops += setup('/summon pig 4.5 220 4.5 '
                     f'{{Tags:["{tag}"],NoAI:1b,NoGravity:1b,PersistenceRequired:1b}}',
                     f'/effect clear {target}')
        ops += [cmd(f'/execute if entity {target}', expect_result=1),
                cmd(f'/execute if entity @e[tag={tag},nbt={{NoAI:1b}}]', expect_result=1),
                cmd(f'/execute unless entity @e[tag={tag},nbt={{active_effects:[{{id:"minecraft:speed"}}]}}]',
                    expect_result=1)]
        ops += apply(target, first, amp, 'true', 1)
        ops += apply(target, second, second_amp, hide, changed)
        ops += fields(target) + setup(f'/kill {target}')

    group = '@e[tag=effect_update_mixed]'
    for name in ('long', 'short', 'new'):
        ops += setup('/summon pig 4.5 220 4.5 '
                     f'{{Tags:["effect_update_mixed","effect_mix_{name}"],'
                     'NoAI:1b,NoGravity:1b,PersistenceRequired:1b}')
    ops += [cmd(f'/execute if entity {group}', expect_result=3),
            cmd('/execute if entity @e[tag=effect_update_mixed,nbt={NoAI:1b}]', expect_result=3),
            cmd('/execute unless entity @e[tag=effect_update_mixed,nbt={active_effects:[{id:"minecraft:speed"}]}]',
                expect_result=1)]
    ops += setup('/effect give @e[tag=effect_mix_long,limit=1] speed 10 1 true',
                 '/effect give @e[tag=effect_mix_short,limit=1] speed 2 1 true')
    ops += apply(group, '5', 1, 'true', 2)  # Two change among three selected mobs.
    ops += apply(group, '5', 1, 'true', 0)  # All three are now unchanged.
    for name in ('long', 'short', 'new'):
        ops += fields(f'@e[tag=effect_mix_{name},limit=1]')
    return ops + setup(f'/kill {group}', f'/scoreboard objectives remove {objective}')
def axolotl_integer_memory_ttl():
    """Loaded integer-memory TTL survives NoAI; behavior writes reset it (#8852)."""
    keys = ('play_dead_ticks', 'temptation_cooldown_ticks')
    ops = setup('/fill 0 217 0 15 217 15 stone', '/fill 0 218 0 15 225 15 water')

    def summon(tag, key, value, ttl, no_ai, x=8.5, z=12.5):
        lifetime = '' if ttl is None else f',ttl:{ttl}'
        return setup(f'/summon axolotl {x} 220 {z} '
                     f'{{Tags:["{tag}"],NoAI:{no_ai}b,NoGravity:1b,PersistenceRequired:1b,'
                     f'Brain:{{memories:{{"minecraft:{key}":{{value:{value}{lifetime}}}}}}}}}')

    def observe(tag, key, value, ttl):
        target = f'@e[tag={tag},limit=1]'
        path = f'Brain.memories."minecraft:{key}"'
        if value is None:
            memory = f'/execute unless data entity {target} {path}'
        else:
            # Typed predicates check the serialized tags without depending on SNBT styling.
            entry = '{value:' + str(value)
            if ttl is not None:
                entry += ',ttl:' + str(ttl) + 'L'
            entry += '}'
            nbt = '{Brain:{memories:{"minecraft:' + key + '":' + entry + '}}}'
            memory = f'/execute if entity @e[tag={tag},limit=1,nbt={nbt}]'
            if ttl is None:
                memory += f' unless data entity {target} {path}.ttl'
        return [cmd(f'/execute if entity {target}', expect_result=1),
                cmd(f'/execute if entity @e[tag={tag},limit=1,nbt={{NoAI:1b}}]'),
                cmd(f'/execute as {target} at @s anchored eyes positioned ^ ^ ^ '
                    'if block ~ ~ ~ minecraft:water', expect_result=1),
                cmd(memory, expect_result=1)]

    codecs = [(v, None, 7, None) for v in ('7', '7L', '7b', '7s', '7.5f', '7.5d')]
    codecs += [('"invalid"', '5L', None, None)]
    codecs += [('7', ttl, 7, 5) for ttl in ('5L', '5', '5b', '5s', '5.5d')]
    codecs += [('7', '"invalid"', 7, None), ('7', '0L', 7, 0), ('7', '-1L', 7, -1)]
    for key in keys:
        for case, (value, ttl, expected_value, expected_ttl) in enumerate(codecs):
            tag = f'axmem_codec_{key}_{case}'
            ops += summon(tag, key, value, ttl, 1)
            ops += observe(tag, key, expected_value, expected_ttl)
            ops += setup(f'/kill @e[tag={tag}]')
    # Clear dead numeric-control subjects before the shared tank's six actual ticks.
    ops.append({'op': 'tick', 'n': 25})
    cases = []
    for no_ai in (0, 1):
        for key in keys:
            for name, ttl in [('absent', None), ('zero', '0L'), ('one', '1L'),
                              ('five', '5L'), ('negative', '-1L'), ('max', '9223372036854775807L')]:
                tag = f'axmem_{no_ai}_{key}_{name}'
                i = len(cases)
                ops += summon(tag, key, '100', ttl, no_ai, 2.5 + (i % 6) * 2, 2.5 + (i // 6) * 2)
                cases.append((tag, key, no_ai, name))
    elapsed = 0
    for tick in (0, 1, 3, 6):
        if tick != elapsed:
            ops.append({'op': 'tick', 'n': tick - elapsed})
        elapsed = tick
        for tag, key, no_ai, name in cases:
            value = None if not no_ai and tick > 0 and name in ('zero', 'negative') else (
                100 if no_ai else 100 - tick)
            ttl = {'zero': 0, 'one': 1, 'five': 5, 'negative': -1}.get(name)
            if not no_ai and tick > 0:
                ttl = None  # The behavior's ordinary setMemory resets expiry.
            ops += observe(tag, key, value, ttl)
    return ops + setup('/kill @e[type=axolotl]', '/fill 0 218 0 15 225 15 air')
def parrot_saved_age():
    """Parrot saved Age is absent even when supplied; pigs retain modeled Age (#8842)."""
    ops = setup('/fill 3 219 3 6 219 6 stone')
    flags = 'NoAI:1b,NoGravity:1b,PersistenceRequired:1b'
    modes = [('plain', ''), ('absent', '{' + flags + '}'),
             ('zero', '{' + flags + ',Age:0}'),
             ('negative', '{' + flags + ',Age:-100}'),
             ('positive', '{' + flags + ',Age:100}')]
    for species in ('parrot', 'pig'):
        for mode, nbt in modes:
            tag = f'saved_age_{species}_{mode}'
            target = f'@e[tag={tag},limit=1]'
            ops += setup(f'/tag @e[type={species}] add h198_seen',
                         f'/kill @e[type={species}]',
                         f'/summon {species} 4.5 220 4.5 {nbt}'.rstrip(),
                         f'/tag @e[type={species},tag=!h198_seen,limit=1] add {tag}',
                         f'/tag {target} add h198_seen')
            for tick in (0, 1):
                if tick:
                    ops.append({'op': 'tick', 'n': 1})
                ops += [cmd(f'/execute if entity {target}', expect_result=1),
                        cmd(f'/data get entity {target} Age')]
            ops += setup(f'/kill {target}')
    return ops
def animal_noai_love():
    """Animal.aiStep love clocks run with NoAI; parrot Age inputs are ignored (#8825)."""
    ops = setup('/fill 3 219 3 6 219 6 stone')
    cases = [('adult', 0, 40), ('negative_age', -100, 40),
             ('positive_age', 100, 40), ('zero', 0, 0), ('negative', 0, -2)]
    for species in ('pig', 'parrot'):
        for no_ai in (0, 1):
            for name, age, love in cases:
                tag = f'love_{species}_{no_ai}_{name}'
                target = f'@e[tag={tag},limit=1]'
                variant = ',Variant:3' if species == 'parrot' else ''
                ops += setup(f'/summon {species} 4.5 220 4.5 '
                             f'{{Tags:["{tag}"],NoAI:{no_ai}b,NoGravity:1b,'
                             f'PersistenceRequired:1b,Age:{age},InLove:{love}{variant}}}')
                elapsed = 0
                for tick in (0, 1, 3):
                    if tick != elapsed:
                        ops.append({'op': 'tick', 'n': tick - elapsed})
                    elapsed = tick
                    ops.append(cmd(f'/execute if entity {target}', expect_result=1))
                    # Raw parrot Age omission is a separate save-field discrepancy.
                    fields = ('Age', 'InLove') if species == 'pig' else ('InLove',)
                    for field in fields:
                        ops.append(cmd(f'/data get entity {target} {field}'))
                    if species == 'parrot':
                        ops.append(cmd(f'/data get entity {target} Variant'))
                ops += setup(f'/kill {target}')
    for name, variant in [('absent', ''), ('high', ',Variant:9'), ('negative', ',Variant:-2')]:
        tag = f'love_parrot_variant_{name}'
        target = f'@e[tag={tag},limit=1]'
        ops += setup('/summon parrot 4.5 220 4.5 '
                     f'{{Tags:["{tag}"],NoAI:1b,NoGravity:1b,PersistenceRequired:1b{variant}}}')
        for tick in (0, 1):
            if tick:
                ops.append({'op': 'tick', 'n': 1})
            ops += [cmd(f'/execute if entity {target}', expect_result=1),
                    cmd(f'/data get entity {target} Variant')]
        ops += setup(f'/kill {target}')
    return ops
def turtle_summon_home():
    """Turtle load defaults precede summon placement; plain summons finalize (#8804)."""
    ops = setup('/forceload add -32 -32 31 31',
                '/fill 9 199 9 11 199 11 stone',
                '/fill -12 200 -14 -9 200 -11 stone')
    flags = 'NoAI:1b,NoGravity:1b,PersistenceRequired:1b'
    modes = [('plain', ''), ('empty', '{}'), ('controlled', '{' + flags + '}'),
             ('home', '{' + flags + ',home_pos:[I;4,5,6]}'),
             ('invalid', '{' + flags + ',home_pos:[I;4,5]}'),
             ('pos', '{' + flags + ',Pos:[-20.5d,210d,-20.5d]}')]
    for site, pos in [('positive', '10.5 200 10.5'), ('negative', '-10.25 201 -12.75')]:
        for mode, nbt in modes:
            tag = f'turtle_home_{site}_{mode}'
            target = f'@e[tag={tag},limit=1]'
            ops += setup('/kill @e[type=turtle]',
                         f'/summon turtle {pos} {nbt}'.rstrip(),
                         f'/tag @e[type=turtle,tag=!h187_seen,limit=1] add {tag}',
                         f'/tag {target} add h187_seen')
            for tick in (0, 1):
                if tick:
                    ops.append({'op': 'tick', 'n': 1})
                ops.append(cmd(f'/execute if entity {target}', expect_result=1))
                if tick and mode in ('plain', 'empty'):
                    # Check a finite position without pinning motion.
                    ops.append(cmd(f'/execute as {target} at @s if entity @s[distance=..0.0001]',
                                   expect_result=1))
                else:
                    ops.append(cmd(f'/data get entity {target} Pos'))
                ops.append(cmd(f'/data get entity {target} home_pos'))
                ops.append(cmd(f'/data get entity {target} has_egg'))
            if mode == 'controlled':
                ops += [cmd(f'/data remove entity {target} home_pos'),
                        cmd(f'/data get entity {target} home_pos'),
                        cmd(f'/data get entity {target} Pos')]
    return ops + setup('/kill @e[type=turtle]')
def command_minecart_loaded_success():
    """Loaded nonempty commands trigger the synced-data count reset (#8772)."""
    ops = setup('/fill 9 219 9 11 219 11 stone')
    for name, command in [('absent', ''), ('empty', ',Command:""'),
                          ('nonempty', ',Command:"say hi"')]:
        for count in (2, -3, 0):
            tag = f'cart_load_{name}_{str(count).replace("-", "n")}'
            target = f'@e[tag={tag},limit=1]'
            ops += setup('/summon command_block_minecart 10.5 220 10.5 '
                         f'{{Tags:["{tag}"],NoGravity:1b,TrackOutput:0b,'
                         f'SuccessCount:{count}{command}}}')
            for tick in (0, 1):
                if tick:
                    ops.append({'op': 'tick', 'n': 1})
                ops.append(cmd(f'/execute if entity {target}'))
                for field in ('Command', 'SuccessCount', 'TrackOutput', 'UpdateLastExecution'):
                    ops.append(cmd(f'/data get entity {target} {field}'))
            ops += setup(f'/kill {target}')
    return ops
def ravager_noai_counters():
    ops = setup('/difficulty normal', '/gamerule spawn_mobs false',
                '/gamerule mob_griefing false', '/forceload add 0 0 31 31',
                '/fill 0 199 0 15 199 15 stone', '/fill 0 200 0 15 206 15 air')
    case = 0
    for no_ai in (0, 1):
        for attack, stun, roar in ((20, 30, 5), (0, 0, 0), (-2, -3, -4), (2, 2, 0)):
            case += 1
            tag = f'h179_{case}'
            target = f'@e[type=minecraft:ravager,tag={tag},limit=1]'
            ops += setup('/summon ravager 10.5 200 10.5 '
                         f'{{Tags:["{tag}"],NoAI:{no_ai}b,NoGravity:1b,Silent:1b,'
                         f'PersistenceRequired:1b,AttackTick:{attack},StunTick:{stun},RoarTick:{roar}}}')
            elapsed = 0
            for tick in (0, 1, 2, 3, 10, 12, 22):
                if tick > elapsed:
                    ops.append({'op': 'tick', 'n': tick - elapsed})
                ops.append(cmd(f'/execute if entity {target}'))
                for field in ('AttackTick', 'StunTick', 'RoarTick'):
                    ops.append(cmd(f'/data get entity {target} {field}'))
                elapsed = tick
            ops += setup(f'/kill {target}')
    return ops + setup('/gamerule mob_griefing true', '/forceload remove 0 0 31 31')


def axolotl_hunting_memory():
    """Boolean memory value/presence and finite/permanent TTL (#8942), using typed predicates."""
    key = 'minecraft:has_hunting_cooldown'
    path = f'Brain.memories."{key}"'
    ops = setup('/forceload add 0 0 15 15', '/fill 0 199 0 15 206 15 glass hollow',
                '/fill 0 199 0 15 199 15 stone', '/fill 1 200 1 14 205 14 water')
    ops += [{'op': 'settle'}, cmd('/execute if loaded 0 201 0', expect_result=1)]
    serial = 0

    def summon(tag, value, ttl, no_ai, x=14.5, z=14.5):
        nonlocal serial
        serial += 1
        fields = [] if value is None else ['value:' + value]
        if ttl is not None:
            fields.append('ttl:' + ttl)
        entry = '{' + ','.join(fields) + '}'
        nbt = ('{NoGravity:1b,PersistenceRequired:1b,Health:14f,NoAI:' + str(no_ai) + 'b,'
               + f'UUID:[I;0,0,218,{serial}],Tags:["{tag}"],'
               + 'Brain:{memories:{"' + key + '":' + entry + '}}}')
        return [cmd(f'/summon axolotl {x} 201 {z} {nbt}', expect_result=1)], serial

    def probe(tag, uuid, no_ai, value, ttl):
        target = f'@e[tag={tag},limit=1]'
        # A missing entry is valid only while its subject exists; false is still a present value.
        exists = f'/execute if entity @e[tag={tag},nbt={{UUID:[I;0,0,218,{uuid}]}}]'
        ai = (f'/execute if entity @e[tag={tag},nbt={{NoAI:1b}}]' if no_ai else
              f'/execute as {target} unless data entity @s NoAI')
        if value is None:
            memory = f'/execute as {target} unless data entity @s {path}'
        else:
            entry = '{value:' + str(int(value)) + 'b'
            if ttl is not None:
                entry += ',ttl:' + str(ttl) + 'L'
            entry += '}'
            nbt = '{Brain:{memories:{"' + key + '":' + entry + '}}}'
            memory = f'/execute if entity @e[tag={tag},nbt={nbt}]'
            if ttl is None:
                memory += f' unless data entity {target} {path}.ttl'
        return [cmd(exists, expect_result=1), cmd(ai, expect_result=1),
                cmd(f'/execute if entity @e[tag={tag},nbt={{Health:14f}}]', expect_result=1),
                cmd(f'/execute as {target} at @s anchored eyes positioned ^ ^ ^ '
                    'if block ~ ~ ~ minecraft:water', expect_result=1),
                cmd(memory, expect_result=1)]

    # NbtOps boolean numbers use doubleValue()!=0, including fractions and integers over a byte.
    codecs = [(v, '5L', b, 5) for v, b in [
        ('2b', True), ('-1b', True), ('0s', False), ('1s', True), ('0', False),
        ('256', True), ('257', True), ('0L', False), ('1L', True), ('0f', False),
        ('0.5f', True), ('-0.5f', True), ('0d', False), ('0.5d', True),
        ('"true"', None), ('{}', None), (None, None)]]
    codecs += [('0b', t, False, expected) for t, expected in [
        ('3', 3), ('3b', 3), ('3s', 3), ('3.5d', 3), ('"invalid"', None)]]
    for i, (value, ttl, expected_value, expected_ttl) in enumerate(codecs):
        tag = f'axhunt_codec_{i}'
        added, uuid = summon(tag, value, ttl, 1)
        ops += added + probe(tag, uuid, 1, expected_value, expected_ttl)
    ops += setup('/kill @e[type=axolotl]')
    ops += [{'op': 'tick', 'n': 25}]
    subjects = []
    for no_ai in (0, 1):
        for value in (False, True):
            for ttl in (None, 3, 0, -1, 9223372036854775807):
                i = len(subjects)
                tag = f'axhunt_life_{i}'
                added, uuid = summon(tag, str(int(value)) + 'b',
                                     None if ttl is None else str(ttl) + 'L', no_ai,
                                     1.5 + (i % 5) * 3, 1.5 + (i // 5) * 3)
                ops += added
                subjects.append((tag, uuid, no_ai, value, ttl))
    for elapsed in (0, 1, 3):
        if elapsed:
            ops += [{'op': 'tick', 'n': 1 if elapsed == 1 else 2}]
        for tag, uuid, no_ai, value, ttl in subjects:
            expected_value = value
            if ttl in (None, 9223372036854775807):
                expected_ttl = None
            elif no_ai or elapsed == 0:
                expected_ttl = ttl
            elif ttl <= 0:
                expected_value, expected_ttl = None, None
            else:
                expected_ttl = ttl - elapsed
            ops += probe(tag, uuid, no_ai, expected_value, expected_ttl)
    return ops + setup('/kill @e[type=axolotl]', '/fill 0 199 0 15 206 15 air',
                       '/forceload remove 0 0')


def mannequin_pose_anchors():
    """Saved mannequin pose -> existing live dimensions -> exact eye anchor."""
    ops = setup('/tp @s 40.5 230 8.5', '/forceload add 32 0 63 31')
    ops += [{'op': 'settle'}]
    ops += [cmd(f'/execute if loaded {x} 200 {z}', expect_result=1)
            for x, z in ((32, 0), (48, 0), (32, 16), (48, 16))]
    ops += setup('/tick freeze', '/fill 32 200 0 47 204 15 air')
    # Raw pose getter normalization is separate; this gate pins live anchors.
    # Java 26.3 scale-1 eye positions, independent of Horizon's pose math.
    cases = ((None, '201.62000000476837'), ('standing', '201.62000000476837'),
             ('crouching', '201.2699999809265'), ('swimming', '200.40000000596046'),
             ('fall_flying', '200.40000000596046'), ('sleeping', '200.20000000298023'),
             ('spin_attack', '201.62000000476837'), ('dying', '201.62000000476837'),
             ('unknown', '201.62000000476837'), ('CROUCHING', '201.62000000476837'))
    for index, (pose, eye_y) in enumerate(cases):
        tag = f'mannequin_pose_{index}'
        target = f'@e[tag={tag},limit=1]'
        uuid = f'0,0,217,{index + 1}'
        pose_nbt = f',pose:"{pose}"' if pose is not None else ''
        ops += setup(f'/summon mannequin 36.5 200 4.5 {{UUID:[I;{uuid}],Tags:["mannequin_pose_actor","{tag}"],NoAI:1b,NoGravity:1b,immovable:1b{pose_nbt}}}')
        for tick in (0, 1):
            if tick:
                ops += setup('/tick step 1')
                ops += [{'op': 'tick', 'n': 1}]
            ops += setup('/kill @e[tag=mannequin_pose_eye]')
            eye_tag = f'mannequin_eye_{index}_{tick}'
            eye = f'@e[tag={eye_tag},limit=1]'
            ops += [cmd(f'/execute if entity {target}', expect_result=1),
                    cmd(f'/execute if entity @e[tag={tag},nbt={{UUID:[I;{uuid}]}}]', expect_result=1),
                    cmd(f'/data get entity {target} Pos'),
                    cmd(f'/execute if entity @e[tag={tag},nbt={{Pos:[36.5d,200d,4.5d]}}]', expect_result=1),
                    cmd(f'/execute as {target} at @s anchored eyes positioned ^ ^ ^ run summon marker ~ ~ ~ {{Tags:["mannequin_pose_eye","{eye_tag}"]}}', expect_result=1),
                    cmd(f'/execute if entity {eye}', expect_result=1),
                    cmd(f'/data get entity {eye} Pos'),
                    cmd(f'/execute if entity @e[tag={eye_tag},nbt={{Pos:[36.5d,{eye_y}d,4.5d]}}]', expect_result=1)]
        ops += setup('/kill @e[tag=mannequin_pose_eye]', '/kill @e[tag=mannequin_pose_actor]', '/tick step 25')
        ops += [{'op': 'tick', 'n': 25}]
    return ops + setup('/tick unfreeze', '/tp @s 8.5 230 8.5')


def primed_tnt_block_state():
    """PrimedTnt decodes block_state and saves its canonical live state."""
    cases = [None, '"minecraft:tnt"', '"minecraft:tnt[unstable=true]"',
             '"minecraft:stone"', '{id:"minecraft:tnt",properties:{unstable:"true"}}',
             '{id:"minecraft:oak_log",properties:{axis:"x"}}', '"audit_missing:block"']
    expected = ['"minecraft:tnt"', '"minecraft:tnt"', '"minecraft:tnt"',
                '"minecraft:stone"', cases[4], cases[5], '"minecraft:tnt"']
    ops = setup('/forceload add 0 16 15 31')
    ops.append(cmd('/execute if loaded 0 206 28', expect_result=1))
    for i, state in enumerate(cases):
        fields = f'UUID:[I;221,0,0,{i + 1}],Tags:["h221_{i}"],NoGravity:1b,fuse:400s'
        if state is not None:
            fields += f',block_state:{state}'
        ops += setup(f'/summon tnt {i + 1}.5 206 28.5 {{{fields}}}')
        target = f'@e[tag=h221_{i},limit=1]'
        ops.append(cmd(f'/execute if entity @e[tag=h221_{i},nbt={{UUID:[I;221,0,0,{i + 1}],fuse:400s}},limit=1]', expect_result=1))
        ops.append(cmd(f'/data get entity {target} block_state'))
        ops.append(cmd(f'/execute if entity @e[tag=h221_{i},nbt={{block_state:{expected[i]}}},limit=1]', expect_result=1))
    return ops


def hurting_projectile_float_inertia():
    """Float inertia widens before double motion multiplication; no impacts."""
    ops = setup('/forceload add 16 0 47 31', '/fill 16 199 0 47 199 31 stone',
                '/fill 16 200 0 47 213 31 air',
                '/fill 25 203 20 38 211 28 glass hollow',
                '/fill 26 204 21 37 210 27 water')
    # Finish terrain work before introducing the measured projectiles.
    ops.append({'op': 'settle'})
    ops.append(cmd('/execute if loaded 30 207 24', expect_result=1))
    ops.append(cmd('/execute if block 30 207 24 water', expect_result=1))
    cases = [('dry', 4.5, '.1'), ('custom', 14.5, '.2'), ('water', 24.5, '.1')]
    for i, (name, z, acceleration) in enumerate(cases):
        ops += setup(f'/summon fireball 30.5 207 {z} {{UUID:[I;222,0,0,{i + 1}],Tags:["h222_{name}"],ExplosionPower:1b,Motion:[.1d,0d,0d],acceleration_power:{acceleration}d}}')
        ops.append(cmd(f'/data get entity @e[tag=h222_{name},limit=1] acceleration_power'))
    dry = [(30.689999997615814, .1899999976158142),
           (30.96549999189377, .27549999427795413),
           (31.322224981981517, .35672499008774766)]
    for tick in range(3):
        ops.append({'op': 'tick', 'n': 1})
        for i, (name, z, acceleration) in enumerate(cases):
            target = f'@e[tag=h222_{name},limit=1]'
            ops.append(cmd(f'/execute as 000000de-0000-0000-0000-{i + 1:012x} if entity @s[tag=h222_{name}]', expect_result=1))
            ops.append(cmd(f'/data get entity {target} Pos'))
            ops.append(cmd(f'/data get entity {target} Motion'))
        x, vx = dry[tick]
        ops.append(cmd(f'/execute if entity @e[tag=h222_dry,nbt={{Pos:[{x}d,207d,4.5d],Motion:[{vx}d,0d,0d]}},limit=1]', expect_result=1))
        ops.append(cmd('/execute if block 31 207 24 water', expect_result=1))
    return ops


def allay_invalid_brain_retention():
    """Live failed Brain loads retain LIKED_PLAYER; valid empty memories clear it."""
    ops = setup('/setblock 6 199 6 stone', '/scoreboard objectives add h226 dummy',
                '/execute store result score before h226 run time query gametime')
    ops.append(cmd('/execute if loaded 6 200 6', expect_result=1))
    edits = [
        ('remove', 'Brain', True),
        ('modify', 'Brain set value 0', True),
        ('modify', 'Brain set value "bad"', True),
        ('modify', 'Brain set value {}', True),
        ('remove', 'Brain.memories', True),
        ('modify', 'Brain.memories set value 0', True),
        ('remove', 'Brain.memories."minecraft:liked_player"', False),
        ('modify', 'Brain set value {memories:{}}', False),
        ('modify', 'Brain.memories set value {}', False),
    ]
    memory = '{memories:{"minecraft:liked_player":{value:[I;1,2,3,4]}}}'
    for i, (mode, edit, retain) in enumerate(edits):
        target = f'000000e2-0000-0000-0000-{i + 1:012x}'
        tag = f'h226_{i}'
        ops += setup(f'/summon allay 6.5 200.5 6.5 {{UUID:[I;226,0,0,{i + 1}],Tags:["{tag}"],NoAI:1b,NoGravity:1b}}')
        ops.append(cmd(f'/data modify entity {target} Brain set value {memory}', expect_result=1))
        guard = f'/execute as {target} if entity @s[tag={tag},nbt={{Pos:[6.5d,200.5d,6.5d],Health:20f,NoAI:1b,NoGravity:1b}}]'
        ops.append(cmd(guard, expect_result=1))
        ops.append(cmd(f'/execute as {target} if entity @s[nbt={{Brain:{memory}}}]', expect_result=1))
        ops.append(cmd(f'/data {mode} entity {target} {edit}', expect_result=1))
        ops.append(cmd(guard, expect_result=1))
        # A valid empty map is success with result 0; a missing key is failure 0.
        for path, result, success in [
            ('Brain.memories', int(retain), 1),
            ('Brain.memories."minecraft:liked_player"', int(retain), int(retain)),
        ]:
            ops.append(cmd(f'/execute store result score result h226 store success score success h226 run data get entity {target} {path}'))
            ops.append(cmd('/scoreboard players get result h226', expect_result=result))
            ops.append(cmd('/scoreboard players get success h226', expect_result=success))
    ops += setup('/execute store result score after h226 run time query gametime')
    ops.append(cmd('/execute if score after h226 = before h226', expect_result=1))
    return ops


def large_fireball_fire():
    """LargeFireball.onHit fire eligibility, not random fire count/positions."""
    ops = setup('/gamerule random_tick_speed 0', '/gamerule fire_spread_radius_around_player 0',
                '/gamemode spectator', '/tp @s 40.5 230 16.5', '/forceload add 0 0 95 31',
                '/scoreboard objectives add fire_probe dummy')
    # Two bounded entity-ticking settle windows cover all six chunk columns;
    # no fixed startup sleeps. Return to the middle and verify all twelve.
    ops += setup('/tp @s 24.5 230 16.5')
    ops += [{'op': 'settle'}]
    ops += setup('/tp @s 72.5 230 16.5')
    ops += [{'op': 'settle'}]
    ops += setup('/tp @s 40.5 230 16.5')
    ops += [{'op': 'settle'}]
    ops += [cmd(f'/execute if loaded {x} 200 {z}', expect_result=1)
            for x in range(0, 96, 16) for z in (0, 16)]
    ops += setup('/tick freeze', '/fill 0 196 0 95 199 31 stone',
                 '/fill 0 200 0 95 208 31 air')
    for index, (label, x, griefing, tnt) in enumerate((('enabled', 40, True, False),
                                                     ('disabled', 60, False, False),
                                                     ('tnt', 80, True, True))):
        tag = f'fireball_fire_{label}'
        uuid = f'0,0,223,{index + 1}'
        kind = 'tnt' if tnt else 'fireball'
        y = '200' if tnt else '201.5'
        data = 'fuse:1s' if tnt else 'ExplosionPower:3,Motion:[0d,-0.5d,0d],acceleration_power:0.1d'
        target = f'@e[tag={tag},limit=1]'
        ops += setup(f'/gamerule mob_griefing {str(griefing).lower()}',
                     f'/summon {kind} {x + 0.5} {y} 16.5 {{UUID:[I;{uuid}],Tags:["fireball_fire_actor","{tag}"],{data}}}')
        # Projectile UUID serialization is a separate view defect. Resolve
        # the actual fixed UUID and require its unique tag, without that getter.
        identity = (f'/execute if entity @e[tag={tag},nbt={{UUID:[I;{uuid}]}}]' if tnt else
                    f'/execute as 00000000-0000-0000-0000-00df{index + 1:08x} if entity @s[tag={tag}]')
        ops += [cmd(f'/execute if entity {target}', expect_result=1),
                cmd(identity, expect_result=1),
                cmd(f'/data get entity {target} Pos', expect_result=3),
                cmd(f'/execute if block {x} 199 16 stone', expect_result=1)]
        ticks = 10 if tnt else 40
        ops += setup(f'/tick step {ticks}')
        ops += [{'op': 'tick', 'n': ticks}]
        ops += [cmd(f'/execute unless entity {target}', expect_result=1),
                cmd(f'/execute if loaded {x - 8} 200 8', expect_result=1),
                cmd(f'/execute if loaded {x + 8} 200 24', expect_result=1)]
        sign = 'unless' if griefing or tnt else 'if'
        ops += [cmd(f'/execute {sign} block {x} 199 16 stone', expect_result=1)]
        ops += setup('/scoreboard players set #fire fire_probe 0')
        # Preserve unmodified counts in both raw records and the report, but
        # compare only presence: explosion random streams/geometry are separate.
        ops += [cmd(f'/execute store result score #fire fire_probe run fill {x - 8} 197 8 {x + 8} 206 24 air replace fire',
                    comparison='positive_result'),
                cmd(f'/execute if score #fire fire_probe matches {"1.." if griefing and not tnt else "0"}', expect_result=1)]
    return ops + setup('/kill @e[tag=fireball_fire_actor]', '/scoreboard objectives remove fire_probe',
                       '/gamerule mob_griefing true', '/gamerule fire_spread_radius_around_player 128',
                       '/gamerule random_tick_speed 3', '/tick unfreeze',
                       '/gamemode creative', '/tp @s 8.5 230 8.5')


def mob_partial_block_suffocation():
    """Real base ticks: suffocation predicates plus the actual shape at the eye."""
    ops = setup('/gamerule random_tick_speed 0', '/gamerule mob_griefing false',
                '/fill 8 198 8 12 199 12 stone')
    ops.append(cmd('/execute if loaded 10 201 10', expect_result=1))
    blocks = ['stone', 'glass', 'red_stained_glass', 'oak_leaves[persistent=true]',
              'copper_grate', 'farmland', 'mud']
    for i, block in enumerate(blocks):
        target = f'000000e1-0000-0000-0000-{i + 1:012x}'
        tag = f'h225_{i}'
        ops += setup('/fill 8 200 8 12 205 12 air', f'/setblock 10 201 10 {block}',
                     f'/summon cow 10.5 200 10.5 {{UUID:[I;225,0,0,{i + 1}],Tags:["{tag}"],NoAI:1b,Silent:1b,PersistenceRequired:1b}}')
        guard = f'/execute as {target} if entity @s[tag={tag},nbt={{Pos:[10.5d,200d,10.5d],NoAI:1b,Silent:1b,PersistenceRequired:1b}}]'
        ops.append(cmd(guard, expect_result=1))
        ops.append(cmd(f'/data get entity {target} Health', expect_result=10))
        for ticks, n in [(1, 1), (21, 20)]:
            ops.append({'op': 'tick', 'n': n})
            ops.append(cmd(guard, expect_result=1))
            ops.append(cmd(f'/data get entity {target} Pos'))
            eye = f'h225_eye_{i}_{ticks}'
            ops += setup(f'/execute as {target} at @s anchored eyes run summon marker ^ ^ ^ {{Tags:["{eye}"]}}')
            ops.append(cmd(f'/execute if entity @e[tag={eye},nbt={{Pos:[10.5d,201.29999995231628d,10.5d]}},limit=1]', expect_result=1))
            ops.append(cmd(f'/data get entity @e[tag={eye},limit=1] Pos'))
            ops.append(cmd(f'/execute at @e[tag={eye},limit=1] if block ~ ~ ~ {block}', expect_result=1))
            hurts = block in ('stone', 'farmland', 'mud')
            health = (9 if ticks == 1 else 7) if hurts else 10
            ops.append(cmd(f'/data get entity {target} Health', expect_result=health))
            ops += setup(f'/kill @e[tag={eye}]')
        ops += setup(f'/kill {target}')
        ops.append({'op': 'tick', 'n': 25})
    return ops



def mob_baby_attribute_reads():
    """Effective/transient command reads, with raw-base and permanent controls."""
    ops = setup('/fill 0 199 0 6 199 6 stone', '/scoreboard objectives add h227 dummy',
                '/execute store result score before h227 run time query gametime')
    ops.append(cmd('/execute if loaded 2 200 2', expect_result=1))
    next_id = 0
    def spawn(kind, baby):
        nonlocal next_id
        next_id += 1
        target = f'000000e3-0000-0000-0000-{next_id:012x}'
        tag = f'h227_{next_id}'
        ops.extend(setup(f'/summon {kind} 2.5 200 2.5 {{UUID:[I;227,0,0,{next_id}],Tags:["{tag}"],NoAI:1b,IsBaby:{int(baby)}b}}'))
        return target, tag
    def reads(subject, baby, effective=None, modifier=None):
        target, tag = subject
        ops.append(cmd(f'/execute as {target} if entity @s[tag={tag},nbt={{Pos:[2.5d,200d,2.5d],Health:20f,NoAI:1b,IsBaby:{int(baby)}b}}]', expect_result=1))
        ops.append(cmd(f'/attribute {target} movement_speed base get 1000000', expect_result=230000))
        ops.append(cmd(f'/attribute {target} movement_speed get 1000000', expect_result=effective if effective is not None else (345000 if baby else 230000)))
        ops.append(cmd(f'/attribute {target} movement_speed modifier value get minecraft:baby 1000000', expect_result=modifier if modifier is not None else (500000 if baby else None)))
    for kind in ['zombie', 'husk', 'drowned', 'zombie_villager', 'zombified_piglin']:
        adult = spawn(kind, False)
        reads(adult, False)
        baby = spawn(kind, True)
        reads(baby, True)
        for state in (True, False):
            ops.append(cmd(f'/data merge entity {adult[0]} {{IsBaby:{int(state)}b}}', expect_result=1))
            reads(adult, state)
    # A distinct permanent modifier remains visible beside the transient one.
    for baby in (False, True):
        subject = spawn('zombie', baby)
        target = subject[0]
        ops.append(cmd(f'/attribute {target} movement_speed modifier add h227:permanent 0.125 add_value', expect_result=1))
        reads(subject, baby, 532500 if baby else 355000)
        ops.append(cmd(f'/attribute {target} movement_speed modifier value get h227:permanent 1000000', expect_result=125000))
        ops.append(cmd(f'/attribute {target} movement_speed modifier remove h227:permanent', expect_result=1))
        reads(subject, baby)
    # The same identifier is a valid permanent modifier on an adult.
    subject = spawn('zombie', False)
    target = subject[0]
    ops.append(cmd(f'/attribute {target} movement_speed modifier add minecraft:baby 0.125 add_value', expect_result=1))
    reads(subject, False, 355000, 125000)
    ops.append(cmd(f'/attribute {target} movement_speed modifier remove minecraft:baby', expect_result=1))
    reads(subject, False)
    ops.extend(setup('/execute store result score after h227 run time query gametime'))
    ops.append(cmd('/execute if score after h227 = before h227', expect_result=1))
    return ops



def cube_saved_age():
    """Age is absent for live cubes, including supplied raw values (#8985)."""
    ops = setup('/fill 0 199 0 15 199 15 stone')
    ops.append(cmd('/execute if loaded 10 200 10', expect_result=1))
    subjects = []
    for row, kind in enumerate(('slime', 'magma_cube')):
        for col, age in enumerate((None, 0, -24000, 37)):
            n = row * 4 + col + 1
            target = f'000000e4-0000-0000-0000-{n:012x}'
            x, z = 2.5 + col * 4, 2.5 + row * 8
            extra = '' if age is None else f',Age:{age}'
            ops += setup(f'/summon minecraft:{kind} {x} 200 {z} '
                         f'{{UUID:[I;228,0,0,{n}],Tags:["h228_{n}"],NoAI:1b,Silent:1b,PersistenceRequired:1b,Size:3{extra}}}')
            subjects.append((target, n, x, z))
    pig = '000000e4-0000-0000-0000-000000000009'
    ops += setup(f'/summon pig 6.5 200 6.5 {{UUID:[I;228,0,0,9],Tags:["h228_pig"],NoAI:1b,Age:37}}')
    for tick in (0, 1):
        if tick:
            ops.append({'op': 'tick', 'n': 1})
        for target, n, x, z in subjects:
            ops.append(cmd(f'/execute as {target} if entity @s[tag=h228_{n},nbt={{Pos:[{x}d,200d,{z}d],Health:16f,NoAI:1b,Silent:1b,PersistenceRequired:1b,Size:3}}]', expect_result=1))
            ops.append(cmd(f'/data get entity {target} Age'))
        ops.append(cmd(f'/execute as {pig} if entity @s[tag=h228_pig,nbt={{Health:10f,NoAI:1b}}]', expect_result=1))
        ops.append(cmd(f'/data get entity {pig} Age', expect_result=37 - tick))
    return ops



def villager_wake_rotation():
    """Use the occupied bed through the live player route, then observe pose."""
    ops = setup('/fill 0 199 3 6 199 9 stone', '/gamemode survival', '/tp @s 4.5 200 4.5', '/time set noon')
    ops.append({'op': 'settle'})
    target = None
    for n, pitch in enumerate((0, 23)):
        if n:
            ops += setup(f'/kill {target}')
            ops.append({'op': 'tick', 'n': 2})
        target = f'000000e7-0000-0000-0000-{n + 1:012x}'
        ops += setup('/setblock 2 200 7 red_bed[facing=north,part=foot,occupied=true] strict',
                     '/setblock 2 200 6 red_bed[facing=north,part=head,occupied=true] strict')
        ops.append({'op': 'settle'})
        ops += setup(f'/summon villager 2.5 200.6875 6.5 {{UUID:[I;231,0,0,{n + 1}],Tags:["h231"],NoAI:1b,Health:20f,sleeping_pos:[I;2,200,6],Rotation:[0f,{pitch}f]}}')
        ops.append(cmd(f'/execute as {target} if entity @s[tag=h231,nbt={{Health:20f,NoAI:1b,Pos:[2.5d,200.6875d,6.5d]}}]', expect_result=1))
        ops.append(cmd('/execute if entity @s[gamemode=survival,nbt={Pos:[4.5d,200d,4.5d]}]', expect_result=1))
        for z in (6, 7):
            ops.append(cmd(f'/execute if block 2 200 {z} red_bed[occupied=true]', expect_result=1))
        ops.append(cmd(f'/data get entity {target} Rotation'))
        ops.append({'op': 'use_block', 'pos': [2, 200, 6], 'face': 1})
        ops.append({'op': 'tick', 'n': 1})
        ops.append(cmd(f'/execute as {target} if entity @s[tag=h231,nbt={{Health:20f,NoAI:1b,Pos:[3.5d,200d,6.5d]}}]', expect_result=1))
        ops.append(cmd(f'/data get entity {target} Rotation'))
        for z in (6, 7):
            ops.append(cmd(f'/execute if block 2 200 {z} red_bed[occupied=false]', expect_result=1))
        ops.append(cmd('/execute unless data entity @s sleeping_pos', expect_result=1))
    ops += setup('/kill @e[tag=h231]', '/setblock 2 200 6 air strict',
                 '/setblock 2 200 7 air strict', '/gamemode creative',
                 '/tp @s 8.5 230 8.5', '/time set 1000')
    ops.append({'op': 'settle'})
    return ops



def awake_villager_saved_position():
    """A real bed-use wake clears stale saved sleeping_pos, with live guards."""
    ops = setup('/fill 0 199 3 6 199 9 stone', '/gamemode survival',
                '/tp @s 4.5 200 4.5', '/time set noon',
                '/setblock 2 200 7 red_bed[facing=north,part=foot,occupied=true] strict',
                '/setblock 2 200 6 red_bed[facing=north,part=head,occupied=true] strict')
    ops.append({'op': 'settle'})
    target = '000000e8-0000-0000-0000-000000000001'
    ops += setup('/summon villager 2.5 200.6875 6.5 {UUID:[I;232,0,0,1],Tags:["h232"],NoAI:1b,Health:20f,sleeping_pos:[I;2,200,6]}')
    ops.append(cmd(f'/execute as {target} if entity @s[tag=h232,nbt={{Health:20f,NoAI:1b,Pos:[2.5d,200.6875d,6.5d]}}]', expect_result=1))
    ops.append(cmd('/execute if entity @s[gamemode=survival,nbt={Pos:[4.5d,200d,4.5d]}]', expect_result=1))
    for z in (6, 7):
        ops.append(cmd(f'/execute if block 2 200 {z} red_bed[occupied=true]', expect_result=1))
    ops.append(cmd(f'/data get entity {target} sleeping_pos', expect_result=3))
    ops += [{'op': 'use_block', 'pos': [2, 200, 6], 'face': 1}, {'op': 'tick', 'n': 1}]
    ops.append(cmd(f'/execute as {target} if entity @s[tag=h232,nbt={{Health:20f,NoAI:1b,Pos:[3.5d,200d,6.5d]}}]', expect_result=1))
    for z in (6, 7):
        ops.append(cmd(f'/execute if block 2 200 {z} red_bed[occupied=false]', expect_result=1))
    ops.append(cmd('/execute unless data entity @s sleeping_pos', expect_result=1))
    ops.append(cmd(f'/data get entity {target} sleeping_pos'))
    ops.append(cmd(f'/execute unless data entity {target} sleeping_pos', expect_result=1))
    # A live villager without supplied sleep data is already saved awake.
    awake = '000000e8-0000-0000-0000-000000000002'
    ops += setup('/summon villager 5.5 200 7.5 {UUID:[I;232,0,0,2],Tags:["h232"],NoAI:1b,Health:20f}')
    ops.append(cmd(f'/execute as {awake} if entity @s[tag=h232,nbt={{Health:20f,NoAI:1b,Pos:[5.5d,200d,7.5d]}}]', expect_result=1))
    ops.append(cmd(f'/data get entity {awake} sleeping_pos'))
    ops.append(cmd(f'/execute unless data entity {awake} sleeping_pos', expect_result=1))
    ops += setup('/kill @e[tag=h232]', '/setblock 2 200 6 air strict',
                 '/setblock 2 200 7 air strict', '/gamemode creative',
                 '/tp @s 8.5 230 8.5', '/time set 1000')
    ops.append({'op': 'settle'})
    return ops



def projectile_saved_uuid():
    """Direct identity guards are separate from repeated saved UUID views."""
    from uuid import UUID
    ops = setup('/scoreboard objectives add h240 dummy',
                '/execute store result score before h240 run time query gametime')
    ops.append(cmd('/execute if loaded 2 200 2', expect_result=1))
    kinds = ['arrow', 'spectral_arrow', 'fireball', 'small_fireball',
             'wind_charge', 'breeze_wind_charge', 'snowball', 'egg',
             'ender_pearl', 'splash_potion', 'lingering_potion',
             'experience_bottle',
             'minecart', 'evoker_fangs', 'marker']
    for n, kind in enumerate(kinds, 1):
        target = str(UUID(int=(240 << 96) | (0xffffffff << 64) | (n << 32) | 0x80000000))
        array = f'[I;240,-1,{n},-2147483648]'
        tag = f'h240_{n}'
        x, z = 2.5 + (n % 4) * 3, 2.5 + (n // 4) * 3
        ops += setup(f'/summon minecraft:{kind} {x} 200 {z} {{UUID:{array},Tags:["{tag}"],NoGravity:1b,HasBeenShot:1b}}')
        guard = f'/execute as {target} if entity @s[tag={tag}]'
        ops.append(cmd(guard, expect_result=1))
        for _ in range(2):
            if kind == 'spectral_arrow':
                # Its separate display-name gap must not mask the UUID value.
                ops += setup('/data remove storage h240:reads current',
                             f'/data modify storage h240:reads current set from entity {target} UUID')
                ops.append(cmd('/data get storage h240:reads current', expect_result=4))
            else:
                ops.append(cmd(f'/data get entity {target} UUID', expect_result=4))
            ops.append(cmd(f'/execute as {target} if entity @s[nbt={{UUID:{array}}}]', expect_result=1))
        ops.append(cmd(guard, expect_result=1))
    ops += setup('/execute store result score after h240 run time query gametime')
    ops.append(cmd('/execute if score after h240 = before h240', expect_result=1))
    ops += setup('/kill @e[tag=h240_1]')
    # Only our fixed, tagged subjects are removed, without advancing the world.
    for n in range(2, len(kinds) + 1):
        ops += setup(f'/kill @e[tag=h240_{n}]')
    ops += setup('/data remove storage h240:reads current',
                 '/scoreboard objectives remove h240')
    return ops



def sulfur_cube_loaded_health():
    # Size is saved as physical size minus one. Babies age even with NoAI.
    cases = [(0, None, -24000, 4), (1, None, 0, 8),
             (3, None, -24000, 16), (0, 2.5, -24000, 2.5),
             (1, 6, 0, 6), (3, 12, -24000, 12),
             (None, None, 0, 4)]
    lines = [cmd('/forceload add 0 0 31 31'), {'op': 'settle'}]
    for x, z in [(0, 0), (16, 0), (0, 16), (16, 16)]:
        lines.append(cmd(f'/execute if loaded {x} 200 {z}', expect_result=1))
    lines.extend(setup('/fill 0 199 0 31 208 31 air'))
    lines.append(cmd('/fill 0 199 0 31 199 31 stone', expect_result=1024))
    for i, (size, saved_health, age, health) in enumerate(cases):
        tag = f'cube_health_{i}'
        uuid = f'00000000-0000-0000-0000-00e60000000{i + 1}'
        x, z = 4.5 + 6 * (i % 4), 4.5 + 10 * (i // 4)
        nbt = (f'UUID:[I;0,0,230,{i + 1}],Tags:["cube_loaded_health","{tag}"],NoAI:1b,'
               f'NoGravity:1b,Silent:1b,PersistenceRequired:1b,Age:{age}')
        if size is not None:
            nbt += f',Size:{size}'
        if saved_health is not None:
            nbt += f',Health:{saved_health}f'
        lines.append(cmd(f'/summon sulfur_cube {x} 200 {z} {{{nbt}}}', expect_result=1))
        for tick in range(2):
            if tick:
                lines.append({'op': 'tick', 'n': 1})
            lines.extend([
                cmd(f'/execute if entity @e[type=sulfur_cube,tag={tag}]', expect_result=1),
                cmd(f'/execute as {uuid} if entity @s[tag={tag},nbt={{NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b}}]', expect_result=1),
                cmd(f'/data get entity {uuid} UUID', expect_result=4),
                cmd(f'/data get entity {uuid} Pos', expect_result=3),
                cmd(f'/data get entity {uuid} Size', expect_result=size or 0),
                cmd(f'/data get entity {uuid} Health 100', expect_result=int(health * 100)),
                cmd(f'/data get entity {uuid} Age', expect_result=age + (tick if age < 0 else 0)),
            ])
    # No NBT invokes spawn finalization: an adult starts at physical size 2.
    selector = '@e[type=sulfur_cube,tag=!cube_loaded_health]'
    lines.append(cmd('/summon sulfur_cube 26.5 200 26.5', expect_result=1))
    for tick in range(2):
        if tick:
            lines.append({'op': 'tick', 'n': 1})
        lines.extend([
            cmd(f'/execute if entity {selector}', expect_result=1),
            cmd(f'/data get entity {selector[:-1]},limit=1] Size', expect_result=1),
            cmd(f'/data get entity {selector[:-1]},limit=1] Health 100', expect_result=800),
        ])
    lines.extend(setup('/kill @e[type=sulfur_cube]', '/forceload remove 0 0 31 31'))
    return lines



def numeric_array_edits(include_storage_side_effects=False):
    """Replay list/string controls before the finite numeric-array edit cases."""
    ops = setup('/tick freeze', '/scoreboard objectives add audit dummy',
                '/execute store result score clockBefore audit run time query gametime')
    def edit(initial, suffix, result, source=None):
        ops.extend(setup('/data remove storage audit:edit value',
                         f'/data modify storage audit:edit value set value {initial}',
                         '/scoreboard players set result audit 17',
                         '/scoreboard players set success audit 17'))
        if source is not None:
            ops.extend(setup(f'/data modify storage audit:source value set value {source}'))
        ops.append(cmd('/execute store result score result audit store success score success audit run data ' + suffix))
        ops.append(cmd('/scoreboard players get result audit', expect_result=result))
        ops.append(cmd('/scoreboard players get success audit', expect_result=int(result > 0)))
        ops.append(cmd('/data get storage audit:edit value'))
    def modify(initial, tail, result=1):
        edit(initial, 'modify storage audit:edit ' + tail, result)
    # The audit's original 22 edit actions, including heterogeneous lists.
    for tail, result in [('value append value 4', 1), ('value append value "four"', 1),
                         ('value prepend value 4b', 1), ('value[] set value 9', 3)]:
        modify('[1,2,3]', tail, result)
    for path, result in [('value[-1]', 1), ('value[9]', 0), ('value[]', 3)]:
        edit('[1,2,3]', 'remove storage audit:edit ' + path, result)
    modify('[1,2,3]', 'value[1] set value 2', 0)
    for index, result in [(0, 1), (3, 1), (4, 0), (-1, 1), (-4, 1), (-5, 0)]:
        modify('[1,2,3]', f'value insert {index} value 8', result)
    modify('[B;1b,2b]', 'value append value 257')
    modify('[I;1,2]', 'value append value 3.5d')
    modify('[L;1L,2L]', 'value append value 3')
    modify('[B;1b,2b]', 'value[0] set value 7b')
    edit('[I;1,2]', 'remove storage audit:edit value[0]', 1)
    modify('[[1,2],[3,4]]', 'value[0] append value 5')
    modify('[[1,2],[3,4]]', 'value[] append value 5', 2)
    modify('[[1,2],[3,4]]', 'value[-1] prepend value 5')
    # The original 13 string actions use UTF-16 boundaries without lone surrogates.
    strings = [('"abcd"', '1 3', 1), ('"abcd"', '-3 -1', 1),
               ('"abcd"', '2 2', 1), ('"abcd"', '3 1', 0), ('"abcd"', '0 5', 0),
               ('"AéB"', '1 2', 1), ('"A😀B"', '1 3', 1), ('"A😀B"', '3 4', 1),
               ('"A😀B"', '-3 -1', 1), ('"A😀B"', '-1', 1),
               ('3.0f', '', 1), ('3.0d', '', 1), ('1.0E20d', '', 1)]
    for source, indices, result in strings:
        suffix = 'modify storage audit:edit value set string storage audit:source value'
        edit('"unchanged"', suffix + (' ' + indices if indices else ''), result, source)
    # Destination-specific negative rounding, admission and raw-tag change counts.
    for kind, initial, empty in [('B', '[B;1b,2b]', '[B;]'),
                                 ('I', '[I;1,2]', '[I;]'), ('L', '[L;1L,2L]', '[L;]')]:
        for value in ('3.5f', '-3.5f', '3.5d', '-3.5d', '"bad"'):
            modify(initial, f'value append value {value}', int(value != '"bad"'))
        for value in ('-3.5d', '"bad"'):
            modify(initial, f'value prepend value {value}', int(value != '"bad"'))
        for value in ('-3.5f', '"bad"'):
            modify(initial, f'value insert 1 value {value}', int(value != '"bad"'))
        for value in ('-3.5f', '-3.5d', '"bad"'):
            modify(initial, f'value[0] set value {value}', int(value != '"bad"'))
            if value != '"bad"' or include_storage_side_effects:
                modify(initial, f'value[] set value {value}', 0 if value == '"bad"' else 2)
        modify(initial, 'value[0] set value 1.0f')
        modify(initial, 'value[] set value 1', 1 if kind == 'I' else 2)
        # AllElementsNode's empty collection branch returns 1 even on rejection.
        modify(empty, 'value[] set value "bad"')
        modify(empty, 'value[] set value -3.5f')
        modify(empty, 'value append value "bad"', 0)
    ops.extend(setup('/execute store result score clockAfter audit run time query gametime'))
    ops.append(cmd('/execute if score clockAfter audit = clockBefore audit', expect_result=1))
    ops.extend(setup('/data remove storage audit:edit value', '/data remove storage audit:source value',
                     '/scoreboard objectives remove audit', '/tick unfreeze'))
    return ops



def sulfur_cube_fuse_ticks(immediate_power=False):
    """SulfurCube.tickFuse runs with NoAI; customServerAiStep timers do not (#9018)."""
    ops = setup('/forceload add 0 0 79 15',
                '/fill 0 199 0 79 199 15 stone', '/fill 0 200 0 79 206 15 air',
                '/gamerule tnt_explodes false')
    ops.append({'op': 'settle'})
    ops.append(cmd('/execute if loaded 68 200 4', expect_result=1))

    def target(n):
        return f'000000f3-0000-0000-0000-{n:012x}'

    def summon(n, x, ai, body, extra=''):
        # Explicit Health isolates the separate absent-Health load issue (#8983).
        ops.extend(setup(f'/summon sulfur_cube {x}.5 200 4.5 '
                         f'{{UUID:[I;243,0,0,{n}],Tags:["sulfur_fuse"],Size:1,Health:4f,'
                         f'NoAI:{ai}b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,'
                         f'equipment:{{body:{{id:"minecraft:{body}"}}}}{extra}}}'))
        ops.append(cmd(f'/execute if entity {target(n)}', expect_result=1))
        if immediate_power:
            # Keep the original 76-observation H249 diagnostic byte-for-byte.
            ops.append(cmd(f'/execute if entity @e[nbt={{UUID:[I;243,0,0,{n}],Health:4f,'
                           f'NoGravity:1b,Size:1,Tags:["sulfur_fuse"]}}]', expect_result=1))
        else:
            ops.append(cmd(f'/execute if entity @e[nbt={{UUID:[I;243,0,0,{n}],Health:4f,'
                           f'NoGravity:1b,Silent:1b,PersistenceRequired:1b,Size:1,'
                           f'equipment:{{body:{{id:"minecraft:{body}"}}}},Tags:["sulfur_fuse"]}}]',
                           expect_result=1))
            mode = 'if' if ai else 'unless'
            ops.append(cmd(f'/execute {mode} entity @e[nbt={{UUID:[I;243,0,0,{n}],NoAI:1b}}]',
                           expect_result=1))

    # Loaded short fuses pin both AI modes, including exact expiry, without a blast.
    # The default short-fuse/cooldown actors share the observer's loaded chunk.
    # The diagnostic retains its original distant positions and readiness guards.
    summon(1, 4 if immediate_power else 2, 1, 'tnt', ',fuse:3')
    summon(2, 16 if immediate_power else 5, 0, 'tnt', ',fuse:3')
    summon(3, 28 if immediate_power else 8, 1, 'tnt')
    summon(4, 40 if immediate_power else 11, 1, 'white_wool', ',pickup_timer:100')
    summon(5, 52 if immediate_power else 14, 0, 'white_wool', ',pickup_timer:100')
    for n in (1, 2, 3):
        ops.append(cmd(f'/data get entity {target(n)} fuse'))
    for tick in (1, 2, 3):
        ops.append({'op': 'tick', 'n': 1})
        for n in (1, 2):
            ops.append(cmd(f'/execute if entity {target(n)}',
                           expect_result=1 if tick < 3 else None))
            if tick < 3:
                ops.append(cmd(f'/data get entity {target(n)} fuse', expect_result=3 - tick))
        ops.append(cmd(f'/data get entity {target(3)} fuse'))
        for n in (4, 5):
            ops.append(cmd(f'/execute if entity {target(n)}', expect_result=1))
            ops.append(cmd(f'/data get entity {target(n)} pickup_timer',
                           expect_result=100 if n == 4 else 100 - tick))
    ops.extend(setup('/kill @e[tag=sulfur_fuse]', '/gamerule tnt_explodes true',
                     '/gamerule mob_griefing true'))
    if immediate_power:
        ops.extend(setup('/setblock 28 199 4 redstone_block'))
    ops.extend(setup('/setblock 6 200 4 dirt'))
    # Actual damage/power priming, a nonexplosive body, and an idle TNT control.
    summon(6, 4, 1, 'tnt')
    summon(7, 16, 1, 'tnt')
    summon(8, 28, 1, 'tnt')
    summon(9, 52, 1, 'tnt')
    summon(10, 68, 1, 'oak_planks')
    if not immediate_power:
        ops.append(cmd(f'/data get entity {target(8)} fuse', expect_result=-1))
    ops.append({'op': 'tick', 'n': 2})
    # Let the loaded body initialize before applying power: first-load equipment
    # collection vs priming order is a separate diagnostic from fuse dispatch.
    if not immediate_power:
        ops.append(cmd(f'/execute if entity {target(8)}', expect_result=1))
        ops.append(cmd(f'/data get entity {target(8)} fuse', expect_result=-1))
        ops.extend(setup('/setblock 28 199 4 redstone_block'))
        ops.append({'op': 'tick', 'n': 1})
        ops.append(cmd(f'/data get entity {target(8)} fuse', expect_result=120))
    ops.append(cmd(f'/damage {target(6)} 1 in_fire'))
    ops.append(cmd(f'/damage {target(7)} 1 explosion', expect_result=1))
    ops.append(cmd(f'/damage {target(10)} 1 in_fire', expect_result=1))
    ops.append({'op': 'tick', 'n': 1})
    for n in (6, 7, 8, 9, 10):
        ops.append(cmd(f'/execute if entity {target(n)}', expect_result=1))
        ops.append(cmd(f'/data get entity {target(n)} Health'))
    for n in (6, 8, 9, 10):
        ops.append(cmd(f'/data get entity {target(n)} fuse'))
    # The explosion primes a random short fuse; assert the independent boundary only.
    ops.append(cmd(f'/execute if entity @e[nbt={{UUID:[I;243,0,0,7],Invulnerable:1b}}]',
                   expect_result=1))
    ops.append({'op': 'tick', 'n': 118})
    ops.append(cmd(f'/execute if entity {target(6)}', expect_result=1))
    ops.append(cmd(f'/data get entity {target(6)} fuse', expect_result=1))
    ops.append(cmd('/execute if block 6 200 4 dirt', expect_result=1))
    ops.append({'op': 'tick', 'n': 2})
    ops.append(cmd(f'/execute if entity {target(6)}'))
    ops.append(cmd('/execute if block 6 200 4 air', expect_result=1))
    for n in (9, 10):
        ops.append(cmd(f'/execute if entity {target(n)}', expect_result=1))
        ops.append(cmd(f'/data get entity {target(n)} fuse'))
    return ops + setup('/kill @e[tag=sulfur_fuse]', '/forceload remove 0 0 79 15')



def mob_full_resistance_hits():
    lines = [cmd('/forceload add 0 0 31 31'), {'op': 'settle'}]
    for x, z in [(0, 0), (16, 0), (0, 16), (16, 16)]:
        lines.append(cmd(f'/execute if loaded {x} 200 {z}', expect_result=1))
    lines.extend(setup('/fill 0 199 0 31 208 31 air'))
    lines.extend([cmd('/fill 0 199 0 31 199 31 stone', expect_result=1024),
                  cmd('/scoreboard objectives add hitstate dummy', expect_result=1)])
    serial = 0

    def pig(resistance=None, health=40, invulnerable=False):
        nonlocal serial
        serial += 1
        tag = f'resisted_hit_{serial}'
        uuid = f'00000000-0000-0000-0000-00f1000000{serial:02x}'
        x, z = 4.5 + 6 * ((serial - 1) % 5), 4.5 + 6 * ((serial - 1) // 5)
        nbt = (f'UUID:[I;0,0,241,{serial}],Tags:["resisted_hit","{tag}"],'
               f'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,'
               f'attributes:[{{id:"minecraft:max_health",base:40.0d}}],Health:{health}f')
        if invulnerable:
            nbt += ',Invulnerable:1b'
        lines.extend([
            cmd(f'/summon pig {x} 200 {z} {{{nbt}}}', expect_result=1),
            cmd(f'/execute if entity @e[type=pig,tag={tag}]', expect_result=1),
            cmd(f'/execute as {uuid} if entity @s[tag={tag},nbt={{NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b}}]', expect_result=1),
            cmd(f'/data get entity {uuid} UUID', expect_result=4),
            cmd(f'/data get entity {uuid} Pos', expect_result=3),
            cmd(f'/attribute {uuid} minecraft:max_health base get', expect_result=40),
        ])
        if resistance is not None:
            lines.append(cmd(f'/effect give {uuid} resistance 10 {resistance} true', expect_result=1))
            if resistance:
                lines.append(cmd(f'/data get entity {uuid} active_effects[{{id:"minecraft:resistance"}}].amplifier',
                                 expect_result=resistance if resistance < 128 else resistance - 256))
        state(uuid, health, 0)
        return uuid

    def state(uuid, health, hurt):
        lines.extend([
            cmd(f'/execute if entity {uuid}', expect_result=1),
            cmd(f'/data get entity {uuid} Health', expect_result=int(health)),
            cmd(f'/execute as {uuid} if entity @s[nbt={{Health:{health}f}}]', expect_result=1),
            cmd(f'/data get entity {uuid} HurtTime', expect_result=hurt),
        ])

    def damage(uuid, amount, accepted):
        lines.extend([
            cmd('/scoreboard players set #r hitstate 31', expect_result=31),
            cmd('/scoreboard players set #s hitstate 17', expect_result=17),
            cmd(f'/execute store result score #r hitstate store success score #s hitstate run damage {uuid} {amount} generic'),
            cmd('/scoreboard players get #r hitstate', expect_result=accepted),
            cmd('/scoreboard players get #s hitstate', expect_result=accepted),
            cmd(f'/execute if score #r hitstate matches {accepted}', expect_result=1),
            cmd(f'/execute if score #s hitstate matches {accepted}', expect_result=1),
        ])

    for amp, health in [(None, 39), (0, 39.2), (3, 39.8),
                        (4, 40), (5, 40), (6, 40), (50, 40),
                        (51, 40), (254, 40), (255, 40)]:
        uuid = pig(amp)
        damage(uuid, 1, 1)
        state(uuid, health, 10)
    uuid = pig(4)
    lines.append(cmd(f'/damage {uuid} 1 generic', expect_result=1))
    state(uuid, 40, 10)
    uuid = pig()
    damage(uuid, 0, 1)
    state(uuid, 40, 10)
    uuid = pig(4, invulnerable=True)
    damage(uuid, 1, 0)
    state(uuid, 40, 0)
    for initial, resistance, after, hurt in [(40, 4, 40, 10), (40, None, 39, 10),
                                             (1, 4, 1, 0), (0.5, 4, 0.5, 0)]:
        uuid = pig(resistance, initial)
        lines.append(cmd(f'/effect give {uuid} poison 10 5 true', expect_result=1))
        lines.append({'op': 'tick', 'n': 1})
        state(uuid, after, hurt)
    for resistance in [None, 4]:
        uuid = pig(resistance)
        damage(uuid, 1, 1)
        state(uuid, 40 if resistance else 39, 10)
        damage(uuid, 1, 0)
        state(uuid, 40 if resistance else 39, 10)
        # Stronger fully resisted hits have a separate cooldown-ordering defect;
        # retain that case in the full audit diagnostic, outside this focused gate.
        if resistance is None:
            damage(uuid, 2, 1)
            state(uuid, 38, 10)
        lines.append({'op': 'tick', 'n': 21})
        damage(uuid, 1, 1)
        state(uuid, 40 if resistance else 37, 10)
    lines.extend(setup('/kill @e[tag=resisted_hit]', '/forceload remove 0 0 31 31',
                       '/scoreboard objectives remove hitstate'))
    return lines



def sulfur_cube_mob_contact():
    """Actual mob ticks deliver hot-body contact damage, including NoAI (#9028)."""
    ops = setup('/forceload add 0 0 15 15', '/fill 0 199 0 15 199 15 stone',
                '/fill 0 200 0 15 206 15 air')
    ops.append({'op': 'settle'})
    ops.append(cmd('/execute if loaded 8 200 8', expect_result=1))
    # Explicit cube size/health exclude the separate absent-Health loader issue.
    cases = [(1, 2.5, 2.5, 'magma_block', 2.5),
             (2, 10.5, 2.5, 'white_concrete_powder', 10.5),
             (3, 2.5, 10.5, 'magma_block', 5.5),
             (4, 10.5, 10.5, None, 10.5)]

    def uuid(n):
        return f'000000f4-0000-0000-0000-{n:012x}'

    def guard(n, kind, x, z, health, body=None):
        ops.append(cmd(f'/execute if entity {uuid(n)}', expect_result=1))
        fields = (f'UUID:[I;244,0,0,{n}],Pos:[{x}d,200d,{z}d],Health:{health}f,'
                  'NoAI:1b,Silent:1b,PersistenceRequired:1b,Tags:["sulfur_contact"]')
        if body:
            fields += f',equipment:{{body:{{id:"minecraft:{body}"}}}}'
        ops.append(cmd(f'/execute if entity @e[type=minecraft:{kind},nbt={{{fields}}}]',
                       expect_result=1))

    for n, x, z, body, pig_x in cases:
        equipment = f',equipment:{{body:{{id:"minecraft:{body}"}}}}' if body else ''
        ops.extend(setup(f'/summon sulfur_cube {x} 200 {z} '
                         f'{{UUID:[I;244,0,0,{n}],Tags:["sulfur_contact"],NoAI:1b,'
                         f'Silent:1b,PersistenceRequired:1b,Size:1,Health:4f{equipment}}}',
                         f'/summon pig {pig_x} 200 {z} '
                         f'{{UUID:[I;244,0,0,{n + 10}],Tags:["sulfur_contact"],'
                         'NoAI:1b,Silent:1b,PersistenceRequired:1b}'))
        guard(n, 'sulfur_cube', x, z, 4, body)
        guard(n + 10, 'pig', pig_x, z, 10)
        ops.append(cmd(f'/data get entity {uuid(n + 10)} Health', expect_result=10))
    ops.append({'op': 'tick', 'n': 3})
    for n, x, z, body, pig_x in cases:
        guard(n, 'sulfur_cube', x, z, 4, body)
        # Guard identity/position independently of the observed health change.
        ops.append(cmd(f'/execute if entity {uuid(n + 10)}', expect_result=1))
        ops.append(cmd(f'/execute if entity @e[type=pig,nbt={{UUID:[I;244,0,0,{n + 10}],'
                       f'Pos:[{pig_x}d,200d,{z}d],NoAI:1b,Tags:["sulfur_contact"]}}]',
                       expect_result=1))
        ops.append(cmd(f'/data get entity {uuid(n + 10)} Health',
                       expect_result=9 if n == 1 else 10))
    return ops + setup('/kill @e[tag=sulfur_contact]', '/forceload remove 0 0 15 15')



def nautilus_saved_sitting():
    """TamableAnimal's ordered Sitting flag survives nautilus NBT edits (#9055).

    Only the saved flag is covered: no sitting AI, pose, riding or reload claim.
    All measured operations run frozen, with an independent zero-tick witness.
    """
    out = []

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    def observe(uid, kind, tag, sitting, owner):
        check(f'/execute as {uid} if entity @s[type=minecraft:{kind},tag={tag}]')
        check(f'/data get entity {uid} UUID', 4)
        check(f'/data get entity {uid} Pos', 3)
        check(f'/data get entity {uid} Health', 15)
        check(f'/execute as {uid} if entity @s[nbt={{NoAI:1b,Silent:1b,'
              'PersistenceRequired:1b,Health:15f,Pos:[10.5d,200d,10.5d]}]')
        if owner:
            check(f'/data get entity {uid} Owner', 4)
        else:
            check(f'/execute unless data entity {uid} Owner')
        check(f'/data get entity {uid} Sitting', sitting)
        check(f'/execute as {uid} if entity @s[nbt={{Sitting:{sitting}b}}]')

    check('/tick freeze')
    check('/gamerule spawn_mobs', 0)
    check('/time set midnight', 18000)
    check('/scoreboard objectives add saved_sit dummy')
    out += setup('/kill @e[type=!player]', '/forceload add 0 0 15 15')
    out.append({'op': 'settle'})
    check('/execute if loaded 0 200 0')
    check('/fill 0 199 0 15 199 15 stone', 256)
    out += setup('/execute store result score #before saved_sit run time query gametime')
    serial = 0
    for kind in ('nautilus', 'zombie_nautilus'):
        for owner in (True, False):
            for sitting in (None, 0, 1):
                serial += 1
                uid = f'00000000-0000-0000-0000-00fd{serial:08x}'
                tag = f'saved_sit_{serial}'
                saved = (f'{{UUID:[I;0,0,253,{serial}],Tags:["saved_sit","{tag}"],'
                         'NoAI:1b,Silent:1b,PersistenceRequired:1b,Health:15f')
                if owner:
                    saved += ',Owner:[I;1,2,3,4]'
                if sitting is not None:
                    saved += f',Sitting:{sitting}b'
                check(f'/summon minecraft:{kind} 10.5 200 10.5 {saved}}}')
                observe(uid, kind, tag, sitting or 0, owner)
                if owner and sitting == 1:
                    for value in (0, 1):
                        check(f'/data merge entity {uid} {{Sitting:{value}b}}')
                        observe(uid, kind, tag, value, True)
                    check(f'/data remove entity {uid} Owner')
                    observe(uid, kind, tag, 1, False)
                out += setup(f'/kill {uid}')
    out += setup('/execute store result score #after saved_sit run time query gametime')
    check('/scoreboard players operation #after saved_sit -= #before saved_sit', 0)
    check('/execute if score #after saved_sit matches 0')
    out += setup('/tick unfreeze')
    out.append({'op': 'tick', 'n': 25})
    out += setup('/kill @e[tag=saved_sit]', '/kill @e[type=item]',
                 '/fill 0 199 0 15 199 15 air', '/time set 1000',
                 '/scoreboard objectives remove saved_sit', '/forceload remove 0 0 15 15')
    return out



def zombie_nautilus_ignored_age():
    """Ignored zombie-nautilus age keys must not survive raw NBT (#9082).

    Zombie probes are zero-tick saved-data checks. A normal nautilus is an
    independent positive age-codec control, with one observed tick only.
    """
    out = []

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    def subject(uid, kind, tag):
        check(f'/execute as {uid} if entity @s[type=minecraft:{kind},tag={tag}]')
        check(f'/data get entity {uid} UUID', 4)
        check(f'/data get entity {uid} Pos', 3)
        check(f'/data get entity {uid} Health', 15)
        check(f'/execute as {uid} if entity @s[nbt={{NoAI:1b,NoGravity:1b,'
              'Silent:1b,PersistenceRequired:1b,Health:15f}]')

    def ignored(uid, tag):
        subject(uid, 'zombie_nautilus', tag)
        for field in ('Age', 'ForcedAge', 'AgeLocked'):
            out.append(cmd(f'/data get entity {uid} {field}'))
            check(f'/execute unless data entity {uid} {field}')

    check('/tick freeze')
    check('/gamerule spawn_mobs', 0)
    check('/time set midnight', 18000)
    check('/scoreboard objectives add ignored_age dummy')
    out += setup('/kill @e[type=!player]', '/forceload add 0 0 15 15')
    out.append({'op': 'settle'})
    check('/execute if loaded 0 200 0')
    check('/fill 0 199 0 15 199 15 stone', 256)
    out += setup('/execute store result score #before ignored_age run time query gametime')
    for serial, age in enumerate((None, -24000, 0, 37), 1):
        uid = f'000000fe-0000-0000-0000-{serial:012x}'
        tag = f'ignored_age_{serial}'
        saved = (f'{{UUID:[I;254,0,0,{serial}],Tags:["ignored_age","{tag}"],'
                 'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:15f')
        if age is not None:
            saved += f',Age:{age},ForcedAge:12,AgeLocked:1b'
        check(f'/summon minecraft:zombie_nautilus 10.5 200 10.5 {saved}}}')
        ignored(uid, tag)
        for merged in (0, -24000, 37):
            check(f'/data merge entity {uid} {{Age:{merged},ForcedAge:12,AgeLocked:1b}}')
            ignored(uid, tag)
        out += setup(f'/kill {uid}')
    out += setup('/execute store result score #after ignored_age run time query gametime')
    check('/scoreboard players operation #after ignored_age -= #before ignored_age', 0)
    check('/execute if score #after ignored_age matches 0')

    uid = '000000fe-0000-0000-0000-000000000005'
    check('/summon minecraft:nautilus 10.5 200 10.5 '
          '{UUID:[I;254,0,0,5],Tags:["ignored_age","normal_age"],NoAI:1b,NoGravity:1b,'
          'Silent:1b,PersistenceRequired:1b,Health:15f,Age:-24000,ForcedAge:12,AgeLocked:0b}')
    for expected in (-24000, -23999):
        if expected == -23999:
            out += setup('/execute store result score #before ignored_age run time query gametime',
                         '/tick step 1')
            out.append({'op': 'tick', 'n': 1})
            out += setup('/execute store result score #after ignored_age run time query gametime')
            check('/scoreboard players operation #after ignored_age -= #before ignored_age', 1)
            check('/execute if score #after ignored_age matches 1')
        subject(uid, 'nautilus', 'normal_age')
        check(f'/data get entity {uid} Age', expected)
        check(f'/data get entity {uid} ForcedAge', 12)
        check(f'/data get entity {uid} AgeLocked', 0)
        check(f'/execute as {uid} if entity @s[nbt={{Age:{expected},ForcedAge:12,AgeLocked:0b}}]')
    out += setup('/kill @e[tag=ignored_age]', '/tick unfreeze')
    out.append({'op': 'tick', 'n': 25})
    out += setup('/kill @e[type=item]', '/fill 0 199 0 15 199 15 air',
                 '/time set 1000', '/scoreboard objectives remove ignored_age',
                 '/forceload remove 0 0 15 15')
    return out



def structured_item_slots():
    range3 = '{type:"minecraft:slot_range",slots:"container.3"}'
    range5 = '{type:"minecraft:slot_range",slots:"container.5"}'
    group = '[' + range3 + ',' + range5 + ']'
    all_slots = '{type:"minecraft:slot_range",slots:"container.*"}'
    limit = '{type:"minecraft:limit_slots",limit:2,slot_source:' + all_slots + '}'
    filtered = '{type:"minecraft:filtered",item_filter:{items:"minecraft:apple"},slot_source:' + all_slots + '}'
    ops = [*setup('/setblock 4 200 4 chest', '/setblock 6 200 4 chest'),
           cmd('/execute if block 4 200 4 chest', expect_result=1)]
    def state():
        return [cmd('/data get block 4 200 4 Items'), cmd('/data get block 6 200 4 Items')]
    def clear():
        return setup('/item override block 4 200 4 container.* with air')
    ops += clear() + [cmd('/item fill block 4 200 4 ' + group + ' with apple')] + state()
    ops += [cmd('/item fill block 4 200 4 ' + limit + ' with gold_ingot')] + state()
    ops += [cmd('/item fill block 4 200 4 ' + filtered + ' with diamond')] + state()
    ops += [cmd('/item fill block 4 200 4 ' + filtered + ' with emerald')] + state()
    ops += clear() + [cmd('/item replace block 4 200 4 ' + group + ' with apple')] + state()
    ops += [cmd('/item override block 4 200 4 ' + group + ' with gold_ingot')] + state()
    ops += [cmd('/item fill block 4 200 4 ' + group + ' with apple'),
            cmd('/item modify block 4 200 4 ' + group + ' {type:"minecraft:set_count",count:3}')] + state()
    ops += [cmd('/item replace block 6 200 4 container.0 from block 4 200 4 ' + group)] + state()
    ops += [cmd('/execute if slots block 4 200 4 ' + group),
            cmd('/execute if items block 4 200 4 ' + group + ' apple'),
            cmd('/item fill block 4 200 4 {type:"minecraft:empty"} with diamond'),
            cmd('/item fill block 4 200 4 [] with diamond'),
            cmd('/item fill block 4 200 4 audit_missing:no_source with diamond')]
    for i, x in enumerate((4.5, 8.5), 1):
        ops += [cmd(f'/summon zombie {x} 200 8.5 {{UUID:[I;247,0,0,{i}],NoAI:1b,NoGravity:1b,Tags:["h247"]}}'),
                cmd(f'/execute if entity 000000f7-0000-0000-0000-{i:012x}', expect_result=1)]
    feet = '{type:"minecraft:limit_slots",limit:1,slot_source:{type:"minecraft:slot_range",slots:"armor.feet"}}'
    ops += [cmd('/item fill entity @e[tag=h247,sort=nearest] ' + feet + ' with iron_boots')]
    for i in (1, 2):
        ops += [cmd(f'/execute if items entity 000000f7-0000-0000-0000-{i:012x} armor.feet iron_boots')]
    # Named controls and composition boundaries; all expected replies come
    # from the same fresh official server, including rejection callbacks.
    ops += clear() + [cmd('/item fill block 4 200 4 container.3 with apple'),
                      cmd('/item replace block 4 200 4 container.5 with apple')]
    explicit_group = '{type:"minecraft:group",terms:' + group + '}'
    ops += [cmd('/item fill block 4 200 4 ' + explicit_group + ' with emerald')] + state()
    ops += clear() + setup('/item fill block 4 200 4 container.5 with apple')
    limited_filter = '{type:"minecraft:limit_slots",limit:1,slot_source:' + filtered + '}'
    ops += [cmd('/item modify block 4 200 4 ' + limited_filter + ' {type:"minecraft:set_count",count:7}')] + state()
    # The repeated filtered slot stops matching after its first write.
    repeated_filter = '{type:"minecraft:filtered",item_filter:{items:"minecraft:apple"},slot_source:[' + range5 + ',' + range5 + ']}'
    ops += [cmd('/item fill block 4 200 4 ' + repeated_filter + ' with diamond')] + state()
    ops += [cmd('/item fill block 4 200 4 container.99 with diamond'),
            cmd('/item replace block 6 200 4 container.0 from block 4 200 4 {type:"minecraft:empty"}')]
    return ops



def hopper_empty_slot_limits():
    """Real item/hopper ticks: setItem clamps an empty slot and consumes the input.

    The harness holds time between operations; only the explicit ticks below run.
    Keep the owner far away so these are hopper transfers, not player pickups.
    """
    apple = '00000110-0000-0000-0000-000000000001'
    stick = '00000110-0000-0000-0000-000000000002'
    pearl = '00000110-0000-0000-0000-000000000003'
    ops = setup('/forceload add 0 0 15 15',
                '/fill 0 99 0 3 99 3 stone', '/fill 0 100 0 3 104 3 air',
                '/setblock 0 100 0 hopper',
                '/scoreboard objectives add hopper_clock dummy',
                '/execute store result score before hopper_clock run time query gametime')
    ops += [cmd('/execute if loaded 0 100 0', expect_result=1),
            cmd('/execute if block 0 100 0 minecraft:hopper', expect_result=1),
            cmd('/data get block 0 100 0 id'),
            cmd('/data get block 0 100 0 Items'),
            cmd('/execute positioned 0.5 101 0.5 unless entity @a[distance=..8]', expect_result=1)]
    for number, item, count in ((1, 'apple', 70), (2, 'stick', 3)):
        target = (apple, stick)[number - 1]
        ops += setup('/summon item 0.5 101 0.5 '
                     f'{{UUID:[I;272,0,0,{number}],Item:{{id:"minecraft:{item}",count:{count}}},'
                     'PickupDelay:0s,Motion:[0d,0d,0d]}')
        ops += [cmd(f'/execute if entity @e[type=item,nbt={{UUID:[I;272,0,0,{number}]}}]', expect_result=1)]
        for field in ('UUID', 'Item.id', 'Item.count', 'Pos', 'Motion', 'PickupDelay'):
            ops.append(cmd(f'/data get entity {target} {field}',
                           expect_result=count if field == 'Item.count' else None))
    elapsed = 0
    for tick in (1, 2, 10, 18, 48):
        ops.append({'op': 'tick', 'n': tick - elapsed})
        ops += setup('/execute store result score elapsed hopper_clock run time query gametime',
                     '/scoreboard players operation elapsed hopper_clock -= before hopper_clock')
        ops += [cmd('/scoreboard players get elapsed hopper_clock', expect_result=tick),
                cmd('/data get block 0 100 0 Items'),
                cmd('/data get block 0 100 0 Items[{Slot:0b}].count', expect_result=64),
                cmd(f'/execute if entity {apple}'),
                cmd(f'/execute if entity {stick}')]
        if tick <= 2:
            ops.append(cmd(f'/data get entity {stick} Item.count', expect_result=3))
        else:
            ops.append(cmd('/data get block 0 100 0 Items[{Slot:1b}].count', expect_result=3))
        elapsed = tick
    # An ordinary stack limit other than 64 guards against a constant clamp.
    ops += setup('/setblock 2 100 0 hopper',
                 '/summon item 2.5 101 0.5 '
                 '{UUID:[I;272,0,0,3],Item:{id:"minecraft:ender_pearl",count:20},'
                 'PickupDelay:0s,Motion:[0d,0d,0d]}')
    ops += [cmd('/execute if entity @e[type=item,nbt={UUID:[I;272,0,0,3]}]', expect_result=1),
            cmd('/data get block 2 100 0 Items'),
            cmd(f'/data get entity {pearl} Item.id'),
            cmd(f'/data get entity {pearl} Item.count', expect_result=20),
            {'op': 'tick', 'n': 1},
            cmd('/data get block 2 100 0 Items'),
            cmd('/data get block 2 100 0 Items[{Slot:0b}].count', expect_result=16),
            cmd(f'/execute if entity {pearl}')]
    return ops + setup('/scoreboard objectives remove hopper_clock')



def comparator_output_data():
    """ComparatorBlockEntity load/save through the live circuit output, without ticks."""
    ops = setup('/forceload add 0 0 15 15', '/setblock 4 199 4 stone',
                '/scoreboard objectives add comparator_data dummy',
                '/execute store result score before comparator_data run time query gametime')
    ops += [cmd('/execute if loaded 4 200 4', expect_result=1),
            cmd('/execute if block 4 199 4 stone', expect_result=1)]
    cases = [(str(n), 0, n) for n in
             (-1, 0, 1, 2, 15, 16, 255, 256, 257, 2147483647, -2147483648)]
    cases += [('2b', 0, 2), ('2s', 0, 2), ('2L', 0, 2), ('2.5f', 0, 2),
              ('-2.5d', 0, -3), ('"bad"', 0, 0), ('{}', 0, 0),
              ('"bad"', 7, 0), ('{}', 7, 0), (None, 7, 0), ('7', 7, 7)]
    for value, before, after in cases:
        # Reusing the coordinate also checks that a replacement is a fresh entity.
        ops += setup('/setblock 4 200 4 air', '/setblock 4 200 4 comparator')
        ops += [cmd('/execute if block 4 200 4 comparator', expect_result=1),
                cmd('/data get block 4 200 4 id'),
                cmd('/data get block 4 200 4 OutputSignal', expect_result=0)]
        if before:
            ops.append(cmd(f'/data modify block 4 200 4 OutputSignal set value {before}', expect_result=1))
            ops.append(cmd('/data get block 4 200 4 OutputSignal', expect_result=before))
        ops += setup('/scoreboard players set result comparator_data -99',
                     '/scoreboard players set success comparator_data -99')
        ops += [cmd('/scoreboard players get result comparator_data', expect_result=-99),
                cmd('/scoreboard players get success comparator_data', expect_result=-99)]
        edit = ('data remove block 4 200 4 OutputSignal' if value is None else
                f'data modify block 4 200 4 OutputSignal set value {value}')
        changed = int(value is None or value != str(before))
        ops += [cmd('/execute store result score result comparator_data '
                    f'store success score success comparator_data run {edit}'),
                cmd('/scoreboard players get result comparator_data', expect_result=changed),
                cmd('/scoreboard players get success comparator_data', expect_result=changed),
                cmd('/data get block 4 200 4 OutputSignal', expect_result=after)]
    # An ordinary block entity and an invalid block keep their existing routes.
    ops += setup('/setblock 6 199 4 stone', '/setblock 6 200 4 chest')
    ops += [cmd('/data get block 6 200 4 Items'),
            cmd('/data modify block 6 200 4 CustomName set value "control"'),
            cmd('/data get block 6 200 4 CustomName'),
            cmd('/data get block 6 199 4 OutputSignal')]
    ops += setup('/execute store result score elapsed comparator_data run time query gametime',
                 '/scoreboard players operation elapsed comparator_data -= before comparator_data')
    return ops + [cmd('/scoreboard players get elapsed comparator_data', expect_result=0)] + setup(
        '/scoreboard objectives remove comparator_data')



def decorated_pot_data(string_sherd=False):
    """Pure DecoratedPotBlockEntity command NBT; no hopper or interaction ticks."""
    ops = setup('/forceload add 0 0 15 15', '/setblock 4 199 4 stone',
                '/scoreboard objectives add pot_data dummy',
                '/execute store result score before pot_data run time query gametime')
    ops += [cmd('/execute if loaded 4 200 4', expect_result=1),
            cmd('/execute if block 4 199 4 stone', expect_result=1)]
    for count in (None, 1, 3, 28):
        ops += setup('/setblock 4 200 4 air')
        nbt = '' if count is None else f'{{item:{{id:"minecraft:wheat",count:{count}}}}}'
        ops += [cmd(f'/setblock 4 200 4 decorated_pot{nbt}', expect_result=1),
                cmd('/execute if block 4 200 4 decorated_pot', expect_result=1),
                cmd('/data get block 4 200 4 id'),
                cmd('/data get block 4 200 4 item')]
        if count is None:
            ops.append(cmd('/data get block 4 200 4'))
        else:
            ops.append(cmd('/data get block 4 200 4 item.count', expect_result=count))
    ops += setup('/setblock 4 200 4 air')
    sherd = ('"minecraft:angler_pottery_sherd"' if string_sherd else
             '{id:"minecraft:angler_pottery_sherd"}')
    ops += [cmd('/setblock 4 200 4 decorated_pot{item:{id:"minecraft:wheat",count:3},'
                f'sherds:{{front:{sherd}}}}}', expect_result=1),
            cmd('/data get block 4 200 4 id'),
            cmd('/data get block 4 200 4 item.count', expect_result=3),
            cmd('/data get block 4 200 4 sherds')]
    edits = [
        ('data modify block 4 200 4 item.count set value 3', 0),
        ('data modify block 4 200 4 item.count set value 5', 1),
        ('data merge block 4 200 4 {item:{count:7}}', 1),
        ('data modify block 4 200 4 item.id set value "minecraft:apple"', 1),
        ('data remove block 4 200 4 item', 1),
        ('data remove block 4 200 4 item', 0),
        ('data modify block 4 200 4 item set value {id:"minecraft:stone",count:1}', 1),
        ('data modify block 4 200 4 item set value {id:"minecraft:stone",count:1}', 0),
    ]
    for edit, changed in edits:
        ops += setup('/scoreboard players set result pot_data -99',
                     '/scoreboard players set success pot_data -99')
        ops += [cmd('/scoreboard players get result pot_data', expect_result=-99),
                cmd('/scoreboard players get success pot_data', expect_result=-99),
                cmd('/execute store result score result pot_data '
                    f'store success score success pot_data run {edit}'),
                cmd('/scoreboard players get result pot_data', expect_result=changed),
                cmd('/scoreboard players get success pot_data', expect_result=changed),
                cmd('/data get block 4 200 4 id'),
                cmd('/data get block 4 200 4 item'),
                cmd('/data get block 4 200 4 sherds')]
    # A data view must leave pending loot metadata pending, not call pot_tag's unpacker.
    ops += setup('/setblock 4 200 4 air')
    ops += [cmd('/setblock 4 200 4 decorated_pot{LootTable:"minecraft:chests/simple_dungeon",LootTableSeed:17L}', expect_result=1),
            cmd('/data get block 4 200 4 id'),
            cmd('/data get block 4 200 4 LootTable'),
            cmd('/data get block 4 200 4 LootTableSeed', expect_result=17),
            cmd('/data get block 4 200 4 item'),
            cmd('/data modify block 4 200 4 LootTableSeed set value 19L', expect_result=1),
            cmd('/data get block 4 200 4 LootTable'),
            cmd('/data get block 4 200 4 LootTableSeed', expect_result=19)]
    ops += setup('/setblock 6 200 4 chest')
    ops += [cmd('/data get block 6 200 4 Items'),
            cmd('/data get block 4 199 4 item')]
    ops += setup('/execute store result score elapsed pot_data run time query gametime',
                 '/scoreboard players operation elapsed pot_data -= before pot_data')
    return ops + [cmd('/scoreboard players get elapsed pot_data', expect_result=0)] + setup(
        '/scoreboard objectives remove pot_data')



def mining_efficiency_9021():
    """Exercise Player.getDestroySpeed through actual block destroy progress."""
    probe = {'op': 'destroy_progress', 'pos': [8, 229, 9]}
    enchanted = 'diamond_pickaxe[minecraft:enchantments={efficiency:5}]'
    attr = '/attribute @s minecraft:mining_efficiency '
    operations = [
        *setup('/gamemode survival', '/effect clear @s',
               '/fill 7 229 7 9 229 9 stone', '/tp @s 8.5 230 8.5',
               '/item replace entity @s weapon.mainhand with diamond_pickaxe',
               attr + 'base set 0'),
        {'op': 'tick', 'n': 1}, dict(probe),
        *setup('/item replace entity @s weapon.mainhand with ' + enchanted),
        {'op': 'tick', 'n': 1}, dict(probe),
        *setup(attr + 'modifier add harness:slow -20 add_value'),
        {'op': 'tick', 'n': 1}, dict(probe),
        *setup(attr + 'modifier remove harness:slow', attr + 'base set 10'),
        {'op': 'tick', 'n': 1}, dict(probe),
        *setup(attr + 'modifier add harness:base 0.5 add_multiplied_base'),
        {'op': 'tick', 'n': 1}, dict(probe),
        *setup(attr + 'modifier add harness:total 0.5 add_multiplied_total'),
        {'op': 'tick', 'n': 1}, dict(probe),
        *setup(attr + 'modifier remove harness:base', attr + 'modifier remove harness:total',
               '/item replace entity @s weapon.mainhand with stick'),
        {'op': 'tick', 'n': 1}, dict(probe),
        *setup('/item replace entity @s weapon.mainhand with diamond_pickaxe', attr + 'base set 0'),
        {'op': 'tick', 'n': 1}, dict(probe),
        *setup('/item replace entity @s weapon.offhand with ' + enchanted),
        {'op': 'tick', 'n': 1}, dict(probe),
        *setup('/item replace entity @s weapon.offhand with air',
               '/item replace entity @s weapon.mainhand with air',
               '/fill 7 229 7 9 229 9 air', '/gamemode creative'),
    ]

    # Independent vanilla controls: effective speeds before hardness/divisor.
    # Round both divisions to Java float precision, as getDestroyProgress does.
    import struct
    def f32(value):
        return struct.unpack('>f', struct.pack('>f', value))[0]
    expected = iter([(8, 30), (34, 30), (14, 30), (44, 30), (62, 30),
                     (89, 30), (1, 100), (8, 30), (8, 30)])
    for operation in operations:
        if operation['op'] == 'destroy_progress':
            speed, divisor = next(expected)
            value = f32(f32(speed / 1.5) / divisor)
            operation['expect_result'] = struct.unpack('>I', struct.pack('>f', value))[0]
            operation['cmd'] = '#destroy_progress'
    return operations



def bell_server_glow():
    """Actual world ticks: near raiders trigger resonance that reaches farther raiders."""
    # Centre the harness owner's loading area so settle explicitly awaits entity ticking
    # in every actor chunk, including the witch at z=48 (outside the old centre).
    ops = setup('/tp @s 8.5 230 24.5', '/forceload add 0 0 31 63') + [{'op': 'settle'}]
    ops += [cmd('/execute if loaded 8 200 48', expect_result=1)]
    ops += setup('/fill 0 199 0 31 199 63 stone',
                 '/fill 0 200 0 31 205 63 air', '/scoreboard objectives add bell_clock dummy')
    # Let newly forced chunks acquire entity-ticking readiness before actors exist.
    ops += [{'op': 'tick', 'n': 20}]
    for scene in range(3):
        if scene:
            ops += setup('/kill @e[tag=h288_bell]') + [{'op': 'tick', 'n': 25}]
            ops += setup('/setblock 9 200 8 air', '/setblock 8 200 8 air')
            ops += [{'op': 'tick', 'n': 70}]
        ops += setup('/setblock 8 200 8 bell[attachment=floor,facing=north]')
        actors = [('witch', '8.5 200 48.5', 1), ('zombie', '13.5 200 8.5', 2)]
        if scene != 1:
            actors.append(('pillager', '18.5 200 8.5', 3))
        for kind, pos, serial in actors:
            tag = f'h288_{scene}_{kind}'
            fields = f'UUID:[I;288,{scene},0,{serial}],Tags:["h288_bell","{tag}"],NoAI:1b,Silent:1b,PersistenceRequired:1b'
            ops += setup(f'/summon {kind} {pos} {{{fields}}}')
            guard = f'@e[type={kind},tag={tag},nbt={{UUID:[I;288,{scene},0,{serial}],NoAI:1b,Pos:[{pos.replace(" ", "d,")}d]}}]'
            ops += [cmd(f'/execute if entity {guard}', expect_result=1),
                    cmd(f'/data get entity @e[tag={tag},limit=1] Health',
                        expect_result={'witch': 26, 'zombie': 20, 'pillager': 24}[kind]),
                    cmd(f'/execute unless data entity @e[tag={tag},limit=1] active_effects[{{id:"minecraft:glowing"}}]', expect_result=1)]
        ops += [{'op': 'tick', 'n': 2}]
        ops += setup('/setblock 9 200 8 redstone_block')
        if scene == 2:
            # Destroy a resonating bell, tick while absent, then ring its replacement.
            ops += [{'op': 'tick', 'n': 10}]
            ops += setup('/setblock 9 200 8 air', '/setblock 8 200 8 air')
            ops += [{'op': 'tick', 'n': 1}]
            ops += setup('/setblock 8 200 8 bell[attachment=floor,facing=north]',
                         '/setblock 9 200 8 redstone_block')
        ops += [cmd('/execute if block 8 199 8 stone', expect_result=1),
                cmd('/execute if block 8 200 8 bell[attachment=floor,facing=north,powered=true]', expect_result=1),
                cmd('/execute if block 9 200 8 redstone_block', expect_result=1)]
        ops += setup('/execute store result score start bell_clock run time query gametime')
        elapsed = 0
        for sample in (41, 43, 44, 45, 46, 47, 48, 49):
            ops += [{'op': 'tick', 'n': sample - elapsed}]
            elapsed = sample
            ops += setup('/execute store result score elapsed bell_clock run time query gametime',
                         '/scoreboard players operation elapsed bell_clock -= start bell_clock')
            ops += [cmd('/scoreboard players get elapsed bell_clock', expect_result=sample)]
            for kind, pos, serial in actors:
                tag = f'h288_{scene}_{kind}'
                guard = f'@e[type={kind},tag={tag},nbt={{UUID:[I;288,{scene},0,{serial}],NoAI:1b,Pos:[{pos.replace(" ", "d,")}d]}}]'
                ops += [cmd(f'/execute if entity {guard}', expect_result=1)]
                ops += setup(f'/execute store result score health bell_clock run data get entity @e[tag={tag},limit=1] Health 1000')
                ops += [cmd('/execute if score health bell_clock matches 1..', expect_result=1)]
                sign = 'if' if scene != 1 and sample >= 45 and kind != 'zombie' else 'unless'
                ops += [cmd(f'/execute {sign} data entity @e[tag={tag},limit=1] active_effects[{{id:"minecraft:glowing"}}]', expect_result=1)]
            if scene != 1 and sample == 49:
                ops += [cmd(f'/data get entity @e[tag=h288_{scene}_witch,limit=1] active_effects[{{id:"minecraft:glowing"}}].duration', expect_result=56)]
    return ops + setup('/scoreboard objectives remove bell_clock')


def advancement_reward_functions():
    """Function-only impossible advancements isolate reward dispatch from triggers and XP/loot."""
    ops = setup('/forceload add 0 0 15 15', '/setblock 8 229 8 stone',
                '/tp @s 8.5 230 8.5 0 0', '/scoreboard objectives add reward_hits dummy',
                '/scoreboard objectives add reward_origin dummy',
                '/scoreboard objectives add reward_nested dummy',
                '/scoreboard objectives add reward_alias dummy',
                '/scoreboard objectives add reward_audit dummy')
    ops += [cmd('/execute if loaded 8 230 8', expect_result=1),
            cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,230d,8.5d],Health:20f}]', expect_result=1)]
    # Snapshot the entire actual UUID; the final no-change write checks identity without
    # assuming the two harnesses choose the same offline player UUID.
    ops += setup('/data modify storage h291:identity value set from entity @s UUID',
                 '/execute store result score clock reward_audit run time query gametime')
    for objective in ('reward_hits', 'reward_origin', 'reward_nested', 'reward_alias'):
        ops += setup(f'/scoreboard players set @s {objective} 0')
    ops += [cmd('/function h291:reward')]
    ops += [cmd('/scoreboard players get @s reward_hits', expect_result=1),
            cmd('/scoreboard players get @s reward_origin', expect_result=1),
            cmd('/scoreboard players get @s reward_nested', expect_result=1),
            cmd('/execute if entity @s[tag=h291_rewarded]', expect_result=1)]
    ops += setup('/scoreboard players set @s reward_hits 0',
                 '/scoreboard players set @s reward_origin 0',
                 '/scoreboard players set @s reward_nested 0', '/tag @s remove h291_rewarded')
    # Reward uses the player's own position, not the granting execute source's position.
    cases = [('execute positioned 40 240 40 run advancement grant @s only h291:reward', 1, 1, 1, True),
             ('advancement grant @s only h291:reward', 0, 1, 1, True),
             ('advancement revoke @s only h291:reward', 1, 1, 1, False),
             ('advancement grant @s only h291:reward', 1, 2, 2, True),
             ('advancement grant @s only h291:no_function', 1, 2, 2, True),
             ('advancement grant @s only h291:no_function', 0, 2, 2, True),
             ('advancement grant @s only h291:missing_function', 1, 2, 2, True),
             ('advancement grant @s only h291:alias', 1, 2, 2, True)]
    for operation, changed, hits, nested, completed in cases:
        ops += [cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,230d,8.5d],Health:20f}]', expect_result=1)]
        ops += setup('/scoreboard players set result reward_audit -99',
                     '/scoreboard players set success reward_audit -99')
        ops += [cmd('/scoreboard players get result reward_audit', expect_result=-99),
                cmd('/scoreboard players get success reward_audit', expect_result=-99),
                cmd('/execute store result score result reward_audit store success score success reward_audit run ' + operation),
                cmd('/scoreboard players get result reward_audit', expect_result=changed),
                cmd('/scoreboard players get success reward_audit', expect_result=changed),
                cmd('/scoreboard players get @s reward_hits', expect_result=hits),
                cmd('/scoreboard players get @s reward_origin', expect_result=hits),
                cmd('/scoreboard players get @s reward_nested', expect_result=nested),
                cmd('/execute if entity @s[tag=h291_rewarded]', expect_result=1)]
        advancement = operation.split(' only ')[1]
        sign = 'if' if completed else 'unless'
        ops += [cmd(f'/execute {sign} entity @s[advancements={{{advancement}=true}}]', expect_result=1)]
    ops += [cmd('/scoreboard players get @s reward_alias', expect_result=1),
            cmd('/function h291:reward'),
            cmd('/scoreboard players get @s reward_hits', expect_result=3),
            cmd('/scoreboard players get @s reward_nested', expect_result=3)]
    ops += setup('/execute store success score identity_changed reward_audit run data modify storage h291:identity value set from entity @s UUID',
                 '/execute store result score elapsed reward_audit run time query gametime',
                 '/scoreboard players operation elapsed reward_audit -= clock reward_audit')
    ops += [cmd('/scoreboard players get identity_changed reward_audit', expect_result=0),
            cmd('/scoreboard players get elapsed reward_audit', expect_result=0)]
    for objective in ('reward_hits', 'reward_origin', 'reward_nested', 'reward_alias', 'reward_audit'):
        ops += setup(f'/scoreboard objectives remove {objective}')
    return ops


def groups():
    try:
        from .loaded_cloud_corpus import loaded_clouds
    except ImportError:
        from loaded_cloud_corpus import loaded_clouds
    return [
        ('setup', group_setup()),
        ('bell server glow', bell_server_glow()),
        ('advancement reward functions', advancement_reward_functions()),
        ('named weather predicates', named_weather_predicates()),
        ('mining_efficiency_9021', mining_efficiency_9021()),
        ('structured item slots', structured_item_slots()),
        ('loaded potion clouds', loaded_clouds(cmd, setup)),
        ('server clock', server_clock()),
        ('inline execute predicates', inline_execute_predicates()),


        ('decorated pot data', decorated_pot_data()),
        ('comparator output data', comparator_output_data()),
        ('hopper empty slot limits', hopper_empty_slot_limits()),
        ('End clock commands', end_clock_commands()),
        ('client commands', client_commands()),
        ('game state', game_state()),
        ('player state', player_state()),
        ('experience level boundaries', experience_level_boundaries()),
        ('blocks and border', world_blocks()),
        ('setblock destroy air', setblock_destroy_air()),
        ('forceload query order', forceload_query_order()),
        ('entities', entities()),
        ('zombie nautilus ignored age', zombie_nautilus_ignored_age()),
        ('nautilus saved Sitting', nautilus_saved_sitting()),
        ('axolotl hunting memory', axolotl_hunting_memory()),
        ('marker data mutation', marker_data_mutation()),
        ('mob effect update result', mob_effect_update_result()),
        ('axolotl integer memory TTL', axolotl_integer_memory_ttl()),
        ('parrot saved Age', parrot_saved_age()),
        ('animal NoAI love', animal_noai_love()),
        ('turtle summon home', turtle_summon_home()),
        ('command minecart loaded success', command_minecart_loaded_success()),
        ('ravager NoAI counters', ravager_noai_counters()),
        ('aquatic FromBucket', aquatic_from_bucket()),
        ('villager command names', villager_command_names()),
        ('item lifetime counters', item_lifetime_counters()),
        ('animal signed InLove', animal_signed_in_love()),
        ('mob fire resistance damage', mob_fire_resistance_damage()),
        ('armor stand climbing', armor_stand_climbing()),
        ('mob armor damage', mob_armor_damage()),
        ('mob max health bounds', mob_max_health_bounds()),
        ('creeper custom fuse', creeper_custom_fuse()),
        ('sulfur cube mob contact', sulfur_cube_mob_contact()),
        ('sulfur cube fuse ticks', sulfur_cube_fuse_ticks()),
        ('wither spawn', wither_spawn()),
        ('riding teleport', riding_teleport()),
        ('mob riding', mob_riding()),
        ('trial spawner passengers', trial_spawner_passengers()),
        ('camel riders', camel_riders()),
        ('scores, teams and bossbars', scores()),
        ('bossbar list order', bossbar_list_order()),
        ('large fireball fire', large_fireball_fire()),
        ('mannequin pose anchors', mannequin_pose_anchors()),
        ('item command damage', item_command_damage()),
        ('execute', execute()),
        ('projectile origin', projectile_origin()),
        ('selector missing predicates', selector_missing_predicates()),
        ('levels', levels()),
        ('end portal entities', end_portal_entities()),
        ('items and loot', items_and_loot()),
        ('numeric array edits', numeric_array_edits()),
        ('projectile saved UUID', projectile_saved_uuid()),
        ('awake villager saved position', awake_villager_saved_position()),
        ('villager wake rotation', villager_wake_rotation()),
        ('cube saved Age', cube_saved_age()),
        ('mob baby attribute reads', mob_baby_attribute_reads()),
        ('mob partial block suffocation', mob_partial_block_suffocation()),
        ('allay invalid Brain retention', allay_invalid_brain_retention()),
        ('hurting projectile float inertia', hurting_projectile_float_inertia()),
        ('primed TNT block state', primed_tnt_block_state()),
        ('allay pocket inventory', allay_pocket_inventory()),
        ('summoned fangs lifetime', summoned_fangs_lifetime()),
        ('summoned fangs edits', summoned_fangs_edits()),
        ('summoned fangs levels', summoned_fangs_levels()),
        ('bird flap sensors', bird_flap_sensors()),
        ('entity landing sensors', entity_landing_sensors()),
        ('loot set damage', loot_set_damage()),
        ('item NBT count bounds', item_nbt_count_bounds()),
        ('item block containers', item_block_containers()),
        ('world', world_misc()),
        ('recipe special arguments', recipe_special_arguments()),
        ('progression', progression()),
        ('functions and packs', functions_and_packs()),
        ('mob full resistance hits', mob_full_resistance_hits()),
        ('sulfur cube loaded health', sulfur_cube_loaded_health()),
        ('function return feedback', function_return_feedback()),
        ('ticking and random', ticking()),
        ('compute', compute()),
        ('social and scoreboard', social()),
        ('test', test_command()),
        ('wither breaks', wither_breaks()),
        ('tnt minecart rails', tnt_minecart_rails()),
        ('creative shulker drops', creative_shulker_drops()),
        ('minecart drag', minecart_drag()),
        ('ender dragon', ender_dragon()),
        ('place structure', place_structure()),
        ('tame wolf kills', tame_wolf_kills()),
        ('experience orb fall', experience_orb_fall()),
        ('piglin pickups', piglin_pickups()),
        ('experience orb stuck', experience_orb_stuck()),
        ('summoned potions', summoned_potions()),
        ('creeper cloud selectors', creeper_cloud_selectors()),
    ]
