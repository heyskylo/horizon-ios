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


# This fixture establishes its own live clock before spawning actors.
ISOLATED_WOLF_ANGER = frozenset({'wolf anger live writes'})


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
    if 'mob custom name components' in group_names:
        try:
            from .mob_name_predicate_corpus import predicates as name_predicates
        except ImportError:
            from mob_name_predicate_corpus import predicates as name_predicates
        fixtures.update(name_predicates())
    if 'mob effect predicate view' in group_names:
        try:
            from .mob_effect_predicate_corpus import predicates as effect_predicates
        except ImportError:
            from mob_effect_predicate_corpus import predicates as effect_predicates
        fixtures.update(effect_predicates())
    try:
        from . import player_food_predicate_corpus as player_food
    except ImportError:
        import player_food_predicate_corpus as player_food
    if player_food.GROUP in group_names:
        fixtures.update(player_food.predicates())
    if 'named smoke predicates' in group_names:
        always = {'type': 'minecraft:all_of', 'terms': []}
        fixtures['command_smoke:always'] = always
        fixtures['command_smoke:never'] = {'type': 'minecraft:inverted', 'term': always}
        for name, smokey in [('smokey', True), ('clear', False)]:
            fixtures['command_smoke:' + name] = {
                'type': 'minecraft:location_check', 'predicate': {'smokey': smokey}}
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


def named_smoke_predicates():
    """CampfireBlock.isSmokeyPos through named execute predicates, without ticks.

    The two slabs distinguish a blocked fire from the immediate-below-blocker
    exception. Unlit, absent and soul fires guard state and tag handling.
    """
    out = []

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    check('/tick freeze')
    check('/execute if dimension minecraft:overworld')
    out.extend(setup('/forceload remove -16 -16 15 15'))
    check('/forceload add -16 -16 15 15', 4)
    out.append({'op': 'settle'})
    for x in [-16, 0]:
        for z in [-16, 0]:
            check(f'/execute if loaded {x} 100 {z}')
    check('/scoreboard objectives add smoke_cmd dummy')
    out.extend(setup('/execute store result score #start smoke_cmd run time query gametime'))
    # Seed the region so clearing it has a positive, deterministic acknowledgement.
    check('/fill -16 94 -16 15 120 15 barrier', 27648)
    check('/fill -16 94 -16 15 120 15 air', 27648)
    check('/fill -16 93 -16 15 93 15 stone', 1024)
    for x, kind, lit in [(0, 'campfire', 'true'), (3, 'campfire', 'true'),
                         (6, 'campfire', 'false'), (9, 'campfire', 'true'),
                         (-3, 'soul_campfire', 'true')]:
        check(f'/setblock {x} 96 0 {kind}[lit={lit}]')
        check(f'/execute if block {x} 96 0 {kind}[lit={lit}]')
    for x, y in [(3, 98), (9, 97)]:
        check(f'/setblock {x} {y} 0 stone_slab[type=bottom]')
        check(f'/execute if block {x} {y} 0 stone_slab[type=bottom]')
    for x, y in [(0, 99), (0, 97), (3, 99), (3, 97), (6, 99),
                 (9, 99), (9, 98), (-3, 99), (12, 96), (12, 99)]:
        check(f'/execute if block {x} {y} 0 air')
    check('/execute if predicate command_smoke:always')
    check('/execute unless predicate command_smoke:never')
    # Each sign has its own exact feedback and independent result/success reads.
    for x, smokey in [(0, True), (3, False), (6, False), (9, True),
                      (12, False), (-3, True)]:
        for predicate, passes in [('smokey', smokey), ('clear', not smokey)]:
            out.extend(setup('/scoreboard players set #r smoke_cmd 31',
                             '/scoreboard players set #s smoke_cmd 17'))
            check('/scoreboard players get #r smoke_cmd', 31)
            check('/scoreboard players get #s smoke_cmd', 17)
            out.append(cmd('/execute store result score #r smoke_cmd '
                           'store success score #s smoke_cmd run execute '
                           f'positioned {x + 0.5} 100 0.5 if predicate command_smoke:{predicate}'))
            check('/scoreboard players get #r smoke_cmd', int(passes))
            check('/scoreboard players get #s smoke_cmd', int(passes))
    out.append(cmd('/execute if predicate command_smoke:missing'))
    out.extend(setup('/execute store result score #clock smoke_cmd run time query gametime'))
    check('/scoreboard players operation #clock smoke_cmd -= #start smoke_cmd', 0)
    out.extend(setup('/fill -16 94 -16 15 120 15 air',
                     '/scoreboard objectives remove smoke_cmd',
                     '/forceload remove -16 -16 15 15', '/tick unfreeze'))
    return out


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
        # A cushion is an entity too (BlockAttachedEntity): found, tagged,
        # read and killed like a painting, with no drop.
        cmd('/setblock 4 209 12 minecraft:oak_slab'),
        cmd('/summon minecraft:cushion 4.5 209.5 12.5 {Tags:["k"]}'),
        cmd('/execute if entity @e[type=minecraft:cushion]'),
        cmd('/tag @e[type=minecraft:cushion,limit=1] list'),
        cmd('/execute if entity @e[type=minecraft:cushion,tag=k]'),
        cmd('/data get entity @e[type=minecraft:cushion,limit=1] color'),
        cmd('/kill @e[type=minecraft:cushion]'),
        cmd('/execute if entity @e[type=minecraft:cushion]'),
        cmd('/execute if entity @e[type=minecraft:item]'),
        # So is a falling block, named after its block (`getTypeName`).
        cmd('/summon minecraft:falling_block 4.5 210 8.5 {BlockState:{Name:"minecraft:sand"},NoGravity:1b,Tags:["f"]}'),
        cmd('/execute if entity @e[type=minecraft:falling_block,tag=f]'),
        # Near the fixture only: terrain the Rust side ticks and the oracle never loads has its own
        # falling gravel (see piglin_pickups).
        cmd('/tag @e[type=minecraft:falling_block,x=4.5,y=212,z=8.5,distance=..16,limit=1] list'),
        cmd('/data get entity @e[type=minecraft:falling_block,x=4.5,y=212,z=8.5,distance=..16,limit=1] BlockState.Name'),
        cmd('/tp @e[type=minecraft:falling_block,x=4.5,y=212,z=8.5,distance=..16] 4.5 215 8.5'),
        cmd('/kill @e[type=minecraft:falling_block,x=4.5,y=212,z=8.5,distance=..16]'),
        cmd('/execute if entity @e[type=minecraft:falling_block,x=4.5,y=212,z=8.5,distance=..16]'),
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


def minecart_numeric_fields():
    """AbstractMinecart / MinecartTNT / MinecartFurnace / MinecartHopper.readAdditionalSaveData read
    their fields through ValueInput's numeric getters, which take any NumericTag: getIntOr
    (fuse, DisplayOffset), getShortOr (Fuel: the low 16 bits), getDoubleOr (PushX, PushZ) and
    getBooleanOr (FlippedRotation, HasTicked, Enabled: byteValue() != 0). Frozen, so nothing ticks."""
    ops = setup('/forceload add 0 0 15 15', '/tick freeze')
    ops += [{'op': 'settle'}]
    cases = (
        ('tnt_minecart', '{fuse:20L}', 'fuse'),
        ('tnt_minecart', '{fuse:20.7f}', 'fuse'),
        ('tnt_minecart', '{fuse:-2.5d}', 'fuse'),
        ('tnt_minecart', '{fuse:4294967316L}', 'fuse'),
        ('tnt_minecart', '{fuse:7b}', 'fuse'),
        ('minecart', '{DisplayOffset:5L,DisplayState:{Name:"minecraft:stone"}}', 'DisplayOffset'),
        ('minecart', '{DisplayOffset:3.9f,DisplayState:{Name:"minecraft:stone"}}', 'DisplayOffset'),
        ('furnace_minecart', '{Fuel:70000}', 'Fuel'),
        ('furnace_minecart', '{Fuel:300L}', 'Fuel'),
        ('furnace_minecart', '{Fuel:2.5f}', 'Fuel'),
        ('furnace_minecart', '{Fuel:100s,PushX:1,PushZ:0.5f}', 'PushX 1000'),
        ('furnace_minecart', '{Fuel:100s,PushX:1,PushZ:0.5f}', 'PushZ 1000'),
        ('hopper_minecart', '{Enabled:256}', 'Enabled'),
        ('hopper_minecart', '{Enabled:0.5f}', 'Enabled'),
        ('hopper_minecart', '{Enabled:2L}', 'Enabled'),
        ('minecart', '{FlippedRotation:256}', 'FlippedRotation'),
        ('minecart', '{FlippedRotation:1.5d}', 'FlippedRotation'),
        ('minecart', '{HasTicked:2L}', 'HasTicked'),
    )
    for i, (kind, nbt, path) in enumerate(cases):
        tag = f'cart_num_{i}'
        extra = nbt[1:-1]
        ops += setup(f'/summon {kind} {2 + (i % 6) * 2}.5 200 {2 + (i // 6) * 2}.5 '
                     f'{{Tags:["{tag}"],NoGravity:1b,{extra}}}')
        ops += [cmd(f'/data get entity @e[tag={tag},limit=1] {path}')]
    return ops + setup(*[f'/kill @e[tag=cart_num_{i}]' for i in range(len(cases))],
                       '/tick unfreeze', '/forceload remove 0 0 15 15')


def enderman_command_damage():
    """DamageCommand -> Enderman.hurtServer: with no living entity behind the damage,
    random.nextInt(10) != 0 teleports the enderman then and there (teleport()), also when
    the hurt cooldown refuses the hit. The world is frozen, so no AI step does it later. The
    enderman floats (NoGravity) 32 blocks over a stone floor, so its teleport lands on it."""
    ops = setup('/forceload add 128 128 223 223', '/fill 144 199 144 208 199 208 minecraft:stone')
    # Every chunk a target can land in holds its entities (else @e no longer sees it there).
    ops += [{'op': 'settle', 'required_chunks': [[x, z] for x in range(9, 14) for z in range(9, 14)]}]
    ops += setup('/tick freeze')
    ender = '@e[tag=hurt_tp,limit=1]'
    ops += setup('/summon enderman 176.5 232 176.5 {Tags:["hurt_tp"],NoGravity:1b,PersistenceRequired:1b}')
    ops += [cmd(f'/execute if entity {ender}', expect_result=1)]
    # Nine draws in ten teleport. Hit again only while it is still where it was (a second
    # teleport from the floor could leave it): four leave it in place one time in 10^4.
    origin = '/execute positioned 176.5 232 176.5 as @e[tag=hurt_tp,distance=..0.5] run '
    ops += [cmd(f'/damage {ender} 1')]
    ops += setup(*[origin + 'damage @s 1'] * 3)
    ops += [cmd(f'/execute if entity {ender}', expect_result=1),
            cmd('/execute if entity @e[tag=hurt_tp,x=144,y=200,z=144,dx=64,dy=0,dz=64]', expect_result=1),
            cmd('/execute positioned 176.5 232 176.5 unless entity @e[tag=hurt_tp,distance=..0.5]',
                expect_result=1)]
    ops += setup('/kill @e[tag=hurt_tp]', '/fill 144 199 144 208 199 208 minecraft:air',
                 '/forceload remove 128 128 223 223', '/tick unfreeze')
    return ops


def shulker_command_damage():
    """DamageCommand -> Shulker.hurtServer: below half health, random.nextInt(4) == 0 teleports
    the shulker then and there (teleportSomewhere()). The world is frozen, so no AI step does it
    later. 32 shulkers cling to stone walls between two-block air gaps, where a shell has room
    to open (canStayAt), so nearly every teleportSomewhere finds a place: one shulker in four
    moves, and none of the 32 does one time in 10^4."""
    ops = setup('/forceload add 144 144 207 207', '/fill 159 203 159 192 221 183 minecraft:stone')
    ops += setup(*[f'/fill {x} 204 160 {x + 1} 220 182 minecraft:air' for x in range(160, 192, 3)])
    ops += [{'op': 'settle', 'required_chunks': [[x, z] for x in range(9, 13) for z in range(9, 12)]}]
    ops += setup('/tick freeze', *[f'/scoreboard objectives add sd_{k} dummy' for k in ('x', 'y', 'z', 'x2', 'y2', 'z2')])
    # Each on the west wall of its gap (AttachFace west), its shell opening east into the gap.
    # Fixed UUIDs: the shulkers stay score holders after the kill (the bodies are
    # unloaded, never removed), and a later `/scoreboard players list` names them.
    spots = [(x, z) for x in range(163, 185, 3) for z in (168, 170, 172, 174)]
    ops += setup(*[f'/summon shulker {x}.5 212 {z}.5 {{UUID:[I;12185,0,0,{n}],Tags:["sd"],AttachFace:4b,PersistenceRequired:1b}}'
                   for n, (x, z) in enumerate(spots)])
    ops += [cmd('/execute if entity @e[tag=sd]', expect_result=32)]
    def store(suffix):
        return [f'/execute as @e[tag=sd] store result score @s sd_{k}{suffix} run data get entity @s Pos[{i}] 10'
                for i, k in enumerate('xyz')]
    ops += setup(*store(''))
    ops += setup('/execute as @e[tag=sd] run damage @s 20')
    ops += setup(*store('2'), *[f'/execute as @e[tag=sd] unless score @s sd_{k} = @s sd_{k}2 run tag @s add sd_moved'
                                 for k in 'xyz'])
    ops += [cmd('/execute if entity @e[tag=sd]', expect_result=32),
            cmd('/execute if entity @e[tag=sd_moved,limit=1]', expect_result=1)]
    ops += setup('/kill @e[tag=sd]', *[f'/scoreboard objectives remove sd_{k}' for k in ('x', 'y', 'z', 'x2', 'y2', 'z2')],
                 '/fill 159 203 159 192 221 183 minecraft:air', '/forceload remove 144 144 207 207', '/tick unfreeze')
    return ops


def snbt_trailing_commas():
    """SnbtGrammar: a compound, a list and a typed array may end with a comma; an empty one
    may not hold only a comma, and two commas in a row are an error."""
    ops = []
    for value in ('{a:1b,}', '{a:1b,b:"x",}', '[1,2,]', '["a",]', '[{},]', '[I;1,2,]', '[B;1b,]', '[L;1L,]',
                  '{a:[1,],b:{c:2,},}', '{,}', '[,]', '[I;,]', '{a:1b,,}', '[1,,2]', '{a:1b,}x'):
        ops += [cmd(f'/data modify storage horizon:snbt v set value {value}'),
                cmd('/data get storage horizon:snbt v')]
    # A text component argument reads the same grammar.
    ops += [cmd('/tellraw @s {text:"comma",color:"red",}'), cmd('/tellraw @s ["a",{text:"b",},]')]
    ops += [cmd('/summon minecraft:marker 0 200 0 {Tags:["snbt_comma",],}'),
            cmd('/data get entity @e[type=minecraft:marker,tag=snbt_comma,limit=1] Tags'),
            cmd('/kill @e[type=minecraft:marker,tag=snbt_comma]'),
            cmd('/data remove storage horizon:snbt v')]
    return ops


