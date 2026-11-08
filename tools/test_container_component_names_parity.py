#!/usr/bin/env python3
"""H210 real live names, destruction and one sealed-process chunk reopen.

The existing embedded official server reopens the same chunk storage, while
creating its usual fresh PrimaryLevelData. This is not full metadata/player or
clock-recovery parity. Every command record and typed disk snapshot is retained.
"""
import argparse,hashlib,json,os,shutil,subprocess,sys,datetime
from pathlib import Path
import anvil_dump
import test_command_exec_parity as runner
from container_component_name_corpus import stage_one,stage_two,overlay_restart_programs

PROGRAMS=[('stage-one',stage_one()),('stage-two',stage_two())]

def digest(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def script(ops):
    return ''.join(json.dumps({k:v for k,v in op.items() if k!='expect_result'},ensure_ascii=False)+'\n' for op in ops)
def records(ops):return [x for x in ops if x['op']=='cmd' and x.get('record',True)]
def load(path):return [json.loads(s) for s in path.read_text().splitlines()]
def admit(ops,rows):
    assert len(records(ops))==len(rows)>0,'complete record count'
    for op,row in zip(records(ops),rows):
        assert op['cmd']==row['cmd'],'complete original order'
        if 'expect_callback' in op:assert row['results']==[op['expect_callback']],(op,row)
def disk(world,out):
    candidates=[world/'dimensions/minecraft/overworld/region/r.0.0.mca',world/'region/r.0.0.mca']
    actual=[p for p in candidates if p.exists()];assert len(actual)==1,actual
    shutil.copy2(actual[0],out.with_suffix('.mca'))
    chunk=anvil_dump.region_chunks(actual[0])[(0,0)]
    be=chunk.get('block_entities',[])
    assert isinstance(be,list) and be,'actual saved block entities'
    selected=[x for x in be if (x.get('x'),x.get('y'),x.get('z')) in [(2,200,4),(5,200,4),(8,200,4),(11,200,4),(11,200,8),(8,200,11)]]
    assert len(selected)==6,'actual full saved scene'
    payload={'region':str(actual[0]),'region_sha256':digest(actual[0]),'block_entities':selected}
    out.write_text(json.dumps(payload,indent=2)+'\n');return payload

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--reference-only',action='store_true');ap.add_argument('--reference-dir',type=Path);ap.add_argument('--binary',type=Path);ap.add_argument('--work-dir',type=Path,required=True);ap.add_argument('--strict-all',action='store_true');ap.add_argument('--original-strict',action='store_true');ap.add_argument('--summarize-only',action='store_true');ap.add_argument('--overlay-restart',action='store_true');args=ap.parse_args()
    global PROGRAMS
    if args.overlay_restart:PROGRAMS=overlay_restart_programs()
    out=args.work_dir.resolve()
    if args.summarize_only:
        assert out.is_dir() and args.reference_dir and args.binary
        return summarize(out,args.reference_dir.resolve(strict=True),digest(args.binary),args.strict_all,args.original_strict)
    out.mkdir(parents=True,exist_ok=False)
    lock,jars,libraries=runner.base.verify_artifacts();dependency=os.pathsep.join(map(str,libraries));cp=None
    for name,ops in PROGRAMS:(out/(name+'-script.jsonl')).write_text(script(ops))
    receipt={'processes':[],'unknown_times':None,'restart_boundary':'same storage, separate JVM/process; official embedded server recreates PrimaryLevelData; no general metadata/player/clock claim'}
    if args.reference_only:
        classes=out/'classes';classes.mkdir();oracle=runner.lab.ROOT/'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaCommandExecOracle.java'
        with (out/'compile-java.log').open('w') as log:subprocess.run(['javac','--release',str(lock['minecraft']['java_major']),'-proc:none','-cp',str(jars['server'])+os.pathsep+dependency,'-d',str(classes),str(oracle)],stdout=log,stderr=subprocess.STDOUT,check=True)
        cp=os.pathsep.join([str(classes),str(jars['server']),dependency]);cwd=out/'java';cwd.mkdir();sealed={str(p.relative_to(classes)):digest(p) for p in classes.rglob('*.class')};assert sealed
    else:
        assert args.reference_dir and args.binary
        ref=args.reference_dir.resolve(strict=True);binary=args.binary.resolve(strict=True);seal=digest(binary);cwd=out/'native';cwd.mkdir()
        for name,ops in PROGRAMS:
            assert (ref/(name+'-script.jsonl')).read_text()==script(ops)
            shutil.copy2(ref/(name+'-java.jsonl'),out/(name+'-java.jsonl'));admit(ops,load(out/(name+'-java.jsonl')))
    for name,ops in PROGRAMS:
        original=out/(name+'-script.jsonl');output=out/(name+('-java.jsonl' if args.reference_only else '-native.jsonl'))
        started=datetime.datetime.now(datetime.timezone.utc).isoformat()
        with (out/(name+'.log')).open('w') as log:
            if args.reference_only:
                assert sealed=={str(p.relative_to(classes)):digest(p) for p in classes.rglob('*.class')}
                command=['java','-Xmx3G','-Duser.timezone=UTC','-Dhorizon.parity.flat=true','-cp',cp,'dev.horizonlab.parity.VanillaCommandExecOracle',str(cwd/'universe'),str(original),str(output),str(runner.SEED)]
                result=subprocess.run(command,cwd=cwd,stdout=log,stderr=subprocess.STDOUT,timeout=600)
                world=cwd/'universe/harness'
            else:
                assert digest(binary)==seal
                env=runner.lab.game_env(HORIZON_PLAYER_NAME='Player',HORIZON_COMMAND_SCRIPT=original,HORIZON_COMMAND_OUTPUT=output,HORIZON_API_DISCOVERY='http://127.0.0.1:9/')
                command=[str(binary),'--resources',str(runner.lab.RESOURCES),'--world','harness','--seed',str(runner.SEED),'--radius','4']
                result=runner.run_game(command,cwd=cwd,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=600)
                world=cwd/'saves/harness'
        receipt['processes'].append({'stage':name,'started_utc':started,'finished_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'pid':None,'argv':command,'exit':result.returncode,'world':str(world),'output_rows':len(output.read_text().splitlines()) if output.exists() else None})
        (out/'restart-receipts.json').write_text(json.dumps(receipt,indent=2)+'\n')
        assert result.returncode==0,'actual process/save boundary failed'
        rows=load(output);assert len(rows)==len(records(ops))
        if args.reference_only:admit(ops,rows)
        snapshot=disk(world,out/(name+'-disk.json'))
        if args.reference_only:
            styled={'text':'Styled','color':'aqua','bold':('b',0),'extra':[{'text':'Box','italic':('b',1)}]}
            translated={'translate':'container.chest','color':'red','italic':('b',0)}
            wanted={(2,200,4):'Box',(5,200,4):styled if name=='stage-one' else None,(8,200,4):translated,(11,200,4):'PlainAgain',(11,200,8):translated,(8,200,11):styled}
            if args.overlay_restart:
                wanted={pos:('Original' if name=='stage-one' or pos not in [(2,200,4),(5,200,4)] else None) for pos in wanted}
            for be in snapshot['block_entities']:
                pos=(be['x'],be['y'],be['z']);want=wanted[pos]
                assert ('CustomName' in be)==(want is not None) and be.get('CustomName')==want,('official disk/name admission',name,pos,be)
                slots={item['Slot'][1]:item for item in be['Items']};assert set(slots)=={0,26}
                assert all(item['Slot'][0]=='b' for item in slots.values())
                assert (slots[0]['id'],slots[0]['count'],slots[26]['id'],slots[26]['count'])==('minecraft:diamond',3,'minecraft:apple',5)
                assert slots[0]['components']['minecraft:custom_data']['h210']==1

    if args.reference_only:
        (out/'sealed-java-classes.json').write_text(json.dumps(sealed,indent=2)+'\n');print('Fresh official Java admitted both stages:',sum(len(records(o)) for _,o in PROGRAMS));return 0
    assert digest(binary)==seal
    return summarize(out,ref,seal,args.strict_all,args.original_strict)

def summarize(out,ref,seal,strict_all=False,original_strict=False):
    disk_results=[]
    for name,_ in PROGRAMS:
        java_disk=json.loads((ref/(name+'-disk.json')).read_text());native_disk=json.loads((out/(name+'-disk.json')).read_text())
        def keyed(data):
            values=data['block_entities'];result={(v['x'],v['y'],v['z']):v for v in values};assert len(result)==len(values)==6;return result
        a,b=keyed(java_disk),keyed(native_disk);assert a.keys()==b.keys()
        for pos in a:
            av,bv=a[pos],b[pos]
            disk_results.append({'stage':name,'pos':pos,'admitted':all(av.get(k)==bv.get(k) for k in ['id','x','y','z','Items','lock']),'name_equal':('CustomName' in av)==('CustomName' in bv) and av.get('CustomName')==bv.get('CustomName'),'java_name':av.get('CustomName'),'native_name':bv.get('CustomName'),'java_name_present':'CustomName' in av,'native_name_present':'CustomName' in bv})
    (out/'typed-disk-comparison.json').write_text(json.dumps(disk_results,indent=2)+'\n')
    report={'records':0,'strict':0,'strict_matches':0,'admissions':0,'failed_admissions':[],'assigned_differences':[],'unassigned_strict_differences':[],'diagnostic_rows':[],'diagnostic_differences':[],'differences':[],'binary_sha256':seal,'original_strict':0,'original_strict_matches':0,'restart_metadata_diagnostic_rows':[],'disk_total':len(disk_results),'disk_matches':sum(r['name_equal'] for r in disk_results),'failed_disk_admissions':[r for r in disk_results if not r['admitted']]}
    i=0
    for name,ops in PROGRAMS:
        java=load(out/(name+'-java.jsonl'));native=load(out/(name+'-native.jsonl'))
        for op,a,b in zip(records(ops),java,native):
            assert op['cmd']==b['cmd'];same=a==b;report['records']+=1
            if not same:report['differences'].append({'stage':name,'row':i,'java':a,'native':b})
            metadata=name=='stage-two' and op['cmd'] in {'/gamerule spawn_mobs false','/gamerule show_advancement_messages false'}
            if not op.get('diagnostic'):
                report['original_strict']+=1;report['original_strict_matches']+=same
            if metadata:report['restart_metadata_diagnostic_rows'].append(i)
            if op.get('diagnostic') or metadata and not original_strict:
                report['diagnostic_rows'].append(i)
                if not same:report['diagnostic_differences'].append(i)
            else:
                report['strict']+=1;report['strict_matches']+=same
                if not same:report['assigned_differences' if op.get('h210_target') else 'unassigned_strict_differences'].append(i)
            if op.get('h210_admission'):
                report['admissions']+=1
                if not same:report['failed_admissions'].append(i)
            i+=1
    (out/'name-report.json').write_text(json.dumps(report,indent=2)+'\n');print(report['strict_matches'],'/',report['strict'],'strict;',report['admissions'],'admissions;',len(report['diagnostic_differences']),'raw diagnostic differences')
    return bool(report['failed_disk_admissions'] or report['disk_matches']!=report['disk_total'] or report['failed_admissions'] or report['unassigned_strict_differences'] or report['strict_matches']!=report['strict'] or strict_all and report['differences'])
if __name__=='__main__':sys.exit(main())
