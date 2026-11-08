"""Entity.load and Entity.saveWithoutId on boats, minecarts and cushions (#12523)."""

OPTIONAL_FLAGS = ('NoGravity', 'Silent', 'Glowing', 'HasVisualFire', 'CustomNameVisible')
COMMON_FLAGS = ('Invulnerable', 'OnGround', *OPTIONAL_FLAGS)


def vehicle_boolean_flags(cmd):
    # A remote forced chunk can unload independently of the connected owner.
    ops = [cmd('/forceload add 4096 0', expect_result=1),
           {'op': 'wait_chunks', 'chunks': [[256, 0]]},
           cmd('/fill 4096 299 0 4111 299 15 stone', expect_result=256)]

    def read_flags(target, label, enabled, *, on_ground=None):
        saved = [f'Invulnerable:{int(enabled)}b']
        if on_ground is not None:
            saved.append(f'OnGround:{int(on_ground)}b')
        if enabled:
            saved.extend(f'{field}:1b' for field in OPTIONAL_FLAGS)
        ops.extend([cmd(f'/data get entity {target} Invulnerable', expect_result=int(enabled)),
                    cmd(f'/execute if entity @e[tag={label},nbt={{{",".join(saved)}}}]',
                        expect_result=1)])
        if enabled:
            ops.append(cmd(f'/data get entity {target} NoGravity', expect_result=1))
        else:
            absent = ' '.join(f'unless data entity {target} {field}' for field in OPTIONAL_FLAGS)
            ops.append(cmd(f'/execute {absent}', expect_result=1))

    actors = []
    case_id = 0x12523
    # The last case stays in the world for live edits and a real chunk reload.
    cases = [(None, False), ('0b', False), ('-1b', True), ('127b', True),
             ('256s', False), ('-0.5f', True), ('"invalid"', False), ('2b', True)]
    for index, kind in enumerate(('minecart', 'hopper_minecart', 'spruce_boat', 'cushion'), 1):
        x = 4096 + index * 3
        y = 300 if kind == 'cushion' else 302
        pos = f'{x}.5 {y} 4.5'
        saved_pos = f'[{x}.5d,{y}d,4.5d]'
        for serial, (value, enabled) in enumerate(cases, 1):
            label = f'h12523_{index}_{serial}'
            target = f'{case_id:08x}-0000-0000-{index:04x}-{serial:012x}'
            fields = [f'UUID:[I;{case_id},0,{index << 16},{serial}]', f'Tags:["{label}"]']
            if kind == 'cushion':
                fields.append(f'block_pos:[I;{x},{y},4]')
            if value is not None:
                fields.extend(f'{flag}:{value}' for flag in COMMON_FLAGS)
            ops.append(cmd(f'/summon minecraft:{kind} {pos} {{{",".join(fields)}}}',
                           expect_result=1))
            read_flags(target, label, enabled, on_ground=enabled)
            if serial == len(cases):
                actors.append((kind, target, label, saved_pos))
            else:
                ops.append(cmd(f'/kill {target}', expect_result=1))

    vehicles = [(target, pos) for kind, target, _, pos in actors if kind != 'cushion']
    ops.append({'op': 'tick', 'n': 2})
    for target, pos in vehicles:
        ops.extend([cmd(f'/execute if data entity {target} {{Pos:{pos}}}', expect_result=1),
                    cmd(f'/data merge entity {target} {{NoGravity:0b}}', expect_result=1),
                    cmd(f'/execute unless data entity {target} NoGravity', expect_result=1)])
    ops.append({'op': 'tick', 'n': 1})
    for target, pos in vehicles:
        ops.extend([cmd(f'/execute unless data entity {target} {{Pos:{pos}}}', expect_result=1),
                    cmd(f'/data merge entity {target} {{Pos:{pos},Motion:[0d,0d,0d],NoGravity:-1b}}',
                        expect_result=1),
                    cmd(f'/data get entity {target} NoGravity', expect_result=1)])

    ops.extend([cmd('/forceload remove 4096 0', expect_result=1),
                {'op': 'tick', 'n': 60},
                cmd('/execute unless loaded 4096 300 0', expect_result=1),
                cmd('/forceload add 4096 0', expect_result=1),
                {'op': 'wait_chunks', 'chunks': [[256, 0]]}])
    for kind, target, label, _ in actors:
        read_flags(target, label, True, on_ground=True if kind == 'cushion' else None)
        ops.append(cmd(f'/kill {target}', expect_result=1))
    ops.append(cmd('/forceload remove 4096 0', expect_result=1))
    return ops
