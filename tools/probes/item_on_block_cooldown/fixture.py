"""Versioned smallest current packet/actor/stat slice; immutable retained inputs live separately."""
import sys,os,json,struct,uuid,hashlib,time,tempfile,shutil
from pathlib import Path
REPO=Path(__file__).resolve().parents[3]
PAPER=Path(os.environ.get('HORIZON_STATISTIC_FRAMEWORK',str(REPO.parent/'paper-pumpkin-parity')))
sys.path.insert(0,str(REPO/'tools'));sys.path.insert(0,str(PAPER/'framework'))
from probe_harness import Probe
from observer import varint,read_varint
import importlib.util
spec=importlib.util.spec_from_file_location("local_observer_adapter",Path(__file__).with_name("observer_adapter_v2.py"));adapter=importlib.util.module_from_spec(spec);spec.loader.exec_module(adapter)
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
# v1: four retained real use-on packets; same sequence and payloads; no synthetic action.
strict=[];bodies=[];inputs=[];packets=[];clocks=[];times=[];phase='setup'
PLAYER='ParityObserver';pu=uuid.UUID(bytes=hashlib.md5(('OfflinePlayer:'+PLAYER).encode()).digest(),version=3)
ints=','.join(str(int.from_bytes(pu.bytes[i:i+4],'big',signed=True)) for i in range(0,16,4))
def hook(pid,payload):
 if pid in (4,22):
  row=dict(id=pid,payload=payload.hex(),phase=phase,monotonic_ns=time.monotonic_ns())
  if pid==22:
   r=io.BytesIO(payload);n=read_varint(r.read);row.update(group=r.read(n).decode(),duration=read_varint(r.read));assert not r.read()
  packets.append(row)
def check(label,actual,expected,target=False):
 strict.append(dict(label=label,actual=actual,expected=expected,passed=actual==expected,target=target))
 if not target:assert actual==expected,(label,actual,expected)
def cmd(text,label,expected=None,target=False):
 reply=probe.command(text,label)
 if expected is not None:check(label,reply,expected,target)
 return reply
def body(label):
 bodies.append(dict(label=label,reply=cmd('data get entity '+PLAYER,label+'/full-body')))
def state(label,item,count,group,target=False):
 body(label+'/early-diagnostic')
 cmd(f'execute as {PLAYER} if entity @s[type=minecraft:player,gamemode=survival]',label+'/identity','Test passed. Count: 1')
 cmd(f'execute if data entity {PLAYER} {{UUID:[I;{ints}],Health:20f,Pos:[4.5d,200d,4.5d]}}',label+'/actor','Test passed. Count: 1')
 cmd(f'execute at {PLAYER} if loaded ~ ~ ~',label+'/loaded','Test passed')
 cmd('execute if block 4 199 4 minecraft:stone',label+'/actor-floor','Test passed')
 stack='{id:"minecraft:'+item+'",count:'+str(count)+',components:{"minecraft:use_cooldown":{seconds:30f,cooldown_group:"probe:'+group+'"}}}'
 cmd('execute if data entity '+PLAYER+' {SelectedItem:'+stack+'}',label+'/typed-stack','Test passed. Count: 1',target)
 cmd('data get entity '+PLAYER+' SelectedItem',label+'/stack-full');body(label)
def hold(item,group,count):
 probe.command('clear '+PLAYER);probe.command('item replace entity '+PLAYER+' weapon.mainhand with minecraft:'+item+'[minecraft:use_cooldown={seconds:30.0f,cooldown_group:"probe:'+group+'"}] '+str(count));observer.barrier()
def block(label,x,kind,target=False):
 cmd(f'execute if block {x} 199 6 minecraft:stone',label+'/support','Test passed')
 cmd(f'execute if block {x} 200 6 minecraft:'+kind,label+'/'+kind,'Test passed',target)
 cmd(f'execute unless block {x} 200 6 minecraft:'+('air' if kind=='oak_planks' else 'oak_planks'),label+'/complement','Test passed',target)
def use(x,sequence,label):
 global phase
 phase=label
 payload=varint(0)+struct.pack('>Q',((x&0x3ffffff)<<38)|(6<<12)|199)+varint(1)+struct.pack('>fff',.5,1,.5)+bytes(2)+varint(sequence)
 before=probe.game_time();start=time.monotonic_ns();observer.send(66,payload);observer.wait_exact_ack(sequence)
 # Explicit normal-rate one-world-tick observation. Player clocks may advance between steps.
 probe.command('tick step 1');deadline=time.monotonic()+20
 while probe.game_time()<before+1:
  assert time.monotonic()<deadline;time.sleep(.002)
 after=probe.game_time();check(label+'/step',after-before,1)
 inputs.append(dict(label=label,id=66,payload=payload.hex(),sequence=sequence,exact_ack=True,start_ns=start,end_ns=time.monotonic_ns(),world_before=before,world_after=after));clocks.append(dict(label=label,before=before,after=after))
try:
 with probe.server():
  deadline=time.monotonic()+120
  while not probe.command('list').startswith('There are '):
   assert time.monotonic()<deadline;time.sleep(.05)
  for c in ['whitelist off','difficulty peaceful','gamerule minecraft:advance_time false','gamerule minecraft:spawn_mobs false','gamerule minecraft:random_tick_speed 0','forceload add 0 0 31 31']:probe.command(c)
  deadline=time.monotonic()+120
  while probe.command('execute if loaded 7 200 6')!='Test passed':
   assert time.monotonic()<deadline;time.sleep(.02)
  probe.command('fill 0 198 0 31 199 31 minecraft:stone strict');probe.command('fill 0 200 0 31 206 31 minecraft:air strict')
  observer=probe.observer=Observer(probe.game,'vanilla',packet_hook=hook,native_ping=(kind=='vanilla'))
  probe.command('gamemode survival '+PLAYER);probe.command('tp '+PLAYER+' 4.5 200 4.5 0 0');probe.command('tick freeze');probe.command('tick rate 20');observer.barrier()
  for x in [5,6,7]:block('initial'+str(x),x,'air')
  hold('oak_planks','group',5);state('first/before','oak_planks',5,'group');interval_start=time.monotonic_ns()
  use(5,1,'first');block('first/after',5,'oak_planks');state('first/after','oak_planks',4,'group')
  state('same/before','oak_planks',4,'group');use(6,2,'same');block('same/after',6,'air',True);state('same/after','oak_planks',4,'group',True)
  interval_end=time.monotonic_ns();elapsed=(interval_end-interval_start)/1e9;check('same/non-expiry-elapsed-under10s',elapsed<10,True);times.append(dict(start_ns=interval_start,end_ns=interval_end,elapsed_seconds=elapsed,configured_seconds=30,tick_rate=20,world_ticks=2,limit_seconds=10))
  hold('stick','other',1);state('failed/before','stick',1,'other');use(7,3,'failed');block('failed/after',7,'air');state('failed/after','stick',1,'other')
  hold('oak_planks','other',5);state('other/before','oak_planks',5,'other');use(7,4,'other');block('other/after',7,'oak_planks');state('other/after','oak_planks',4,'other')
  check('exact-world-ticks',sum(c['after']-c['before'] for c in clocks),4)
finally:
 for name,value in [('strict-checks.json',strict),('raw-bodies.json',bodies),('inputs.json',inputs),('packets.json',packets),('clocks.json',clocks),('times.json',times),('protocol-receipts.json',getattr(locals().get('observer'),'protocol_receipts',[]))]:probe.record(name,value)
 print(json.dumps(dict(kind=kind,checks=len(strict),passed=sum(x['passed'] for x in strict))),flush=True)
