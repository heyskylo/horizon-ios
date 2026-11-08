"""An inline `/loot` table's `minecraft:loot_table` entry is checked against the loot table registry.

`ResourceOrIdArgument.lootTable` decodes an inline table with `LootTable.DIRECT_CODEC` while the command
is parsed. A `minecraft:loot_table` entry (`NestedLootTable`) holds a `HolderSet<LootTable>`: a `#tag`,
a list, or one holder, an id the registry must hold or an inline table. A missing id fails the decode,
so the command is refused before it runs: no callback, no store, no item.

The world pack `command-selector-fixture` adds `il_one` (one diamond, no random function) and `il_empty`
(no pools); `il_none` never exists. The pack's function `il_bad` names the missing table inline, and
`il_good` the present one. The world is frozen. Each `/loot` runs bare, then under `execute store result`
and `execute store success` with the scores set to 31 first (fresh sentinels), and the player's whole
inventory is read after it.

Label classes, declared before any native run: `target-` rows are an inline reference to the missing
table (at the top, in an `alternatives` child, in a nested inline table, and in a pack function);
`one-` and `empty-` rows are valid inline references to the present tables; `named-` rows name the
present table; `diag-` rows are recorded diagnostics; every other row is a setup admission or a control.
"""

ONE = {'pools': [{'rolls': 1, 'entries': [{'type': 'minecraft:item', 'name': 'minecraft:diamond'}]}]}
EMPTY = {'pools': []}
TABLES = {'parity:il_one': ONE, 'parity:il_empty': EMPTY}


def ref(value):
    return '{pools:[{rolls:1,entries:[{type:"minecraft:loot_table",value:' + value + '}]}]}'


MISSING = ref('"parity:il_none"')
FUNCTIONS = {'parity:il_bad': f'loot give @s loot {MISSING}\n',
             'parity:il_good': f'loot give @s loot {ref(chr(34) + "parity:il_one" + chr(34))}\n'}
CASES = [
    ('one', ref('"parity:il_one"')),
    ('empty', ref('"parity:il_empty"')),
    ('target-top', MISSING),
    ('target-child', '{pools:[{rolls:1,entries:[{type:"minecraft:alternatives",children:[{type:"minecraft:loot_table",'
                     'value:"parity:il_none"}]}]}]}'),
    ('target-nested', ref('{pools:[{rolls:1,entries:[{type:"minecraft:loot_table",value:"parity:il_none"}]}]}')),
    ('named', 'parity:il_one'),
]


def inline_loot_reference(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule advance_time false', 'rule-time', admission=False)
    cmd('/datapack list enabled', 'pack-enabled', 2)
    cmd('/forceload add 0 0 0 0', 'force', admission=False)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/execute if loaded 2 220 2', 'loaded', 1)
    cmd('/gamemode survival', 'survival', admission=False)
    cmd('/clear @s', 'clear', admission=False)
    cmd('/data get entity @s Inventory', 'inventory-empty', admission=False)
    cmd('/scoreboard objectives add il dummy', 'objective', 1)
    cmd('/execute store result score start il run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)
    for name, table in CASES:
        text = f'loot give @s loot {table}'
        cmd('/' + text, f'{name}-give', admission=False)
        for kind in ('result', 'success'):
            cmd(f'/scoreboard players set #{kind} il 31', f'{name}-{kind}-fresh', 31)
            cmd(f'/execute store {kind} score #{kind} il run {text}', f'{name}-{kind}-run', admission=False)
            cmd(f'/scoreboard players get #{kind} il', f'{name}-{kind}', admission=False)
        cmd('/data get entity @s Inventory', f'{name}-inventory', admission=False)
        cmd('/clear @s', f'{name}-clear', admission=False)
    # A pack function that names the missing table inline is refused when the pack loads.
    cmd('/function parity:il_bad', 'target-function', admission=False)
    cmd('/data get entity @s Inventory', 'target-function-inventory', admission=False)
    cmd('/function parity:il_good', 'one-function', admission=False)
    cmd('/data get entity @s Inventory', 'one-function-inventory', admission=False)
    cmd('/clear @s', 'one-function-clear', admission=False)
    cmd('/execute store result score now il run time query gametime', 'diag-clock-now',
        admission=False, diagnostic=True)
    cmd('/scoreboard players operation now il -= start il', 'clock', 0)
    return ops
