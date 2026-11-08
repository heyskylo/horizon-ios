#!/usr/bin/env python3
"""Folder pack metadata admission using real independent functions."""
import hashlib,json,sys,tempfile
from pathlib import Path
import test_command_exec_parity as driver
PACKS={
 'broken':'{', 'root_empty':'{}', 'root_array':'[]',
 'pack_empty':'{"pack":{}}', 'pack_string':'{"pack":"bad"}',
 'missing':None,
 'valid':'{"pack":{"description":"H146 full","pack_format":121,"min_format":[121,0],"max_format":[121,0]}}',
 'description':'{"pack":{"description":"H146 description only"}}',
}
VALUES={name:i+11 for i,name in enumerate(PACKS)}
VALID={'valid','description'}
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256);cmd('/tp @s 4.5 200 4.5 0 0','owner-pos',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[4.5d,200d,4.5d]}]','owner-body',1);cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/execute if loaded 4 200 4','loaded',1);cmd('/execute if block 4 199 4 stone','support',1)
 cmd('/scoreboard objectives add h146 dummy','objective');cmd('/execute store result score start h146 run time query gametime','clock-start',trace=True)
 for phase in ['initial','reloaded']:
  if phase=='reloaded':
   for name in PACKS:cmd(f'/scoreboard players set {name} h146 -99',f'{phase}-{name}-before-reload',-99)
   cmd('/reload','reload')
   for name in PACKS:cmd(f'/scoreboard players get {name} h146',f'{phase}-{name}-after-reload',-99)
  cmd('/datapack list enabled',phase+'-enabled',trace=True);cmd('/datapack list available',phase+'-available',trace=True)
  for name in PACKS:
   label=f'{phase}-{name}'
   cmd(f'/scoreboard players set {name} h146 -99',label+'-reset',-99);cmd(f'/scoreboard players get {name} h146',label+'-before',-99)
   for operation in ['availability','function']:
    for key in ['result','success']:cmd(f'/scoreboard players set {key} h146 -99',f'{label}-{operation}-reset-{key}',-99)
    action=f'datapack enable "file/h146-{name}"' if operation=='availability' else f'function h146:{name}'
    cmd(f'/execute store result score result h146 store success score success h146 run {action}',f'{label}-{operation}',admission=False)
    for key in ['result','success']:cmd(f'/scoreboard players get {key} h146',f'{label}-{operation}-{key}',admission=False)
   cmd(f'/scoreboard players get {name} h146',label+'-after',VALUES[name] if name in VALID else -99,admission=name in VALID)
  cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[4.5d,200d,4.5d]}]',phase+'-owner-still-valid',1)
 cmd('/execute store result score end h146 run time query gametime','clock-end',trace=True)
 cmd('/scoreboard players operation end h146 -= start h146','clock-delta-operation',0);cmd('/scoreboard players get end h146','clock-delta',0)
 return ops

def main():
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).resolve();del sys.argv[i:i+2]
 if '--work-dir' not in sys.argv:
  parent=Path(tempfile.mkdtemp(prefix='folder-metadata-',dir=driver.lab.LOCAL));sys.argv+=['--work-dir',str(parent/'run')]
 build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not build.exists(),'preserve attempts';build.mkdir(parents=True)
 ops=program();recorded=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 (build/'declaration.json').write_text(json.dumps(dict(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),records=len(recorded),labels=[o['label'] for o in recorded],packs=PACKS,values=VALUES,scope='all complete canonical records strict; complete pack lists/absolute clocks separate predeclared raw traces'),indent=2)+'\n')
 install=driver.install_datapack_fixtures
 def install_packs(world,*args,**kwargs):
  install(world,*args,**kwargs)
  for name,meta in PACKS.items():
   pack=world/'datapacks'/f'h146-{name}';target=pack/'data/h146/function'/f'{name}.mcfunction';target.parent.mkdir(parents=True,exist_ok=True)
   target.write_text(f'scoreboard players set {name} h146 {VALUES[name]}\n')
   if meta is not None:(pack/'pack.mcmeta').write_text(meta)
 driver.install_datapack_fixtures=install_packs
 driver.corpus_module.function_return_fixtures=lambda names:{}
 driver.corpus_module.groups=lambda:[('Folder metadata admission',ops)]
 expected=reference.read_bytes() if reference else None
 sys.argv.append('--allow-gaps');driver.main()
 java_bytes=(build/'vanilla.jsonl').read_bytes()
 if expected is not None:assert java_bytes==expected and reference.read_bytes()==expected,'reference bytes changed'
 if '--reference-only' in sys.argv:return
 java=[json.loads(x) for x in java_bytes.splitlines()];native=[json.loads(x) for x in (build/'rust.jsonl').read_bytes().splitlines()];assert len(recorded)==len(java)==len(native)
 residuals=[dict(label=o['label'],expected=a,actual=b) for o,a,b in zip(recorded,java,native) if a!=b]
 report=dict(records=len(recorded),passed=len(recorded)-len(residuals),residuals=residuals,reference_sha256=hashlib.sha256(java_bytes).hexdigest());(build/'strict-report.json').write_text(json.dumps(report,indent=2)+'\n')
 for o,a,b in zip(recorded,java,native):
  if o['admission']:assert a==b,('admission',o,a,b)
 print(f'Strict records: {report["passed"]}/{report["records"]}')
 if residuals:raise SystemExit(1)
if __name__=='__main__':main()
