"""Versioned smallest current packet/actor/stat slice; immutable retained inputs live separately."""
import sys,os,json,struct,uuid,hashlib,time,tempfile,shutil
from pathlib import Path
REPO=Path(__file__).resolve().parents[3]
PAPER=Path(os.environ.get('HORIZON_STATISTIC_FRAMEWORK',str(REPO.parent/'paper-pumpkin-parity')))
sys.path.insert(0,str(REPO/'tools'));sys.path.insert(0,str(PAPER/'framework'))
from probe_harness import Probe
from observer import varint,read_varint
import importlib.util
spec=importlib.util.spec_from_file_location("local_observer_adapter",Path(__file__).with_name("observer_adapter.py"));adapter=importlib.util.module_from_spec(spec);spec.loader.exec_module(adapter)
Observer=adapter.LocalObserver
from integration_env import game_environment
import io
# Supervisor readiness/shutdown are distinct from the game RCON lifetime.
class CurrentProbe(Probe):
 def _stop(self, cleanup_name):
  if kind != 'horizon':return super()._stop(cleanup_name)
  import http.client
  endpoint=json.loads((self.work/'.horizon/api.json').read_text())
  host,port=endpoint['address'].rsplit(':',1)
  conn=http.client.HTTPConnection(host,int(port),timeout=30)
  conn.request('POST','/api/v1/stop','{}',{'Authorization':'Bearer '+endpoint['token'],'Content-Type':'application/json'})
  response=conn.getresponse();status=response.status;response.read();conn.close()
  assert status==200,status
  self.process.wait(timeout=60)
  self.record(cleanup_name,dict(pid=self.process.pid,exit_code=self.process.returncode,supervisor_stop_status=status))
  assert self.process.returncode==0
kind,binary,out,*extra=sys.argv[1:];binary=Path(binary).resolve();out=Path(out).resolve()
assert kind in ['vanilla','horizon']
scratch=Path(tempfile.mkdtemp(prefix='horizon-use-statistic-safe-'))
env=game_environment(os.environ,scratch)
probe=CurrentProbe(__file__,'vanilla',binary,out,observer=True,extra_env=env)
if kind=='horizon':
 supervisor=Path(extra[0]).resolve();probe.kind='horizon';probe.launch=[str(supervisor),'--dir',str(probe.work),'run']
 (probe.work/'server.toml').write_text(f'''[server]\nbind = "127.0.0.1"\nport = {probe.game}\nonline_mode = false\nwhitelist = false\nview_distance = 2\nsimulation_distance = 2\n[engine]\npath = {json.dumps(str(binary))}\nresources = {json.dumps(str(REPO/'.local/horizon-client/26.3'))}\nrestart_on_crash = false\n[backups]\nevery = "0"\n[panel]\nbind = ""\n''')
 pro=json.loads((out/'provenance.json').read_text());pro.update(kind=kind,supervisor=str(supervisor),supervisor_sha256=hashlib.file_digest(supervisor.open('rb'),'sha256').hexdigest(),launch=probe.launch);probe.record('provenance.json',pro)
# Keep ordinary damage/permission policy; no anticheat/actor/attribute bypass is installed.
ids={};ack=[0];inputs=[];packets=[];bodies=[];strict=[];clocks=[];seq=0
PLAYER='ParityObserver';player_uuid=uuid.UUID(bytes=hashlib.md5(('OfflinePlayer:'+PLAYER).encode()).digest(),version=3)
player_ints=[int.from_bytes(player_uuid.bytes[i:i+4],'big',signed=True) for i in range(0,16,4)]
def hook(pid,payload):
 if pid==1:
  r=io.BytesIO(payload);eid=read_varint(r.read);uid=uuid.UUID(bytes=r.read(16));ids[uid]=eid;packets.append({'id':pid,'payload':payload.hex(),'uuid':str(uid),'entity_id':eid})
 if pid==4:
  ack[0]=max(ack[0],read_varint(io.BytesIO(payload).read));packets.append({'id':pid,'payload':payload.hex(),'ack':ack[0]})
def check(label,actual,expected):
 strict.append(dict(label=label,actual=actual,expected=expected,passed=actual==expected))
 assert actual==expected,(label,actual,expected)
def cmd(text,label,expected=None,diagnostic=False):
 reply=probe.command(text,label)
 if expected is not None:check(label,reply,expected)
 if diagnostic:bodies.append(dict(label=label,command=text,reply=reply))
 return reply
def step(n,label):
 before=probe.game_time();probe.step(n);after=probe.game_time();check(label+'/delta',after-before,n);clocks.append(dict(label=label,before=before,after=after,delta=after-before))