def dragon_command_damage():
    """DamageCommand -> EnderDragon.hurtServer (the body: a quarter plus up to 1, health only
    with a player behind it or #is_explosion, success either way) and EndCrystal.hurtServer
    (removed unless invulnerable, fire or the dragon's; generic_kill bypasses Invulnerable)."""
    ops = setup('/forceload add 0 0 63 47', '/tick freeze')
    ops += [{'op': 'settle'}]
    dragon = '@e[tag=dmg_dragon,limit=1]'
    ops += setup('/summon ender_dragon 8.5 160 8.5 {Tags:["dmg_actor","dmg_dragon"],NoAI:1b,DragonPhase:0}')
    ops += [cmd(f'/execute if entity {dragon}', expect_result=1),
            cmd(f'/data get entity {dragon} Health', expect_result=200)]
    for damage, health in (
        ('8', 200),                         # no player, not an explosion: lands, no health
        ('8 minecraft:explosion', 197),     # 8 / 4 + 1
        ('40 minecraft:generic by @s', 189),  # 11 within the cooldown: only the excess over 3
        ('4 minecraft:in_fire by @s', 189),  # reallyHurt refuses fire; hurt still succeeds
        ('0', 189),                         # under 0.01: refused, invulnerable
    ):
        ops += [cmd(f'/damage {dragon} {damage}'),
                cmd(f'/data get entity {dragon} Health', expect_result=health)]
    for i, (label, nbt, hits) in enumerate((
        ('blast', '', (('1', False),)),
        ('explosion', '', (('1 minecraft:explosion', False),)),
        ('fire', '', (('1 minecraft:in_fire', True),)),
        ('shield', ',Invulnerable:1b', (('1', True), ('1 minecraft:generic_kill', False))),
        ('by_dragon', '', ((f'1 minecraft:generic by {dragon}', True), ('1', False))),
    )):
        crystal = f'@e[tag=dmg_{label},limit=1]'
        x, z = (40, 56, 40, 56, 40)[i], (8, 8, 24, 24, 40)[i]
        ops += setup(f'/summon end_crystal {x}.5 160 {z}.5 {{Tags:["dmg_actor","dmg_{label}"],ShowBottom:0b{nbt}}}')
        ops += [cmd(f'/execute if entity {crystal}', expect_result=1)]
        for damage, stays in hits:
            ops += [cmd(f'/damage {crystal} {damage}'),
                    cmd(f'/execute {"if" if stays else "unless"} entity {crystal}', expect_result=1)]
    # The dragons are summoned holding their pattern (0): a summoned dragon hovers (10), which
    # is sitting, and Horizon summons it holding (a separate gap).
    # One dragon at a time from here. dealDefaultKnockback: fireworks are #is_explosion (the
    # hit counts) and not #no_knockback, so the dragon is pushed away from the `at` position.
    knock = '@e[tag=dmg_knock,limit=1]'
    ops += setup('/kill @e[tag=dmg_dragon]',
                 '/summon ender_dragon 24.5 160 40.5 {Tags:["dmg_actor","dmg_knock"],NoAI:1b,DragonPhase:0}')
    ops += [cmd(f'/damage {knock} 10 minecraft:fireworks at 44.5 160 40.5'),
            cmd(f'/data get entity {knock} Health', expect_result=196),
            cmd(f'/data get entity {knock} Motion[0] 1000'),
            cmd(f'/data get entity {knock} Motion[2] 1000')]
    # AbstractDragonSittingPhase.onHurt: a perched dragon takes nothing from a direct arrow,
    # so the hit is under 0.01 and refused.
    perch = '@e[tag=dmg_perch,limit=1]'
    ops += setup('/kill @e[tag=dmg_knock]',
                 '/summon ender_dragon 8.5 160 40.5 {Tags:["dmg_actor","dmg_perch"],NoAI:1b,DragonPhase:6}',
                 '/summon arrow 8.5 170 40.5 {Tags:["dmg_actor","dmg_arrow"],NoGravity:1b}')
    ops += [cmd(f'/damage {perch} 10 minecraft:arrow by @e[tag=dmg_arrow,limit=1] from @s'),
            cmd(f'/data get entity {perch} Health', expect_result=200),
            cmd(f'/damage {perch} 10 minecraft:arrow by @s'),
            cmd(f'/data get entity {perch} Health', expect_result=196)]  # 196.5
    return ops + setup('/kill @e[tag=dmg_actor]', '/tick unfreeze', '/forceload remove all')


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
        # Far from the End's centre: the oracle's settles keep their tickets for the rest of the run, so a
        # group that settles in the End (gateway passengers) leaves 0 64 0 loaded on the Java side only.
        cmd('/execute in minecraft:the_end if block 1000 64 1000 minecraft:air'),
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


