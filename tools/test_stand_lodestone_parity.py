#!/usr/bin/env python3
"""Actual ArmorStand equipment and loaded lodestone removal; complete raw records."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import test_command_exec_parity as runner
from stand_lodestone_corpus import program


def sha(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source,'sha256').hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work-dir',type=Path,required=True)
    parser.add_argument('--reference-only',action='store_true')
    parser.add_argument('--reference-dir',type=Path)
    parser.add_argument('--binary',type=Path)
    args=parser.parse_args()
    out=args.work_dir.resolve();out.mkdir(parents=True,exist_ok=False)
    ops=program()
    recorded=[x for x in ops if x['op'] in ('tick','player_loop') or x['op']=='cmd' and x.get('record',True)]
    script=''.join(json.dumps({k:v for k,v in op.items() if k!='expect_result'})+'\n' for op in ops)
    source=out/'script.jsonl';source.write_text(script)
    declaration=dict(records=len(recorded),labels=[x['label'] for x in recorded],
        categories=[x['category'] for x in recorded],script_sha256=sha(source),
        comparison='every full chronological record including ALL canonicals; no masks, sorting, random/epoch normalization',
        runtime='actual armor-stand main/offhand compass tick; loaded block only, no unloaded/POI closure',
        unknown_phase_times=None)
    (out/'declaration.json').write_text(json.dumps(declaration,indent=2)+'\n')

    def read(path):
        rows=[json.loads(line) for line in path.read_text().splitlines()]
        assert len(rows)==len(recorded),(len(rows),len(recorded))
        for op,row in zip(recorded,rows):
            assert row['cmd']==(op['cmd'] if op['op']=='cmd' else '#player_loop '+op['label'] if op['op']=='player_loop' else '#tick')
        return rows

    def admit(rows):
        failures=[]
        for op,row in zip(recorded,rows):
            if 'expect_result' in op and row['results']!=[[True,op['expect_result']]]:
                failures.append(dict(label=op['label'],expected=op['expect_result'],actual=row))
            if 'expect_results' in op and row['results']!=op['expect_results']:
                failures.append(dict(label=op['label'],expected=op['expect_results'],actual=row))
            if 'expect_observation' in op and row.get('state')!=op['expect_observation']:
                failures.append(dict(label=op['label'],expected=op['expect_observation'],actual=row))
        (out/'java-admission.json').write_text(json.dumps(dict(records=len(rows),failures=failures),indent=2)+'\n')
        assert not failures,('fresh Java admission',failures)

    if args.reference_only:
        lock,jars,libraries=runner.base.verify_artifacts()
        classes=out/'classes';classes.mkdir()
        dependency=os.pathsep.join(map(str,libraries))
        oracle=runner.lab.ROOT/'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaCommandExecOracle.java'
        with (out/'compile-java.log').open('w') as log:
            subprocess.run(['javac','--release',str(lock['minecraft']['java_major']),'-proc:none','-cp',str(jars['server'])+os.pathsep+dependency,'-d',str(classes),str(oracle)],stdout=log,stderr=subprocess.STDOUT,check=True)
        cwd=out/'java';cwd.mkdir()
        with (out/'java.log').open('w') as log:
            subprocess.run(['java','-Xmx3G','-Duser.timezone=UTC','-cp',os.pathsep.join([str(classes),str(jars['server']),dependency]),'dev.horizonlab.parity.VanillaCommandExecOracle',str(cwd/'universe'),str(source),str(out/'java.jsonl'),str(runner.SEED)],cwd=cwd,stdout=log,stderr=subprocess.STDOUT,check=True)
        rows=read(out/'java.jsonl');admit(rows)
        print('Fresh Java admitted',len(rows),'complete records',flush=True)
        return 0
    assert args.reference_dir and args.binary
    ref=args.reference_dir.resolve(strict=True);binary=args.binary.resolve(strict=True)
    assert (ref/'script.jsonl').read_text()==script,'frozen script changed'
    reference_bytes=(ref/'java.jsonl').read_bytes();reference_sha=sha(ref/'java.jsonl')
    java=read(ref/'java.jsonl');admit(java)
    (out/'java.jsonl').write_bytes(reference_bytes)
    binary_sha=sha(binary);cwd=out/'native';cwd.mkdir()
    env=runner.lab.game_env(HORIZON_PLAYER_NAME='Player',HORIZON_COMMAND_SCRIPT=source,HORIZON_COMMAND_OUTPUT=out/'native.jsonl',HORIZON_API_DISCOVERY='http://127.0.0.1:9/')
    with (out/'native.log').open('w') as log:
        result=runner.run_game([str(binary),'--resources',str(runner.lab.RESOURCES),'--world','harness','--seed',str(runner.SEED),'--radius','4'],cwd=cwd,env=env,stdout=log,stderr=subprocess.STDOUT)
    assert result.returncode==0,result.returncode
    native=read(out/'native.jsonl')
    report=dict(records=len(java),matched=0,admissions=0,failed_admissions=[],tracker_total=0,tracker_matched=0,canonical_total=0,canonical_matched=0,differences=[],binary_sha256=binary_sha,reference_sha256=reference_sha)
    for i,(op,a,b) in enumerate(zip(recorded,java,native)):
        same=a==b;report['matched']+=same
        if op['admission']:
            report['admissions']+=1
            if not same:report['failed_admissions'].append(op['label'])
        for category in ['tracker','canonical']:
            if op['category']==category:
                report[category+'_total']+=1;report[category+'_matched']+=same
        if not same:report['differences'].append(dict(index=i,label=op['label'],category=op['category'],java=a,native=b))
    assert sha(binary)==binary_sha and sha(ref/'java.jsonl')==reference_sha
    (out/'strict-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print('Full strict',report['matched'],'/',report['records'],'tracker',report['tracker_matched'],'/',report['tracker_total'],'canonicals',report['canonical_matched'],'/',report['canonical_total'],'admissions',report['admissions'],'failed',report['failed_admissions'],flush=True)
    return int(bool(report['differences']))


if __name__=='__main__':
    raise SystemExit(main())
