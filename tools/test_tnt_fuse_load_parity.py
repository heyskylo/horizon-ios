#!/usr/bin/env python3
"""Actual Game/fresh Java numeric TNT fuse load/save, with retained body diagnostics.

Pass the common runner's --binary and --work-dir. Full bodies remain in the
same script and raw captures, but are reported separately from the fuse gate.
"""
import json
from pathlib import Path
import sys

import test_command_exec_parity as runner
from tnt_fuse_load_corpus import cases


def main():
    runner.corpus_module.groups = lambda: [
        ('setup', runner.corpus_module.group_setup()), ('TNT numeric fuse load', cases())]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(runner.lab.LOCAL / 'tnt-fuse-load-parity')]
    if '--allow-gaps' not in sys.argv:
        sys.argv.append('--allow-gaps')
    runner.main()
    if '--reference-only' in sys.argv:
        return 0
    folder = Path(sys.argv[sys.argv.index('--work-dir') + 1])
    script = [json.loads(line) for line in (folder / 'script.jsonl').read_text().splitlines()]
    ops = [op for op in script if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(line) for line in (folder / 'vanilla.jsonl').read_text().splitlines()]
    native = [json.loads(line) for line in (folder / 'rust.jsonl').read_text().splitlines()]
    assert len(ops) == len(java) == len(native)
    report = {'canonical': 0, 'canonical_failures': [], 'diagnostics': 0, 'diagnostic_failures': []}
    for i, (op, left, right) in enumerate(zip(ops, java, native)):
        diagnostic = bool(op.get('diagnostic'))
        key = 'diagnostics' if diagnostic else 'canonical'
        report[key] += 1
        if runner.compare(left, right):
            report['diagnostic_failures' if diagnostic else 'canonical_failures'].append(i)
    (folder / 'fuse-report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))
    return bool(report['canonical_failures'])


if __name__ == '__main__':
    sys.exit(main())