def loot_set_name_and_components():
    """SetNameFunction and SetComponentsFunction in /loot's items engine."""
    import json
    cases = [
        ('minecraft:stone', {'type': 'minecraft:set_name', 'name': 'Rock'}),
        ('minecraft:stone', {'type': 'minecraft:set_name', 'target': 'item_name',
                             'name': {'text': 'Old Rock', 'italic': False, 'color': 'gold'}}),
        # No name: the stack stays as it was.
        ('minecraft:stone', {'type': 'minecraft:set_name'}),
        ('minecraft:paper', {'type': 'minecraft:set_components', 'components': {
            'minecraft:lore': [{'text': 'A torn page', 'color': 'gray'}],
            'minecraft:custom_data': {'page': 3}}}),
        # The contents make it a potion of swiftness: the feedback names it so.
        ('minecraft:potion', {'type': 'minecraft:set_components', 'components': {
            'minecraft:potion_contents': {'potion': 'minecraft:swiftness'}}}),
        ('minecraft:potion', [
            {'type': 'minecraft:set_components', 'components': {'minecraft:potion_contents': {
                'custom_effects': [{'id': 'minecraft:speed', 'duration': 600}]}}},
            {'type': 'minecraft:set_name', 'name': {'text': 'Potion of Escape', 'italic': False}}]),
        # A removal patch drops a component the item has by default.
        ('minecraft:apple', {'type': 'minecraft:set_components', 'components': {'!minecraft:food': {}}}),
    ]
    ops = setup('/setblock 2 220 2 minecraft:chest')
    for item, modifier in cases:
        table = {'pools': [{'rolls': 1, 'entries': [
            {'type': 'minecraft:item', 'name': item, 'modifier': modifier}]}]}
        ops += [
            cmd('/loot replace block 2 220 2 container.0 loot ' + json.dumps(table)),
            cmd('/data get block 2 220 2 Items'),
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


def loot_world_pack_tables():
    """ResourceOrIdArgument looks loot tables up in the server's registries: a world pack's
    table (advancement_loot_fixtures) is named by /loot and by a function of the pack."""
    return [
        *setup('/clear @s', '/setblock 2 220 2 minecraft:chest'),
        cmd('/loot give @s loot parity:pack_loot'),
        cmd('/clear @s minecraft:emerald 0'),
        cmd('/loot replace block 2 220 2 container.0 loot parity:pack_loot'),
        cmd('/data get block 2 220 2 Items'),
        cmd('/function parity:pack_loot_give'),
        cmd('/clear @s minecraft:emerald 0'),
        cmd('/loot give @s loot parity:missing'),
        *setup('/clear @s', '/setblock 2 220 2 minecraft:air'),
    ]


def loot_give_full_inventory():
    """LootCommand.playerGive: `Inventory.add` keeps what fits and throws nothing out. A drop
    counts only when all of it went in, and always for a player with infinite materials
    (creative)."""
    import json
    two_dirt = {'pools': [{'rolls': 1, 'entries': [{'type': 'minecraft:item', 'name': 'minecraft:dirt',
                                                     'modifier': {'type': 'minecraft:set_count', 'count': 2}}]}]}
    give = '/loot give @s loot ' + json.dumps(two_dirt)
    dropped = '/execute if entity @e[type=minecraft:item,distance=..8]'
    ops = setup('/clear @s', '/gamemode survival', '/kill @e[type=minecraft:item]',
                *[f'/item replace entity @s container.{i} with minecraft:stone 64' for i in range(35)],
                '/item replace entity @s container.35 with minecraft:dirt 63')
    return ops + [
        # One of the two fits: it stays, the other is lost, and the drop doesn't count.
        cmd(give),
        cmd('/clear @s minecraft:dirt 0'),
        cmd(dropped),
        # None fits.
        cmd(give),
        cmd(dropped),
        *setup('/gamemode creative'),
        cmd(give),
        cmd(dropped),
        *setup('/clear @s', '/kill @e[type=minecraft:item]'),
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


def template_container_loot_table():
    """A template's chest keeps its loot table (`RandomizableContainer.tryLoadLootTable`)."""
    return [
        *setup('/forceload add 0 0 15 15'),
        cmd('/place template minecraft:village/plains/houses/plains_small_house_7 0 200 0'),
        cmd('/data get block 4 201 5 LootTable'),
        *setup('/fill 0 200 0 15 215 15 minecraft:air', '/forceload remove 0 0 15 15'),
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
    if 'crafting recipe priority' in group_names:
        return {f'h295:{name}': {'criteria': {'crafted': {
                    'trigger': 'minecraft:crafter_recipe_crafted', 'conditions': {'recipes': recipe}}}}
                for name, recipe in [('custom_crafted', 'parity:aaa_star'), ('wrong_recipe', 'parity:zzz_star')]}
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
    if 'fork function feedback' in group_names:
        try:
            from .fork_function_feedback_corpus import functions as fork_functions
        except ImportError:
            from fork_function_feedback_corpus import functions as fork_functions
        functions.update(fork_functions())
    if 'function condition fallthrough' in group_names:
        try:
            from .function_condition_corpus import functions as condition_functions
        except ImportError:
            from function_condition_corpus import functions as condition_functions
        functions.update(condition_functions())
    if any(name.startswith('wolf anger ') for name in group_names):
        try:
            from .wolf_anger_corpus import functions as anger_functions
        except ImportError:
            from wolf_anger_corpus import functions as anger_functions
        functions.update(anger_functions())
    if 'macro source type error' in group_names:
        functions.update({'command_macro:apply': '$scoreboard players add #hits macro_error $(amount)\n',
                          'command_macro:admit': 'scoreboard players add #admit macro_error 1\n',
                          'command_macro:fail': 'return fail\n',
                          'command_macro:zero': 'return 0\n'})
    if 'loot world pack tables' in group_names:
        functions['parity:pack_loot_give'] = 'loot give @s loot parity:pack_loot\n'
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


def macro_source_type_error():
    ops = [cmd('/tick freeze'), cmd('/datapack list enabled'),
           cmd('/scoreboard objectives add macro_error dummy', expect_result=1),
           cmd('/scoreboard players set #hits macro_error 0', expect_result=0),
           cmd('/scoreboard players set #admit macro_error 0', expect_result=0),
           cmd('/data modify storage command_macro:args number set value 7', expect_result=1),
           cmd('/data modify storage command_macro:args text set value "seven"', expect_result=1),
           cmd('/data modify storage command_macro:args valid set value {amount:2}', expect_result=1),
           cmd('/data get storage command_macro:args number', expect_result=7),
           cmd('/data get storage command_macro:args text', expect_result=5),
           cmd('/execute if data storage command_macro:args {number:7,text:"seven",valid:{amount:2}}', expect_result=1),
           cmd('/function command_macro:admit'),
           cmd('/scoreboard players get #admit macro_error', expect_result=1)]
    ops += setup('/execute store result score #clock macro_error run time query gametime')
    count = 0
    for path in ['valid', 'number', 'text', 'missing']:
        call = f'function command_macro:apply with storage command_macro:args {path}'
        direct = cmd('/' + call)
        if path != 'valid':
            direct['expect_callback'] = [False, 0]
        ops.append(direct)
        count += 2 if path == 'valid' else 0
        ops += [cmd('/scoreboard players get #hits macro_error', expect_result=count),
                cmd('/scoreboard players set #r macro_error 31', expect_result=31),
                cmd('/scoreboard players set #s macro_error 17', expect_result=17),
                cmd('/execute store result score #r macro_error store success score #s macro_error run ' + call),
                # Independently measured: no return leaves sentinels; errors store failure.
                cmd('/scoreboard players get #r macro_error', expect_result=31 if path == 'valid' else 0),
                cmd('/scoreboard players get #s macro_error', expect_result=17 if path == 'valid' else 0)]
        count += 2 if path == 'valid' else 0
        ops.append(cmd('/scoreboard players get #hits macro_error', expect_result=count))
    for name in ['fail', 'zero']:
        call = cmd(f'/function command_macro:{name}')
        call['expect_callback'] = [name == 'zero', 0]
        ops += [call,
                cmd('/scoreboard players set #r macro_error 31', expect_result=31),
                cmd('/scoreboard players set #s macro_error 17', expect_result=17),
                cmd(f'/execute store result score #r macro_error store success score #s macro_error run function command_macro:{name}'),
                cmd('/scoreboard players get #r macro_error', expect_result=0),
                cmd('/scoreboard players get #s macro_error', expect_result=int(name == 'zero')),
                cmd('/scoreboard players get #hits macro_error', expect_result=count)]
    ops += setup('/execute store result score #elapsed macro_error run time query gametime',
                 '/scoreboard players operation #elapsed macro_error -= #clock macro_error')
    return ops + [cmd('/scoreboard players get #elapsed macro_error', expect_result=0)]


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


def fork_function_feedback_cases():
    try:
        from .fork_function_feedback_corpus import cases
    except ImportError:
        from fork_function_feedback_corpus import cases
    return cases(cmd)


def function_condition_fallthrough():
    try:
        from .function_condition_corpus import conditions
    except ImportError:
        from function_condition_corpus import conditions
    return conditions()


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


def summoned_dragon_hovers():
    """EnderDragon's constructor sets EnderDragonPhase.HOVERING; only EnderDragonFight.createNewDragon
    moves its dragon to the holding pattern. A dragon from /summon hovers where it was put: the
    hover phase's target is its own position, and no fight takes it on outside the End."""
    dragon = '@e[tag=hover_dragon,limit=1]'
    ops = setup('/forceload add 0 0 31 31')
    ops += [{'op': 'settle'}]
    ops += setup('/summon ender_dragon 16.5 160 16.5 {Tags:["hover_dragon"]}')
    ops += [cmd(f'/data get entity {dragon} DragonPhase', expect_result=10)]
    for _ in range(6):
        ops += [{'op': 'tick', 'n': 20},
                cmd(f'/data get entity {dragon} DragonPhase'),
                *[cmd(f'/data get entity {dragon} Pos[{i}] 100') for i in range(3)]]
    # DragonHoverPhase.getFlySpeed is 1: a hovering dragon thrown upwards climbs back to its
    # target at up to 0.01 a tick (ydd / horizontal distance clamped to the fly speed).
    rise = '@e[tag=hover_rise,limit=1]'
    ops += setup('/kill @e[tag=hover_dragon]',
                 '/summon ender_dragon 16.5 160 16.5 {Tags:["hover_rise"],DragonPhase:10,Motion:[0.0d,0.5d,0.0d]}')
    for _ in range(4):
        ops += [{'op': 'tick', 'n': 10},
                *[cmd(f'/data get entity {rise} Pos[{i}] 1000') for i in range(3)]]
    return ops + setup('/kill @e[tag=hover_rise]', '/forceload remove all')


def dragon_scanning_turn():
    """DragonSittingScanningPhase: the dragon turns towards the player only while they are more
    than 10 degrees off its heading. Summoned 6 blocks south of a survival player with yaw 0
    (looking towards -z, at them), it holds still; one with yaw 90 turns. Its parts are at the
    origin until its first aiStep, so its first turn aims from there (far away: a small step)."""
    ops = setup('/gamemode survival', '/execute at @s run forceload add ~-32 ~-32 ~32 ~32')
    ops += [{'op': 'settle'}]
    for label, yaw in (('facing', 0), ('away', 90)):
        dragon = f'@e[tag=scan_{label},limit=1]'
        ops += setup(f'/execute at @s run summon ender_dragon ~ ~ ~6 {{Tags:["scan_{label}"],DragonPhase:6,Rotation:[{yaw}.0f,0.0f]}}')
        for _ in range(3):
            ops += [{'op': 'tick', 'n': 2},
                    cmd(f'/data get entity {dragon} Rotation[0] 1000'),
                    cmd(f'/data get entity {dragon} DragonPhase')]
        ops += setup(f'/kill {dragon}')
    return ops + setup('/gamemode creative', '/forceload remove all')


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


def forceload_loads_the_chunk():
    """`ServerLevel.setChunkForced` loads a newly forced chunk before the next command."""
    return [
        cmd('/execute if loaded 4000 200 4000'),
        cmd('/forceload add 4000 4000'),
        cmd('/execute if loaded 4000 200 4000'),
        cmd('/setblock 4000 200 4000 minecraft:stone'),
        cmd('/execute if block 4000 200 4000 minecraft:stone'),
        *setup('/setblock 4000 200 4000 minecraft:air', '/forceload remove 4000 4000'),
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
    # The item checks look in the pen only: the Rust side block-ticks a wider ring of terrain than the
    # oracle loads, where generated kelp breaks and gravel falls into flooded caves (as vanilla's own
    # would once ticked), and those drops are no piglin's.
    PEN = 'x=4.5,y=210,z=4.5,distance=..16'
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
            cmd(f'/execute if entity @e[type=minecraft:item,{PEN}]'),
            {'op': 'tick', 'n': 140},
            cmd('/execute if items entity @e[tag=barter,limit=1] weapon.offhand *'),
            cmd('/execute if items entity @e[tag=barter,limit=1] weapon.mainhand minecraft:gold_ingot'),
            # A baby drops nothing. What an adult's barter throws comes from the level's random (vanilla's is
            # unseeded), and the piglin picks some of it back up, so only the gold's fate is fixed: none of it lies
            # on the ground.
            cmd(f'/execute if entity @e[type=minecraft:item,{PEN}]' if baby
                else f'/execute if items entity @e[type=minecraft:item,{PEN}] contents minecraft:gold_ingot'),
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
            cmd(f'/execute if entity @e[type=minecraft:item,{PEN}]'),
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


def potion_contents_names():
    """`PotionItem.getName` and `TippedArrowItem.getName`: a potion or a tipped arrow is
    named by its contents (`PotionContents.getName`: their own `custom_name`, the potion,
    else `empty`), over its `item_name`; without contents it takes the item's own name, and a
    custom name still wins."""
    stacks = [
        'minecraft:potion[minecraft:item_name="Bottle",minecraft:potion_contents={potion:"minecraft:long_swiftness"}]',
        'minecraft:potion',
        'minecraft:splash_potion[minecraft:potion_contents={custom_effects:[{id:"minecraft:speed",duration:600}]}]',
        'minecraft:lingering_potion[minecraft:item_name="Cloud",minecraft:potion_contents={potion:"minecraft:poison"}]',
        'minecraft:tipped_arrow[minecraft:item_name="Dart",minecraft:potion_contents={potion:"minecraft:poison"}]',
        'minecraft:potion[minecraft:potion_contents={potion:"minecraft:water",custom_name:"swiftness"}]',
        'minecraft:potion[!minecraft:potion_contents,minecraft:item_name="Bottle"]',
        'minecraft:potion[minecraft:custom_name="Mine",minecraft:potion_contents={potion:"minecraft:swiftness"}]',
    ]
    return [*setup('/clear @s'), *[cmd(f'/give @s {s}') for s in stacks], *setup('/clear @s')]


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


def block_predicate_nbt():
    """A block predicate's `{...}` partially matches the block entity's full
    saved data (BlockPredicateArgument, NbtUtils.compareNbt) (#12030)."""
    named, plain, stone = '26 220 30', '27 220 30', '28 220 30'
    out = setup('/tick freeze', '/fill 26 220 30 29 220 32 minecraft:air strict',
                f'/setblock {named} minecraft:chest{{CustomName:"expected",Items:['
                '{Slot:0b,id:"minecraft:apple",count:3},{Slot:2b,id:"minecraft:stone",count:1}]}',
                f'/setblock {plain} minecraft:chest', f'/setblock {stone} minecraft:stone')
    for kind in ('if', 'unless'):
        for pos, predicate in [
            (named, 'minecraft:chest'),
            (named, 'minecraft:chest{}'),
            (named, 'minecraft:chest{CustomName:"expected"}'),
            (named, 'minecraft:chest{CustomName:"different"}'),
            (named, 'minecraft:chest{CustomName:"\\u0065xpected"}'),
            (named, 'minecraft:chest[type=single]{CustomName:"expected"}'),
            (named, 'minecraft:chest[type=left]{CustomName:"expected"}'),
            (named, 'minecraft:chest{id:"minecraft:chest",x:26,y:220,z:30}'),
            (named, 'minecraft:chest{x:27}'),
            (named, 'minecraft:chest{Items:[{id:"minecraft:stone"}]}'),
            (named, 'minecraft:chest{Items:[{id:"minecraft:stone"},{id:"minecraft:apple"}]}'),
            (named, 'minecraft:chest{Items:[{id:"minecraft:diamond"}]}'),
            (named, 'minecraft:chest{Items:[{Slot:0b,count:3}]}'),
            (named, 'minecraft:chest{Items:[{Slot:0b,count:3b}]}'),
            (named, 'minecraft:chest{Items:[]}'),
            (named, 'minecraft:barrel{CustomName:"expected"}'),
            (named, '#minecraft:guarded_by_piglins{CustomName:"expected"}'),
            (named, '#minecraft:guarded_by_piglins{CustomName:"different"}'),
            (plain, 'minecraft:chest{}'),
            (plain, 'minecraft:chest{CustomName:"expected"}'),
            (plain, 'minecraft:chest{Items:[]}'),
            (stone, 'minecraft:stone'),
            (stone, 'minecraft:stone{}'),
            (stone, '#minecraft:base_stone_overworld{}'),
        ]:
            out.append(cmd(f'/execute {kind} block {pos} {predicate}'))
    # FillCommand and CloneCommands test the same predicate at each position.
    out += [cmd('/clone 26 220 30 27 220 30 26 220 32 filtered minecraft:chest{CustomName:"expected"}'),
            cmd('/execute if block 26 220 32 minecraft:chest{CustomName:"expected"}'),
            cmd('/execute if block 27 220 32 minecraft:air')]
    # (no items to spill: the fill cases test only which blocks the filter takes)
    out += setup('/data remove block 26 220 30 Items')
    out += [cmd('/fill 26 220 30 28 220 30 minecraft:barrel replace minecraft:chest{CustomName:"expected"}'),
            cmd('/execute if block 26 220 30 minecraft:barrel'),
            cmd('/execute if block 27 220 30 minecraft:chest'),
            cmd('/fill 26 220 30 28 220 30 minecraft:barrel replace #minecraft:guarded_by_piglins{}'),
            cmd('/execute if block 27 220 30 minecraft:barrel'),
            cmd('/fill 26 220 30 28 220 30 minecraft:barrel replace minecraft:stone{}')]
    out += setup('/kill @e[type=minecraft:item]', '/fill 26 220 30 29 220 32 minecraft:air strict',
                 '/tick unfreeze')
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
    return ops + setup('/scoreboard objectives remove h226')


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
    # Later groups run with the world's rules: piglins pick nothing up without mob_griefing.
    return ops + setup('/gamerule mob_griefing true', '/gamerule random_tick_speed 3')



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
    return ops + setup('/scoreboard objectives remove h227')



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



def mob_saved_pickup_loot():
    """Valid boolean inputs only; full bodies retain unrelated save diagnostics."""
    ops = [cmd('/tick freeze'), cmd('/forceload add 160 160 175 175'),
           {'op': 'wait_chunks', 'chunks': [[10, 10]]},
           cmd('/fill 160 199 160 175 199 175 stone', expect_result=256),
           cmd('/execute if loaded 164 200 164', expect_result=1),
           cmd('/execute if block 164 199 164 stone', expect_result=1),
           cmd('/scoreboard objectives add pickup_save dummy', expect_result=1)]
    ops += setup('/execute store result score start pickup_save run time query gametime')
    for species, name in enumerate(['pig', 'allay', 'sulfur_cube']):
        for value, supplied in enumerate(['', ',CanPickUpLoot:0b', ',CanPickUpLoot:1b']):
            n = 1 + species * 3 + value
            uuid = f'0000002e-0000-0000-0000-{n:012x}'
            array = f'[I;46,0,0,{n}]'
            tag = f'pickup_save_{n}'
            body = 'Pos:[164.5d,200d,164.5d],Motion:[0d,0d,0d],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:4f'
            expected = int(name == 'sulfur_cube' or (name == 'pig' and value == 2))
            ops += [cmd(f'/summon minecraft:{name} 164.5 200 164.5 {{UUID:{array},Tags:["{tag}"],{body}{supplied}}}', expect_result=1),
                    cmd(f'/execute as {uuid} if entity @s[type=minecraft:{name},tag={tag}]', expect_result=1),
                    cmd(f'/execute if data entity {uuid} {{UUID:{array},{body}}}', expect_result=1)]
            for _ in range(2):
                ops += setup('/scoreboard players set value pickup_save 317',
                             '/scoreboard players set success pickup_save 17')
                ops += [cmd(f'/data get entity {uuid} CanPickUpLoot', expect_result=expected),
                        cmd(f'/execute store result score value pickup_save store success score success pickup_save run data get entity {uuid} CanPickUpLoot', expect_result=expected),
                        cmd('/scoreboard players get value pickup_save', expect_result=expected),
                        cmd('/scoreboard players get success pickup_save', expect_result=1),
                        cmd(f'/execute if data entity {uuid} {{CanPickUpLoot:{expected}b}}', expect_result=1)]
            full = cmd(f'/data get entity {uuid}', record=False)
            full['trace'] = True
            ops.append(full)
    ops += setup('/execute store result score elapsed pickup_save run time query gametime',
                 '/scoreboard players operation elapsed pickup_save -= start pickup_save')
    return ops + [cmd('/scoreboard players get elapsed pickup_save', expect_result=0)]


def orb_saved_uuid():
    """Zero-tick identity gate; complete entity bodies remain raw diagnostics."""
    ops = [cmd('/tick freeze'), cmd('/forceload add 160 160 175 175'),
           {'op': 'wait_chunks', 'chunks': [[10, 10]]},
           cmd('/fill 160 199 160 175 199 175 stone', expect_result=256),
           cmd('/execute if loaded 164 200 164', expect_result=1),
           cmd('/execute if block 164 199 164 stone', expect_result=1),
           cmd('/scoreboard objectives add orb_identity dummy', expect_result=1)]
    ops += setup('/execute store result score start orb_identity run time query gametime')
    for kind, target, array, x, fields in [
        ('experience_orb', '00000000-0000-0000-0000-000000000002', '[I;0,0,0,2]', 164.5,
         'Value:1s,Age:0s,Health:5s'),
        ('pig', '0000000b-0000-000c-0000-000d0000000e', '[I;11,12,13,14]', 166.5,
         'Health:10f,NoAI:1b,Silent:1b,PersistenceRequired:1b'),
    ]:
        tag = f'orb_identity_{kind}'
        body = f'Pos:[{x}d,200d,164.5d],Motion:[0d,0d,0d],NoGravity:1b,{fields}'
        ops += [cmd(f'/summon minecraft:{kind} {x} 200 164.5 {{UUID:{array},Tags:["{tag}"],{body}}}', expect_result=1),
                # No saved-UUID predicate in admission: that is the defect under test.
                cmd(f'/execute as {target} if entity @s[type=minecraft:{kind},tag={tag}]', expect_result=1),
                cmd(f'/execute if data entity {target} {{{body}}}', expect_result=1)]
        for _ in range(3):
            ops += setup('/scoreboard players set value orb_identity 317',
                         '/scoreboard players set success orb_identity 17')
            ops += [cmd(f'/data get entity {target} UUID', expect_result=4),
                    cmd(f'/execute store result score value orb_identity store success score success orb_identity run data get entity {target} UUID', expect_result=4),
                    cmd('/scoreboard players get value orb_identity', expect_result=4),
                    cmd('/scoreboard players get success orb_identity', expect_result=1),
                    cmd(f'/execute if data entity {target} {{UUID:{array}}}', expect_result=1),
                    cmd(f'/execute as {target} if entity @s[type=minecraft:{kind},tag={tag}]', expect_result=1)]
            full = cmd(f'/data get entity {target}', record=False)
            full['trace'] = True
            ops.append(full)
    ops += setup('/execute store result score elapsed orb_identity run time query gametime',
                 '/scoreboard players operation elapsed orb_identity -= start orb_identity')
    return ops + [cmd('/scoreboard players get elapsed orb_identity', expect_result=0)]


def stand_saved_fall():
    """Five real tick boundaries; whole bodies remain separate raw diagnostics."""
    ops = [cmd('/tick freeze'), cmd('/forceload add 0 0 15 15'),
           {'op': 'settle', 'required_chunks': [[0, 0]]},
           {'op': 'wait_chunks', 'chunks': [[0, 0]]},
           cmd('/fill 0 199 0 15 199 15 stone', expect_result=256),
           cmd('/fill 0 200 0 15 210 15 air'),
           cmd('/execute if loaded 4 205 4', expect_result=1),
           cmd('/scoreboard objectives add stand_fall dummy', expect_result=1)]
    actors = [('0000001c-0000-0000-0000-000000000001', '[I;28,0,0,1]', 4.5, 0),
              ('0000001c-0000-0000-0000-000000000002', '[I;28,0,0,2]', 12.5, 1)]
    for uuid, array, x, gravity in actors:
        cell = int(x)
        ops += [cmd(f'/execute if block {cell} 199 4 stone', expect_result=1),
                cmd(f'/execute if block {cell} 205 4 air', expect_result=1),
                cmd(f'/execute if block {cell} 206 4 air', expect_result=1),
                cmd(f'/summon armor_stand {x} 205 4.5 {{UUID:{array},Tags:["stand_fall_{gravity}"],Health:20f,NoGravity:{gravity}b,Motion:[0d,0d,0d]}}', expect_result=1),
                cmd(f'/execute if data entity {uuid} {{Pos:[{x}d,205d,4.5d],Motion:[0d,0d,0d]}}', expect_result=1)]
    ops += setup('/execute store result score start stand_fall run time query gametime')
    for ticks, step in [(0, 0), (1, 1), (2, 1), (3, 1), (5, 2)]:
        if step:
            ops.append(cmd(f'/tick step {step}'))
            ops.append({'op': 'tick', 'n': step})
        ops += setup('/execute store result score elapsed stand_fall run time query gametime',
                     '/scoreboard players operation elapsed stand_fall -= start stand_fall')
        ops.append(cmd('/scoreboard players get elapsed stand_fall', expect_result=ticks))
        for uuid, array, x, gravity in actors:
            ops += [cmd(f'/execute as {uuid} if entity @s[type=minecraft:armor_stand,tag=stand_fall_{gravity}]', expect_result=1),
                    cmd(f'/execute if data entity {uuid} {{UUID:{array},Health:20f}}', expect_result=1),
                    cmd(f'/data get entity {uuid} Pos', expect_result=3),
                    cmd(f'/data get entity {uuid} Motion', expect_result=3),
                    cmd(f'/data get entity {uuid} fall_distance', expect_result=0),
                    cmd(f'/data get entity {uuid} OnGround', expect_result=0)]
            if gravity:
                ops.append(cmd(f'/execute if data entity {uuid} {{NoGravity:1b,Pos:[{x}d,205d,4.5d],Motion:[0d,0d,0d],fall_distance:0d}}', expect_result=1))
            else:
                ops.append(cmd(f'/execute unless data entity {uuid} {{NoGravity:1b}}', expect_result=1))
                # Independently measured on Java 26.3; the suffix also checks Double type.
                distance = {0: '0.0', 1: '0.0', 2: '0.07840000092983246',
                            3: '0.23363199830055237', 5: '0.7684761881828308'}[ticks]
                ops.append(cmd(f'/execute if data entity {uuid} {{fall_distance:{distance}d}}', expect_result=1))
            full = cmd(f'/data get entity {uuid}', record=False)
            full['trace'] = True
            ops.append(full)
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



def mob_nbt_damage_cooldown():
    """Unrelated NBT edits retain a live mob's internal damage protection."""
    objective = 'rewrite_hit'
    out = setup('/gamerule minecraft:spawn_mobs false', '/forceload remove 0 0 31 15')

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    def action(command, accepted=1):
        check(f'/scoreboard players set #r {objective} 31', 31)
        check(f'/scoreboard players set #s {objective} 17', 17)
        out.append(cmd(f'/execute store result score #r {objective} '
                       f'store success score #s {objective} run {command.lstrip("/")}'))
        check(f'/scoreboard players get #r {objective}', accepted)
        check(f'/scoreboard players get #s {objective}', accepted)

    def state(health, hurt, custom='before', silent=False):
        # Identity and actual values precede every rejected-hit observation.
        check(f'/execute as {uid} if entity @s[type=minecraft:pig,tag=rewrite_hit_{serial}]')
        check(f'/execute if data entity {uid} {{UUID:[I;294,0,0,{serial}],'
              f'Health:{health}f,HurtTime:{hurt}s,Pos:[{x}d,200d,10.5d],'
              f'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,CustomName:"{custom}"}}')
        check(f'/data get entity {uid} Health', health)
        check(f'/data get entity {uid} HurtTime', hurt)
        check(f'/data get entity {uid} CustomName', len(custom))
        check(f'/execute as {uid} {"if" if silent else "unless"} entity @s[nbt={{Silent:1b}}]')
        out.extend(setup(f'/execute store result score #clock {objective} run time query gametime'))
        check(f'/scoreboard players operation #clock {objective} -= #before {objective}', elapsed)

    check('/tick freeze')
    check('/gamerule minecraft:spawn_mobs', 0)
    check('/forceload add 0 0 31 15', 2)
    out.append({'op': 'settle'})
    check('/execute if loaded 0 200 0')
    check('/execute if loaded 31 200 15')
    out.extend(setup('/kill @e[type=!player]', '/fill 0 199 0 31 203 15 air'))
    check(f'/scoreboard objectives add {objective} dummy')
    cases = ['no_write', 'name', 'silent', 'unchanged', 'stronger', 'expiry',
             'fresh_write', 'health_write', 'hurt_write']
    for serial, case in enumerate(cases, 1):
        x = 2.5 + 3 * (serial - 1)
        uid = f'00000126-0000-0000-0000-{serial:012x}'
        elapsed = 0
        check(f'/execute if block {int(x)} 200 10 air')
        check(f'/setblock {int(x)} 199 10 stone')
        check(f'/execute if block {int(x)} 199 10 stone')
        out.extend(setup(f'/execute store result score #before {objective} run time query gametime'))
        check(f'/summon pig {x} 200 10.5 {{UUID:[I;294,0,0,{serial}],'
              f'Tags:["rewrite_hit","rewrite_hit_{serial}"],NoAI:1b,NoGravity:1b,'
              'Silent:0b,PersistenceRequired:1b,Health:10f,CustomName:"before"}')
        check(f'/data get entity {uid} UUID', 4)
        check(f'/data get entity {uid} Pos', 3)
        state(10, 0)
        if case == 'fresh_write':
            action(f'/data merge entity {uid} {{CustomName:"after"}}')
            state(10, 0, 'after')
            action(f'/damage {uid} 1 generic')
            state(9, 10, 'after')
            action(f'/damage {uid} 1 generic', 0)
            state(9, 10, 'after')
        else:
            action(f'/damage {uid} 1 generic')
            state(9, 10)
            action(f'/damage {uid} 1 generic', 0)
            state(9, 10)
            custom, silent, health, hurt = 'before', False, 9, 10
            if case != 'no_write':
                changes = {'name': 'CustomName:"after"', 'silent': 'Silent:1b',
                           'unchanged': 'CustomName:"before"', 'stronger': 'Silent:1b',
                           'expiry': 'CustomName:"after"', 'health_write': 'Health:6f,Silent:1b',
                           'hurt_write': 'HurtTime:0s,CustomName:"after"'}[case]
                action(f'/data merge entity {uid} {{{changes}}}', int(case != 'unchanged'))
                if case in ('name', 'expiry', 'hurt_write'):
                    custom = 'after'
                if case in ('silent', 'stronger', 'health_write'):
                    silent = True
                if case == 'health_write':
                    health = 6
                if case == 'hurt_write':
                    hurt = 0
                state(health, hurt, custom, silent)
            if case == 'expiry':
                check('/tick step 9')
                out.append({'op': 'tick', 'n': 9})
                elapsed = 9
                state(9, 1, custom)
                action(f'/damage {uid} 1 generic', 0)
                state(9, 1, custom)
                check('/tick step 1')
                out.append({'op': 'tick', 'n': 1})
                elapsed = 10
                state(9, 0, custom)
                action(f'/damage {uid} 1 generic')
                state(8, 10, custom)
            elif case == 'stronger':
                action(f'/damage {uid} 2 generic')
                state(8, 10, custom, silent)
                action(f'/damage {uid} 2 generic', 0)
                state(8, 10, custom, silent)
            else:
                action(f'/damage {uid} 1 generic', 0)
                state(health, hurt, custom, silent)
                if case == 'no_write':
                    action(f'/damage {uid} 2 generic')
                    state(8, 10)
                    # Prove ordinary expiry independently of any NBT rewrite.
                    check('/tick step 9')
                    out.append({'op': 'tick', 'n': 9})
                    elapsed = 9
                    state(8, 1)
                    action(f'/damage {uid} 2 generic', 0)
                    state(8, 1)
                    check('/tick step 1')
                    out.append({'op': 'tick', 'n': 1})
                    elapsed = 10
                    state(8, 0)
                    action(f'/damage {uid} 2 generic')
                    state(6, 10)
        out.extend(setup(f'/kill {uid}'))
    return out + setup(f'/scoreboard objectives remove {objective}',
                       '/fill 0 199 0 31 203 15 air', '/forceload remove 0 0 31 15', '/tick unfreeze')


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
    # /fill counts changed blocks; prior groups may already have stone here.
    out += setup('/fill 0 199 0 15 199 15 air')
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
    # /fill counts changed blocks; prior groups may already have stone here.
    out += setup('/fill 0 199 0 15 199 15 air')
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



def boat_passenger_tree_cases(original_full=False):
    try:
        from .boat_passenger_tree_corpus import cases
    except ImportError:
        from boat_passenger_tree_corpus import cases
    return cases(cmd, original_full)


def effective_component_presence_cases():
    try:
        from .effective_component_presence_corpus import cases
    except ImportError:
        from effective_component_presence_corpus import cases
    return cases(cmd)


def jukebox_saved_clock_cases():
    try:
        from .jukebox_saved_clock_corpus import cases
    except ImportError:
        from jukebox_saved_clock_corpus import cases
    return cases(cmd)


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



def crafter_map_extension():
    """Actual empty-map use, then scheduled crafter matching and saved-map derivation."""
    def map_state(id, value):
        return {'op': 'map_state', 'id': id, 'expect_map': value}
    ops = setup('/forceload add 0 0 15 15', '/fill 0 199 0 15 199 15 stone',
                '/tp @s 8.5 200 8.5 0 0', '/gamemode survival', '/clear @s',
                '/item replace entity @s weapon.mainhand with minecraft:map',
                '/scoreboard objectives add crafter_map dummy',
                '/data modify storage h298:identity uuid set from entity @s UUID')
    ops += [cmd('/execute if loaded 4 200 4', expect_result=1),
            cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,200d,8.5d],Health:20f}]', expect_result=1),
            cmd('/data get entity @s SelectedItem'),
            map_state(0, 'missing'), {'op': 'use_item'},
            cmd('/data get entity @s SelectedItem'),
            cmd('/execute if items entity @s weapon.mainhand minecraft:filled_map[minecraft:map_id=0]', expect_result=1),
            map_state(0, 'scale=0,locked=false,center=0,0'),
            {'op': 'crafter_max_map_fixture'},
            map_state(31, 'scale=4,locked=false,center=960,960'),
            map_state(99, 'missing')]
    ops += setup('/clear @s')
    for case, map_id in [('extend', 0), ('missing', 99), ('max_scale', 31), ('ordinary', None)]:
        ops += setup('/setblock 4 200 5 air', '/setblock 4 200 4 air', '/setblock 4 200 3 air',
                     '/setblock 4 200 3 chest[facing=south]',
                     '/setblock 4 200 4 crafter[orientation=north_up]')
        for i in range(9):
            item = ('minecraft:oak_log' if i == 0 else 'minecraft:air') if map_id is None else (
                f'minecraft:filled_map[minecraft:map_id={map_id}]' if i == 4 else 'minecraft:paper')
            ops += setup(f'/item replace block 4 200 4 container.{i} with {item}')
        ops += [cmd('/execute if block 4 200 4 minecraft:crafter[orientation=north_up,triggered=false]', expect_result=1),
                cmd('/execute if block 4 199 4 minecraft:stone', expect_result=1),
                cmd('/execute if block 4 200 3 minecraft:chest[facing=south]', expect_result=1),
                cmd('/data get block 4 200 4 id'), cmd('/data get block 4 200 4 Items'),
                cmd('/data get block 4 200 4 disabled_slots'), cmd('/data get block 4 200 3 Items')]
        ops += setup('/execute store result score start crafter_map run time query gametime')
        ops += [cmd('/setblock 4 200 5 redstone_block'),
                cmd('/execute if block 4 200 4 minecraft:crafter[triggered=true]', expect_result=1),
                {'op': 'tick', 'n': 10},
                cmd('/data get block 4 200 4 Items'), cmd('/data get block 4 200 3 Items')]
        if case == 'extend':
            ops += [cmd('/execute if items block 4 200 3 container.0 minecraft:filled_map[minecraft:map_id=1]', expect_result=1),
                    cmd('/execute unless data block 4 200 3 Items[0].components."minecraft:map_post_processing"', expect_result=1),
                    map_state(1, 'scale=1,locked=false,center=64,64')]
        ops += [map_state(0, 'scale=0,locked=false,center=0,0'),
                map_state(31, 'scale=4,locked=false,center=960,960')]
        ops += setup('/execute store result score elapsed crafter_map run time query gametime',
                     '/scoreboard players operation elapsed crafter_map -= start crafter_map')
        ops += [cmd('/scoreboard players get elapsed crafter_map', expect_result=10)]
    ops += setup('/execute store success score identity_changed crafter_map run data modify storage h298:identity uuid set from entity @s UUID')
    ops += [cmd('/scoreboard players get identity_changed crafter_map', expect_result=0),
            cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,200d,8.5d],Health:20f}]', expect_result=1)]
    # Later groups build on the setup's empty, creative world: take the room, its floor and its chunk back.
    return ops + setup('/setblock 4 200 3 air', '/setblock 4 200 4 air', '/setblock 4 200 5 air',
                       '/fill 0 199 0 15 199 15 air', '/gamemode creative', '/clear @s',
                       '/scoreboard objectives remove crafter_map', '/forceload remove 0 0 15 15')


