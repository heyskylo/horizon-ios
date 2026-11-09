#!/usr/bin/env python3
"""Two unused feature startup cases; complete logs and independent readiness/service/exit facts."""
import argparse,datetime,hashlib,json,os,socket,struct,subprocess,sys,threading,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
import lab
META='{"pack":{"description":"H259 unused feature","min_format":[121,0],"max_format":[121,0]}}\n'

def sha(b):return hashlib.sha256(b).hexdigest()
def receive(s,n):
 b=b''
 while len(b)<n:
  q=s.recv(n-len(b))
  if not q:raise EOFError('RCON closed before complete frame')
  b+=q
 return b

def rcon(port,commands):
 facts=[]
 with socket.create_connection(('127.0.0.1',port),timeout=5) as s:
  s.settimeout(10)
  for i,text in enumerate(['h259-fixture-password']+commands,1):
   body=struct.pack('<ii',i,3 if i==1 else 2)+text.encode()+b'\0\0';packet=struct.pack('<i',len(body))+body;s.sendall(packet)
   responses=[]
   while True:
    header=receive(s,4);length=struct.unpack('<i',header)[0];assert 10<=length<=2**20,length
    payload=receive(s,length);rid,kind=struct.unpack('<ii',payload[:8]);assert payload[-2:]==b'\0\0'
    responses.append(dict(id=rid,kind=kind,text=payload[8:-2].decode(),raw=(header+payload).hex()))
    if i!=1 or kind==2:break
   assert responses[-1]['id']==i,responses
   facts.append(dict(command='AUTH' if i==1 else text,responses=responses))
 return facts

