#!/usr/bin/env python3
"""Block loot's `match_tool` with `minecraft:custom_data` against vanilla 26.3 (#12196).

`CustomDataPredicate` -> `NbtPredicate.matches` -> `CustomData.matchedBy`:
`NbtUtils.compareNbt(pattern, data, true)`, with a stack's absent custom data
read as `{}`. A data pack gives `minecraft:blocks/dirt` one pool per pattern,
each behind `match_tool`; `loot give @s mine` runs it as the player with sticks
carrying different custom data, and `clear` counts which pools dropped. Every
record is compared whole, with no masks. (The loot engine's `filtered` and
`equipment` paths share the predicate; Horizon's commands can't reach them
with a data pack's table, so horizon-world's `custom_data_predicate_tests`
holds the same matrix from this oracle.)
"""
import hashlib
import json
import sys
import tempfile
from pathlib import Path

try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver

# Patterns as data packs write them: SNBT strings, plus a JSON object.
PATTERNS = [
    ('byte', '{h:1b}'),
    ('typed_int', '{h:1}'),
    ('nested', '{nested:{k:7}}'),
    ('list_subset', '{entries:[{k:2b}]}'),
    ('list_order', '{entries:[{k:2b},{k:1b}]}'),
    ('empty_list', '{entries:[]}'),
    ('long', '{wide:9007199254740993L}'),
    ('wrong_long', '{wide:9007199254740992L}'),
    ('empty', '{}'),
    ('object', {'h': 1}),
]
COLOURS = ['white', 'orange', 'magenta', 'light_blue', 'yellow', 'lime', 'pink', 'gray',
           'light_gray', 'cyan']
REWARDS = {name: f'minecraft:{colour}_wool' for (name, _), colour in zip(PATTERNS, COLOURS)}
RICH = '{h:1b,nested:{k:7,extra:9},entries:[{k:1b,extra:2b},{k:2b}],wide:9007199254740993L,spare:"keep"}'
TOOLS = [
    ('rich', RICH),
    ('wrong', '{h:2b}'),
    ('int', '{h:1}'),
    ('empty_list', '{entries:[]}'),
    ('plain', None),
]
DIRT = '8 200 4'


def predicate(pattern):
    return {'items': 'minecraft:stick', 'predicates': {'minecraft:custom_data': pattern}}


def loot_tables():
    # 26.3's loot format: `condition` and `modifier`, each keyed by `type`.
    pools = [{'rolls': 1, 'entries': [{'type': 'minecraft:item', 'name': REWARDS[name]}],
              'condition': {'type': 'minecraft:match_tool', 'predicate': predicate(pattern)}}
             for name, pattern in PATTERNS]
    tables = {'minecraft:blocks/dirt': {'type': 'minecraft:block', 'pools': pools,
                                        'random_sequence': 'minecraft:blocks/dirt'}}
    return tables


def program():
    ops = []

    def cmd(text, label, result=None, admission=True):
        op = dict(op='cmd', cmd=text, label=label, admission=admission)
        if result is not None:
            op['expect_result'] = result
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawns')
    cmd('/gamerule random_tick_speed 0', 'no-random-ticks')
    cmd('/forceload add 0 0', 'forceload', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 14.5 200 14.5 0 0', 'owner-position', 1)
    cmd('/clear @s', 'empty-inventory', admission=False)
    cmd(f'/setblock {DIRT} dirt', 'dirt', 1)
    cmd(f'/execute if block {DIRT} dirt', 'dirt-placed', 1)
    for tool, data in TOOLS:
        stack = 'minecraft:stick' + ('' if data is None else f'[minecraft:custom_data={data}]')
        cmd(f'/loot give @s mine {DIRT} {stack}', f'mine-{tool}', admission=False)
        for name, _ in PATTERNS:
            cmd(f'/clear @s {REWARDS[name]}', f'mine-{tool}-{name}', admission=False)
    cmd(f'/execute if block {DIRT} dirt', 'dirt-unchanged', 1)
    cmd('/forceload remove 0 0', 'cleanup-force', 1)
    return ops


def main():
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='loot-custom-data-', dir=driver.lab.LOCAL))
        sys.argv += ['--work-dir', str(parent / 'run')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    ops = program()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    tables = loot_tables()
    driver.corpus_module.groups = lambda: [('Loot custom_data item predicates', ops)]
    driver.corpus_module.advancement_loot_fixtures = lambda groups: ({}, dict(tables))
    sys.argv.append('--allow-gaps')
    driver.main()
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_bytes().splitlines()]
    if '--reference-only' in sys.argv:
        return
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_bytes().splitlines()]
    assert len(recorded) == len(java) == len(native)
    for op, row in zip(recorded, java):
        if 'expect_result' in op:
            assert row.get('results') == [[True, op['expect_result']]], ('Java fixture', op, row)
    residuals = [dict(label=op['label'], expected=a, actual=b)
                 for op, a, b in zip(recorded, java, native) if a != b]
    report = dict(records=len(recorded), passed=len(recorded) - len(residuals), residuals=residuals,
                  reference_sha256=hashlib.sha256((build / 'vanilla.jsonl').read_bytes()).hexdigest())
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
    for residual in residuals[:20]:
        print(json.dumps(residual))
    print(f'Strict records: {report["passed"]}/{report["records"]}')
    if residuals:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
