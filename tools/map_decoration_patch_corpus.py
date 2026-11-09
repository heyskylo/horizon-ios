"""Item-aware empty decoration patches; no backing MapData or namespace dependency."""
from command_exec_corpus import cmd


# Existing item-command bridge diagnostic, retained by the strict full147 mode.
REMOVAL_DIAGNOSTIC_LABELS = {
    'removed-inventory', 'removed-selected', 'removed-decorations', 'removed-removal',
    'removed_keep-inventory', 'removed_keep-selected', 'removed_keep-removal',
}


def map_decoration_patches(full_diagnostic=False):
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
    run('/scoreboard objectives add patch_clock dummy', 1)
    run('/data get entity @s UUID', 4)
    run('/data get entity @s Health', 20)
    run('/data get entity @s Pos')
    run('/data get entity @s playerGameType', 1)
    run('/data get entity @s SelectedItemSlot', 0)
    run('/data modify storage h321:identity uuid set from entity @s UUID', record=False)
    run('/execute store result score start patch_clock run time query gametime', record=False)
    nonempty = 'map_decorations={"a+b":{type:"minecraft:red_x",x:-1.25d,z:2.75d,rotation:-45.5f}}'
    cases = [
        ('default', 'filled_map', ''),
        ('empty', 'filled_map', 'map_decorations={}'),
        ('empty_keep', 'filled_map', 'map_decorations={},custom_data={keep:7}'),
        ('nonempty', 'filled_map', nonempty + ',custom_data={keep:7}'),
        ('removed', 'filled_map', '!minecraft:map_decorations'),
        ('removed_keep', 'filled_map', '!minecraft:map_decorations,custom_data={keep:7}'),
        ('stone_empty', 'stone', 'map_decorations={}'),
        ('blank_map_empty', 'map', 'map_decorations={}'),
        ('stone_default', 'stone', ''),
        ('stone_nonempty', 'stone', nonempty),
    ]
    for name, item, components in cases:
        run('/clear @s')
        run('/execute unless data entity @s Inventory[]', 1)
        suffix = '[' + components + ']' if components else ''
        run(f'/item replace entity @s hotbar.0 with minecraft:{item}{suffix}', 1, name + '-write')
        run(f'/execute if entity @s[nbt={{Inventory:[{{Slot:0b,id:"minecraft:{item}",count:1}}]}}]', 1)
        run('/data get entity @s Inventory', label=name + '-inventory')
        run('/data get entity @s SelectedItem', label=name + '-selected')
        run('/data get entity @s Inventory[{Slot:0b}].components."minecraft:map_decorations"', label=name + '-decorations')
        if name.startswith('removed'):
            run('/data get entity @s Inventory[{Slot:0b}].components."!minecraft:map_decorations"', label=name + '-removal')
        run('/execute unless data entity @s Inventory[{Slot:0b}].components."minecraft:map_id"', 1)
        run('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[0.5d,200d,0.5d]}]', 1)
        run('/execute if loaded 0 200 0', 1)
        run('/execute if block 0 199 0 stone', 1)
        run('/execute store success score changed patch_clock run data modify storage h321:identity uuid set from entity @s UUID', record=False)
        run('/scoreboard players get changed patch_clock', 0)
        run('/execute store result score elapsed patch_clock run time query gametime', record=False)
        run('/scoreboard players operation elapsed patch_clock -= start patch_clock', record=False)
        run('/scoreboard players get elapsed patch_clock', 0)
    if not full_diagnostic:
        assert sum(op.get('label') in REMOVAL_DIAGNOSTIC_LABELS for op in ops) == 7
        ops = [op for op in ops if op.get('label') not in REMOVAL_DIAGNOSTIC_LABELS]
    return ops
