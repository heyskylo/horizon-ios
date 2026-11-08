#!/usr/bin/env python3
"""Actual tag commands and saved empty Tags omission, with full raw diagnostics.

The scoped program keeps membership, presence, callbacks, actual typed bodies and
relative clock strict. --full-diagnostic additionally compares whole entity tags
and absolute clocks; those commands remain traced in the scoped program too.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver

A = '00000148-0000-0000-0000-000000000001'
B = '00000148-0000-0000-0000-000000000002'


def program(full=False):
    ops = []

    def cmd(text, label, result=None, callback=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if callback is not None:
            op['expect_callback'] = callback
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 8.5 200 12.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h328 dummy', 'objective')
    cmd('/execute store result score start h328 run time query gametime', 'clock-start', diagnostic=True)
    for n, x, extra in [(1, 4.5, ''), (2, 8.5, ',Tags:[]')]:
        cmd(f'/execute if loaded {int(x)} 200 4', f'load-{n}', 1)
        cmd(f'/execute if block {int(x)} 200 4 air', f'gap-{n}', 1)
        cmd(f'/summon pig {x} 200 4.5 {{UUID:[I;328,0,0,{n}],Health:10f,NoAI:1b,'
            f'NoGravity:1b,PersistenceRequired:1b,PortalCooldown:{17 if n == 1 else 23}{extra}}}',
            f'summon-{n}', 1)

    def owner(label):
        cmd('/execute if entity @s[type=player,name=Player]', label + '-owner-type', 1)
        for field, result in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
            cmd(f'/data get entity @s {field}', label + '-owner-' + field, result)

    owner('before')

    def state(label, uuid, x, tag, cooldown):
        present = tag is not None
        cmd(f'/execute as {uuid} if entity @s[type=pig]', label + '-actual-type', 1)
        cmd(f'/execute if loaded {x} 200 4', label + '-loaded', 1)
        cmd(f'/execute if block {x} 199 4 stone', label + '-floor', 1)
        for field, result in [('UUID', 4), ('Health', 10), ('Pos', 3), ('NoAI', 1),
                              ('NoGravity', 1), ('PersistenceRequired', 1), ('PortalCooldown', cooldown)]:
            cmd(f'/data get entity {uuid} {field}', label + '-' + field, result)
        cmd(f'/tag {uuid} list', label + '-live-tags', 1 if present else 0)
        cmd(f'/data get entity {uuid}', label + '-full-tag', diagnostic=True)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h328 -99', label + '-reset-' + holder, -99)
        cmd(f'/execute store result score result h328 store success score success h328 '
            f'run data get entity {uuid} Tags', label + '-Tags',
            callback=[True, 1] if present else [False, 0], admission=present)
        cmd('/scoreboard players get result h328', label + '-result', 1 if present else 0, admission=present)
        cmd('/scoreboard players get success h328', label + '-success', 1 if present else 0, admission=present)
        for sign, matches in [('if', present), ('unless', not present)]:
            cmd('/scoreboard players set presence h328 -99', label + '-' + sign + '-reset', -99)
            cmd(f'/execute {sign} data entity {uuid} Tags run scoreboard players set presence h328 1',
                label + '-' + sign + '-presence', admission=present)
            ops[-1]['expect_results'] = [[True, 1]] if matches else []
            cmd('/scoreboard players get presence h328', label + '-' + sign + '-result',
                1 if matches else -99, admission=present)

    for label, tagged in [('initial', False), ('added', True), ('removed', False),
                          ('readded', True), ('removed-again', False)]:
        if label != 'initial':
            cmd(f'/tag {A} {"add" if tagged else "remove"} audit', label + '-mutation', 1)
        state(label, A, 4, 'audit' if tagged else None, 17)
    state('explicit-empty-load', B, 8, None, 23)
    cmd(f'/tag {B} add keep', 'retained-mutation', 1)
    state('retained', B, 8, 'keep', 23)
    owner('after')
    cmd('/execute store result score now h328 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation now h328 -= start h328', 'clock-delta-operation', 0)
    cmd('/scoreboard players get now h328', 'clock-delta', 0)
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
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'saved-empty-tags-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    ops = program(full)
    driver.corpus_module.groups = lambda: [('saved empty entity Tags', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    for op, row in zip(recorded, java):
        if 'expect_results' in op:
            assert row['results'] == op['expect_results'], ('Java action admission', op, row)
    if '--reference-only' in sys.argv:
        return
    source = reference or build / 'vanilla.jsonl'
    if reference:
        assert source.read_bytes() == (build / 'vanilla.jsonl').read_bytes(), 'fresh Java bytes changed'
    expected = [json.loads(s) for s in source.read_text().splitlines()]
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
    assert len(expected) == len(native) == len(recorded)
    differences = [{'label': op['label'], 'expected': a, 'actual': b}
                   for op, a, b in zip(recorded, expected, native) if a != b]
    report = {'records': len(native), 'passed': len(native) - len(differences),
              'reference_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
              'comparison': 'complete raw records, no masks/tolerances', 'differences': differences}
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
    if not full:
        for op, a, b in zip(recorded, expected, native):
            if op['admission']:
                assert a == b, ('actual fixture admission', op, a, b)
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
