"""Survival boat-holder walk/stop probe; see docs/parity/boat-holder-movement.md."""
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
from behavior_snbt import parse_snbt
import re
Y=100;BOAT=uuid.UUID('00000007-0000-0007-0000-000700000007');PLAYER='ParityObserver'
PU=uuid.UUID(bytes=hashlib.md5(('OfflinePlayer:'+PLAYER).encode()).digest(),version=3)
PINTS=[int.from_bytes(PU.bytes[i:i+4],'big',signed=True) for i in range(0,16,4)]
ids={};strict=[];raw=[];inputs=[];packets=[];steps=[];markers=[];projections=[];phase='setup';seq=0;expected_x=.5
original=[{'command': 'execute if data entity @e[type=minecraft:oak_boat,limit=1] leash', 'reply': 'Test passed. Count: 1', 'label': 'tied/has leash'}, {'command': 'pos', 'reply': [0.5, 100.0, 2.5], 'label': 'walk/boat after 10 ticks'}, {'command': 'pos', 'reply': [1.9168, 100.0, 2.0625], 'label': 'walk/boat after 20 ticks'}, {'command': 'pos', 'reply': [5.1811, 100.0, 1.485], 'label': 'walk/boat after 30 ticks'}, {'command': 'pos', 'reply': [6.0676, 100.0, 1.3736], 'label': 'stop/boat after 10 still ticks'}]
def hook(pid,payload):
 if pid in (1,4,50,73,101):
  row=dict(id=pid,payload=payload.hex(),phase=phase,monotonic_ns=time.monotonic_ns());packets.append(row)
  if pid==1:
   f=io.BytesIO(payload);eid=read_varint(f.read);uid=uuid.UUID(bytes=f.read(16));ids[uid]=eid;row.update(entity_id=eid,uuid=str(uid))
def check(label,actual,expected,target=False):
 strict.append(dict(label=label,actual=actual,expected=expected,passed=actual==expected,target=target))
 if not target:assert actual==expected,(label,actual,expected)
def cmd(text,label):return probe.command(text,label)
def body(who,label):
 s=cmd('data get entity '+who,label);raw.append(dict(label=label,body=s));assert ' data: ' in s,(label,s);return parse_snbt(s.split(' data: ',1)[1]),s
def player(label,await_position=False):
 deadline=time.monotonic()+20
 while True:
  p,s=body(PLAYER,label+'/player')
  if not await_position or p.get('Pos')==[expected_x,100.,.5]:break
  assert time.monotonic()<deadline,('accepted position prerequisite',label,p.get('Pos'));time.sleep(.002)
 check(label+'/UUID',p['UUID'],PINTS);check(label+'/HP',p['Health'],20.);check(label+'/mode',p['playerGameType'],0);check(label+'/position',p['Pos'],[expected_x,100.,.5]);check(label+'/type',cmd('execute as '+str(PU)+' if entity @s[type=minecraft:player]',label+'/type'),'Test passed. Count: 1');return p
def empty(p,label):
 slot=p['SelectedItemSlot'];active=[x for x in p['Inventory'] if x.get('Slot')==slot and x.get('count',0)>0];check(label+'/selected-slot',slot,0);check(label+'/actual-mainhand-empty',active,[]);check(label+'/noSelectedItem','SelectedItem' in p,False)
def relation(label,tied):
 b,s=body(str(BOAT),label+'/boat');check(label+'/boat-UUID',b['UUID'],[7,7,7,7]);check(label+'/boat-type',cmd('execute as '+str(BOAT)+' if entity @s[type=minecraft:oak_boat]',label+'/boat-type'),'Test passed. Count: 1');check(label+'/live',cmd('execute as '+str(BOAT)+' on leasher if entity @s[name=ParityObserver]',label+'/live'),'Test passed. Count: 1' if tied else '');check(label+'/saved-holder',b.get('leash'),{'UUID':PINTS} if tied else None)
 if tied:check(label+'/typed-holder','leash: {UUID: [I;' in s,True)
 return b

def marker(label):
 global seq
 seq+=1;p=player(label+'/before',await_position=True);empty(p,label+'/before');b=relation(label+'/before',True);rot=p['Rotation'];payload=varint(0)+varint(seq)+struct.pack('>ff',*rot);observer.send(67,payload);start=time.monotonic_ns();observer.wait_exact_ack(seq)
 q=player(label+'/after');empty(q,label+'/after');c=relation(label+'/after',True)
 for k in ['Rotation','Pos','Inventory','SelectedItemSlot']:check(label+'/unchanged-'+k,q[k],p[k])
 check(label+'/boat-before-step-Pos',c['Pos'],b['Pos']);check(label+'/boat-before-step-Motion',c['Motion'],b['Motion'])
 inputs.append(dict(label=label,kind='DECLARED_EMPTY_HAND_RECEIPT_MARKER',id=67,payload=payload.hex(),sequence=seq,orientation=rot,exact_ack=seq));markers.append(dict(label=label,sequence=seq,start_ns=start,receipt_ns=time.monotonic_ns(),before=p,after=q,Java_active_use_observed=None,Java_empty_return_source_only=True))
