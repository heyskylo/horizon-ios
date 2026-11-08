#!/usr/bin/env python3
"""Strict actual Game/Java `/loot` of an inline table referencing a world pack's loot table."""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver
from inline_loot_reference_corpus import FUNCTIONS, TABLES, inline_loot_reference

if __name__ == '__main__':
    full = '--full-diagnostic' in sys.argv
    if full:
        sys.argv.remove('--full-diagnostic')
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference')
        reference = Path(sys.argv[i + 1]).resolve()
        del sys.argv[i:i + 2]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'inline-loot-reference-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    ops = inline_loot_reference(full)
    driver.corpus_module.groups = lambda: [('inline loot reference', ops)]
    # The world pack's tables and function, for this group only.
    loot_fixtures = driver.corpus_module.advancement_loot_fixtures
    function_fixtures = driver.corpus_module.function_return_fixtures

    def with_tables(names):
        advancements, tables = loot_fixtures(names)
        tables.update(TABLES)
        return advancements, tables

    def with_functions(names):
        functions = function_fixtures(names)
        functions.update(FUNCTIONS)
        return functions
    driver.corpus_module.advancement_loot_fixtures = with_tables
    driver.corpus_module.function_return_fixtures = with_functions
    sys.argv.append('--allow-gaps')
    driver.main()
    # The installed pack files' exact bytes on both sides.
    installed = {}
    for side, world in (('vanilla', build / 'vanilla/universe/harness'), ('rust', build / 'rust/saves/harness')):
        pack = world / 'datapacks/command-selector-fixture/data/parity'
        for rel in ('loot_table/il_one.json', 'loot_table/il_empty.json', 'function/il_bad.mcfunction'):
            if (pack / rel).exists():
                installed[f'{side}:{rel}'] = hashlib.sha256((pack / rel).read_bytes()).hexdigest()
    (build / 'installed-files.json').write_text(json.dumps(installed, indent=2) + '\n')
    print('installed files', installed)
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    assert len(java) == len(recorded)
    if '--reference-only' not in sys.argv:
        source = build / 'vanilla.jsonl' if full else (reference or build / 'vanilla.jsonl')
        if reference and not full:
            assert source.read_bytes() == (build / 'vanilla.jsonl').read_bytes(), 'fresh Java bytes changed'
        expected = [json.loads(s) for s in source.read_text().splitlines()]
        native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
        assert len(expected) == len(native) == len(recorded)
        for op, a, b in zip(recorded, expected, native):
            if op['admission'] and not full:
                assert a == b, ('actual setup/control admission', op, a, b)
        differences = [{'label': op['label'], 'expected': a, 'actual': b}
                       for op, a, b in zip(recorded, expected, native) if a != b]
        report = {'records': len(native), 'passed': len(native) - len(differences),
                  'comparison': 'complete raw records, no masks/tolerances',
                  'reference_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                  'fresh_java_sha256': hashlib.sha256((build / 'vanilla.jsonl').read_bytes()).hexdigest(),
                  'differences': differences}
        (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(f'Strict raw records: {report["passed"]}/{report["records"]}')
        if differences:
            raise SystemExit(1)
