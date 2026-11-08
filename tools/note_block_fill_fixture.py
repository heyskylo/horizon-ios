import hashlib,json,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
CASES=[('ordinary','fill','','','stone'),('strict','fill','','strict','stone'),('explicit','fill','[instrument=banjo,note=7]','','stone'),('strict-explicit','fill','[instrument=banjo,note=7]','strict','stone'),('air','fill','','','air'),('keep','fill','','keep','stone'),('filtered','fill','','replace air','stone'),('setblock','setblock','','','stone'),('setblock-explicit','setblock','[instrument=banjo,note=7]','','stone'),('explicit-default','fill','[instrument=harp,note=7]','','stone'),('existing-default','fill','','','stone')]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False,expected=None):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h164 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h164 store success score success h164 run '+text,label+'-read',result=expected,admission=admission)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h164',label+'-'+k,admission=admission)
 cmd('/tick freeze','freeze')
 for g in ['spawn_mobs','advance_time','advance_weather','mob_griefing']:cmd('/gamerule '+g+' false',g)
 cmd('/gamerule random_tick_speed 0','random0');cmd('/difficulty normal','normal')
 cmd('/forceload add 0 0 31 31','forceload',4);ops.append(dict(op='settle',required_chunks=[[0,0],[0,1],[1,0],[1,1]]))
 cmd('/fill 0 198 0 31 199 31 stone','floor',2048);cmd('/fill 0 200 0 31 204 31 barrier','prefill',5120);cmd('/fill 0 200 0 31 204 31 air','air',5120)
 cmd('/setblock 30 200 30 glass strict','neighbor',1)
 cmd('/tp @s 2.5 200 2.5','player-pos',1);cmd('/gamemode creative @s','player-mode')
 cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[2.5d,200d,2.5d],playerGameType:1}]','player-body',1);cmd('/data get entity @s UUID','player-uuid',4)
 cmd('/scoreboard objectives add h164 dummy','objective');cmd('/execute store result score start h164 run time query gametime','start',trace=True)
 for n,(name,verb,props,mode,support) in enumerate(CASES):
  x=6+n%3*4;z=6+n//3*4;pos=f'{x} 200 {z}'
  if support=='air':cmd(f'/setblock {x} 199 {z} air strict',name+'-air-support',1)
  if name=='existing-default':cmd(f'/setblock {pos} note_block strict',name+'-initial-note',1)
  initial='note_block[instrument=harp,note=0,powered=false]' if name=='existing-default' else 'air'
  for text,label in [(f'if loaded {pos}','loaded'),(f'if block {x} 198 {z} stone','floor'),(f'if block {x} 199 {z} {support}','support'),(f'if block {pos} {initial}','initial-state'),(f'if block {x} 201 {z} air','top-air')]:cmd('/execute '+text,name+'-'+label,1)
  line=f'{verb} {pos} '+(pos+' ' if verb=='fill' else '')+'note_block'+props+(' '+mode if mode else '')
  read(line,name+'-place',False,1)
  cmd('/execute if block '+pos+' note_block',name+'-actual-type',1)
  for instrument in ['harp','basedrum','banjo']:
   read('execute if block '+pos+' note_block[instrument='+instrument+']',name+'-'+instrument)
  read('execute if block '+pos+' note_block[note='+('7' if props else '0')+',powered=false]',name+'-other-properties',True,1)
  cmd(f'/execute if block {x} 199 {z} {support}',name+'-support-preserved',1)
  if name in ['keep','filtered']:
   suffix='keep' if name=='keep' else 'replace glass'
   read(f'fill {pos} {pos} note_block[instrument=banjo,note=12] '+suffix,name+'-skip')
   read('execute if block '+pos+' note_block[instrument=basedrum,note=0,powered=false]',name+'-skip-preserved')
 cmd('/execute if block 30 200 30 glass','neighbor-preserved',1)
 cmd('/execute store result score now h164 run time query gametime','end-absolute',trace=True);cmd('/scoreboard players operation now h164 -= start h164','relative-op',0);cmd('/scoreboard players get now h164','relative-clock',0)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h164/capture')).resolve();assert not (build/'vanilla.jsonl').exists();ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 build.mkdir(parents=True,exist_ok=True);(build/'declaration.json').write_text(json.dumps({'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'strict_records':sum(not o.get('diagnostic',False) for o in rows),'diagnostic_labels':[o['label'] for o in rows if o.get('diagnostic',False)],'labels':[o['label'] for o in rows],'cases':CASES,'samples':[0],'scope':'Note-block fill instrument with stone/air support, explicit properties, strict/keep/filtered controls and setblock counterparts; all callbacks, complementary predicates, stores and relative zero clock strict.'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Note block fill instrument',ops)];sys.argv.append('--allow-gaps');driver.main()
 fresh=(build/'vanilla.jsonl').read_bytes()
 if reference:assert fresh==reference,'fresh reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(x) for x in fresh.splitlines()];b=[json.loads(x) for x in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows)
 diffs=[dict(label=o['label'],expected=x,actual=y,diagnostic=o.get('diagnostic',False)) for o,x,y in zip(rows,a,b) if x!=y]
 (build/'full-report.json').write_text(json.dumps(dict(records=len(rows),passed=len(rows)-len(diffs),differences=diffs),indent=2)+'\n')
 strict=[o for o in rows if not o.get('diagnostic',False)];gaps=[d for d in diffs if not d['diagnostic']]
 (build/'strict-report.json').write_text(json.dumps(dict(records=len(strict),passed=len(strict)-len(gaps),differences=gaps),indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('full',len(rows)-len(diffs),'/',len(rows),'strict',len(strict)-len(gaps),'/',len(strict));sys.exit(bool(gaps))