def step(n,label):
 before=probe.game_time();probe.step(n);after=probe.game_time();check(label+'/delta',after-before,n)
 if steps:check(label+'/continuity',before,steps[-1]['after'])
 steps.append(dict(label=label,requested=n,before=before,after=after));p=player(label);b=relation(label,len(steps)>1);return b
# probe.step's final barrier is redundant after exact marker ACK and would spam native /list.
def step_exact(n):
 if probe._fast_rate!=1000:probe.command('tick rate 1000');probe._fast_rate=1000
 before=probe.game_time();probe.command('tick step '+str(n));deadline=time.monotonic()+60
 while probe.game_time()<before+n:
  assert time.monotonic()<deadline;time.sleep(.002)
probe.step=step_exact
try:
 with probe.server():
  deadline=time.monotonic()+120
  while not probe.command('list').startswith('There are '):assert time.monotonic()<deadline;time.sleep(.05)
  for c in ['whitelist off','gamerule minecraft:advance_time false','forceload add -16 -16 47 15']:probe.command(c)
  deadline=time.monotonic()+120
  while probe.command('execute if loaded 47 100 15')!='Test passed':assert time.monotonic()<deadline;time.sleep(.02)
  probe.command('fill -16 99 -16 47 99 15 minecraft:stone');probe.command('fill -16 100 -16 47 104 15 minecraft:air');probe.command('kill @e[type=!minecraft:player]')
  observer=probe.observer=Observer(probe.game,'vanilla',packet_hook=hook,native_ping=(kind=='vanilla'));probe.command('gamemode survival '+PLAYER);probe.command('tick freeze');probe.command('tp '+PLAYER+' .5 100 .5 0 0');observer.barrier()
  for label,c in [('min','execute if loaded -16 100 -16'),('max','execute if loaded 47 100 15'),('floor','execute if block 0 99 0 minecraft:stone')]:check('setup/'+label,cmd(c,'setup/'+label),'Test passed')
  check('summon',cmd('summon minecraft:oak_boat .5 100 2.5 {UUID:[I;7,7,7,7]}','summon'),'Summoned new Oak Boat');player('initial');relation('initial',False);step(2,'setup-step')
  probe.command('item replace entity '+PLAYER+' weapon.mainhand with minecraft:lead');observer.barrier();p=player('attach/before');check('attach/original-lead-count',p['SelectedItem']['count'],1);check('attach/original-lead-id',p['SelectedItem']['id'],'minecraft:lead');deadline=time.monotonic()+20
  while BOAT not in ids:
   assert time.monotonic()<deadline,'actual UUID-bound boat spawn packet missing';time.sleep(.002)
  phase='attach';payload=varint(ids[BOAT])+varint(0)+varint(0)+bytes([0]);observer.send(26,payload);inputs.append(dict(label=phase,id=26,payload=payload.hex(),boat_uuid=str(BOAT),actual_entity_id=ids[BOAT]))
  # Attach itself has no sequence ACK; the following declared marker is its causal receipt.
  # Link must already be resolved before the marker's independent relation guard.
  # A Java native ping/native setup-only list processes the attach before admitting its state.
  observer.barrier();marker('attach-marker');step(1,'attach-step')
  reply=cmd('execute if data entity @e[type=minecraft:oak_boat,limit=1] leash','tied/has leash');projections.append(dict(command=original[0]['command'],reply=reply,label=original[0]['label']));check('original/tied',reply,original[0]['reply'],True)
  for tick in range(1,31):
   phase='walk/'+str(tick);expected_x+=.4;move=struct.pack('>dddB',expected_x,100.,.5,1);observer.send(30,move);observer.send(13);inputs.extend([dict(label=phase,id=30,payload=move.hex()),dict(label=phase,id=13,payload='')]);marker(phase+'/marker');b=step(1,phase)
   if tick%10==0:
    want=original[tick//10];got=[round(v,4) for v in b['Pos']];projections.append(dict(command='pos',reply=got,label=want['label']));check('original/'+want['label'],got,want['reply'],True)
  for tick in range(1,11):
   phase='still/'+str(tick);observer.send(13);inputs.append(dict(label=phase,id=13,payload=''));marker(phase+'/marker');b=step(1,phase)
  want=original[4];got=[round(v,4) for v in b['Pos']];projections.append(dict(command='pos',reply=got,label=want['label']));check('original/'+want['label'],got,want['reply'],True)
  check('original-count',len(projections),5);check('moves',sum(x['id']==30 for x in inputs),30);check('tick-ends',sum(x['id']==13 for x in inputs),40);check('markers',len(markers),41);check('world-ticks',sum(x['requested'] for x in steps),43)
finally:
 for name,value in [('strict-checks.json',strict),('raw-bodies.json',raw),('inputs.json',inputs),('packets.json',packets),('steps.json',steps),('markers.json',markers),('original-projections.json',projections),('protocol-receipts.json',getattr(locals().get('observer'),'protocol_receipts',[]))]:probe.record(name,value)
 print(json.dumps(dict(kind=kind,checks=len(strict),passed=sum(x['passed'] for x in strict),world_ticks=sum(x['requested'] for x in steps))),flush=True)
