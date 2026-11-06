#!/usr/bin/env python3
"""Real command/listener damage timing; retain complete H064/H191 diagnostics.

Use --reference-only --work-dir REF to admit fresh Java. Then reuse REF for
old/fixed normal binaries with --reference-dir REF --binary GAME --work-dir OUT.
Each native-only OUT must be fresh; setup epochs stay in the unmodified records.
"""
import json
from pathlib import Path
import shutil
import sys

import test_command_exec_parity as runner
from mob_command_damage_vibration_corpus import cases


def callback(record):
    result = record.get('results')
    assert isinstance(result, list) and len(result) == 1
    assert isinstance(result[0], list) and len(result[0]) == 2
    assert type(result[0][0]) is bool and type(result[0][1]) is int
    return result


def native_from_reference(reference, folder, program):
    """Use one fresh qualified Java capture for both sides of a source fix.

    Absolute setup epochs remain in the raw diagnostics. The command program
    and every declared Java guard must match before the real Game is started.
    """
    runner.base.verify_artifacts()
    expected = ''.join(json.dumps({k: v for k, v in op.items() if k != 'expect_result'},
                                  ensure_ascii=False) + '\n' for op in program)
    assert (reference / 'script.jsonl').read_text() == expected
    java = [json.loads(line) for line in (reference / 'vanilla.jsonl').read_text().splitlines()]
    recorded = [op for op in program if op['op'] == 'cmd' and op.get('record', True)]
    assert len(java) == len(recorded)
    for op, row in zip(recorded, java):
        assert row['cmd'] == op['cmd']
        if 'expect_result' in op:
            assert row['results'] == [[True, op['expect_result']]], (op, row)
        if 'expect_callback' in op:
            assert callback(row) == [op['expect_callback']], (op, row)
    assert '--binary' in sys.argv, 'a frozen normal Game binary is required'
    binary = Path(sys.argv[sys.argv.index('--binary') + 1]).resolve(strict=True)
    folder.mkdir(parents=True, exist_ok=True)
    for name in ['script.jsonl', 'vanilla.jsonl']:
        shutil.copyfile(reference / name, folder / name)
    scratch = folder / 'rust'
    scratch.mkdir(exist_ok=False)
    env = runner.lab.game_env(HORIZON_PLAYER_NAME='Player',
                              HORIZON_COMMAND_SCRIPT=folder / 'script.jsonl',
                              HORIZON_COMMAND_OUTPUT=folder / 'rust.jsonl',
                              HORIZON_API_DISCOVERY='http://127.0.0.1:9/')
    with (folder / 'rust.log').open('w') as log:
        result = runner.run_game([str(binary), '--resources', str(runner.lab.RESOURCES),
                                  '--world', 'harness', '--seed', str(runner.SEED), '--radius', '4'],
                                 cwd=scratch, env=env, stdout=log, stderr=runner.subprocess.STDOUT)
    assert result.returncode == 0, f'native process failed; see {folder / "rust.log"}'


def main():
    reference = None
    if '--reference-dir' in sys.argv:
        index = sys.argv.index('--reference-dir')
        reference = Path(sys.argv[index + 1]).resolve(strict=True)
        del sys.argv[index:index + 2]
    runner.corpus_module.groups = lambda: [
        ('setup', runner.corpus_module.group_setup()),
        ('mob command damage vibration timing', cases())]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(runner.lab.LOCAL / 'mob-command-damage-vibration-parity')]
    if '--allow-gaps' not in sys.argv:
        sys.argv.append('--allow-gaps')
    folder = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    if reference is None:
        runner.main()
    else:
        assert '--reference-only' not in sys.argv
        program = [*runner.corpus_module.group_setup(), *cases()]
        native_from_reference(reference, folder, program)
    script = [json.loads(line) for line in (folder / 'script.jsonl').read_text().splitlines()]
    ops = [op for op in script if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(line) for line in (folder / 'vanilla.jsonl').read_text().splitlines()]
    assert len(ops) == len(java) and len(ops) > 0
    for op, reference in zip(ops, java):
        if 'canonical_callback' in op:
            assert callback(reference) == [op['canonical_callback']], (op, reference)
    if '--reference-only' in sys.argv:
        return 0
    native = [json.loads(line) for line in (folder / 'rust.jsonl').read_text().splitlines()]
    assert len(ops) == len(java) == len(native)
    report = {'canonical_records': 0, 'canonical_callback_projections': 0,
              'canonical_failures': [], 'diagnostic_records': 0, 'diagnostic_failures': []}
    raw_mismatches = []
    for i, (op, left, right) in enumerate(zip(ops, java, native)):
        assert left['cmd'] == right['cmd'] == op['cmd']
        if runner.compare(left, right):
            raw_mismatches.append({'row': i, 'java': left, 'native': right})
        if op.get('diagnostic'):
            report['diagnostic_records'] += 1
            if runner.compare(left, right):
                report['diagnostic_failures'].append({'row': i, 'scope': op['diagnostic']})
            if 'canonical_callback' in op:
                report['canonical_callback_projections'] += 1
                try:
                    matches = callback(right) == callback(left) == [op['canonical_callback']]
                except AssertionError:
                    matches = False
                if not matches:
                    report['canonical_failures'].append({'row': i, 'kind': 'callback'})
        else:
            report['canonical_records'] += 1
            kind = runner.compare(left, right)
            if kind:
                report['canonical_failures'].append({'row': i, 'kind': kind})
    (folder / 'damage-vibration-report.json').write_text(json.dumps(report, indent=2) + '\n')
    (folder / 'raw-mismatches.jsonl').write_text(''.join(json.dumps(row) + '\n' for row in raw_mismatches))
    print(json.dumps(report))
    return bool(report['canonical_failures'])


if __name__ == '__main__':
    sys.exit(main())
