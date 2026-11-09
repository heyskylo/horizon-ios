import argparse,hashlib,json,os,re,shutil,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import anvil_dump as nbt
import test_locator_bar_parity as net
import net_capture as wire
from test_disconnect_crafting_parity import generate,offline,uuid_words,wait_new
PLAYERS=['Alice','Bob','Carol']
def program():
 ops=[]
 def cmd(text,label,who='Alice',guard=None,trace=False):
  ops.append(dict(op='cmd',cmd=text,label=label,who=who,guard=guard,trace=trace))
 def action(text,label,who='Alice'):
  for field,typ in [('result',''),('success','b')]:cmd(f'data modify storage h082:proof {label}.{field} set value -99{typ}',label+'-reset-'+field,who)
  cmd(f'execute store result storage h082:proof {label}.result int 1 store success storage h082:proof {label}.success byte 1 run '+text,label+'-action',who)
  for field in ['result','success']:cmd(f'data get storage h082:proof {label}.{field}',label+'-'+field,who)
 def witness(phase):
  for i,name in enumerate(PLAYERS):
   words=','.join(map(str,uuid_words(name)))
   cmd(f'execute if entity @a[name={name},type=player,nbt={{UUID:[I;{words}],Health:20f,Pos:[{4.5+i*3}d,100d,4.5d],playerGameType:1}}]',phase+'-'+name+'-identity',guard='Test passed')
   cmd(f'execute if loaded {4+i*3} 100 4',phase+'-'+name+'-loaded',guard='Test passed')
   cmd(f'execute if block {4+i*3} 99 4 stone',phase+'-'+name+'-floor',guard='Test passed')
   cmd(f'data get entity {name} UUID',phase+'-'+name+'-uuid')
   cmd(f'data get entity {name}',phase+'-'+name+'-body',trace=True)
   action('attribute @s minecraft:armor base get',phase+'-'+name+'-actual-base',name)
  cmd('execute store result score now h082 run time query gametime',phase+'-absolute-clock',trace=True)
  cmd('scoreboard players operation now h082 -= start h082',phase+'-clock-operation')
  cmd('scoreboard players get now h082',phase+'-relative-clock')
 for name in PLAYERS:
  cmd(f'gamemode creative {name}','setup-mode-'+name)
  cmd(f'tp {name} {4.5+PLAYERS.index(name)*3} 100 4.5 0 0','setup-pos-'+name)
 cmd('gamerule show_advancement_messages false','no-advancements')
 cmd('summon pig 13.5 100 4.5 {UUID:[I;82,0,0,1],NoAI:1b,NoGravity:1b,Health:10f}','pig-summon',guard='Summoned')
 cmd('execute if entity @e[type=pig,nbt={UUID:[I;82,0,0,1],Health:10f,NoAI:1b,NoGravity:1b,Pos:[13.5d,100d,4.5d]}]','pig-admission',guard='Test passed')
 cmd('scoreboard objectives add h082 dummy','clock-objective')
 cmd('execute store result score start h082 run time query gametime','absolute-start',trace=True)
 witness('initial')
 for text,label in [('base get','owner-get'),('base set 4','owner-set'),('base get','owner-get4')]:action('attribute Bob minecraft:armor '+text,label)
 witness('owner4')
 action('attribute Bob minecraft:armor base reset','owner-reset');witness('reset0')
 for text,label in [('base set 3','guest-set'),('base get','guest-get3')]:action('attribute @s minecraft:armor '+text,label,'Bob')
 action('attribute Bob minecraft:armor base get','owner-reads-guest3');witness('guest3')
 action('attribute @s minecraft:armor base reset','guest-reset','Bob')
 for text,label in [('base set 2','host-set'),('base get','host-get2'),('base reset','host-reset')]:action('attribute @s minecraft:armor '+text,label)
 for text,label in [('base get','mob-get'),('base set 2','mob-set'),('base get','mob-get2'),('base reset','mob-reset')]:action('attribute 00000052-0000-0000-0000-000000000001 minecraft:armor '+text,label)
 witness('final')
 return ops

