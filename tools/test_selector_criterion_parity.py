#!/usr/bin/env python3
"""Strict actual owner-player Game/Java selector criterion admission."""
import hashlib
import json
from pathlib import Path
import sys
import zipfile
import test_command_exec_parity as driver
from selector_criterion_corpus import criterion_selectors, ROOT, MISSING, UUID

if __name__=='__main__':
    full='--full-diagnostic' in sys.argv
    if full: sys.argv.remove('--full-diagnostic')
    reference=None
    if '--reference' in sys.argv:
        i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).resolve();del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:sys.argv+=['--work-dir',str(driver.lab.LOCAL/'selector-criterion-parity')]
    build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    ops=criterion_selectors(full)
    driver.corpus_module.groups=lambda:[('owner selector criterion membership',ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    # Actual shared vanilla definition, independent of selector/progress results.
    with zipfile.ZipFile(driver.lab.LOCAL/'minecraft/26.3/jars/server-game.jar') as jar:
        definition=json.loads(jar.read('data/minecraft/advancement/story/root.json'))
    assert set(definition['criteria'])=={'crafting_table'} and MISSING not in definition['criteria']
    path=driver.lab.LOCAL/'horizon-client/26.3/data/minecraft/advancement/story/root.json'
    prepared=json.loads(path.read_text())
    assert prepared['criteria']==definition['criteria'] and prepared['requirements']==definition['requirements']
    (build/'definition-witness.json').write_text(json.dumps({'id':ROOT,'criteria':list(prepared['criteria']),'requirements':prepared['requirements'],'prepared_sha256':hashlib.sha256(path.read_bytes()).hexdigest()},indent=2)+'\n')
    recorded=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
    java=[json.loads(s) for s in (build/'vanilla.jsonl').read_text().splitlines()]
    assert len(java)==len(recorded)
    for op,row in zip(recorded,java):
        if 'expect_results' in op:assert row['results']==op['expect_results'],('Java action/callback',op,row)
        if op['label'].endswith('-UUID') and '-pig-' not in op['label']:
            text=''.join(x[0] for x in row['messages'][0]);assert '[I; '+', '.join(map(str,UUID))+']' in text,('actual owner UUID',text)
    if '--reference-only' not in sys.argv:
        source=reference or build/'vanilla.jsonl'
        if reference and not full:assert source.read_bytes()==(build/'vanilla.jsonl').read_bytes(),'fresh Java reference bytes changed'
        expected=[json.loads(s) for s in source.read_text().splitlines()]
        native=[json.loads(s) for s in (build/'rust.jsonl').read_text().splitlines()]
        assert len(expected)==len(native)==len(recorded)
        for op,a,b in zip(recorded,expected,native):
            if op['admission']:assert a==b,('actual identity/progress/control admission',op,a,b)
        diffs=[{'label':op['label'],'expected':a,'actual':b} for op,a,b in zip(recorded,expected,native) if a!=b]
        report={'records':len(native),'passed':len(native)-len(diffs),'comparison':'complete raw records; no masks/tolerances',
                'reference_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
                'fresh_java_sha256':hashlib.sha256((build/'vanilla.jsonl').read_bytes()).hexdigest(),'differences':diffs}
        (build/'strict-report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(f'Strict raw records: {report["passed"]}/{report["records"]}')
        if diffs:raise SystemExit(1)
