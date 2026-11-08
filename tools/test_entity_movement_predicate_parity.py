"""Actual named movement predicates over a frozen, UUID-bound unmounted pig."""
import argparse,hashlib,json,sys
from pathlib import Path
try:
 from . import test_command_exec_parity as driver
except ImportError:
 import test_command_exec_parity as driver
ACTOR='000002a1-0000-0000-0000-000000000001'
def predicates():
 def entity(p):return {'type':'minecraft:entity_properties','entity':'this','predicate':p}
 movement=lambda p:entity({'minecraft:entity_type':'minecraft:pig','minecraft:movement':p})
 always={'type':'minecraft:all_of','terms':[]}
 return {'motion:always':always,'motion:identity':entity({'minecraft:entity_type':'minecraft:pig'}),
 'motion:high':movement({'x':{'min':100}}),'motion:zero':movement({'x':{'min':0,'max':0}}),
 'motion:empty':movement({}), 'motion:not_high':{'type':'minecraft:inverted','term':movement({'x':{'min':100}})},
 'motion:not_zero':{'type':'minecraft:inverted','term':movement({'x':{'min':0,'max':0}})}}
def program():
 ops=[]
 def cmd(s,label,result=None,callback=None,admission=True,diagnostic=None):
  o={'op':'cmd','cmd':s,'label':label,'admission':admission}
  if result is not None:o['expect_result']=result
  if callback is not None:o['expect_callback']=callback
  if diagnostic:o['diagnostic']=diagnostic
  ops.append(o)
 cmd('/tick freeze','freeze',1);cmd('/gamerule spawn_mobs false','no-spawns');cmd('/forceload add 0 0','force',1)
 ops.append({'op':'settle','required_chunks':[[0,0]]})
 cmd('/fill 0 199 0 15 199 15 stone','floor',256);cmd('/tp @s 8.5 200 8.5 0 0','owner-pos',1)
 cmd('/scoreboard objectives add motion dummy','objective',1);cmd('/datapack list enabled','pack-enabled')
 cmd('/execute if predicate motion:always','pack-positive',1)
 cmd('/execute store result score start motion run time query gametime','absolute-start',diagnostic='absolute startup epoch')
 body='UUID:[I;673,0,0,1],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:10f,Motion:[0d,0d,0d]'
 cmd('/summon pig 4.5 200 4.5 {'+body+'}','summon',1)
 for phase in ['before','after']:
  cmd(f'/execute as {ACTOR} if entity @s[type=pig,nbt={{{body},Pos:[4.5d,200d,4.5d]}}]',phase+'-actual-body',1)
  cmd(f'/execute unless data entity {ACTOR} Passengers',phase+'-no-passengers',1)
  cmd('/execute if loaded 4 200 4',phase+'-loaded',1);cmd('/execute if block 4 199 4 stone',phase+'-floor',1)
  for field,result in [('UUID',4),('Health',10),('Motion',3),('Pos',3)]:cmd(f'/data get entity {ACTOR} {field}',phase+'-'+field,result)
  cmd(f'/data get entity {ACTOR}',phase+'-full-body',diagnostic='complete typed actor/body/epoch, unclaimed ordering')
  cmd(f'/execute as {ACTOR} if predicate motion:identity',phase+'-named-identity',1)
  if phase=='before':
   for name,want in [('high',False),('zero',True),('empty',True),('not_high',True),('not_zero',False)]:
    cmd('/scoreboard players set result motion 91',name+'-fresh-result',91);cmd('/scoreboard players set success motion 17',name+'-fresh-success',17)
    cmd(f'/execute store result score result motion store success score success motion as {ACTOR} if predicate motion:{name}',name+'-if',callback=[want,int(want)],admission=False)
    cmd('/scoreboard players get result motion',name+'-result',int(want),admission=False);cmd('/scoreboard players get success motion',name+'-success',int(want),admission=False)
    cmd(f'/execute as {ACTOR} unless predicate motion:{name}',name+'-unless',callback=[not want,int(not want)],admission=False)
 cmd('/execute store result score end motion run time query gametime','absolute-end',diagnostic='absolute startup epoch')
 cmd('/execute if score end motion = start motion','relative-zero',1)
 cmd('/kill '+ACTOR,'cleanup-actor',1);cmd('/forceload remove 0 0','cleanup-force',1);cmd('/scoreboard objectives remove motion','cleanup-objective');cmd('/tick unfreeze','cleanup-unfreeze',0)
 return ops

def main():
 parser=argparse.ArgumentParser(add_help=False);parser.add_argument('--reference-dir',type=Path);options,rest=parser.parse_known_args();sys.argv=[sys.argv[0],*rest]
 folder=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not folder.exists()
 driver.corpus_module.groups=lambda:[('Actual named entity movement',program())]
 driver.corpus_module.predicate_selector_fixtures=lambda groups:predicates()
 if '--allow-gaps' not in sys.argv:sys.argv.append('--allow-gaps')
 driver.main()
 if '--reference-only' in sys.argv:return 0
 read=lambda p:[json.loads(s) for s in p.read_text().splitlines()]
 ops=[o for o in program() if o['op']=='cmd'];reference=(options.reference_dir/'vanilla.jsonl') if options.reference_dir else folder/'vanilla.jsonl'
 java,native,fresh=read(reference),read(folder/'rust.jsonl'),read(folder/'vanilla.jsonl');assert len(ops)==len(java)==len(native)==len(fresh)
 report={'strict':0,'admissions':0,'failures':[],'diagnostics':[],'reference_sha256':hashlib.sha256(reference.read_bytes()).hexdigest()}
 for i,(op,j,n,f) in enumerate(zip(ops,java,native,fresh)):
  if op.get('diagnostic'):
   report['diagnostics'].append({'index':i,'label':op['label'],'java':j,'native':n});continue
  assert j==f,('independent fresh Java strict reference changed',op,j,f)
  report['strict']+=1
  if op['admission']:report['admissions']+=1;assert j==n,('admission',op,j,n)
  if j!=n:report['failures'].append({'index':i,'label':op['label'],'java':j,'native':n})
 (folder/'movement-report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report));return bool(report['failures'])
if __name__=='__main__':raise SystemExit(main())
