#!/usr/bin/env python3
"""Sculk placement tags survive ticking and chunk reload in both games."""
from command_exec_corpus import cmd, group_setup, setup
import harness_args
import test_command_exec_parity as driver


def blocks(x, y, z):
    return [
        (f'{x + 3} {y} {z}', 'sculk_sensor', 'last_vibration_frequency', 5),
        (f'{x + 7} {y} {z}', 'calibrated_sculk_sensor', 'last_vibration_frequency', 11),
        (f'{x + 3} {y} {z + 4}', 'sculk_shrieker', 'warning_level', 2),
    ]


def place(sculk):
    return [cmd(f'/setblock {pos} {block}{{{field}:{value}}}', expect_result=1)
            for pos, block, field, value in sculk]


def read(sculk):
    return [cmd(f'/data get block {pos} {field}', expect_result=value)
            for pos, _, field, value in sculk]


def item_placement():
    """BlockItem loads saved data after the block's placement initialization."""
    pos = '2 230 11'
    operations = setup('/gamemode creative', '/tp @s 4.5 230 11.5',
                       '/setblock 2 229 11 stone')
    cases = [
        ('sculk_sensor', 'last_vibration_frequency', 6),
        ('calibrated_sculk_sensor', 'last_vibration_frequency', 11),
        ('sculk_shrieker', 'warning_level', 2),
        ('sculk_sensor', 'last_vibration_frequency', 0),
    ]
    for block, field, value in cases:
        item = block
        if value:
            item += f'[block_entity_data={{id:"minecraft:{block}",{field}:{value}}}]'
        operations += setup(f'/setblock {pos} air',
                            f'/item replace entity @s weapon.mainhand with {item}')
        operations.append({'op': 'use_block', 'pos': [2, 229, 11], 'face': 1})
        operations.append(cmd(f'/execute if block {pos} {block}', expect_result=1))
        operations.append(cmd(f'/data get block {pos} {field}', expect_result=value))
        operations.append({'op': 'tick', 'n': 1})
        operations.append(cmd(f'/data get block {pos} {field}', expect_result=value))
    operations += setup(f'/setblock {pos} air',
                        '/item replace entity @s weapon.mainhand with air')
    return operations


def groups():
    sculk = blocks(4, 230, 4)
    ticking = [
        *setup('/gamerule random_tick_speed 0', '/gamemode survival @s',
               '/fill 0 229 0 15 229 15 stone', '/tp @s 4.5 230 4.5'),
        cmd('/execute if block 4 229 4 stone', expect_result=1),
        *place(sculk),
        *read(sculk),
        {'op': 'tick', 'n': 1},
        *read(sculk),
        # Both sensors hear player damage. Their player-attributed tendril
        # clicks then trigger a shriek.
        cmd('/damage @s 2 minecraft:cactus', expect_result=1),
        {'op': 'tick', 'n': 20},
        cmd('/data get block 7 230 4 last_vibration_frequency', expect_result=7),
        cmd('/data get block 11 230 4 last_vibration_frequency', expect_result=7),
        cmd('/execute if block 7 230 8 sculk_shrieker[shrieking=true]', expect_result=1),
        cmd('/data get block 7 230 8 warning_level', expect_result=0),
    ]

    pos = '14 230 14'
    frequency = f'/data get block {pos} last_vibration_frequency'
    replacements = [
        cmd(f'/setblock {pos} sculk_sensor{{last_vibration_frequency:10}}', expect_result=1),
        cmd(f'/setblock {pos} air', expect_result=1),
        cmd(f'/setblock {pos} calibrated_sculk_sensor{{last_vibration_frequency:9}}',
            expect_result=1),
        cmd(f'/setblock {pos} sculk_sensor{{last_vibration_frequency:13}}', expect_result=1),
        cmd(frequency, expect_result=13),
        {'op': 'tick', 'n': 1},
        cmd(frequency, expect_result=13),
        cmd(f'/setblock {pos} sculk_sensor{{last_vibration_frequency:3}}', expect_result=1),
        {'op': 'tick', 'n': 1},
        cmd(frequency, expect_result=3),
        cmd(f'/setblock {pos} air', expect_result=1),
        cmd(f'/setblock {pos} sculk_sensor', expect_result=1),
        {'op': 'tick', 'n': 1},
        cmd(frequency, expect_result=0),
    ]

    distant = blocks(4096, 300, 4096)
    reload_chunk = [
        *setup('/forceload add 4096 4096'),
        {'op': 'wait_chunks', 'chunks': [[256, 256]]},
        *place(distant),
        {'op': 'tick', 'n': 1},
        *read(distant),
        *setup('/forceload remove 4096 4096'),
        {'op': 'tick', 'n': 60},
        cmd('/execute unless loaded 4096 300 4096', expect_result=1),
        *setup('/forceload add 4096 4096'),
        {'op': 'wait_chunks', 'chunks': [[256, 256]]},
        {'op': 'tick', 'n': 1},
        *read(distant),
    ]
    return [('setup', group_setup()), ('sculk ticking', ticking),
            ('sculk replacements', replacements), ('sculk item placement', item_placement()),
            ('sculk chunk reload', reload_chunk)]


if __name__ == '__main__':
    harness_args.ensure_work_dir(driver.lab.LOCAL / 'sculk-saved-fields-parity')
    driver.corpus_module.groups = groups
    driver.main()