def advancement_loot_fixtures(group_names):
    advancements, tables = {}, {}
    if 'advancement world loot rewards' in group_names:
        criteria = {'never': {'trigger': 'minecraft:impossible'}}
        advancements = {'parity:loot_reward': {'criteria': criteria,
                                             'rewards': {'loot': ['parity:reward']}},
                        'parity:no_loot': {'criteria': criteria}}
        tables['parity:reward'] = {'type': 'minecraft:advancement_reward',
                                  'pools': [{'rolls': 1, 'entries': [
                                      {'type': 'minecraft:item', 'name': 'minecraft:diamond'}]}]}
    if 'advancement empty loot reference' in group_names:
        tables['parity:empty_reward'] = {'type': 'minecraft:advancement_reward', 'pools': []}
    if 'loot world pack tables' in group_names:
        tables['parity:pack_loot'] = {'type': 'minecraft:chest', 'pools': [{'rolls': 1, 'entries': [
            {'type': 'minecraft:item', 'name': 'minecraft:emerald',
             'modifier': {'type': 'minecraft:set_count', 'count': 3}}]}]}
    try:
        from . import player_food_predicate_corpus as player_food
    except ImportError:
        import player_food_predicate_corpus as player_food
    if player_food.GROUP in group_names:
        advancements.update(player_food.advancements())
    return advancements, tables


