import argparse,json,sys,time,hashlib,io
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_locator_bar_parity as net
import test_respawn_world_point_parity as common
import test_hostile_sound_categories_parity as sound
import net_capture as wire
import anvil_dump as nbt
from test_dedicated_server_parity import game_time
INLINE='{material:{palette_id:"minecraft:gold",description:"Inline material"},pattern:{asset_id:"minecraft:coast",description:"Inline pattern",decal:0b}}'
NAMED='{material:"minecraft:gold",pattern:"minecraft:coast"}'
CASES=[('inline',INLINE),('named',NAMED),('plain',None)]
def run(label,command,client_command,build,world):
 directory=build/label;port=net.server_dir(directory,world);server=net.Server(command,directory,label)
 capture=build/f'{label}-packets.jsonl';proxy=net.free_port();stop=wire.start_background(proxy,f'127.0.0.1:{port}',capture,full=True)
 log=(build/f'{label}-client.log').open('w');out=build/f'{label}-client.txt';client=None;timeline=[]
 def dump(stage):common.dump(client,log,stage)
 try:
  server.wait_for(r'Done \(');common.console(server,'tick freeze','frozen')
  setup=sound.Commands(server,build/f'{label}-setup-commands.json')
  setup.reply('forceload add 0 0 15 15')
  deadline=time.monotonic()+60
  while 'Test passed' not in setup.reply('execute if loaded 4 200 4'):
   assert time.monotonic()<deadline;time.sleep(.05)
  setup.reply('fill 0 199 0 15 199 15 stone');setup.passed('execute if block 4 199 4 stone')
  client=net.Client(client_command,proxy,out,log)
  for c in ['menus on','chat on','state on','players on','step join','join Alice']:client.tell(c)
  client.sync(['Alice']);before=game_time(server);common.console(server,'tick step 2','Stepping')
  deadline=time.monotonic()+30
  while game_time(server)<before+2:
   assert time.monotonic()<deadline;time.sleep(.01)
  client.sync(['Alice']);start=game_time(server);assert start==before+2
  commands=setup
  def cmd(text):client.tell('send-cmd Alice '+text);client.sync(['Alice'])
  for c in ['gamemode creative @s','tp @s 8.5 200 8.5 0 0','data get entity @s UUID','data get entity @s Health','data get entity @s playerGameType','data get entity @s Pos','execute if loaded 4 200 4','execute if block 4 199 4 stone','data get block 4 200 4 id']:cmd(c)
  commands.passed('execute if entity @a[name=Alice,type=player,nbt={UUID:[I;'+sound.uuid_words('Alice')+'],Health:20f,playerGameType:1,Pos:[8.5d,200d,8.5d]}]')
  commands.passed('execute if loaded 4 200 4');commands.passed('execute if block 4 199 4 stone')
  for slot in range(3):commands.passed(f'execute unless data entity Alice Inventory[{{Slot:{slot}b}}]')
  dump('identity')
  for slot,(case,trim) in enumerate(CASES):
   client.tell('step '+case)
   cmd(f'item replace entity @s hotbar.{slot} with diamond_chestplate[custom_data={{keep:284}}'+(',trim='+trim if trim else '')+']')
   cmd('data get entity @s Inventory')
   commands.passed(f'execute if data entity Alice Inventory[{{Slot:{slot}b,id:"minecraft:diamond_chestplate",count:1}}]')
   current=game_time(server);common.console(server,'tick step 1','Stepping');deadline=time.monotonic()+30
   while game_time(server)<current+1:
    assert time.monotonic()<deadline;time.sleep(.01)
   client.sync(['Alice']);dump(case)
   absolute=game_time(server);assert absolute-start==slot+1,(case,absolute,start)
   timeline.append(dict(stage=case,absolute_clock=absolute,relative_clock=absolute-start))
  client.tell('step end');client.tell('quit Alice');client.end()
 finally:
  if client:
   try:dump('partial')
   except Exception:pass
   client.kill()
  server.stop();stop.set();log.close()
  (build/f'{label}-clocks.json').write_text(json.dumps(timeline,indent=2)+'\n')
 rows=[]
 for line in out.read_text().splitlines():
  if ' menu-wire ' in line:
   prefix,data=line.split(' menu-wire ',1);row=json.loads(data);row['stream']=prefix;rows.append(row)
 (build/f'{label}-decoded-packets.json').write_text(json.dumps(rows,indent=2)+'\n')
 facts=[]
 for index,(case,_) in enumerate(CASES):
  candidates=[]
  for row in rows:
   if f'step={case}' not in row['stream']:continue
   stacks=[row.get('item')] if 'item' in row else row.get('items',[])
   for value in stacks:
    if value:
     stack=nbt.read_named(bytes.fromhex(value['nbt']))
     if stack.get('id')=='minecraft:diamond_chestplate' and stack.get('components',{}).get('minecraft:custom_data',{}).get('keep')==284:
      if row.get('slot') in [index,index+36] or row.get('kind')=='content':candidates.append(stack)
  assert candidates,('missing actual packet stack',case)
  facts.append(dict(case=case,item=candidates[-1]['id'],count=candidates[-1]['count'],trim=candidates[-1].get('components',{}).get('minecraft:trim'),custom=candidates[-1].get('components',{}).get('minecraft:custom_data')))
 (build/f'{label}-facts.json').write_text(json.dumps(facts,indent=2)+'\n');return facts
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--world',type=Path,required=True);p.add_argument('--reference-only',action='store_true');a=p.parse_args();build=a.work_dir.resolve();build.mkdir(parents=True,exist_ok=False)
 (build/'predeclared.json').write_text(json.dumps(dict(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),cases=CASES,clock_deltas=[1,2,3],scope='Actual guest inventory packet holder/item/custom facts; all complete bytes/order/fields retained; no full packet-family equality'),indent=2)+'\n')
 _,_,java,native,client=net.sides(str(build.relative_to(net.lab.LOCAL)),a.binary.resolve())
 expected=run('java',java,client,build,a.world.resolve())
 assert isinstance(expected[0]['trim']['material'],dict) and expected[0]['trim']['material']['description']=='Inline material',expected
 assert expected[1]['trim']=={'material':'minecraft:gold','pattern':'minecraft:coast'},expected
 assert expected[2]['trim'] is None,expected
 if not a.reference_only:
  actual=run('native',native,client,build,a.world.resolve());assert actual==expected,(actual,expected)
 print('actual packet holder facts admitted')
