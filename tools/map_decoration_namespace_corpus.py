"""Map decoration resource IDs through the live item component codec, without MapData."""
from command_exec_corpus import cmd


def map_decoration_namespaces(empty_diagnostic=False):
    ops = []

    def run(command, expected=None, label=None, record=True):
        op = cmd(command, expect_result=expected, record=record)
        if label:
            op['label'] = label
        ops.append(op)

    run('/tick freeze', 1)
    for command in ['/gamerule spawn_mobs false', '/gamerule advance_time false',
                    '/gamemode creative', '/forceload add 0 0 15 15']:
        run(command)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    run('/execute if loaded 0 200 0', 1)
    run('/fill 0 199 0 15 199 15 stone', 256)
    run('/fill 0 200 0 15 204 15 air')
    run('/tp @s 0.5 200 0.5 0 0')
    run('/scoreboard objectives add deco_clock dummy', 1)
    run('/data get entity @s UUID', 4)
    run('/data get entity @s Pos')
    run('/data get entity @s Health', 20)
    run('/data get entity @s SelectedItemSlot', 0)
    run('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[0.5d,200d,0.5d]}]', 1)
    run('/data modify storage h318:identity uuid set from entity @s UUID', record=False)
    run('/execute store result score start deco_clock run time query gametime', record=False)
    cases = [
        ('two_entries', 'filled_map',
         'map_decorations={a:{type:"minecraft:mansion",x:10.5d,z:-20.0d,rotation:90.0f},b:{type:"red_x",x:1.0d,z:2.0d,rotation:0.0f}}'),
        ('bare', 'filled_map',
         'map_decorations={bare:{type:"mansion",x:-1.25d,z:2.75d,rotation:-45.5f}}'),
        ('qualified', 'filled_map',
         'map_decorations={qualified:{type:"minecraft:red_x",x:-0.5d,z:-2.25d,rotation:17.25f}}'),
        ('plus_key', 'filled_map',
         'map_decorations={"a+b":{type:"red_x",x:-10.5d,z:20.25d,rotation:180.5f}},custom_data={audit:"keep",counter:7}'),
        ('empty', 'filled_map', 'map_decorations={}'),
        ('plain', 'filled_map', ''),
        ('ordinary', 'stone', 'custom_data={audit:"plain",counter:3}'),
    ]
    for name, item, components in cases:
        run('/clear @s')
        run('/execute unless data entity @s Inventory[]', 1)
        suffix = '[' + components + ']' if components else ''
        run(f'/item replace entity @s hotbar.0 with minecraft:{item}{suffix}', 1, name+'-replace')
        run(f'/execute if entity @s[nbt={{Inventory:[{{Slot:0b,id:"minecraft:{item}",count:1}}]}}]', 1)
        run('/data get entity @s Inventory', label=name+'-inventory')
        run('/data get entity @s SelectedItem', label=name+'-selected')
        run('/data get entity @s Inventory[{Slot:0b}].components."minecraft:map_decorations"', label=name+'-decorations')
        run('/execute unless data entity @s Inventory[{Slot:0b}].components."minecraft:map_id"', 1)
        run('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[0.5d,200d,0.5d]}]', 1)
        run('/execute if loaded 0 200 0', 1)
        run('/execute if block 0 199 0 stone', 1)
        run('/execute store success score changed deco_clock run data modify storage h318:identity uuid set from entity @s UUID', record=False)
        run('/scoreboard players get changed deco_clock', 0)
        run('/execute store result score elapsed deco_clock run time query gametime', record=False)
        run('/scoreboard players operation elapsed deco_clock -= start deco_clock', record=False)
        run('/scoreboard players get elapsed deco_clock', 0)
    if not empty_diagnostic:
        # Separate default-elision lead: retain its write ACK and every guard.
        excluded = {'empty-inventory', 'empty-selected', 'empty-decorations'}
        assert sum(op.get('label') in excluded for op in ops) == 3
        ops = [op for op in ops if op.get('label') not in excluded]
    return ops