def advancement_world_loot_rewards(named_table=False, include_missing_ack=False):
    """One world-only, modifier-free reward table; no XP, recipe or function reward."""
    ops = setup('/forceload add 0 0 15 15', '/setblock 8 229 8 stone',
                '/tp @s 8.5 230 8.5 0 0', '/clear @s',
                '/scoreboard objectives add reward_loot dummy')
    ops += [cmd('/execute if loaded 8 230 8', expect_result=1),
            cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,230d,8.5d],Health:20f}]', expect_result=1),
            cmd('/data get entity @s Inventory')]
    ops += setup('/data modify storage h292:identity value set from entity @s UUID',
                 '/execute store result score clock reward_loot run time query gametime')
    def reference(table):
        return ('{type:"minecraft:advancement_reward",pools:[{rolls:1,entries:['
                '{type:"minecraft:loot_table",value:"' + table + '"}]}]}')
    target = 'parity:reward' if named_table else reference('parity:reward')
    if not named_table:
        # Same reference shape with a missing key must leave the inventory empty.
        # Only its three codec/ACK rows are excluded from focused97; full100
        # records them unchanged. Both Inventory guards always remain recorded.
        ops += setup('/scoreboard players set missing_result reward_loot -99',
                     '/scoreboard players set missing_success reward_loot -99')
        ops += [cmd('/scoreboard players get missing_result reward_loot', expect_result=-99),
                cmd('/scoreboard players get missing_success reward_loot', expect_result=-99),
                cmd('/execute store result score missing_result reward_loot store success score missing_success reward_loot run loot give @s loot ' + reference('parity:missing_reward'), record=include_missing_ack),
                cmd('/scoreboard players get missing_result reward_loot', record=include_missing_ack),
                cmd('/scoreboard players get missing_success reward_loot', record=include_missing_ack),
                cmd('/data get entity @s Inventory', expect_result=0),
                cmd('/execute unless data entity @s Inventory[]', expect_result=1)]
    # A reference, never an inline diamond definition, proves the world-only table.
    ops += [cmd('/loot give @s loot ' + target, expect_result=1),
            cmd('/data get entity @s Inventory'),
            cmd('/execute if entity @s[nbt={Inventory:[{Slot:0b,id:"minecraft:diamond",count:1}]}]', expect_result=1),
            cmd('/clear @s', expect_result=1),
            cmd('/data get entity @s Inventory')]
    cases = [('grant', 'parity:loot_reward', 1, 1, True),
             ('grant', 'parity:loot_reward', 0, 1, True),
             ('revoke', 'parity:loot_reward', 1, 1, False),
             ('grant', 'parity:loot_reward', 1, 2, True),
             ('grant', 'parity:no_loot', 1, 2, True),
             ('grant', 'parity:no_loot', 0, 2, True),
             ('revoke', 'parity:no_loot', 1, 2, False),
             ('grant', 'parity:no_loot', 1, 2, True)]
    for verb, advancement, changed, count, complete in cases:
        ops += [cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,230d,8.5d],Health:20f}]', expect_result=1)]
        ops += setup('/scoreboard players set result reward_loot -99',
                     '/scoreboard players set success reward_loot -99')
        ops += [cmd('/scoreboard players get result reward_loot', expect_result=-99),
                cmd('/scoreboard players get success reward_loot', expect_result=-99),
                cmd(f'/execute store result score result reward_loot store success score success reward_loot run advancement {verb} @s only {advancement}'),
                cmd('/scoreboard players get result reward_loot', expect_result=changed),
                cmd('/scoreboard players get success reward_loot', expect_result=changed),
                cmd('/data get entity @s Inventory'),
                cmd(f'/execute if entity @s[nbt={{Inventory:[{{Slot:0b,id:"minecraft:diamond",count:{count}}}]}}]', expect_result=1)]
        ops += setup('/execute store result score count reward_loot run clear @s minecraft:diamond 0')
        ops += [cmd('/scoreboard players get count reward_loot', expect_result=count),
                cmd(f'/execute {"if" if complete else "unless"} entity @s[advancements={{{advancement}=true}}]', expect_result=1)]
    ops += [cmd('/loot give @s loot ' + target, expect_result=1),
            cmd('/data get entity @s Inventory'),
            cmd('/execute if entity @s[nbt={Inventory:[{Slot:0b,id:"minecraft:diamond",count:3}]}]', expect_result=1)]
    ops += setup('/execute store success score identity_changed reward_loot run data modify storage h292:identity value set from entity @s UUID',
                 '/execute store result score elapsed reward_loot run time query gametime',
                 '/scoreboard players operation elapsed reward_loot -= clock reward_loot')
    ops += [cmd('/scoreboard players get identity_changed reward_loot', expect_result=0),
            cmd('/scoreboard players get elapsed reward_loot', expect_result=0)]
    return ops + setup('/scoreboard objectives remove reward_loot', '/clear @s',
                       '/forceload remove 0 0 15 15')


def advancement_empty_loot_reference():
    """Separate valid-empty table control: success with zero output, not a missing reference."""
    reference = '{type:"minecraft:advancement_reward",pools:[{rolls:1,entries:[{type:"minecraft:loot_table",value:"parity:empty_reward"}]}]}'
    ops = setup('/forceload add 0 0 15 15', '/setblock 8 229 8 stone',
                '/tp @s 8.5 230 8.5 0 0', '/clear @s',
                '/scoreboard objectives add empty_loot dummy')
    ops += [cmd('/execute if loaded 8 230 8', expect_result=1),
            cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,230d,8.5d],Health:20f}]', expect_result=1),
            cmd('/data get entity @s Inventory', expect_result=0)]
    ops += setup('/data modify storage h292:empty_identity value set from entity @s UUID',
                 '/execute store result score clock empty_loot run time query gametime',
                 '/scoreboard players set result empty_loot -99',
                 '/scoreboard players set success empty_loot -99')
    ops += [cmd('/scoreboard players get result empty_loot', expect_result=-99),
            cmd('/scoreboard players get success empty_loot', expect_result=-99),
            cmd('/execute store result score result empty_loot store success score success empty_loot run loot give @s loot ' + reference, expect_result=0),
            cmd('/scoreboard players get result empty_loot', expect_result=0),
            cmd('/scoreboard players get success empty_loot', expect_result=1),
            cmd('/data get entity @s Inventory', expect_result=0),
            cmd('/execute unless data entity @s Inventory[]', expect_result=1)]
    ops += setup('/execute store success score identity_changed empty_loot run data modify storage h292:empty_identity value set from entity @s UUID',
                 '/execute store result score elapsed empty_loot run time query gametime',
                 '/scoreboard players operation elapsed empty_loot -= clock empty_loot')
    ops += [cmd('/scoreboard players get identity_changed empty_loot', expect_result=0),
            cmd('/scoreboard players get elapsed empty_loot', expect_result=0)]
    return ops + setup('/scoreboard objectives remove empty_loot', '/forceload remove 0 0 15 15')


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


def crafting_priority_recipes(group_names):
    if 'crafting recipe priority' not in group_names:
        return {}
    def recipe(inputs, item, count=1):
        return {'type': 'minecraft:crafting_shapeless', 'ingredients': ['minecraft:' + x for x in inputs],
                'result': {'id': 'minecraft:' + item, 'count': count}}
    # Write deliberately out of selection order; resource registration decides priority.
    return {'parity:zzz_star': recipe(['gunpowder', 'red_dye'], 'emerald'),
            'parity:aaa_star': recipe(['gunpowder', 'blue_dye'], 'diamond'),
            'a_priority:z_h295_pick': recipe(['gold_nugget', 'paper'], 'diamond'),
            'z_priority:a_h295_pick': recipe(['gold_nugget', 'paper'], 'emerald'),
            'z_priority:h295_equal': recipe(['coal', 'paper'], 'emerald'),
            'a_priority:h295_equal': recipe(['coal', 'paper'], 'gold_nugget', 3),
            'parity:h295_resource': recipe(['clay_ball', 'paper'], 'brick'),
            'parity:h295_resource-early': recipe(['clay_ball', 'paper'], 'nether_brick'),
            'minecraft:oak_planks': recipe(['oak_log'], 'gold_ingot', 2),
            'parity:zzzz_h295_unique': recipe(['apple', 'amethyst_shard'], 'quartz', 5)}


def crafting_recipe_priority(world=True):
    """Fresh natural crafters distinguish world selection, resource order and ID replacement."""
    ops = setup('/forceload add 0 0 15 15', '/fill 0 199 0 15 199 15 stone',
                '/tp @s 8.5 200 8.5 0 0', '/clear @s', '/recipe take @s *',
                '/scoreboard objectives add recipe_order dummy',
                '/data modify storage h295:identity uuid set from entity @s UUID')
    ops += [cmd('/execute if loaded 4 200 4', expect_result=1),
            cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,200d,8.5d],Health:20f}]', expect_result=1)]
    if world:
        for id in crafting_priority_recipes(['crafting recipe priority']):
            ops += [cmd('/recipe give @s ' + id, expect_result=1)]
        cases = [('early', ['gunpowder', 'blue_dye'], 'diamond', 1),
                 ('late', ['gunpowder', 'red_dye'], 'firework_star', 1),
                 ('path_before_namespace', ['gold_nugget', 'paper'], 'emerald', 1),
                 ('same_path_namespace', ['coal', 'paper'], 'gold_nugget', 3),
                 ('resource_file_prefix', ['clay_ball', 'paper'], 'nether_brick', 1),
                 ('base_id_replacement', ['oak_log'], 'gold_ingot', 2),
                 ('late_world_only', ['apple', 'amethyst_shard'], 'quartz', 5),
                 ('ordinary', ['birch_log'], 'birch_planks', 4)]
    else:
        cases = [('base_star', ['gunpowder', 'blue_dye'], 'firework_star', 1),
                 ('base_planks', ['oak_log'], 'oak_planks', 4),
                 ('ordinary', ['birch_log'], 'birch_planks', 4)]
    for label, inputs, output, count in cases:
        ops += setup('/setblock 4 200 5 air', '/setblock 4 200 4 air', '/setblock 4 200 3 air',
                     '/setblock 4 200 3 chest[facing=south]', '/setblock 4 200 4 crafter[orientation=north_up]')
        for i, item in enumerate(inputs):
            ops += setup(f'/item replace block 4 200 4 container.{i} with minecraft:{item} 1')
        ops += [cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,200d,8.5d],Health:20f}]', expect_result=1),
                cmd('/execute if block 4 199 4 stone', expect_result=1),
                cmd('/execute if block 4 200 4 crafter[orientation=north_up,triggered=false]', expect_result=1),
                cmd('/execute if block 4 200 3 chest[facing=south]', expect_result=1),
                cmd('/data get block 4 200 4 id'), cmd('/data get block 4 200 4 Items'),
                cmd('/data get block 4 200 4 disabled_slots'), cmd('/data get block 4 200 3 Items')]
        for i, item in enumerate(inputs):
            ops += [cmd(f'/execute if items block 4 200 4 container.{i} minecraft:{item}', expect_result=1)]
        ops += setup('/execute store result score start recipe_order run time query gametime')
        ops += [cmd('/setblock 4 200 5 redstone_block'),
                cmd('/execute if block 4 200 4 crafter[triggered=true]', expect_result=1),
                {'op': 'tick', 'n': 10}, cmd('/data get block 4 200 4 Items'),
                cmd('/execute unless data block 4 200 4 Items[]', expect_result=1),
                cmd('/data get block 4 200 3 Items'),
                cmd(f'/execute if items block 4 200 3 container.0 minecraft:{output}', expect_result=count),
                cmd(f'/execute if data block 4 200 3 Items[{{Slot:0b,id:"minecraft:{output}",count:{count}}}]', expect_result=1)]
        ops += setup('/execute store result score elapsed recipe_order run time query gametime',
                     '/scoreboard players operation elapsed recipe_order -= start recipe_order')
        ops += [cmd('/scoreboard players get elapsed recipe_order', expect_result=10)]
    if world:
        # Open-air ejection exercises the existing trigger route, independently
        # of insertion into a chest. The wrong recipe is a negative control.
        for advancement in ['h295:custom_crafted', 'h295:wrong_recipe']:
            ops += [cmd('/advancement grant @s only ' + advancement, expect_result=1),
                    cmd('/advancement revoke @s only ' + advancement, expect_result=1),
                    cmd('/execute unless entity @s[advancements={' + advancement + '=true}]', expect_result=1)]
        ops += setup('/kill @e[type=item]', '/setblock 4 200 5 air', '/setblock 4 200 4 air',
                     '/setblock 4 200 3 air', '/setblock 4 200 4 crafter[orientation=north_up]',
                     '/item replace block 4 200 4 container.0 with gunpowder 1',
                     '/item replace block 4 200 4 container.1 with blue_dye 1')
        ops += [cmd('/execute if entity @s[type=player,nbt={Pos:[8.5d,200d,8.5d],Health:20f}]', expect_result=1),
                cmd('/execute if block 4 199 4 stone', expect_result=1),
                cmd('/execute if block 4 200 3 air', expect_result=1),
                cmd('/execute if block 4 200 4 crafter[orientation=north_up,triggered=false]', expect_result=1),
                cmd('/execute if items block 4 200 4 container.0 gunpowder', expect_result=1),
                cmd('/execute if items block 4 200 4 container.1 blue_dye', expect_result=1)]
        ops += setup('/execute store result score start recipe_order run time query gametime')
        # Invoke the real block tick in the connected owner's context. Ordinary
        # server ticks have no acting player in this harness; recipient dispatch
        # is a separate route and is not repaired by recipe-ID normalization.
        ops += [cmd('/setblock 4 200 5 redstone_block'),
                {'op': 'crafter_tick', 'pos': [4, 200, 4]}, {'op': 'tick', 'n': 10},
                cmd('/execute unless data block 4 200 4 Items[]', expect_result=1),
                cmd('/execute if entity @e[type=item,nbt={Item:{id:"minecraft:diamond",count:1}}]', expect_result=1),
                cmd('/execute if entity @s[advancements={h295:custom_crafted=true}]', expect_result=1),
                cmd('/execute unless entity @s[advancements={h295:wrong_recipe=true}]', expect_result=1)]
        ops += setup('/execute store result score elapsed recipe_order run time query gametime',
                     '/scoreboard players operation elapsed recipe_order -= start recipe_order')
        ops += [cmd('/scoreboard players get elapsed recipe_order', expect_result=10)]
    ops += setup('/execute store success score identity_changed recipe_order run data modify storage h295:identity uuid set from entity @s UUID')
    ops += [cmd('/scoreboard players get identity_changed recipe_order', expect_result=0)]
    return ops + setup('/setblock 4 200 5 air', '/scoreboard objectives remove recipe_order')


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


def moving_piston_presentation_diagnostic():
    try:
        from .moving_piston_data_corpus import moving_piston_presentation
    except ImportError:
        from moving_piston_data_corpus import moving_piston_presentation
    return moving_piston_presentation(cmd, setup)


def piston_entity_tick_order():
    """#9337: entities travel before moving pistons push them, not afterward."""
    ops = []
    for block in ('stone', 'slime_block'):
        ops += setup('/kill @e[type=!player]', '/fill 0 200 0 5 215 5 air',
                     '/setblock 1 200 1 piston[facing=up]',
                     f'/setblock 1 201 1 {block}',
                     '/summon wither_skeleton 1.5 202 1.5 '
                     '{PersistenceRequired:1b,Tags:["piston_rider"]}',
                     '/attribute @e[tag=piston_rider,limit=1] movement_speed base set 0',
                     '/setblock 2 200 1 redstone_block')
        for _ in range(6):
            ops.append({'op': 'tick', 'n': 1})
            ops.append(cmd('/data get entity @e[tag=piston_rider,limit=1] Pos'))
            ops.append(cmd('/data get entity @e[tag=piston_rider,limit=1] Motion'))
    ops += setup('/kill @e[type=!player]', '/fill 0 200 0 5 215 5 air')
    return ops


def item_block_escape():
    """#12018: an item inside a block falls before it is pushed towards the open side."""
    ops = setup('/kill @e[type=!player]', '/fill 0 200 0 5 215 5 air',
                '/fill 0 200 1 2 200 1 stone', '/fill 1 200 0 1 200 2 stone',
                '/setblock 1 201 1 stone', '/setblock 0 200 1 stone',
                '/setblock 2 200 1 air',
                '/summon item 1.5 200.25 1.5 {Item:{id:"minecraft:diamond",count:1},'
                'PickupDelay:32767s,Tags:["escape"],Motion:[0.0d,0.2d,0.0d]}')
    item = '@e[tag=escape,limit=1]'
    # The east speed is random, but for two ticks the item's middle stays in
    # the stone whatever it is: it escapes east both times, so its height and
    # vertical motion are exact.
    for _ in range(2):
        ops.append({'op': 'tick', 'n': 1})
        ops.append(cmd(f'/data get entity {item} Pos[1]'))
        ops.append(cmd(f'/data get entity {item} Motion[1]'))
    ops += setup('/kill @e[type=!player]', '/fill 0 200 0 5 215 5 air')
    return ops