def run(kind,case,parent,binary,timeout):
 out=parent/f'{kind}-{case}';out.mkdir(parents=True,exist_ok=False)
 game=out/'game';game.mkdir();pack=game/'world/datapacks/h259';entry=pack/'data/h259/worldgen/feature/unused_probe.json';entry.parent.mkdir(parents=True)
 data=(json.dumps(dict(type='minecraft:fill_layer',height=0 if case=='valid' else -1,state='minecraft:stone'),separators=(',',':'))+'\n').encode()
 entry.write_bytes(data);(pack/'pack.mcmeta').write_text(META)
 port,rport=lab.free_port(),lab.free_port()
 props={'server-ip':'127.0.0.1','server-port':port,'online-mode':'false','level-name':'world','level-seed':259,'level-type':'minecraft:flat','generator-settings':json.dumps({'layers':[{'block':'minecraft:bedrock','height':1},{'block':'minecraft:dirt','height':2},{'block':'minecraft:grass_block','height':1}],'biome':'minecraft:plains','features':False,'lakes':False,'structure_overrides':[]},separators=(',',':')),'generate-structures':'false','initial-enabled-packs':'vanilla,file/h259','view-distance':2,'simulation-distance':2,'enable-rcon':'true','rcon.port':rport,'rcon.password':'h259-fixture-password','max-tick-time':-1,'pause-when-empty-seconds':-1}
 (game/'server.properties').write_text(''.join(f'{k}={v}\n' for k,v in props.items()));(game/'eula.txt').write_text('eula=true\n')
 command=([str(Path('/usr/lib/jvm/java-25-openjdk-amd64/bin/java')),'-Xms256M','-Xmx2G','-jar',str(ROOT/'.local/minecraft/26.3/jars/server.jar'),'--nogui'] if kind=='java' else [str(binary),'--resources',str(ROOT/'.local/horizon-client/26.3'),'--dedicated','--nogui'])
 env=lab.game_env(HORIZON_HOME=out/'homes/horizon',HORIZON_LAUNCHER_HOME=out/'homes/launcher',XDG_DATA_HOME=out/'homes/xdg-data',XDG_CONFIG_HOME=out/'homes/xdg-config');start=time.monotonic();facts=dict(start_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),kind=kind,case=case,command=command,timeout_seconds=timeout,input_sha256=sha(data),metadata_sha256=sha(META.encode()),ready=False,service=False,timed_out=False,probe_termination=[],return_code=None,reaped=False,ready_seconds=None,service_seconds=None,exit_seconds=None,rcon=[],unused={'custom_files':[str(entry.relative_to(pack))],'no_references':True,'flat_features':False,'flat_lakes':False,'structures':False})
 (out/'inputs.json').write_text(json.dumps(facts,indent=2))
 lines=[];log=(out/'server.log').open('w');p=subprocess.Popen(command,cwd=game,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,encoding='utf-8',errors='replace',start_new_session=True)
 def pump():
  for line in p.stdout:
   lines.append(line);log.write(line);log.flush()
 thread=threading.Thread(target=pump);thread.start()
 try:
  while time.monotonic()-start<timeout and p.poll() is None:
   if any('Done (' in line and 'For help' in line for line in lines):
    facts['ready']=True;facts['ready_seconds']=time.monotonic()-start;break
   time.sleep(.1)
  if not facts['ready'] and p.poll() is None:facts['timed_out']=True
  if facts['ready']:
   commands=['datapack list enabled','scoreboard objectives add h259 dummy','scoreboard players set probe h259 259','scoreboard players get probe h259','time query gametime','execute if score probe h259 matches 259 run say H259_SERVICE_OK']
   offset=len(lines);facts['console_requests']=commands
   for text in commands:p.stdin.write(text+'\n')
   p.stdin.flush();deadline=time.monotonic()+20
   while time.monotonic()<deadline and p.poll() is None:
    if any(line.rstrip().endswith('[Server] H259_SERVICE_OK') for line in lines[offset:]):break
    time.sleep(.1)
   facts['console_reply_lines']=lines[offset:].copy()
   assert any(line.rstrip().endswith('[Server] H259_SERVICE_OK') for line in facts['console_reply_lines']),facts['console_reply_lines']
   assert any('file/h259' in line for line in facts['console_reply_lines']),facts['console_reply_lines']
   facts['service']=True;facts['service_seconds']=time.monotonic()-start
 except Exception as e:facts['probe_error']=repr(e)
 finally:
  if p.poll() is None:
   facts['probe_termination'].append(dict(action='console stop',seconds=time.monotonic()-start))
   try:p.stdin.write('stop\n');p.stdin.flush();p.wait(timeout=45)
   except (BrokenPipeError,subprocess.TimeoutExpired):
    facts['probe_termination'].append(dict(action='kill',seconds=time.monotonic()-start));p.kill();p.wait(timeout=10)
  else:p.wait()
  facts['return_code']=p.returncode;facts['reaped']=True;facts['exit_seconds']=time.monotonic()-start;thread.join(timeout=10);assert not thread.is_alive();log.close()
  facts['log_sha256']=sha((out/'server.log').read_bytes());assert entry.read_bytes()==data
  (out/'outcome.json').write_text(json.dumps(facts,indent=2)+'\n')
 print(kind,case,json.dumps({k:facts[k] for k in ['ready','service','timed_out','return_code','reaped','ready_seconds','exit_seconds']}),flush=True)
 return facts

def main():
 a=argparse.ArgumentParser();a.add_argument('--kind',choices=['java','native'],required=True);a.add_argument('--output',type=Path,required=True);a.add_argument('--binary',type=Path);a.add_argument('--timeout',type=float,default=240);a.add_argument('--verify-only',action='store_true');o=a.parse_args()
 results=([json.loads((o.output/f'{o.kind}-{c}/outcome.json').read_text()) for c in ['valid','invalid']] if o.verify_only else [run(o.kind,c,o.output.resolve(),o.binary,o.timeout) for c in ['valid','invalid']])
 for c,facts in zip(['valid','invalid'],results):
  folder=o.output/f'{o.kind}-{c}';assert sha((folder/'server.log').read_bytes())==facts['log_sha256'];assert sha((folder/'game/world/datapacks/h259/data/h259/worldgen/feature/unused_probe.json').read_bytes())==facts['input_sha256'];assert facts['reaped']
 good,bad=results
 assert good['ready'] and good['service'] and not good.get('probe_error'),good
 if o.kind=='java':
  text=(o.output/'java-invalid/server.log').read_text()
  assert not bad['ready'] and not bad['service'] and not bad['timed_out'] and not bad['probe_termination'],bad
  assert 'h259:unused_probe' in text and 'Value -1 outside of range [0:4064]' in text,text[-6000:]
 return 0
if __name__=='__main__':sys.exit(main())
