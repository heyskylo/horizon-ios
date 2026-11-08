import hashlib,json,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
INLINE='{material:{palette_id:"minecraft:gold",description:"Inline material"},pattern:{asset_id:"minecraft:coast",description:"Inline pattern",decal:0b}}'
NAMED='{material:"minecraft:gold",pattern:"minecraft:coast"}'
CASES=[('inline',INLINE),('named',NAMED),('plain',None)]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False):
  row=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:row['expect_result']=result
  if diagnostic:row.update(record=False,trace=True)
  ops.append(row)
 def read(path,label,admission=False):
  for field in ['result','success']:cmd(f'/scoreboard players set {field} h284 -99',label+'-reset-'+field,-99)
  cmd('/execute store result score result h284 store success score success h284 run data get block 4 200 4 '+path,label+'-read',admission=admission)
  for field in ['result','success']:cmd(f'/scoreboard players get {field} h284',label+'-'+field,admission=admission)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-time');cmd('/forceload add 0 0','force',1)
 ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256);cmd('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest',1)
 ops.append(dict(op='settle',required_chunks=[[0,0]]))
 for query,label in [('loaded 4 200 4','loaded'),('block 4 199 4 stone','support'),('block 4 200 4 chest[facing=north,type=single,waterlogged=false]','block')]:cmd('/execute if '+query,label,1)
 cmd('/tp @s 8.5 200 8.5 0 0','owner-position',1);cmd('/gamemode creative @s','mode');cmd('/execute if entity @s[type=player,name=Player]','owner-type',1)
 for key,n in [('UUID',4),('Health',20),('playerGameType',1),('Pos',3)]:cmd('/data get entity @s '+key,'owner-'+key,n)
 for key,n in [('id',15),('x',4),('y',200),('z',4),('Items',0)]:cmd('/data get block 4 200 4 '+key,'chest-'+key,n)
 cmd('/scoreboard objectives add h284 dummy','objective');cmd('/execute store result score start h284 run time query gametime','clock-start',diagnostic=True)
 for slot,(case,trim) in enumerate(CASES):
  path=f'Items[{{Slot:{slot}b}}]'
  cmd('/execute unless data block 4 200 4 '+path,case+'-empty',1)
  for field in ['result','success']:cmd(f'/scoreboard players set {field} h284 -99',case+'-write-reset-'+field,-99)
  components='custom_data={keep:284}'+(',trim='+trim if trim else '')
  cmd(f'/execute store result score result h284 store success score success h284 run item replace block 4 200 4 container.{slot} with diamond_chestplate[{components}]',case+'-write',1)
  for field in ['result','success']:cmd(f'/scoreboard players get {field} h284',case+'-write-'+field)
  for key,n in [('id',len('minecraft:diamond_chestplate')),('count',1),('Slot',slot)]:cmd('/data get block 4 200 4 '+path+'.'+key,case+'-'+key,n)
  read(path,case+'-stack');read(path+'.components',case+'-components');read(path+'.components."minecraft:trim"',case+'-trim')
  read(path+'.components."minecraft:custom_data".keep',case+'-custom',True)
  if trim:
   for field in ['material','pattern']:read(path+'.components."minecraft:trim".'+field,case+'-'+field)
   cmd('/execute if data block 4 200 4 '+f'Items[{{Slot:{slot}b,components:{{"minecraft:trim":{trim}}}}}]',case+'-typed-trim',1,False)
  else:cmd('/execute unless data block 4 200 4 '+path+'.components."minecraft:trim"',case+'-no-trim',1)
 cmd('/data get block 4 200 4','full-body',diagnostic=True)
 cmd('/execute store result score end h284 run time query gametime','clock-end',diagnostic=True);cmd('/scoreboard players operation end h284 -= start h284','clock-delta-op',0);cmd('/scoreboard players get end h284','clock-delta',0)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).resolve().read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h284/capture')).resolve();ops=program()
 driver.corpus_module.groups=lambda:[('inline trim holders',ops)]
 sys.argv.append('--allow-gaps');driver.main()
 recorded=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 actual=(build/'vanilla.jsonl').read_bytes();java=[json.loads(l) for l in actual.splitlines()]
 assert len(recorded)==len(java)
 if reference is not None:assert reference==actual,'fresh Java reference changed'
 if '--reference-only' in sys.argv:print('Java records',len(java));sys.exit(0)
 native=[json.loads(l) for l in (build/'rust.jsonl').read_bytes().splitlines()];assert len(native)==len(java)
 diffs=[dict(label=o['label'],expected=a,actual=b) for o,a,b in zip(recorded,java,native) if a!=b]
 (build/'strict-report.json').write_text(json.dumps(dict(records=len(java),passed=len(java)-len(diffs),differences=diffs),indent=2)+'\n')
 for o,a,b in zip(recorded,java,native):
  if o['admission']:assert a==b,('admission',o,a,b)
 print(f'{len(java)-len(diffs)}/{len(java)}');sys.exit(bool(diffs))