def scheduled_item_birth():
    """#12017: an item a scheduled block tick drops ticks in that tick's entity pass;
    one an entity's block contact drops does not."""
    ops = []
    for front in ('air', 'powder_snow'):
        ops += setup('/kill @e[type=!player]', '/fill 0 200 0 5 215 5 air',
                     '/setblock 1 200 1 dropper[facing=up]{Items:[{Slot:0b,id:"minecraft:stone",count:1}]}',
                     f'/setblock 1 201 1 {front}',
                     '/setblock 2 200 1 redstone_block')
        item = '@e[type=item,x=1.5,y=201.5,z=1.5,distance=..3,limit=1]'
        # The dropper's tick is due 4 ticks after it was powered.
        for _ in range(6):
            ops.append({'op': 'tick', 'n': 1})
            ops.append(cmd(f'/execute if entity {item}'))
            ops.append(cmd(f'/data get entity {item} Age'))
    # A boat breaks a lily pad in its own tick (`applyEffectsFromBlocks`), inside
    # the entity pass: that drop waits for the next tick.
    ops += setup('/kill @e[type=!player]', '/fill 0 200 0 5 215 5 air',
                 '/fill 0 200 0 2 200 2 water', '/setblock 1 201 1 lily_pad',
                 '/summon oak_boat 1.5 201 1.5 {NoGravity:1b}')
    item = '@e[type=item,x=1.5,y=201.5,z=1.5,distance=..3,limit=1]'
    for _ in range(3):
        ops.append({'op': 'tick', 'n': 1})
        ops.append(cmd('/execute if block 1 201 1 lily_pad'))
        ops.append(cmd(f'/execute if entity {item}'))
        ops.append(cmd(f'/data get entity {item} Age'))
    ops += setup('/kill @e[type=!player]', '/fill 0 200 0 5 215 5 air')
    return ops


def piston_push_ground():
    """#12029, #12100: `Entity.move(MoverType.PISTON)` ends in `setOnGroundWithMovement`
    on the server, so a push sets an item's, primed TNT's or experience orb's OnGround to whether
    its downward part was blocked: a sideways push off the stone it rests on
    clears it, and so does a free push down. The entity ticks before the piston
    moves it (#9337), so the reading after each tick is the push's. Only
    OnGround is compared: the float air drags (#11794) shift Motion."""
    ops = []
    for kind, fields in (('item', 'Item:{id:"minecraft:diamond",count:1},PickupDelay:32767s'),
                         ('tnt', 'fuse:200s'), ('experience_orb', 'Value:1s')):
        selector = f'@e[type=minecraft:{kind},tag=piston_ground,limit=1]'
        for case in ('sideways', 'down'):
            ops += setup('/kill @e[type=!player]', '/fill 28 208 28 36 218 34 air',
                         '/fill 28 209 28 36 209 34 stone')
            if case == 'sideways':
                ops += setup('/setblock 30 210 31 piston[facing=east]', '/setblock 31 210 31 stone',
                             f'/summon minecraft:{kind} 32.25 210 31.5 '
                             f'{{{fields},Motion:[-0.05d,0.0d,0.0d],Tags:["piston_ground"]}}')
                power = '/setblock 30 211 31'
            else:
                ops += setup('/setblock 32 216 31 piston[facing=down]', '/setblock 32 215 31 stone',
                             f'/summon minecraft:{kind} 32.5 213.9 31.5 '
                             f'{{{fields},NoGravity:1b,Tags:["piston_ground"]}}')
                power = '/setblock 32 217 31'
            ops.append({'op': 'tick', 'n': 1})
            ops.append(cmd(f'/data get entity {selector} OnGround'))
            ops += setup(f'{power} redstone_block')
            for _ in range(3):
                ops.append({'op': 'tick', 'n': 1})
                ops.append(cmd(f'/data get entity {selector} OnGround'))
    ops += setup('/kill @e[type=!player]', '/fill 28 208 28 36 218 34 air')
    return ops


def primed_tnt_box():
    """#12020: `EntityTypes.TNT` is sized 0.98F, so its box is half 0.49F wide
    and 0.98F tall, both widened from float: TNT stopped by a wall ends 0.49F
    short of it and TNT stopped by a ceiling 0.98F below it, not at 0.49 and
    0.98. NoGravity keeps the stops independent of gravity and drag."""
    ops = []
    for case, motion, blocks in (('wall', '0.6d,0.0d,0.0d', '/fill 33 209 30 33 213 33 stone'),
                                 ('ceiling', '0.0d,0.6d,0.0d', '/fill 30 212 30 33 212 33 stone')):
        ops += setup('/kill @e[type=!player]', '/fill 28 208 28 36 218 36 air', blocks,
                     '/summon minecraft:tnt 31.5 210 31.5 '
                     f'{{fuse:200s,NoGravity:1b,Motion:[{motion}],Tags:["tnt_box"]}}')
        for _ in range(3):
            ops.append({'op': 'tick', 'n': 1})
            ops.append(cmd('/data get entity @e[type=minecraft:tnt,tag=tnt_box,limit=1] Pos'))
    ops += setup('/kill @e[type=!player]', '/fill 28 208 28 36 218 36 air')
    return ops


def piston_tnt_effects():
    """#12023: `Entity.move(MoverType.PISTON)` uses up primed TNT's pending
    stuck multiplier without scaling the push, adds to its fall and resets it
    on landing, and applies the block speed factor (soul sand); then
    `moveEntityByPiston` runs `applyEffectsFromBlocks`, so powder snow it is
    pushed into makes it stuck before its next move and slime under it slows
    it. After a fall over 2.5, powder snow stops the push. The TNT ticks before
    the piston moves it (#9337). Positions are read to a tenth: the float box
    (#12020) moves a stopped push by 1e-8."""
    sel = '@e[type=minecraft:tnt,tag=piston_tnt,limit=1]'
    east = ('/setblock 30 210 31 piston[facing=east]',)
    down = ('/setblock 32 212 31 piston[facing=down]',)
    ops = []
    # blocks, summon x and y, TNT fields, power
    for blocks, at, fields, power in (
            (east + ('/setblock 32 210 31 powder_snow',), '31.5 210',
             'Motion:[0.0d,0.1d,0.0d]', '/setblock 30 211 31'),
            (east + ('/setblock 32 210 31 powder_snow',), '31.5 210',
             'fall_distance:3.0d', '/setblock 30 211 31'),
            (east + ('/fill 30 209 31 34 209 31 soul_sand',), '31.5 210',
             'Motion:[0.0d,0.0d,0.05d]', '/setblock 30 211 31'),
            (down + ('/setblock 32 210 31 stone',), '32.5 211',
             'fall_distance:3.0d', '/setblock 32 213 31'),
            (down + ('/setblock 32 210 31 slime_block',), '32.5 211',
             'fall_distance:3.0d,Motion:[0.0d,0.0d,0.05d]', '/setblock 32 213 31'),
            # Two pistons push it in one tick: the second finds the tick's
            # 0.51 spent, but `moveEntityByPiston` still runs the slime's stepOn.
            (down + ('/setblock 33 212 31 piston[facing=down]', '/fill 32 210 31 33 210 31 slime_block'),
             '33.0 211', 'Motion:[0.0d,0.0d,0.05d]', '/fill 32 213 31 33 213 31'),
            # Onto slime beside a rising bubble column: `stepOn` slows it before
            # the column lifts it (`applyEffectsFromBlocks` order).
            (down + ('/setblock 32 210 31 slime_block', '/setblock 33 210 31 soul_sand',
                     '/setblock 33 211 31 bubble_column[drag=false]'),
             '32.6 211', 'Motion:[0.0d,0.0d,0.05d]', '/setblock 32 213 31')):
        ops += setup('/kill @e[type=!player]', '/fill 28 208 28 36 218 34 air', *blocks,
                     f'/summon minecraft:tnt {at} 31.5 '
                     f'{{fuse:200s,NoGravity:1b,{fields},Tags:["piston_tnt"]}}')
        for tick in range(5):
            if tick == 1:
                ops += setup(f'{power} redstone_block')
            ops.append({'op': 'tick', 'n': 1})
            for path in ('Pos[0] 10', 'Pos[1] 10', 'Motion', 'fall_distance'):
                ops.append(cmd(f'/data get entity {sel} {path}'))
    ops += setup('/kill @e[type=!player]', '/fill 28 208 28 36 218 34 air')
    return ops


def primed_tnt_fall():
    """#12021: primed TNT is an `Entity`: `checkFallDamage` adds each step down,
    narrowed to a float, to its double `fall_distance`, which it saves and
    `/data` reads and writes. `PowderSnowBlock` holds it on a top 0.9 high
    once that is over 2.5 (`getCollisionShape`), and otherwise makes it stuck
    (`entityInside` -> `makeStuckInBlock`, 0.9F/1.5/0.9F), so it sinks; a
    cobweb makes it stuck the same way (0.25/0.05F/0.25)."""
    sel = '@e[type=minecraft:tnt,tag=tnt_fall,limit=1]'
    ops = []

    def track(ticks):
        out = []
        for _ in range(ticks):
            out.append({'op': 'tick', 'n': 1})
            for path in ('Pos', 'Motion', 'fall_distance'):
                out.append(cmd(f'/data get entity {sel} {path}'))
        return out

    # block at 32 210 32, summon y, fields, ticks recorded
    for block, y, fields, ticks in (
            ('air', 216, 'Motion:[0.0d,-1.0d,0.0d]', 3),
            ('stone', 211.1, 'fall_distance:3.0d,Motion:[0.0d,-1.0d,0.0d]', 2),
            ('powder_snow', 216, 'Motion:[0.0d,-1.0d,0.0d]', 8),
            ('powder_snow', 210.75, 'Motion:[0.0d,0.0d,0.0d]', 4),
            # It passes a fall of 2.5 just above the snow: its 0.9-high top
            # only counts once the TNT reaches it.
            ('powder_snow', 213.86, 'Motion:[0.0d,0.0d,0.0d]', 14),
            ('powder_snow', 211.1, 'fall_distance:3.0d,Motion:[0.0d,-1.0d,0.0d]', 4),
            ('cobweb', 212, 'Motion:[0.0d,-0.5d,0.0d]', 5)):
        ops += setup('/kill @e[type=!player]', '/fill 28 205 28 36 220 36 air',
                     '/fill 30 209 30 34 209 34 stone', f'/setblock 32 210 32 {block}',
                     f'/summon minecraft:tnt 32.5 {y} 32.5 {{fuse:200s,{fields},Tags:["tnt_fall"]}}')
        ops += track(ticks)
    # /data writes the field the next fall adds to.
    ops += setup('/kill @e[type=!player]', '/fill 28 205 28 36 220 36 air',
                 '/fill 30 209 30 34 209 34 stone',
                 '/summon minecraft:tnt 32.5 216 32.5 {fuse:200s,NoGravity:1b,Tags:["tnt_fall"]}')
    ops.append(cmd(f'/data modify entity {sel} fall_distance set value 2.25d'))
    ops.append(cmd(f'/data get entity {sel} fall_distance'))
    ops.append(cmd(f'/data merge entity {sel} {{NoGravity:0b,Motion:[0.0d,-0.5d,0.0d]}}'))
    ops += track(2)
    ops += setup('/kill @e[type=!player]', '/fill 28 205 28 36 220 36 air')
    return ops


def loaded_ticket_removal(blocks=False):
    """Entity-ticking command visibility after forced tickets leave; FULL is separate."""
    targets = [(d, x, z) for d in range(2, 7)
               for x, z in [(0, d * 16 + 3), (d * 16 + 3, d * 16 + 3), (-d * 16 + 3, 0)]]
    ops = setup('/scoreboard objectives add loaded_audit dummy', '/forceload remove all',
                '/tp @s 0.5 -63 0.5 0 0')
    ops += [cmd('/forceload add -128 -128 127 127', expect_result=256),
            {'op': 'settle'},
            {'op': 'wait_chunks', 'chunks': [[x >> 4, z >> 4] for _, x, z in targets]}]
    for _, x, z in targets:
        ops += [{'op': 'chunk_ready', 'chunk': [x >> 4, z >> 4], 'expect_values': [1, 1, 1]}]
        ops += setup(f'/setblock {x} -63 {z} air')
    ops += setup('/setblock 0 -64 0 stone', '/setblock 0 -63 0 air', '/setblock 0 -62 0 air',
                 '/data modify storage h010:identity uuid set from entity @s UUID')
    ops += [cmd('/data get entity @s UUID', expect_result=4),
            cmd('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[0.5d,-63d,0.5d]}]', expect_result=1),
            cmd('/forceload query', expect_result=256), {'op': 'server_distances', 'expect_values': [2, 2]}]
    for phase, ticks in [('before', 40), ('after', 120)]:
        if phase == 'after':
            ops += [cmd('/forceload remove all', expect_result=0), cmd('/forceload query', expect_result=0)]
        ops += setup('/execute store result score start loaded_audit run time query gametime')
        ops += [{'op': 'tick', 'n': ticks, 'pace_ms': 50}]
        ops += setup('/execute store result score elapsed loaded_audit run time query gametime',
                     '/scoreboard players operation elapsed loaded_audit -= start loaded_audit')
        ops += [cmd('/scoreboard players get elapsed loaded_audit', expect_result=ticks),
                {'op': 'server_distances', 'expect_values': [2, 2]},
                cmd('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[0.5d,-63d,0.5d]}]', expect_result=1)]
        for distance, x, z in targets:
            present = phase == 'before' or distance == 2
            for sign in ['if', 'unless']:
                accepted = present if sign == 'if' else not present
                op = cmd(f'/execute {sign} loaded {x} -63 {z}')
                op['label'] = f'h010 {phase}/{distance}/{x},{z}/{sign}'
                op['expect_callback'] = [accepted, int(accepted)]
                ops.append(op)
            if blocks:
                op = cmd(f'/execute if block {x} -63 {z} air')
                op['label'] = f'h010 FULL diagnostic/{phase}/{distance}/{x},{z}'
                ops.append(op)
    ops += setup('/execute store success score changed loaded_audit run data modify storage h010:identity uuid set from entity @s UUID')
    ops += [cmd('/scoreboard players get changed loaded_audit', expect_result=0),
            cmd('/data get entity @s UUID', expect_result=4)]
    return ops
