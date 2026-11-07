"""Authoritative crafting-table map takes; menu snapshots are state observations.

Slots carry [item ID, count, map ID, map post-processing ID]. Full saved input
and output stacks are independently read with /data; revision values are raw
transport diagnostics, not a prediction/packet-family contract.
"""

def cases(cmd, setup):
    def stack(item, count=1, map_id=None, post=None):
        return ['minecraft:' + item, count, map_id, post]

    def snapshot(label, window, present, carried=None):
        slots = [None] * 46
        for slot, value in present.items():
            slots[slot] = value
        return {'op': 'craft_menu_state', 'label': label,
                'expect_observation': {'window': window, 'kind': 'minecraft:crafting',
                                       'slots': slots, 'carried': [] if carried is None else carried}}

    def click(slot, button=0, mode=0):
        return {'op': 'craft_menu_click', 'slot': slot, 'button': button, 'input': mode}

    ops = [cmd('/tick freeze'), cmd('/forceload add 0 0 31 31'),
           {'op': 'wait_chunks', 'chunks': [[0, 0], [1, 0], [0, 1], [1, 1]]}]
    for x, z in [(0, 0), (16, 0), (0, 16), (16, 16)]:
        ops.append(cmd(f'/execute if loaded {x} 100 {z}', expect_result=1))
    ops += [cmd('/fill 0 99 0 31 99 31 stone', expect_result=1024),
            cmd('/fill 0 100 0 31 104 31 air'),
            cmd('/setblock 6 100 4 crafting_table'),
            cmd('/gamemode survival @s'), cmd('/tp @s 4.5 100 4.5 0 0'),
            cmd('/clear @s'), cmd('/scoreboard objectives add craft_map dummy')]
    ops += setup('/execute store result score start craft_map run time query gametime',
                 '/data modify storage craft_map:owner uuid set from entity @s UUID')
    ops += [cmd('/execute if entity @s[type=player,gamemode=survival,nbt={Pos:[4.5d,100d,4.5d],Health:20f}]', expect_result=1),
            cmd('/data get entity @s UUID'),
            cmd('/execute if block 4 99 4 stone', expect_result=1),
            cmd('/execute if block 6 100 4 crafting_table', expect_result=1),
            cmd('/item replace entity @s hotbar.0 with map', expect_result=1),
            {'op': 'use_item', 'sequence': 1, 'yaw': 0, 'pitch': 0},
            cmd('/data get entity @s Inventory'),
            {'op': 'map_state', 'id': 0, 'expect_map': 'scale=0,locked=false,center=0,0'}]
    for window, (name, input_mode) in enumerate([('swap', 2), ('pickup', 0), ('quick_move', 1)], 1):
        if window > 1:
            ops += [cmd('/clear @s'),
                    cmd('/item replace entity @s hotbar.0 with filled_map[map_id=0]', expect_result=1)]
        ops += [cmd('/item replace entity @s hotbar.1 with paper 8', expect_result=1),
                cmd('/data get entity @s Inventory'), {'op': 'use_block', 'pos': [6, 100, 4]},
                snapshot(name + ' open', window, {37: stack('filled_map', map_id=0), 38: stack('paper', 8)}),
                click(38)]
        for slot in [1, 2, 3, 4, 6, 7, 8, 9]:
            ops.append(click(slot, 1))
        ops += [click(37), click(5)]
        grid = {i: stack('paper') for i in [1, 2, 3, 4, 6, 7, 8, 9]}
        grid[5] = stack('filled_map', map_id=0)
        grid[0] = stack('filled_map', map_id=0, post=1)
        ops += [snapshot(name + ' preview', window, grid), click(0, 2 if input_mode == 2 else 0, input_mode)]
        output = stack('filled_map', map_id=window)
        slots = {39: output} if input_mode == 2 else ({45: output} if input_mode == 1 else {})
        ops += [snapshot(name + ' taken', window, slots, output if input_mode == 0 else None)]
        if input_mode == 0:
            ops.append(click(39))
        ops += [{'op': 'craft_menu_close'}, cmd('/data get entity @s Inventory'),
                cmd('/clear @s paper 0'),
                {'op': 'map_state', 'id': 0, 'expect_map': 'scale=0,locked=false,center=0,0'},
                {'op': 'map_state', 'id': window, 'expect_map': 'scale=1,locked=false,center=64,64'}]
    # A normal recipe through the same empty-hotbar SWAP callback.
    ops += [cmd('/clear @s'), cmd('/item replace entity @s hotbar.0 with oak_log', expect_result=1),
            {'op': 'use_block', 'pos': [6, 100, 4]}, click(37), click(1),
            snapshot('planks preview', 4, {0: stack('oak_planks', 4), 1: stack('oak_log')}),
            click(0, 2, 2), snapshot('planks taken', 4, {39: stack('oak_planks', 4)}),
            {'op': 'craft_menu_close'}, cmd('/data get entity @s Inventory'),
            cmd('/clear @s oak_planks 0', expect_result=4)]
    ops += setup('/execute store result score elapsed craft_map run time query gametime',
                 '/scoreboard players operation elapsed craft_map -= start craft_map',
                 '/execute store success score changed craft_map run data modify storage craft_map:owner uuid set from entity @s UUID')
    ops += [cmd('/scoreboard players get elapsed craft_map', expect_result=0),
            cmd('/scoreboard players get changed craft_map', expect_result=0),
            cmd('/execute if entity @s[type=player,gamemode=survival,nbt={Pos:[4.5d,100d,4.5d],Health:20f}]', expect_result=1),
            cmd('/save-all flush')]
    # /save-all is unavailable in the integrated command tree; keep its raw
    # negative diagnostic above, then exercise the actual saved-data flush.
    def saved(scale, center):
        return {'scale': scale, 'locked': False, 'center': center,
                'dimension': 'minecraft:overworld', 'byte_array_length': 16384}
    # Crafting QUICK_MOVE calls onCraftedBy on the transferred stack and
    # the pre-transfer copy: Java saves an additional unused scaled map.
    ops += [{'op': 'map_files', 'ids': [0, 1, 2, 3, 4, 5],
             'expect_observation': {'maps': [saved(0, [0, 0]),
                                    *[saved(1, [64, 64]) for _ in range(4)], None], 'last_id': 4}}]
    return ops
