#!/usr/bin/env python3
"""Saved hive block-state prototype elision through the existing actual Game route.

Every saved stack/component/presence/callback/store record is strict. Only the
complete parent block tag and absolute startup clocks are executable diagnostics.
No bees, loot, placement, simulation or general component-default claim.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver

CASES = [
    ('hive_zero', 'beehive', '{honey_level:"0"}', 'missing', False),
    ('nest_zero', 'bee_nest', '{honey_level:"0"}', 'missing', False),
    ('hive_plain', 'beehive', None, 'missing', False),
    ('nest_plain', 'bee_nest', None, 'missing', False),
    ('hive_one', 'beehive', '{honey_level:"1"}', 'one', False),
    ('nest_one', 'bee_nest', '{honey_level:"1"}', 'one', False),
    ('stone_zero', 'stone', '{honey_level:"0"}', 'zero', False),
    ('hive_empty', 'beehive', '{}', 'empty', False),
    ('nest_empty', 'bee_nest', '{}', 'empty', False),
    ('hive_keep', 'beehive', '{honey_level:"0"}', 'missing', True),
    ('nest_keep', 'bee_nest', '{honey_level:"0"}', 'missing', True),
    ('hive_extra', 'beehive', '{honey_level:"0",facing:"south"}', 'extra', False),
    ('nest_extra', 'bee_nest', '{honey_level:"0",facing:"south"}', 'extra', False),
]


def program(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def read(path, label, expected=None, admission=False):
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h034 -99', label + '-reset-' + holder, -99)
        cmd(f'/execute store result score result h034 store success score success h034 '
            f'run data get block 4 200 4 {path}', label + '-read', expected, admission)
        cmd('/scoreboard players get result h034', label + '-result', expected, admission)
        cmd('/scoreboard players get success h034', label + '-success', admission=admission)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]', 'chest-place', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/execute if loaded 4 200 4', 'chest-loaded', 1)
    cmd('/execute if block 4 199 4 stone', 'chest-support', 1)
    cmd('/execute if block 4 200 4 chest[facing=north,type=single,waterlogged=false]', 'chest-state', 1)
    cmd('/tp @s 8.5 200 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    for key, expected in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd('/data get entity @s ' + key, 'owner-' + key, expected)
    for key, expected in [('id', 15), ('x', 4), ('y', 200), ('z', 4), ('Items', 0)]:
        cmd('/data get block 4 200 4 ' + key, 'initial-' + key, expected)
    cmd('/scoreboard objectives add h034 dummy', 'objective')
    cmd('/execute store result score start h034 run time query gametime', 'clock-start', diagnostic=True)
    for slot, (case, item, source, mode, keep) in enumerate(CASES):
        path = f'Items[{{Slot:{slot}b}}]'
        cmd(f'/execute unless data block 4 200 4 {path}', case + '-initial-slot-absent', 1)
        parts = [] if source is None else ['minecraft:block_state=' + source]
        if keep:
            parts.append('minecraft:custom_data={keep:34}')
        spec = 'minecraft:' + item + ('[' + ','.join(parts) + ']' if parts else '')
        cmd(f'/item replace block 4 200 4 container.{slot} with {spec}', case + '-insert', 1)
        for key, expected in [('id', len('minecraft:' + item)), ('count', 1), ('Slot', slot)]:
            cmd(f'/data get block 4 200 4 {path}.{key}', case + '-' + key, expected)
        present = mode != 'missing'
        comp = path + '.components'
        state = comp + '."minecraft:block_state"'
        read(path, case + '-stack', 4 if present or keep else 3)
        read(comp, case + '-components', 1 if present or keep else None)
        read(state, case + '-block-state', {'empty': 0, 'extra': 2}.get(mode, 1) if present else None)
        read(state + '.honey_level', case + '-honey', 1 if mode in ['one', 'zero', 'extra'] else None)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h034 -99', case + '-presence-reset-' + holder, -99)
        cmd(f'/execute store result score result h034 store success score success h034 '
            f'run execute if data block 4 200 4 {state}', case + '-presence-read', admission=False)
        cmd('/scoreboard players get result h034', case + '-presence-result', admission=False)
        cmd('/scoreboard players get success h034', case + '-presence-success', admission=False)
        if mode == 'extra':
            read(state + '.facing', case + '-facing', 5)
        if keep:
            read(comp + '."minecraft:custom_data".keep', case + '-custom', 34)
    cmd('/data get block 4 200 4 Items', 'final-all-stacks', 13, admission=False)
    cmd('/data get block 4 200 4', 'final-parent-block', diagnostic=True)
    cmd('/execute store result score end h034 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h034 -= start h034', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h034', 'clock-delta', 0)
    return ops


def main():
    full = '--full-diagnostic' in sys.argv
    if full:
        sys.argv.remove('--full-diagnostic')
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference')
        reference = Path(sys.argv[i + 1]).resolve()
        del sys.argv[i:i + 2]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'hive-default-patch-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    reference_bytes = reference.read_bytes() if reference else None
    assert reference is None or reference != build / 'vanilla.jsonl', 'reference must stay separate and immutable'
    ops = program(full)
    driver.corpus_module.groups = lambda: [('saved hive block-state default', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    assert len(recorded) == len(java)
    by_label = {op['label']: row for op, row in zip(recorded, java)}
    # Missing-field and empty-compound callbacks are independently measured on
    # fresh Java, including the actual post-command sentinels (never inferred).
    for case, _, _, mode, _ in CASES:
        expected = [[True, 1]] if mode != 'missing' else [[False, 0]]
        assert by_label[case + '-presence-read']['results'] == expected
        if mode in ['empty', 'missing']:
            expected = [[True, 0]] if mode == 'empty' else [[False, 0]]
            assert by_label[case + '-block-state-read']['results'] == expected
    if '--reference-only' in sys.argv:
        return
    comparison_bytes = reference_bytes or (build / 'vanilla.jsonl').read_bytes()
    if reference_bytes and not full:
        assert reference_bytes == (build / 'vanilla.jsonl').read_bytes(), 'fresh reference bytes changed'
    # Full diagnostics retain each fresh run's unequal absolute startup clocks.
    # Compare both binaries to the same immutable external reference, without
    # replacing captured records or weakening any whole-record comparison.
    (build / 'comparison-reference.jsonl').write_bytes(comparison_bytes)
    java = [json.loads(s) for s in comparison_bytes.decode().splitlines()]
    assert len(java) == len(recorded)
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
    assert len(recorded) == len(native)
    differences = [{'label': op['label'], 'expected': a, 'actual': b}
                   for op, a, b in zip(recorded, java, native) if a != b]
    report = {'records': len(native), 'passed': len(native) - len(differences), 'differences': differences,
              'comparison': 'complete raw records, no masks/tolerances',
              'reference_sha256': hashlib.sha256(comparison_bytes).hexdigest()}
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
    if not full:
        for op, a, b in zip(recorded, java, native):
            if op['admission']:
                assert a == b, ('fixture admission', op, a, b)
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
