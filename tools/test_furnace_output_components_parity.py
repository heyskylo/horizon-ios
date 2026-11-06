#!/usr/bin/env python3
"""Actual world recipes/furnace ticks; strict complete output-component records."""
import hashlib
import json
from pathlib import Path
import sys
import test_command_exec_parity as driver
from furnace_output_components_corpus import output_components, recipes

if __name__ == '__main__':
    full = '--full-diagnostic' in sys.argv
    if full: sys.argv.remove('--full-diagnostic')
    reference = None
    if '--reference' in sys.argv:
        at = sys.argv.index('--reference');reference=Path(sys.argv[at+1]).resolve();del sys.argv[at:at+2]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(driver.lab.LOCAL/'furnace-output-components-parity')]
    build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    ops=output_components(full)
    driver.corpus_module.groups=lambda:[('furnace output components',ops)]
    driver.corpus_module.crafting_priority_recipes=lambda _:recipes()
    sys.argv.append('--allow-gaps')
    driver.main()
    relative=Path('datapacks/command-selector-fixture/data/h327/recipe/hot_iron.json')
    assert json.loads((build/'vanilla/universe/harness'/relative).read_text())==recipes()['h327:hot_iron']
    if '--reference-only' not in sys.argv:
        assert json.loads((build/'rust/saves/harness'/relative).read_text())==recipes()['h327:hot_iron']
        source=reference or build/'vanilla.jsonl'
        if reference and not full:
            assert source.read_bytes()==(build/'vanilla.jsonl').read_bytes(), 'fresh Java reference bytes changed'
        java=[json.loads(s) for s in source.read_text().splitlines()]
        native=[json.loads(s) for s in (build/'rust.jsonl').read_text().splitlines()]
        recorded=[op for op in ops if op['op']=='cmd' and op.get('record',True)]
        assert len(java)==len(native)==len(recorded)
        for op,a,b in zip(recorded,java,native):
            if op['admission']: assert a==b, ('actual recipe/slot/control admission',op,a,b)
        differences=[{'label':op['label'],'expected':a,'actual':b} for op,a,b in zip(recorded,java,native) if a!=b]
        report={'records':len(java),'passed':len(java)-len(differences),
                'comparison':'all declared complete raw records; no masks/tolerances',
                'reference_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
                'fresh_java_sha256':hashlib.sha256((build/'vanilla.jsonl').read_bytes()).hexdigest(),
                'differences':differences}
        (build/'strict-report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(f'Strict raw records: {report["passed"]}/{report["records"]}')
        if differences: raise SystemExit(1)