def player(label,item):
 cmd(f'execute as {PLAYER} if entity @s[type=minecraft:player,gamemode=survival]',label+'/identity','Test passed. Count: 1')
 ints=','.join(map(str,player_ints))
 cmd(f'execute if data entity {PLAYER} {{UUID:[I;{ints}],Health:20f,foodLevel:20,Pos:[6.5d,200d,13.5d],SelectedItem:{{id:"minecraft:{item}",count:1}}}}',label+'/state','Test passed. Count: 1')
 cmd(f'execute at {PLAYER} if loaded ~ ~ ~',label+'/loaded','Test passed')
 cmd('execute if block 6 199 13 minecraft:stone',label+'/floor','Test passed')
 for field in ['UUID','Pos','Health','SelectedItem','Inventory']:
  cmd(f'data get entity {PLAYER} {field}',label+'/player/'+field,diagnostic=True)
 cmd(f'data get entity {PLAYER}',label+'/player/body',diagnostic=True)
def target(label,uid,health):
 cmd(f'execute as {uid} if entity @s[type=minecraft:pig]',label+'/type','Test passed. Count: 1')
 ints=','.join(str(int.from_bytes(uid.bytes[i:i+4],'big',signed=True)) for i in range(0,16,4))
 cmd(f'execute if data entity {uid} {{UUID:[I;{ints}],Health:{health}f,NoAI:1b,Pos:[6.5d,200d,12.5d]}}',label+'/live-state','Test passed. Count: 1')
 cmd(f'data get entity {uid} Health',label+'/health',f'Pig has the following entity data: {health:.1f}f')
 cmd(f'data get entity {uid}',label+'/body',diagnostic=True)
def hold(item):
 probe.command(f'clear {PLAYER}');probe.command(f'give {PLAYER} minecraft:{item} 1');observer.barrier()
def objective(name,item):
 cmd(f'scoreboard objectives add {name} minecraft.used:minecraft.{item}',name+'/add',f'Created new objective [{name}]')
 cmd(f'scoreboard players get {PLAYER} {name}',name+'/unset',f"Can't get value of {name} for {PLAYER}; none is set")
def attack(uid,label):
 assert uid in ids and ids[uid]>0,('actual entity-id admission',uid,ids)
 payload=varint(ids[uid]);observer.send(1,payload);inputs.append(dict(label=label,kind='attack',id=1,payload=payload.hex(),target_uuid=str(uid),entity_id=ids[uid]))
 # Declared benign held-item use marker follows attack on the SAME connection.
 # Both are action packets on the same ordered Game dispatch path.
 marker_seq=0 if label=='sword' else 2
 marker=varint(0)+varint(marker_seq)+struct.pack('>ff',180,0)
 observer.send(67,marker);observer.wait_exact_ack(marker_seq)
 inputs.append(dict(label=label+'/action-receipt-marker',kind='use_item_marker',id=67,payload=marker.hex(),sequence=marker_seq,actual_ack=marker_seq))
 observer.barrier()
def air_fail():
 global seq
 seq+=1;payload=varint(0)+varint(seq)+struct.pack('>ff',180,0);observer.send(67,payload)
 observer.wait_exact_ack(seq)
 observer.barrier();inputs.append(dict(label='stick-air-negative',kind='use_item',id=67,payload=payload.hex(),sequence=seq,ack=ack[0]))
phase_files=[];phase_times=[]
def record_file(label):
 path=probe.work/'world'/'players/stats'/(str(player_uuid)+'.json')
 row=dict(label=label,path=str(path),utc=__import__('datetime').datetime.now(__import__('datetime').timezone.utc).isoformat(),monotonic_ns=time.monotonic_ns(),exists=path.is_file())
 if row['exists']:
  raw=path.read_bytes();(out/(label+'-stats.raw.json')).write_bytes(raw);data=json.loads(raw);used=data.get('stats',{}).get('minecraft:used',{})
  row.update(sha256=hashlib.sha256(raw).hexdigest(),mtime_ns=path.stat().st_mtime_ns,bytes=len(raw),sword_present='minecraft:diamond_sword' in used,sword_value=used.get('minecraft:diamond_sword'))
 else:row.update(sword_present=None,sword_value=None)
 phase_files.append(row);return row

def connected_save(label,command,value):
 cmd(f'execute if entity @a[name={PLAYER}]',label+'/connected-before','Test passed. Count: 1')
 before=time.monotonic_ns();reply=cmd(command,label+'/completion','Saving the game (this may take a moment!)Saved the game')
 phase_times.append(dict(label=label,command=command,before_monotonic_ns=before,completion_monotonic_ns=time.monotonic_ns(),game_time=probe.game_time()))
 row=record_file(label)
 # Keep all actual missing/stale states and continue after target differences.
 strict.append(dict(label=label+'/saved-file-present',actual=row['exists'],expected=True,passed=row['exists'] is True))
 expected=dict(present=value is not None,value=value)
 actual=dict(present=row['sword_present'],value=row['sword_value'])
 strict.append(dict(label=label+'/saved-used',actual=actual,expected=expected,passed=actual==expected))
 cmd(f'execute if entity @a[name={PLAYER}]',label+'/connected-after','Test passed. Count: 1')
 observer.barrier()