def mob_team_command_names():
    """Live mob command decoration; no damage, entity ticks or rendering claims."""
    uid = '00000125-0000-0000-0000-000000000001'
    team = 'command_red'
    objective = 'team_name_cmd'
    out = []

    def check(command, result=1):
        out.append(cmd(command, expect_result=result))

    check('/tick freeze')
    check('/gamerule minecraft:spawn_mobs', 0)
    out.extend(setup('/forceload remove 0 0'))
    check('/forceload add 0 0')
    out.append({'op': 'settle'})
    check('/execute if loaded 10 200 10')
    check('/setblock 10 199 10 stone')
    check('/execute if block 10 199 10 stone')
    check(f'/scoreboard objectives add {objective} dummy')
    out.extend(setup(f'/execute store result score #before {objective} run time query gametime'))
    check('/summon zombie 10.5 200 10.5 {UUID:[I;293,0,0,1],Tags:["command_team_subject"],'
          'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b}')

    def observe(member, host_member, custom):
        # Guard the actual actor and values before absence/default assertions.
        check(f'/execute as {uid} if entity @s[type=minecraft:zombie,tag=command_team_subject]')
        check(f'/execute if data entity {uid} {{UUID:[I;293,0,0,1],Health:20f,'
              'Pos:[10.5d,200d,10.5d],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b}')
        check(f'/data get entity {uid} UUID', 4)
        check(f'/data get entity {uid} Pos', 3)
        check(f'/data get entity {uid} Health', 20)
        check(f'/execute as {uid} {"if" if member else "unless"} entity @s[team={team}]')
        check(f'/execute {"if" if host_member else "unless"} entity @s[team={team}]')
        if custom:
            check(f'/data get entity {uid} CustomName', 3)
        else:
            out.append(cmd(f'/data get entity {uid} CustomName'))
        check(f'/tp {uid} 10.5 200 10.5')
        check('/tp @s 8.5 230 8.5')
        out.extend(setup(f'/execute store result score #clock {objective} run time query gametime'))
        check(f'/scoreboard players operation #clock {objective} -= #before {objective}', 0)

    observe(False, False, False)
    check(f'/team add {team}')
    check(f'/team modify {team} prefix "[P] "')
    check(f'/team modify {team} suffix " [S]"')
    check(f'/team modify {team} color red', 0)
    check(f'/team join {team} @s')
    # The host witnesses the same stored formatting before the mob joins.
    observe(False, True, False)
    check(f'/team join {team} {uid}')
    observe(True, True, False)
    check(f'/data merge entity {uid} {{CustomName:"Bob"}}')
    observe(True, True, True)
    check(f'/team modify {team} prefix ""')
    observe(True, True, True)
    check(f'/team modify {team} suffix ""')
    observe(True, True, True)
    check(f'/team leave {uid}')
    observe(False, True, True)
    check('/team leave @s')
    check(f'/data remove entity {uid} CustomName')
    observe(False, False, False)
    check(f'/team remove {team}', 0)
    out.extend(setup(f'/kill {uid}', f'/scoreboard objectives remove {objective}',
                     '/setblock 10 199 10 air', '/forceload remove 0 0', '/tick unfreeze'))
    return out


def reinforcement_room(legacy=False):
    ops = setup('/tick freeze', '/gamerule spawn_mobs false', '/gamerule spawn_monsters true',
                '/gamerule mob_drops false', '/gamerule advance_time false', '/scoreboard objectives add h300 dummy',
                '/forceload add -64 -64 63 63', '/tp @s 0.5 -63 3.5 0 0')
    ops += [{'op': 'settle'}]
    for x in range(-64, 64, 16):
        for z in range(-64, 64, 16):
            ops += [cmd(f'/execute if loaded {x} -63 {z}', expect_result=1)]
    if not legacy:
        # Reinforcement foot offsets can reachY-23. Solid foot cells exclude
        # generated caves above the room; the original roof/floor stay unchanged.
        # Prime each band with air so its stone ACK proves the complete volume.
        for y in range(-59, -22, 3):
            top = min(y + 2, -23)
            ops += setup(f'/fill -50 {y} -50 50 {top} 50 air')
            ops += [cmd(f'/fill -50 {y} -50 50 {top} 50 stone', expect_result=101 * 101 * (top - y + 1))]
        for pos, block in [('-40 -23 -40', 'stone'), ('40 -59 40', 'stone'),
                           ('0 -56 0', 'stone')]:
            ops += [cmd(f'/execute if block {pos} {block}', expect_result=1)]
    ops += setup('/fill -50 -63 -50 50 -61 50 air', '/fill -50 -64 -50 50 -64 50 air',
                 '/fill -50 -60 -50 50 -60 50 air')
    ops += [cmd('/fill -50 -64 -50 50 -64 50 stone', expect_result=10201),
            cmd('/fill -50 -60 -50 50 -60 50 stone', expect_result=10201), {'op': 'settle'}]
    if not legacy:
        for pos, block in [('0 -63 0', 'air'), ('0 -60 0', 'stone'), ('0 -64 0', 'stone')]:
            ops += [cmd(f'/execute if block {pos} {block}', expect_result=1)]
    for x, z in [(0, 0), (-16, -16), (-16, 16), (16, -16), (16, 16)]:
        ops += [{'op': 'light', 'pos': [x, -63, z], 'expect_light': [0, 0]}]
    ops += setup('/gamerule spawn_mobs true', '/kill @e[type=zombie]', '/kill @e[type=zombified_piglin]',
                 '/execute store result score start h300 run time query gametime',
                 '/data modify storage h300:actor uuid set from entity @s UUID')
    ops += [cmd('/gamerule spawn_mobs', expect_result=1), cmd('/gamerule spawn_monsters', expect_result=1),
            cmd('/execute if entity @s[type=player,nbt={Pos:[0.5d,-63d,3.5d],Health:20f}]', expect_result=1)]
    return ops

def command_zombie_reinforcements(once=False, raw=False, legacy=False):
    """Frozen accepted command hits, with typed random facts and additive raw snapshots."""
    ops = reinforcement_room(legacy) + setup('/scoreboard objectives add parity dummy')
    original_rows = []
    def original(label, command):
        op = cmd(command)
        op['label'] = 'h300 original/' + label
        original_rows.append(label)
        return [op]
    def clean(difficulty):
        # Cleanup must not make a hurt callee call again: peaceful before /kill.
        return setup('/difficulty peaceful', '/kill @e[type=zombie]', '/kill @e[type=zombified_piglin]',
                     '/difficulty ' + difficulty)
    def subject(kind, serial, tag):
        return setup(f'/summon {kind} 0.5 -63 0.5 {{UUID:[I;300,0,0,{serial}],NoAI:1b,Tags:["{tag}"]}}') + [
            cmd(f'/execute if entity @e[type={kind},tag={tag},nbt={{UUID:[I;300,0,0,{serial}],NoAI:1b,Health:20f,Pos:[0.5d,-63d,0.5d]}}]', expect_result=1),
            cmd('/execute if entity @s[type=player,gamemode=creative,nbt={Pos:[0.5d,-63d,3.5d],Health:20f}]', expect_result=1)]
    def chance(selector, value=1):
        return [cmd(f'/attribute {selector} minecraft:spawn_reinforcements base set {value}', expect_result=1),
                cmd(f'/attribute {selector} minecraft:spawn_reinforcements base get', expect_result=value),
                cmd(f'/attribute {selector} minecraft:spawn_reinforcements get', expect_result=1)]
    def read(command, success, result_name='#value', objective='parity'):
        return setup(f'/scoreboard players set {result_name} {objective} -99',
                     '/scoreboard players set read_ok h300 -99',
                     f'/execute store result score {result_name} {objective} store success score read_ok h300 run {command}') + [
                         cmd('/scoreboard players get read_ok h300', expect_result=success)]
    def clock(expected):
        return setup('/execute store result score elapsed h300 run time query gametime',
                     '/scoreboard players operation elapsed h300 -= start h300') + [cmd('/scoreboard players get elapsed h300', expect_result=expected)]
    def callee(kind, parent_tag, case):
        target = f'@e[type={kind},tag=!{parent_tag},limit=1]'
        charge = 'minecraft:reinforcement_callee_charge'
        out = [cmd(f'/execute if entity @e[type={kind},tag=!{parent_tag}]', expect_result=1)]
        out += setup(f'/data modify storage h300:callees {case} set from entity {target}')
        out += read(f'data get entity {target} UUID', 1, 'uuid_length', 'h300')
        out += [cmd('/scoreboard players get uuid_length h300', expect_result=4),
                cmd(f'/execute if data entity {target} attributes[{{id:"minecraft:spawn_reinforcements"}}].modifiers[{{id:"{charge}",amount:-0.05000000074505806d,operation:"add_value"}}]', expect_result=1)]
        out += read(f'data get entity {target} Health 1000', 1, 'health', 'h300')
        out += [cmd('/execute if score health h300 matches 1..', expect_result=1)]
        for axis, bounds in enumerate(['-40..40', '-63', '-40..40']):
            out += read(f'data get entity {target} Pos[{axis}]', 1, 'coordinate', 'h300')
            out += [cmd(f'/execute if score coordinate h300 matches {bounds}', expect_result=1)]
        out += [cmd(f'/execute at {target} if block ~ ~-1 ~ stone', expect_result=1)]
        out += read(f'attribute {target} minecraft:knockback_resistance modifier value get minecraft:random_spawn_bonus 1000', 1)
        out += [cmd('/execute if score #value parity matches 0..50', expect_result=1)]
        if not legacy:
            # A random baby may be a chicken jockey. These strict conditional
            # facts never equate a missing entity with a successful query. The raw
            # diagnostic retains vehicle presence and full snapshots separately.
            out += setup('/scoreboard players set vehicle_present h300 0',
                         f'/execute as {target} on vehicle run scoreboard players set vehicle_present h300 1',
                         f'/data remove storage h300:vehicles {case}',
                         f'/execute as {target} on vehicle run data modify storage h300:vehicles {case} set from entity @s',
                         f'/data modify storage h300:rider uuid set from entity {target} UUID')
            facts = [
                (f'execute as {target} on vehicle if entity @s[type=chicken,nbt={{Health:4f}}]', '1', 1),
                (f'execute as {target} on vehicle run data get entity @s UUID', '4', 1),
                (f'execute as {target} on vehicle run data get entity @s Pos[0]', '-40..40', 1),
                (f'execute as {target} on vehicle run data get entity @s Pos[1]', '-63', 1),
                (f'execute as {target} on vehicle run data get entity @s Pos[2]', '-40..40', 1),
                (f'execute as {target} on vehicle at @s if block ~ ~-1 ~ stone', '1', 1),
                (f'execute as {target} on vehicle on passengers if entity @s[type={kind},tag=!{parent_tag}]', '1', 1),
                (f'execute as {target} on vehicle on passengers run data modify storage h300:rider uuid set from entity @s UUID', '0', 0),
            ]
            for command, bounds, success in facts:
                out += setup('/scoreboard players set vehicle_ok h300 1',
                             '/execute if score vehicle_present h300 matches 1 run scoreboard players set vehicle_ok h300 0',
                             '/scoreboard players set vehicle_value h300 -99',
                             '/scoreboard players set vehicle_read h300 -99',
                             f'/execute store result score vehicle_value h300 store success score vehicle_read h300 run {command}',
                             f'/execute if score vehicle_present h300 matches 1 if score vehicle_read h300 matches {success} if score vehicle_value h300 matches {bounds} run scoreboard players set vehicle_ok h300 1')
                out += [cmd('/scoreboard players get vehicle_ok h300', expect_result=1)]
        if raw:
            if not legacy:
                out += [cmd(f'/data get storage h300:vehicles {case}')]
            op = cmd(f'/data get storage h300:callees {case}')
            op['label'] = 'h300 raw snapshot/' + case
            out.append(op)
        return out

    if once:
        ops += clean('hard') + subject('zombie', 30, 'origin') + chance('@e[tag=origin,limit=1]')
        ops += [cmd('/difficulty', expect_result=3),
                cmd('/damage @e[tag=origin,limit=1] 1 minecraft:player_attack by @s', expect_result=1)]
        ops += callee('zombie', 'origin', 'once') + clock(0)
        # Guarantee eligibility if an erroneously retained event runs later: .95 is not sufficient.
        ops += chance('@e[tag=origin,limit=1]', 2)
        marked = '@e[type=zombie,nbt={attributes:[{id:"minecraft:spawn_reinforcements",modifiers:[{id:"minecraft:reinforcement_callee_charge"}]}]}]'
        ops += [cmd('/execute if entity ' + marked, expect_result=1)]
        ops += setup('/data modify storage h300:once uuid set from entity @e[type=zombie,tag=!origin,limit=1] UUID', '/tick unfreeze')
        ops += [{'op': 'tick', 'n': 1}]
        ops += setup('/tick freeze')
        ops += [cmd('/execute if entity ' + marked, expect_result=1),
                cmd('/execute if data entity @e[tag=origin,limit=1] attributes[{id:"minecraft:spawn_reinforcements"}].modifiers[{id:"minecraft:reinforcement_caller_charge",amount:-0.05d}]', expect_result=1)]
        ops += setup('/execute store success score changed h300 run data modify storage h300:once uuid set from entity ' + marked[:-1] + ',limit=1] UUID')
        ops += [cmd('/execute if entity ' + marked, expect_result=1), cmd('/scoreboard players get changed h300', expect_result=0)] + clock(1)
    else:
        for difficulty, serial in [('easy', 10), ('hard', 11)]:
            ops += clean(difficulty) + subject('zombie', serial, 'origin')
            ops += [cmd('/difficulty', expect_result=1 if difficulty == 'easy' else 3)]
            # These are the same six original reads, with their failure flags retained separately.
            ops += read('attribute @e[tag=origin,limit=1] minecraft:spawn_reinforcements base get 1000', 1)
            ops += original(difficulty + '/summoned base range', '/execute if score #value parity matches 0..100')
            ops += read('attribute @e[tag=origin,limit=1] minecraft:knockback_resistance modifier value get minecraft:random_spawn_bonus 1000', 0)
            ops += original(difficulty + '/summoned knockback range', '/execute if score #value parity matches 0..50')
            ops += setup('/execute store success score #value parity run attribute @e[tag=origin,limit=1] minecraft:knockback_resistance modifier value get minecraft:random_spawn_bonus')
            ops += original(difficulty + '/summoned knockback exists', '/execute if score #value parity matches 1')
            ops += chance('@e[tag=origin,limit=1]')
            ops += original(difficulty + '/first hurt', '/damage @e[tag=origin,limit=1] 1 minecraft:player_attack by @s')
            ops += original(difficulty + '/birth', '/execute if entity @e[type=zombie,tag=!origin]')
            ops += original(difficulty + '/birth floor', '/execute if entity @e[type=zombie,tag=!origin,x=-60,y=-63.5,z=-60,dx=120,dy=1,dz=120]')
            ops += original(difficulty + '/caller charge', '/attribute @e[tag=origin,limit=1] minecraft:spawn_reinforcements modifier value get minecraft:reinforcement_caller_charge')
            ops += original(difficulty + '/callee charge', '/execute as @e[type=zombie,tag=!origin,limit=1] run attribute @s minecraft:spawn_reinforcements modifier value get minecraft:reinforcement_callee_charge')
            ops += read('attribute @e[type=zombie,tag=!origin,limit=1] minecraft:spawn_reinforcements base get 1000', int(difficulty == 'hard'))
            ops += original(difficulty + '/callee base range', '/execute if score #value parity matches 0..100')
            ops += original(difficulty + '/callee target', '/execute as @e[type=zombie,tag=!origin,limit=1] on target run data get entity @s Pos')
            if difficulty == 'hard':
                ops += callee('zombie', 'origin', 'hard')
            ops += setup('/data modify storage h300:health value set from entity @e[tag=origin,limit=1] Health',
                         '/scoreboard players set rejected_result h300 -99', '/scoreboard players set rejected_success h300 -99')
            ops += [cmd('/execute store result score rejected_result h300 store success score rejected_success h300 run damage @e[tag=origin,limit=1] 1 minecraft:player_attack by @s'),
                    cmd('/scoreboard players get rejected_result h300', expect_result=0), cmd('/scoreboard players get rejected_success h300', expect_result=0)]
            ops += setup('/execute store success score changed h300 run data modify storage h300:health value set from entity @e[tag=origin,limit=1] Health')
            ops += [cmd('/scoreboard players get changed h300', expect_result=0)]
            ops += original(difficulty + '/charge after rejected hit', '/attribute @e[tag=origin,limit=1] minecraft:spawn_reinforcements modifier value get minecraft:reinforcement_caller_charge')
            ops += original(difficulty + '/birth after rejected hit', '/execute if entity @e[type=zombie,tag=!origin]')
        ops += clean('hard') + subject('zombified_piglin', 12, 'piglin')
        ops += original('piglin/default chance', '/attribute @e[tag=piglin,limit=1] minecraft:spawn_reinforcements base get')
        ops += chance('@e[tag=piglin,limit=1]')
        ops += original('piglin/hurt', '/damage @e[tag=piglin,limit=1] 1 minecraft:player_attack by @s')
        ops += original('piglin/birth', '/execute if entity @e[type=zombified_piglin,tag=!piglin]')
        ops += callee('zombified_piglin', 'piglin', 'piglin')
        ops += clean('hard') + setup('/summon zombified_piglin 0.5 -63 0.5 {UUID:[I;300,0,0,13],NoAI:1b,Tags:["piglin"],CanBreakDoors:1b}')
        ops += [cmd('/execute if entity @e[type=zombified_piglin,tag=piglin,nbt={UUID:[I;300,0,0,13]}]', expect_result=1)]
        ops += original('piglin/saved door flag', '/data get entity @e[tag=piglin,limit=1] CanBreakDoors')
        ops += clean('hard') + subject('zombie', 14, 'origin') + chance('@e[tag=origin,limit=1]')
        ops += original('no attacker/hurt', '/damage @e[tag=origin,limit=1] 1 minecraft:generic')
        ops += original('no attacker/no birth', '/execute if entity @e[type=zombie,tag=!origin]')
        assert len(original_rows) == 30
        ops += clock(0)
    ops += setup('/execute store success score changed h300 run data modify storage h300:actor uuid set from entity @s UUID')
    ops += [cmd('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[0.5d,-63d,3.5d]}]', expect_result=1), cmd('/scoreboard players get changed h300', expect_result=0)]
    return ops


