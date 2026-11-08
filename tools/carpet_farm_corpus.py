"""Carpet farms exercised through the real server command/tick path."""


def carpet_farms(cmd, setup):
    """#9389: count carpets in stacks, so item merging cannot hide production."""
    ops = setup('/scoreboard objectives add carpet_farm_probe dummy')
    items = '@e[type=item,nbt={Item:{id:"minecraft:red_carpet"}}]'

    def count():
        return [*setup('/scoreboard players set total carpet_farm_probe 0',
            f'/execute as {items} store result score @s carpet_farm_probe run data get entity @s Item.count',
            f'/execute as {items} run scoreboard players operation total carpet_farm_probe += @s carpet_farm_probe'),
            cmd('/scoreboard players get total carpet_farm_probe')]

    def machine(coral):
        return setup('/kill @e[type=!player]', '/fill 0 200 0 8 215 8 air',
            '/fill 0 200 0 8 200 8 stone', '/setblock 3 201 2 sticky_piston[facing=west]',
            '/fill 2 201 2 2 205 2 slime_block',
            f'/setblock 1 201 2 {coral}[facing=west,waterlogged=false]',
            f'/setblock 1 203 2 {coral}[facing=west,waterlogged=false]',
            '/setblock 1 202 2 red_carpet', '/setblock 1 204 2 red_carpet')

    for coral in ('dead_fire_coral_wall_fan', 'fire_coral_wall_fan'):
        for pulse in range(1, 5):
            ops += machine(coral)
            for _ in range(3):
                ops += setup('/setblock 4 201 2 redstone_block')
                ops.append({'op': 'tick', 'n': pulse})
                ops += setup('/setblock 4 201 2 air')
                ops.append({'op': 'tick', 'n': pulse})
                for y in (202, 204):
                    ops.append(cmd(f'/execute if block 1 {y} 2 red_carpet'))
                ops += count()
        # Facing observers keep the piston clocked without commands supplying
        # subsequent pulses. Sample for long enough to exercise repeated moves.
        ops += machine(coral)
        ops += setup('/setblock 4 201 2 observer[facing=east]',
                     '/setblock 5 201 2 observer[facing=west]')
        for _ in range(12):
            ops.append({'op': 'tick', 'n': 4})
            ops += count()
    ops += setup('/kill @e[type=!player]', '/fill 0 200 0 8 215 8 air',
                 '/scoreboard objectives remove carpet_farm_probe')
    return ops