closed_receipts=[]
def disconnect_positive(label,expected,rejoin):
 global observer
 before=record_file(label+'/before'.replace('/','-'))
 observer.barrier();observer.close();closed_receipts.extend(observer.protocol_receipts)
 deadline=time.monotonic()+30
 path=probe.work/'world'/'players/stats'/(str(player_uuid)+'.json')
 while True:
  absent=probe.command(f'execute unless entity @a[name={PLAYER}]')
  changed=path.is_file() and path.stat().st_mtime_ns!=before.get('mtime_ns')
  if absent=='Test passed' and changed:break
  assert time.monotonic()<deadline,('actual disconnect save missing',absent,changed)
  time.sleep(.01)
 row=record_file(label)
 check(label+'/positive',dict(exists=row['exists'],value=row['sword_value']),dict(exists=True,value=expected))
 if rejoin:
  ids.clear();observer=probe.observer=Observer(probe.game,'vanilla',packet_hook=hook);observer.barrier()

try:
 with probe.server():
  deadline=time.monotonic()+120
  while not probe.command('list').startswith('There are '):
   assert time.monotonic()<deadline,'actual game not ready';time.sleep(.05)
  probe.command('whitelist off');probe.command('difficulty peaceful');probe.command('gamerule minecraft:spawn_mobs false');probe.command('gamerule minecraft:advance_time false');probe.command('gamerule minecraft:random_tick_speed 0');probe.command('forceload add 0 0 15 15')
  deadline=time.monotonic()+120
  while probe.command('execute if loaded 6 200 12')!='Test passed':
   assert time.monotonic()<deadline,'arena not loaded';time.sleep(.02)
  probe.command('fill 0 199 0 15 199 15 minecraft:stone strict');probe.command('fill 0 200 0 15 204 15 minecraft:air strict')
  observer=probe.observer=Observer(probe.game,'vanilla',packet_hook=hook)
  probe.command(f'gamemode survival {PLAYER}');probe.command(f'tp {PLAYER} 6.5 200 13.5 180 0');probe.command('tick freeze');observer.barrier()
  record_file('initial-disk')
  for index,uid in enumerate([uuid.UUID('00000005-0000-0005-0000-000500000005'),uuid.UUID('00000005-0000-0005-0000-000500000006')],1):
   hold('diamond_sword');cmd(f'summon minecraft:pig 6.5 200 12.5 {{NoAI:1b,Silent:1b,UUID:[I;5,5,5,{4+index}]}}',f'hit{index}/summon','Summoned new Pig');step(2,f'hit{index}/setup')
   if index==1:
    objective('mini_sword','diamond_sword');player('initial','diamond_sword');target('initial',uid,10)
    connected_save('initial-save','save-all',None);connected_save('initial-flush','save-all flush',None)
   player(f'hit{index}/before','diamond_sword');target(f'hit{index}/before',uid,10)
   attack(uid,'sword' if index==1 else 'second-sword');step(2,f'hit{index}/attack');target(f'hit{index}/after',uid,3);player(f'hit{index}/after','diamond_sword')
   cmd(f'scoreboard players get {PLAYER} mini_sword',f'hit{index}/live-used',f'{PLAYER} has {index} [mini_sword]')
   connected_save(f'hit{index}-save','save-all',index);connected_save(f'hit{index}-flush','save-all flush',index);connected_save(f'hit{index}-no-new-stat-flush','save-all flush',index)
   disconnect_positive(f'hit{index}-disconnect',index,index==1)
   if index==1:probe.command('kill @e[type=!minecraft:player]')
finally:
 for name,value in [('strict-checks.json',strict),('raw-bodies.json',bodies),('inputs.json',inputs),('received-input-related-packets.json',packets),('clocks.json',clocks),('phase-files.json',phase_files),('phase-times.json',phase_times),('protocol-receipts.json',closed_receipts+(getattr(locals().get('observer'),'protocol_receipts',[]) if not getattr(locals().get('observer'),'stopped',True) else []))]:probe.record(name,value)
 print(json.dumps(dict(kind=kind,checks=len(strict),passed=sum(x['passed'] for x in strict),world_ticks=sum(x['delta'] for x in clocks))),flush=True)
