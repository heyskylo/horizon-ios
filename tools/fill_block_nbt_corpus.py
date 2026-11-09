"""/fill loads the block's NBT into every block entity it places (#12814).

FillCommand.fillBlocks places each block with BlockInput.place, which loads the
tag into the block entity there. A block already in that state still counts
when the tag changed its block entity, and a hollow fill's core is plain air.
"""


def fill_block_nbt(cmd, setup):
    diamonds = '{Items:[{Slot:0b,id:"minecraft:diamond",count:3}]}'
    apple = '{Items:[{Slot:1b,id:"minecraft:apple",count:1}]}'
    ops = setup('/fill 0 239 0 6 239 6 stone', '/fill 0 240 0 6 243 6 air')
    ops += [cmd('/execute if block 0 239 0 stone', expect_result=1),
            cmd(f'/fill 1 240 1 2 240 1 chest[type=single]{diamonds}', expect_result=2),
            cmd('/data get block 1 240 1 Items'),
            cmd('/data get block 2 240 1 Items'),
            # The same chests take a new tag: both count.
            cmd(f'/fill 1 240 1 2 240 1 chest[type=single]{apple}', expect_result=2),
            cmd('/data get block 1 240 1 Items'),
            # The same tag again changes nothing.
            cmd(f'/fill 1 240 1 2 240 1 chest[type=single]{apple}'),
            cmd('/data get block 2 240 1 Items'),
            cmd('/fill 4 240 1 4 240 1 calibrated_sculk_sensor{last_vibration_frequency:11}',
                expect_result=1),
            cmd('/data get block 4 240 1 last_vibration_frequency'),
            # A block without a block entity ignores the tag.
            cmd('/fill 5 240 1 5 240 1 stone{CustomName:"Rock"}', expect_result=1),
            cmd('/fill 1 241 3 3 243 5 barrel{CustomName:"Box"} hollow'),
            cmd('/data get block 1 241 3 CustomName'),
            cmd('/data get block 3 243 5'),
            cmd('/execute if block 2 242 4 air', expect_result=1)]
    ops += setup('/fill 0 240 0 6 243 6 air', '/fill 0 239 0 6 239 6 air')
    return ops
