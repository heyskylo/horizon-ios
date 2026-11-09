"""Predeclared owner three-field gate; tracker9965 is external-owned diagnostic."""
import hashlib
import uuid
import argparse
import json
from pathlib import Path
import sys

import test_command_exec_parity as runner
from command_exec_corpus import cmd, setup


def cases():
    identity = uuid.UUID(bytes=hashlib.md5(b'OfflinePlayer:Player').digest(), version=3)
    parts = [int.from_bytes(identity.bytes[i:i+4], 'big', signed=True) for i in range(0, 16, 4)]
    saved_uuid = '[I;' + ','.join(map(str, parts)) + ']'
    ops = setup('/forceload add 0 0 15 15', '/fill 0 199 0 15 199 15 stone')
    ops += [{'op': 'settle'}]
    ops += [cmd('/tick freeze', expect_result=1),
            cmd('/scoreboard objectives add h157 dummy', expect_result=1)]
    ops += setup('/tp @s 8.5 200 8.5 0 0')
    ops += [cmd('/execute if loaded 8 200 8', expect_result=1),
            cmd('/execute if block 8 199 8 stone', expect_result=1),
            cmd(f'/execute if entity @s[type=minecraft:player,nbt={{UUID:{saved_uuid},Health:20f,playerGameType:1,Pos:[8.5d,200d,8.5d]}}]', expect_result=1),
            cmd('/data get entity @s UUID'),
            cmd('/data get entity @s Health', expect_result=20),
            cmd('/data get entity @s playerGameType', expect_result=1),
            cmd('/data get entity @s Pos'),
            cmd('/data get entity @s foodLevel', expect_result=20),
            cmd('/data get entity @s SelectedItemSlot', expect_result=0)]
    clock = cmd('/execute store result score before h157 run time query gametime')
    clock['diagnostic'] = 'absolute clock; relative zero remains a strict control'
    ops.append(clock)
    # Entire unmodified four-field/body evidence stays in the same program.
    for path, expected, shape in [
            ('Brain', 1, 'Brain:{memories:{}}'),
            ('Brain.memories', 0, 'Brain:{memories:{}}'),
            ('current_impulse_context_reset_grace_time', 0,
             'current_impulse_context_reset_grace_time:0'),
            ('spawn_extra_particles_on_fall', 0, 'spawn_extra_particles_on_fall:0b')]:
        ops += [cmd(f'/execute if data entity @s {path}', expect_result=1),
                cmd(f'/execute if entity @s[nbt={{{shape}}}]', expect_result=1),
                cmd(f'/data get entity @s {path}', expect_result=expected)]
        ops += setup('/scoreboard players set result h157 31',
                     '/scoreboard players set success h157 17')
        ops += [cmd(f'/execute store result score result h157 store success score success h157 run data get entity @s {path}', expect_result=expected),
                cmd('/scoreboard players get result h157', expect_result=expected),
                cmd('/scoreboard players get success h157', expect_result=1)]
    for label, mutation in [('original', None), ('tracker_set', '/warden_spawn_tracker set 3'),
                            ('tracker_clear', '/warden_spawn_tracker clear')]:
        if mutation:
            row = cmd(mutation, expect_result=1);row['diagnostic'] = 'external-owned tracker9965 command';ops.append(row)
        for probe in ['/data get entity @s warden_spawn_tracker',
                      '/data get entity @s warden_spawn_tracker.warning_level',
                      '/data get entity @s']:
            row = cmd(probe);row['diagnostic'] = ('complete player body; order/other fields retained' if probe == '/data get entity @s'
                                               else 'external-owned tracker9965; NOT H157 pass')
            row['diagnostic_phase'] = label;ops.append(row)
    clock = cmd('/execute store result score after h157 run time query gametime')
    clock['diagnostic'] = 'absolute clock; relative zero remains a strict control'
    ops.append(clock)
    ops += [cmd('/execute if score before h157 = after h157', expect_result=1),
            cmd('/scoreboard objectives remove h157', expect_result=0),
            cmd('/tick unfreeze', expect_result=0)]
    return ops


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--reference-dir', type=Path)
    options, rest = parser.parse_known_args()
    sys.argv = [sys.argv[0], *rest]
    runner.corpus_module.groups = lambda: [
        ('setup', runner.corpus_module.group_setup()), ('owner player save projection', cases())]
    # Existing symmetric developer command switch admits the tracker diagnostic.
    for option in ('--allow-gaps', '--raid-village-fixture'):
        if option not in sys.argv:
            sys.argv.append(option)
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(runner.lab.LOCAL / 'player-save-fields-parity')]
    runner.main()
    if '--reference-only' in sys.argv:
        return 0
    folder = Path(sys.argv[sys.argv.index('--work-dir') + 1])
    script = [json.loads(line) for line in (folder / 'script.jsonl').read_text().splitlines()]
    ops = [op for op in script if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(line) for line in (folder / 'vanilla.jsonl').read_text().splitlines()]
    native = [json.loads(line) for line in (folder / 'rust.jsonl').read_text().splitlines()]
    fresh_java = java
    reference_path = (options.reference_dir / 'vanilla.jsonl' if options.reference_dir
                      else folder / 'vanilla.jsonl')
    java = [json.loads(line) for line in reference_path.read_text().splitlines()]
    assert len(ops) == len(java) == len(native) == len(fresh_java)
    # Entire predeclared diagnostic rows stay raw; they are not a reference-equivalence gate.
    assert all(a == b for op, a, b in zip(ops, java, fresh_java) if not op.get('diagnostic'))
    import hashlib
    report = {'strict': 0, 'strict_failures': [], 'diagnostic_records': 0,
              'diagnostic_differences': [],
              'external_tracker': 'issue9965: excluded, never an H157 pass',
              'reference_sha256': hashlib.sha256(reference_path.read_bytes()).hexdigest(),
              'fresh_strict_matches_frozen': True}
    for i, (op, left, right) in enumerate(zip(ops, java, native)):
        diagnostic = bool(op.get('diagnostic'))
        report['diagnostic_records' if diagnostic else 'strict'] += 1
        if left != right:
            report['diagnostic_differences' if diagnostic else 'strict_failures'].append(i)
    (folder / 'player-save-report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))
    return bool(report['strict_failures'])


if __name__ == '__main__':
    sys.exit(main())
