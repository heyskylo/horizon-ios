"""Conduits and enchanting tables placed by /setblock have block entities (#12519).

ConduitBlockEntity saves its Target (an EntityReference), EnchantingTableBlockEntity
its CustomName. Both are read and changed through /data, then read again after
their chunk is saved, unloaded and loaded. The conduit stands in air: it never
becomes active, so serverTick never reaches updateAndAttackTarget and a Target
set by /data stays.
"""

X, Y, Z = 4096, 300, 4096  # far from the player, above any terrain
CHUNK = [[X >> 4, Z >> 4]]
RELOAD_TICKS = 60  # both engines save and drop a chunk once its forced ticket goes


def conduit_table_entities(cmd, setup):
    conduit = f'{X + 2} {Y} {Z + 2}'
    table = f'{X + 4} {Y} {Z + 2}'
    named = f'{X + 6} {Y} {Z + 2}'
    ops = setup(f'/forceload add {X} {Z}')
    ops += [{'op': 'wait_chunks', 'chunks': CHUNK}]
    ops += setup(f'/fill {X} {Y - 1} {Z} {X + 15} {Y - 1} {Z + 15} stone')
    ops += [cmd(f'/execute if block {X + 2} {Y - 1} {Z + 2} stone', expect_result=1),
            cmd(f'/setblock {conduit} conduit', expect_result=1),
            cmd(f'/setblock {table} enchanting_table', expect_result=1),
            cmd(f'/setblock {named} enchanting_table{{CustomName:{{text:"Tome",color:"gold"}}}}',
                expect_result=1)]

    def read():
        out = []
        for pos in (conduit, table, named):
            out += [cmd(f'/data get block {pos}'), cmd(f'/data get block {pos} id')]
        out += [cmd(f'/data get block {conduit} Target'),
                cmd(f'/data get block {table} CustomName'),
                cmd(f'/data get block {named} CustomName')]
        return out

    ops += read()
    ops += [cmd(f'/data merge block {conduit} {{Target:[I;1,2,3,4]}}', expect_result=1),
            cmd(f'/data get block {conduit} Target[3]', expect_result=4),
            cmd(f'/data merge block {table} {{CustomName:"Book"}}', expect_result=1),
            cmd(f'/data remove block {named} CustomName', expect_result=1)]
    ops += read()
    ops += setup(f'/forceload remove {X} {Z}')
    ops += [{'op': 'tick', 'n': RELOAD_TICKS},
            cmd(f'/execute unless loaded {conduit}', expect_result=1)]
    ops += setup(f'/forceload add {X} {Z}')
    ops += [{'op': 'wait_chunks', 'chunks': CHUNK}]
    ops += read()
    # Using the table still opens its enchanting menu: an item and a lapis slot.
    ops += setup('/gamemode survival', '/clear @s', f'/tp @s {X + 4}.5 {Y} {Z + 4}.5 0 0')
    ops += [{'op': 'tick', 'n': 1},
            cmd(f'/execute if entity @s[x={X + 4}.5,y={Y},z={Z + 4}.5,distance=..0.01]', expect_result=1),
            {'op': 'menu_state', 'label': 'before-table-use', 'expect_observation': {'own_slots': 0}},
            {'op': 'use_block', 'pos': [X + 4, Y, Z + 2], 'face': 1, 'sequence': 12519},
            {'op': 'menu_state', 'label': 'after-table-use', 'expect_observation': {'own_slots': 2}},
            {'op': 'close_menu'}]
    ops += setup('/gamemode creative', '/tp @s 8.5 230 8.5 0 0',
                 f'/fill {X} {Y - 1} {Z} {X + 15} {Y + 1} {Z + 15} air',
                 f'/forceload remove {X} {Z}')
    return ops
