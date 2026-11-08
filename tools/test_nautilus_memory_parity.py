#!/usr/bin/env python3
"""H140 actual Nautilus cooldown memory projection through real load/tick/save commands."""
import hashlib
import json
from pathlib import Path
import shutil
import sys
import test_command_exec_parity as runner
from nautilus_memory_corpus import cases

GROUP = 'nautilus memory lifetime'

def recorded(program):
    return [op for op in program if (op['op'] == 'cmd' and op.get('record', True)) or (op['op'] == 'tick' and op.get('record'))]

def admit(ops, rows, reference_recipient=True):
    assert len(ops) == len(rows) and rows
    for op, row in zip(ops, rows):
        assert op.get('cmd', '#tick') == row['cmd']
        if 'expect_result' in op and (reference_recipient or not op.get('h140_target')):
            assert row['results'] == [[True, op['expect_result']]], (op, row)
        if reference_recipient and 'expect_callback' in op:
            assert row['results'] == [op['expect_callback']], (op,row)

def main():
    program = cases(runner.corpus_module.cmd)
    runner.corpus_module.groups = lambda: [(GROUP, program)]
    reference = None
    if '--reference-dir' in sys.argv:
        i = sys.argv.index('--reference-dir'); reference = Path(sys.argv[i + 1]).resolve(strict=True)
        del sys.argv[i:i + 2]
    folder = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    if reference is None:
        assert '--reference-only' in sys.argv
        sys.argv += ['--only', GROUP]
        runner.main()
    else:
        expected = ''.join(json.dumps({k: v for k, v in op.items() if k != 'expect_result'}, ensure_ascii=False) + '\n' for op in program)
        assert (reference / 'script.jsonl').read_text() == expected
        java = [json.loads(s) for s in (reference / 'vanilla.jsonl').read_text().splitlines()]
        admit(recorded(program), java)
        _lock, jars, _libraries = runner.base.verify_artifacts()
        binary = Path(sys.argv[sys.argv.index('--binary') + 1]).resolve(strict=True)
        folder.mkdir(parents=True, exist_ok=False)
        for name in ['script.jsonl', 'vanilla.jsonl']: shutil.copyfile(reference / name, folder / name)
        scratch = folder / 'rust'; scratch.mkdir()
        env = runner.lab.game_env(HORIZON_PLAYER_NAME='Player', HORIZON_COMMAND_SCRIPT=folder/'script.jsonl', HORIZON_COMMAND_OUTPUT=folder/'rust.jsonl', HORIZON_API_DISCOVERY='http://127.0.0.1:9/')
        with (folder / 'rust.log').open('w') as log:
            result = runner.run_game([str(binary), '--resources', str(runner.lab.RESOURCES), '--world', 'harness', '--seed', str(runner.SEED), '--radius', '4'], cwd=scratch, env=env, stdout=log, stderr=runner.subprocess.STDOUT)
        assert result.returncode == 0, 'native process prerequisite failed'
    java = [json.loads(s) for s in (folder/'vanilla.jsonl').read_text().splitlines()]
    ops = recorded(program); admit(ops, java)
    if reference is None:
        print('Fresh Java admitted', len(java), 'complete memory/control records'); return 0
    native = [json.loads(s) for s in (folder/'rust.jsonl').read_text().splitlines()]
    admit(ops, native, reference_recipient=False)
    report = dict(records=len(ops), strict=0, strict_matches=0, admissions=0, failed_admissions=[], diagnostic_rows=[], diagnostic_differences=[], differences=[])
    for i,(op,a,b) in enumerate(zip(ops, java, native)):
        same = a == b
        if not same: report['differences'].append({'row':i,'java':a,'native':b})
        if op.get('diagnostic'):
            report['diagnostic_rows'].append(i)
            if not same: report['diagnostic_differences'].append(i)
        else:
            report['strict'] += 1; report['strict_matches'] += same
        if op.get('h140_admission'):
            report['admissions'] += 1
            if not same: report['failed_admissions'].append(i)
    (folder/'memory-report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(report['strict_matches'], '/', report['strict'], 'strict; full raw diagnostics retained')
    return bool(report['failed_admissions'] or report['strict_matches'] != report['strict'])

if __name__ == '__main__': sys.exit(main())
