"""Five loaded furnace totals, observed at cumulative 0/1/4/5/6 real ticks."""
CASES = [('missing', None, 0), ('zero', 0, 0), ('five', 5, 0),
         ('normal', 200, 0), ('started', 5, 1)]


def loaded_totals():
    ops = []
    def cmd(text, label, result=None):
        row = {'op': 'cmd', 'cmd': text, 'label': label}
        if result is not None:
            row['expect_result'] = result
        ops.append(row)
    cmd('/tick freeze', 'freeze')
    for rule, value in [('spawn_mobs', 'false'), ('advance_time', 'false'), ('random_tick_speed', '0')]:
        cmd(f'/gamerule {rule} {value}', 'rule-'+rule)
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 99 0 15 99 15 stone', 'floor', 256)
    cmd('/fill 0 100 0 15 105 15 air', 'air')
    cmd('/scoreboard objectives add h119 dummy', 'objective')
    cmd('/execute store result score start h119 run time query gametime', 'clock-start')
    for i, (name, total, spent) in enumerate(CASES):
        x = 2 + i * 2
        for y, block in [(99, 'stone'), (100, 'air'), (101, 'air')]:
            cmd(f'/execute if block {x} {y} 2 {block}', name+f'-setup-{y}', 1)
        extra = '' if total is None else f',cooking_total_time:{total}'
        tag = ('{lit_time_remaining:20,lit_total_time:20,'
               f'cooking_time_spent:{spent}{extra},'
               'Items:[{Slot:0b,id:"minecraft:raw_iron",count:3}]}')
        cmd(f'/setblock {x} 100 2 minecraft:furnace[lit=true]{tag}', name+'-load', 1)
    previous = 0
    for elapsed in [0, 1, 4, 5, 6]:
        if elapsed:
            cmd(f'/tick step {elapsed-previous}', f'{elapsed}-step', 1)
            ops.append({'op': 'tick', 'n': elapsed-previous})
        previous = elapsed
        cmd('/execute store result score now h119 run time query gametime', f'{elapsed}-clock-raw')
        cmd('/scoreboard players operation now h119 -= start h119', f'{elapsed}-clock-delta-op', elapsed)
        cmd('/scoreboard players get now h119', f'{elapsed}-clock-delta', elapsed)
        for i, (name, supplied, initial) in enumerate(CASES):
            x = 2 + i * 2
            at = f'{x} 100 2'
            key = f'{name}-{elapsed}'
            # These are hypotheses admitted independently by fresh Java before
            # either native capture; they are never used to update a furnace.
            first = 1 if supplied in (None, 0) else supplied-initial
            completed = elapsed >= first
            spent = elapsed-first if completed else elapsed+initial
            total = 200 if completed else (supplied or 0)
            cmd(f'/execute if loaded {at}', key+'-loaded', 1)
            cmd(f'/execute if block {x} 99 2 stone', key+'-floor', 1)
            cmd(f'/execute if block {at} minecraft:furnace[lit=true]', key+'-lit', 1)
            cmd(f'/execute if data block {at} {{id:"minecraft:furnace",x:{x},y:100,z:2}}', key+'-identity', 1)
            cmd(f'/data get block {at}', key+'-full-tag')
            cmd(f'/data get block {at} Items', key+'-all-slots')
            for field, expected in [('cooking_time_spent', spent), ('cooking_total_time', total),
                                    ('lit_time_remaining', 20-elapsed), ('lit_total_time', 20)]:
                cmd(f'/data get block {at} {field}', key+'-'+field, expected)
            cmd(f'/execute if data block {at} {{Items:[{{Slot:0b,id:"minecraft:raw_iron",count:{2 if completed else 3}}}]}}', key+'-input', 1)
            cmd(f'/execute unless data block {at} Items[{{Slot:1b}}]', key+'-no-fuel', 1)
            if completed:
                cmd(f'/execute if data block {at} {{Items:[{{Slot:2b,id:"minecraft:iron_ingot",count:1}}]}}', key+'-output', 1)
            else:
                cmd(f'/execute unless data block {at} Items[{{Slot:2b}}]', key+'-no-output', 1)
    return ops

# Authorized before canonical paired captures. Full373 remains unchanged.
DIAGNOSTIC_LABELS = frozenset(
    [f'{name}-{tick}-full-tag' for name, _, _ in CASES for tick in [0, 1, 4, 5, 6]]
    + ['clock-start'] + [f'{tick}-clock-raw' for tick in [0, 1, 4, 5, 6]])


def selected_totals(full=False):
    ops = loaded_totals()
    if not full:
        for op in ops:
            if op.get('label') in DIAGNOSTIC_LABELS:
                # Commands still execute, with actual trace bodies retained.
                # Full diagnostic compares their complete records separately.
                op.update(record=False, trace=True)
    return ops


def recipes_used_totals():
    """Separate25 reads; all original actions/guards still run and are traced."""
    out = []
    for op in loaded_totals():
        if op['op'] == 'cmd':
            op.update(record=False, trace=True)
        out.append(op)
        label = op.get('label', '')
        if label.endswith('-all-slots'):
            name, elapsed, *_ = label.split('-')
            elapsed = int(elapsed)
            i, (_, total, spent) = next((i, c) for i, c in enumerate(CASES) if c[0]==name)
            first = 1 if total in (None, 0) else total-spent
            value = '{"minecraft:iron_ingot_from_smelting_raw_iron": 1}' if elapsed >= first else '{}'
            x = 2+i*2
            out.append({'op': 'cmd', 'cmd': f'/data get block {x} 100 2 RecipesUsed',
                        'label': f'{name}-{elapsed}-recipes-used', 'trace': True,
                        'expect_result': int(elapsed >= first), 'expected_text': f'{x}, 100, 2 has the following block data: {value}'})
    return out