# Groups the full run leaves out: each runs alone in its own world (--only), so
# what they leave behind reaches no other group.
ISOLATED_REINFORCEMENTS = frozenset({'command zombie reinforcements', 'command reinforcement once'})
# These overlays replace built-in recipes, never beside the corpus's fixed vanilla recipe-count controls.
ISOLATED_RECIPES = frozenset({'crafting recipe priority', 'crafting base recipe control', 'transmute result patches'})
ISOLATED_FRAME_MAPS = frozenset({'frame live map detachment'})
ISOLATED_SPECTRAL_DURATION = frozenset({'spectral saved duration'})
ISOLATED_FRAME_MAPS = frozenset({'frame live map detachment'})
ISOLATED_ITEM_HEALTH = frozenset({'item Health short load'})
ISOLATED_FRAME_MAPS = frozenset({'frame live map detachment', 'treasure map copies'})
ISOLATED_MAP_CRAFTING = frozenset({'craft map takes'})
ISOLATED_MACRO_TYPE = frozenset({'macro source type error'})
ISOLATED_PICKUP_SAVE = frozenset({'mob saved pickup loot'})
ISOLATED_ORB_UUID = frozenset({'orb saved UUID'})
ISOLATED_STAND_FALL = frozenset({'stand saved fall distance'})
ISOLATED_POTION_ICONS = frozenset({'potion custom effect icons'})
ISOLATED_JUKEBOX_CLOCK = frozenset({'jukebox saved clock'})
ISOLATED_FUNCTION_FEEDBACK = frozenset({'fork function feedback'})
ISOLATED_EFFECTIVE_PRESENCE = frozenset({'effective component presence'})
ISOLATED_BOAT_TREE = frozenset({'boat passenger tree'})
# Its sync turns on fixed seeds for the whole script: the groups after it would draw from them (tools/test_seed_sync_parity.py).
ISOLATED_SEED_SYNC = frozenset({'seed sync proof'})
ISOLATED_EFFECTIVE_PRESENCE = frozenset({'effective component presence'})
ISOLATED_BOAT_TREE = frozenset({'boat passenger tree'})
ISOLATED_GUEST_CLOSE = frozenset({'guest container close event'})
ISOLATED_GROUPS = ISOLATED_REINFORCEMENTS | ISOLATED_RECIPES | ISOLATED_WOLF_ANGER | ISOLATED_FRAME_MAPS | ISOLATED_SPECTRAL_DURATION | ISOLATED_PICKUP_SAVE | ISOLATED_ORB_UUID | ISOLATED_STAND_FALL | ISOLATED_MAP_CRAFTING | ISOLATED_MACRO_TYPE | ISOLATED_ITEM_HEALTH | ISOLATED_POTION_ICONS | ISOLATED_JUKEBOX_CLOCK | ISOLATED_FUNCTION_FEEDBACK | ISOLATED_SEED_SYNC | ISOLATED_EFFECTIVE_PRESENCE | ISOLATED_BOAT_TREE | ISOLATED_GUEST_CLOSE


def groups():
    try:
        from .spectral_duration_corpus import cases as spectral_saved_duration
    except ImportError:
        from spectral_duration_corpus import cases as spectral_saved_duration
    try:
        from .item_health_load_corpus import cases as item_health_load
    except ImportError:
        from item_health_load_corpus import cases as item_health_load
    try:
        from .vehicle_kill_corpus import vehicle_kill_contents
    except ImportError:
        from vehicle_kill_corpus import vehicle_kill_contents
    try:
        from .vehicle_container_corpus import vehicle_containers
    except ImportError:
        from vehicle_container_corpus import vehicle_containers
    try:
        from .data_literal_corpus import storage_literals
    except ImportError:
        from data_literal_corpus import storage_literals
    try:
        from .craft_map_take_corpus import cases as craft_map_takes
    except ImportError:
        from craft_map_take_corpus import cases as craft_map_takes
    try:
        from .potion_effect_icon_corpus import cases as potion_effect_icons
    except ImportError:
        from potion_effect_icon_corpus import cases as potion_effect_icons
    try:
        from .trader_llama_delay_corpus import llama_delay
    except ImportError:
        from trader_llama_delay_corpus import llama_delay
    try:
        from .guest_close_event_corpus import cases as guest_close_events
    except ImportError:
        from guest_close_event_corpus import cases as guest_close_events
    try:
        from .transmute_patch_corpus import cases as transmute_patches
    except ImportError:
        from transmute_patch_corpus import cases as transmute_patches
    try:
        from .mob_effect_predicate_corpus import cases as mob_effect_predicates
    except ImportError:
        from mob_effect_predicate_corpus import cases as mob_effect_predicates
    try:
        from .seed_sync_corpus import cases as seed_sync_proof
    except ImportError:
        from seed_sync_corpus import cases as seed_sync_proof
    try:
        from .mob_name_predicate_corpus import cases as mob_name_predicates
    except ImportError:
        from mob_name_predicate_corpus import cases as mob_name_predicates
    try:
        from . import player_food_predicate_corpus as player_food
    except ImportError:
        import player_food_predicate_corpus as player_food
    try:
        from .frame_map_corpus import frame_maps, treasure_map_copies
        from .allay_damage_corpus import allay_player_damage
        from .wolf_anger_corpus import groups as wolf_anger_groups
        from .allay_healing_corpus import allay_healing
        from .loaded_cloud_corpus import loaded_clouds
        from .moving_piston_data_corpus import moving_piston_data
        from .carpet_farm_corpus import carpet_farms
        from .passenger_teleport_corpus import passenger_teleports
        from .gateway_passenger_corpus import gateway_passengers
        from .gateway_item_motion_corpus import gateway_item_motion
        from .item_merge_corpus import stack_limit_merges
    except ImportError:
        from frame_map_corpus import frame_maps, treasure_map_copies
        from allay_damage_corpus import allay_player_damage
        from wolf_anger_corpus import groups as wolf_anger_groups
        from allay_healing_corpus import allay_healing
        from loaded_cloud_corpus import loaded_clouds
        from moving_piston_data_corpus import moving_piston_data
        from carpet_farm_corpus import carpet_farms
        from passenger_teleport_corpus import passenger_teleports
        from gateway_passenger_corpus import gateway_passengers
        from gateway_item_motion_corpus import gateway_item_motion
        from item_merge_corpus import stack_limit_merges
    result = [
        ('setup', group_setup()),
        ('guest container close event', guest_close_events()),
        ('spectral saved duration', spectral_saved_duration(cmd, setup)),
        ('item Health short load', item_health_load(cmd, setup)),
        ('vehicle container saved fields', vehicle_containers(cmd)),
        ('macro source type error', macro_source_type_error()),
        ('potion custom effect icons', potion_effect_icons(cmd, setup)),
        ('stand saved fall distance', stand_saved_fall()),
        ('orb saved UUID', orb_saved_uuid()),
        ('mob saved pickup loot', mob_saved_pickup_loot()),
        ('transmute result patches', transmute_patches(cmd, setup)),
        ('mob effect predicate view', mob_effect_predicates(cmd, setup)),
        ('mob custom name components', mob_name_predicates(cmd, setup)),
        ('frame live map detachment', frame_maps(cmd, setup)),
        ('treasure map copies', treasure_map_copies(cmd, setup)),
        ('craft map takes', craft_map_takes(cmd, setup)),
        ('allay player damage immunity', allay_player_damage(cmd, setup)),
        *wolf_anger_groups(cmd, setup),
        ('allay passive healing', allay_healing(cmd, setup)),
        ('crafter map extension', crafter_map_extension()),


        ('command zombie reinforcements', command_zombie_reinforcements()),
        ('command reinforcement once', command_zombie_reinforcements(True)),
        ('advancement world loot rewards', advancement_world_loot_rewards()),
        ('advancement empty loot reference', advancement_empty_loot_reference()),
        ('crafting recipe priority', crafting_recipe_priority()),
        ('crafting base recipe control', crafting_recipe_priority(False)),
        ('item stack-size override merges', stack_limit_merges(cmd, setup)),
        ('moving piston command data', moving_piston_data(cmd, setup)),
        ('mob team command names', mob_team_command_names()),
        ('bell server glow', bell_server_glow()),
        ('advancement reward functions', advancement_reward_functions()),
        ('named weather predicates', named_weather_predicates()),
        ('named smoke predicates', named_smoke_predicates()),
        ('mining_efficiency_9021', mining_efficiency_9021()),
        ('structured item slots', structured_item_slots()),
        ('boat passenger tree', boat_passenger_tree_cases()),
        ('effective component presence', effective_component_presence_cases()),
        ('jukebox saved clock', jukebox_saved_clock_cases()),
        ('loaded potion clouds', loaded_clouds(cmd, setup)),
        ('server clock', server_clock()),
        ('inline execute predicates', inline_execute_predicates()),


        ('decorated pot data', decorated_pot_data()),
        ('modern data literal values', storage_literals(cmd)),
        ('comparator output data', comparator_output_data()),
        ('hopper empty slot limits', hopper_empty_slot_limits()),
        ('End clock commands', end_clock_commands()),
        ('client commands', client_commands()),
        ('game state', game_state()),
        ('player state', player_state()),
        ('experience level boundaries', experience_level_boundaries()),
        ('blocks and border', world_blocks()),
        ('setblock destroy air', setblock_destroy_air()),
        ('block predicate NBT', block_predicate_nbt()),
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
        ('seed sync proof', seed_sync_proof(cmd, setup)),
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
        ('piston entity tick order', piston_entity_tick_order()),
        ('primed tnt box', primed_tnt_box()),
        ('primed tnt fall', primed_tnt_fall()),
        ('item block escape', item_block_escape()),
        ('scheduled block item birth', scheduled_item_birth()),
        ('piston push ground', piston_push_ground()),
        ('piston tnt effects', piston_tnt_effects()),
        ('carpet farms', carpet_farms(cmd, setup)),
        ('passenger teleports', passenger_teleports(cmd, setup)),
        ('mob riding', mob_riding()),
        ('trial spawner passengers', trial_spawner_passengers()),
        ('gateway passengers', gateway_passengers(cmd, setup)),
        ('gateway item motion', gateway_item_motion(cmd, setup)),
        ('camel riders', camel_riders()),
        ('scores, teams and bossbars', scores()),
        ('bossbar list order', bossbar_list_order()),
        ('large fireball fire', large_fireball_fire()),
        ('mannequin pose anchors', mannequin_pose_anchors()),
        ('item command damage', item_command_damage()),
        ('dragon command damage', dragon_command_damage()),
        ('minecart numeric fields', minecart_numeric_fields()),
        ('enderman command damage', enderman_command_damage()),
        ('shulker command damage', shulker_command_damage()),
        ('SNBT trailing commas', snbt_trailing_commas()),
        ('execute', execute()),
        ('projectile origin', projectile_origin()),
        ('selector missing predicates', selector_missing_predicates()),
        ('levels', levels()),
        ('end portal entities', end_portal_entities()),
        ('items and loot', items_and_loot()),
        ('loot world pack tables', loot_world_pack_tables()),
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
        ('loot set name and components', loot_set_name_and_components()),
        ('loot give full inventory', loot_give_full_inventory()),
        ('item NBT count bounds', item_nbt_count_bounds()),
        ('item block containers', item_block_containers()),
        ('world', world_misc()),
        ('recipe special arguments', recipe_special_arguments()),
        ('progression', progression()),
        ('functions and packs', functions_and_packs()),
        ('mob NBT damage cooldown', mob_nbt_damage_cooldown()),
        ('mob full resistance hits', mob_full_resistance_hits()),
        ('sulfur cube loaded health', sulfur_cube_loaded_health()),
        ('function return feedback', function_return_feedback()),
        ('fork function feedback', fork_function_feedback_cases()),
        ('function condition fallthrough', function_condition_fallthrough()),
        ('ticking and random', ticking()),
        ('compute', compute()),
        ('social and scoreboard', social()),
        ('test', test_command()),
        ('wither breaks', wither_breaks()),
        ('tnt minecart rails', tnt_minecart_rails()),
        ('creative shulker drops', creative_shulker_drops()),
        ('minecart drag', minecart_drag()),
        ('ender dragon', ender_dragon()),
        ('summoned dragon hovers', summoned_dragon_hovers()),
        ('dragon scanning turn', dragon_scanning_turn()),
        ('place structure', place_structure()),
        ('template container loot table', template_container_loot_table()),
        ('tame wolf kills', tame_wolf_kills()),
        ('forceload loads the chunk', forceload_loads_the_chunk()),
        ('experience orb fall', experience_orb_fall()),
        ('piglin pickups', piglin_pickups()),
        ('experience orb stuck', experience_orb_stuck()),
        ('summoned potions', summoned_potions()),
        ('potion contents names', potion_contents_names()),
        ('creeper cloud selectors', creeper_cloud_selectors()),
        # Last: it leaves the player in other food than it found.
        (player_food.GROUP, player_food.cases(cmd, setup)),
    ]
    result.insert(1, ('vehicle kill contents', vehicle_kill_contents(cmd)))
    result.insert(1, ('trader llama NoAI delay', llama_delay(cmd)))
    return result
