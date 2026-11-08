#!/usr/bin/env python3
"""Strict actual Game/Java world data pack test definition proof (H275)."""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver
from world_test_def_corpus import PACK_TESTS, world_test_def

_install = driver.install_datapack_fixtures


def install_with_tests(world, predicates, functions, jar, *args, **kwargs):
    """The harness's own pack, with this scene's test instances in it."""
    # The pack only exists when it has something to install.
    functions = dict(functions)
    functions.setdefault('h275:noop', '# keeps the fixture pack present\n')
    _install(world, predicates, functions, jar, *args, **kwargs)
    pack = Path(world) / 'datapacks' / 'command-selector-fixture'
    for identifier, body in PACK_TESTS.items():
        namespace, path = identifier.split(':', 1)
        target = pack / 'data' / namespace / 'test_instance' / (path + '.json')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(body), encoding='utf-8')


driver.install_datapack_fixtures = install_with_tests

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
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'world-test-def-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    ops = world_test_def(full)
    driver.corpus_module.groups = lambda: [('world test definition', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    assert len(java) == len(recorded)
    if '--reference-only' not in sys.argv:
        source = reference or build / 'vanilla.jsonl'
        if reference and not full:
            assert source.read_bytes() == (build / 'vanilla.jsonl').read_bytes(), 'fresh Java bytes changed'
        expected = [json.loads(s) for s in source.read_text().splitlines()]
        native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
        assert len(expected) == len(native) == len(recorded)
        for op, a, b in zip(recorded, expected, native):
            if op['admission'] and not full:
                assert a == b, ('actual setup/guard admission', op, a, b)
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
