#!/usr/bin/env python3
"""Fresh Squid/GlowSquid damage results through actual Game and pinned Java.

No world steps. Full bodies, Motion and absolute clocks remain executable raw
traces; --full-diagnostic strictly compares those too. No RNG/ink/timer claim.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver

COW = '000000b1-0000-0000-0000-000000000064'
CASES = [('glow_ai0_cow', 'glow_squid', 0, True),
         ('glow_ai0_generic', 'glow_squid', 0, False),
         ('glow_ai1_cow', 'glow_squid', 1, True),
         ('glow_ai1_generic', 'glow_squid', 1, False),
         ('squid_cow', 'squid', 1, True), ('squid_generic', 'squid', 1, False)]


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
    cmd('/fill 1 200 1 14 204 14 glass', 'walls-roof', 980)
    cmd('/fill 2 200 2 13 203 13 water[level=0]', 'water', 576)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/tp @s 8.5 205 14.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/execute if block 8 204 14 glass', 'owner-support', 1)
    cmd('/scoreboard objectives add h177 dummy', 'objective')
    cmd('/execute store result score start h177 run time query gametime', 'clock-start', diagnostic=True)
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    for field, result in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd(f'/data get entity @s {field}', 'owner-' + field, result)
    cmd('/summon cow 12.5 201 10.5 {UUID:[I;177,0,0,100],Health:10f,NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,CustomName:"H177_cow"}', 'cow-summon', 1)

    def cow(label):
        cmd(f'/execute as {COW} if entity @s[type=minecraft:cow]', label + '-cow-type', 1)
        cmd('/execute if loaded 12 201 10', label + '-cow-loaded', 1)
        cmd('/execute if block 12 201 10 water[level=0]', label + '-cow-water', 1)
        for field, result in [('UUID', 4), ('Health', 10), ('Pos', 3), ('NoAI', 1), ('NoGravity', 1)]:
            cmd(f'/data get entity {COW} {field}', label + '-cow-' + field, result)
        cmd(f'/data get entity {COW}', label + '-cow-body', diagnostic=True)

    cow('before')
    for n, (case, kind, no_ai, attributed) in enumerate(CASES, 1):
        x = n * 2
        uuid = f'000000b1-0000-0000-0000-{n:012x}'
        dark = ',DarkTicksRemaining:0' if kind == 'glow_squid' else ''
        cmd(f'/execute if loaded {x} 201 4', case + '-loaded', 1)
        cmd(f'/execute if block {x} 201 4 water[level=0]', case + '-water', 1)
        cmd(f'/execute if block {x} 199 4 stone', case + '-floor', 1)
        cmd(f'/summon {kind} {x + .5} 201 4.5 {{UUID:[I;177,0,0,{n}],Health:10f,NoAI:{no_ai}b,'
            f'NoGravity:1b,Silent:1b,PersistenceRequired:1b,CustomName:"H177_{case}"{dark}}}', case + '-summon', 1)
        for phase in ['before', 'after']:
            if phase == 'after':
                for holder in ['result', 'success']:
                    cmd(f'/scoreboard players set {holder} h177 -99', case + '-reset-' + holder, -99)
                source = f' by {COW}' if attributed else ''
                cmd(f'/execute store result score result h177 store success score success h177 '
                    f'run damage {uuid} 1 minecraft:generic{source}', case + '-hit', admission=attributed)
                for holder in ['result', 'success']:
                    cmd(f'/scoreboard players get {holder} h177', case + '-' + holder, admission=attributed)
            cmd(f'/execute as {uuid} if entity @s[type=minecraft:{kind}]', case + '-' + phase + '-type', 1)
            for field, result in [('UUID', 4), ('Health', 10 if phase == 'before' else 9), ('Pos', 3),
                                  ('NoGravity', 1), ('Silent', 1), ('PersistenceRequired', 1)]:
                cmd(f'/data get entity {uuid} {field}', case + '-' + phase + '-' + field, result)
            cmd(f'/data get entity {uuid} NoAI', case + '-' + phase + '-NoAI',
                callback=[True, 1] if no_ai else [False, 0])
            if kind == 'glow_squid':
                expected = 100 if phase == 'after' and attributed else 0
                cmd(f'/data get entity {uuid} DarkTicksRemaining', case + '-' + phase + '-dark', expected)
            cmd(f'/data get entity {uuid} Motion', case + '-' + phase + '-Motion', diagnostic=True)
            cmd(f'/data get entity {uuid}', case + '-' + phase + '-body', diagnostic=True)
    cow('after')
    cmd('/execute store result score now h177 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation now h177 -= start h177', 'clock-delta-operation', 0)
    cmd('/scoreboard players get now h177', 'clock-delta', 0)
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
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'squid-damage-result-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    ops = program(full)
    driver.corpus_module.groups = lambda: [('squid inherited damage result', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    for op, row in zip(recorded, java):
        if 'expect_results' in op:
            assert row['results'] == op['expect_results'], ('Java action admission', op, row)
    rows_by_label = {op['label']: row for op, row in zip(recorded, java)}
    # Measured on the fresh reference before either native capture: an error
    # here invokes false/0, not an assumed missing callback or stale sentinel.
    for case, _, _, attributed in CASES:
        expected = [[True, 1]] if attributed else [[False, 0]]
        assert rows_by_label[case + '-hit']['results'] == expected
        for holder in ['result', 'success']:
            assert rows_by_label[case + '-' + holder]['results'] == [[True, int(attributed)]]
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