def run(label,command,client_command,build,world,ops):
 directory=build/label;port=net.server_dir(directory,world);server=net.Server(command,directory,label)
 packet=build/(label+'-packets.jsonl');proxy=net.free_port();stop=wire.start_background(proxy,f'127.0.0.1:{port}',packet,full=True)
 transcript=build/(label+'-wire.txt');log=(build/(label+'-client.log')).open('w');client=None;clocks={}
 def save_clock(name):
  client.tell('step _save_'+name);start=len(server.lines);server.console('save-all flush');wait_new(server,start,'Saved the game');client.sync(PLAYERS)
  p=directory/'world/level.dat';shutil.copy2(p,build/(label+'-'+name+'-level.dat'));clocks[name]=nbt.read_dat(p)['Data']['Time']
 try:
  server.wait_for(r'Done \(');start=len(server.lines);server.console('tick freeze');wait_new(server,start,'frozen')
  client=net.Client(client_command,proxy,transcript,log);client.tell('chat on');client.tell('flat-chat on');client.tell('state on');client.tell('players on')
  for name in PLAYERS:client.tell('join '+name);client.sync([name]);client.tell('fly '+name+' on');client.sync([name])
  save_clock('start')
  for op in ops:
   client.tell('step '+op['label']);client.tell(f'send-cmd {op["who"]} '+op['cmd']);client.sync(PLAYERS)
  save_clock('end');client.end()
 finally:
  if client:
   if client.process.poll() is None:
    try:client.end()
    except Exception:client.kill()
  server.stop();log.close();stop.set()
 storage=[p for p in (directory/'world').rglob('*command_storage*.dat') if 'h082' in p.parts or 'h082' in p.name];assert len(storage)==1,storage
 body=nbt.read_dat(storage[0]);(build/(label+'-storage.json')).write_text(json.dumps(body,indent=2)+'\n');proof=body['data']['contents']['proof']
 grouped={op['label']:{name:[] for name in PLAYERS} for op in ops}
 for line in transcript.read_text().splitlines():
  if ' chat-flat ' not in line:continue
  who,rest=line.split(' step=',1);step,raw=rest.split(' chat-flat ',1)
  if step in grouped:grouped[step][who].append(json.loads(raw))
 facts={}
 for op in ops:
  if not op['trace']:facts[op['label']+'/feedback']=grouped[op['label']]
  if op['guard']:
   plain=''.join(seg[0] for message in grouped[op['label']][op['who']] for seg in message)
   assert op['guard'] in plain,(op,grouped[op['label']])
 facts['actual-callback-stores']=proof
 facts['world-clock']={'start_type':clocks['start'][0],'end_type':clocks['end'][0],'actual_delta':clocks['end'][1]-clocks['start'][1]};assert facts['world-clock']=={'start_type':'l','end_type':'l','actual_delta':0},clocks
 (build/(label+'-absolute-clocks.json')).write_text(json.dumps(clocks,indent=2)+'\n');(build/(label+'-facts.json')).write_text(json.dumps(facts,indent=2)+'\n');return json.loads((build/(label+'-facts.json')).read_text())

def main():
 p=argparse.ArgumentParser();p.add_argument('--side',choices=['java','native'],required=True);p.add_argument('--label');p.add_argument('--binary',type=Path,required=True);p.add_argument('--reference',type=Path);a=p.parse_args()
 net.PLAYERS=PLAYERS;net.PROPERTIES.update({'force-gamemode':'false','view-distance':'2','simulation-distance':'2'})
 _,build,java,native,client=net.sides('h082-connected-guest-attributes-v5',a.binary.resolve());ops=program();script=build/'program.json';text=json.dumps(ops,indent=2)+'\n'
 if script.exists():assert script.read_text()==text
 else:script.write_text(text)
 (build/'declaration.json').write_text(json.dumps({'program_sha256':hashlib.sha256(script.read_bytes()).hexdigest(),'scope':'Actual Alice-issued direct Bob base commands, Bob self commands, Alice self and actual Mob controls. Carol isolation. All resolved feedback/styles, callback/store types and actual clocks strict. Full bodies, packets and absolute clocks raw. No no-entity-console attribute runtime claim.','ops':len(ops)},indent=2)+'\n')
 world=build/'generate/world'
 if not world.exists():world=generate(java,build)
 label=a.label or a.side;actual=run(label,java if a.side=='java' else native,client,build,world,ops)
 if a.reference:
  ref=json.loads(a.reference.read_text());assert ref.keys()==actual.keys();d=[dict(label=k,expected=ref[k],actual=actual[k]) for k in ref if ref[k]!=actual[k]];(build/(label+'-report.json')).write_text(json.dumps(dict(records=len(ref),passed=len(ref)-len(d),differences=d),indent=2)+'\n');print(len(ref)-len(d),'/',len(ref));raise SystemExit(bool(d))
 print(len(actual),'records captured')
if __name__=='__main__':main()
