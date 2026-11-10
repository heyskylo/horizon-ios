#!/usr/bin/env python3
"""Differential input oracle against explicitly supplied, pinned GPL reference files.

No reference implementation is bundled or fetched automatically. The native
method bodies compile only in the user's reference-cache directory, outside Git.
"""
import argparse,hashlib,json,math,os,random,struct,subprocess,sys,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/"tools"))
import lab
PINS={'pojav':('PojavLauncherTeam/PojavLauncher_iOS','1eeab5c1ee6f80991e09cc411054419374852cff'), 'amethyst':('AngelAuraMC/Amethyst-iOS','f98c4133c72d39e06ceac42b57cc4477c52105fc')}

SOURCE_HASHES={'pojav/LICENSE': '525f91adf41b2ea18a5ba4000c34937ea6de77cfdedf0621184e16777a8db6e9', 'pojav/Natives/customcontrols/ControlJoystick.m': 'f172702c93a82bf7dc94b5aed424b50925c3be0f8d58ca5f22d9b563d16d54d0', 'pojav/Natives/SurfaceViewController.m': 'dc3795b176f9bf740822f50d0c5141804e38c8c8057ac79761d4d7ddf4bfbb61', 'pojav/Natives/input/ControllerInput.m': '49446f79728295a283037c9ee3ffb390b85cd160633b8c645a26926c5312fdb1', 'amethyst/LICENSE': '525f91adf41b2ea18a5ba4000c34937ea6de77cfdedf0621184e16777a8db6e9', 'amethyst/Natives/customcontrols/ControlJoystick.m': '101734657a4e129ae67bf02d7f9872ab2da45e8cce4947b5a007d989e208c98d', 'amethyst/Natives/SurfaceViewController.m': '8ec5ea2e4ebccb6082de9c977e59a4e34ddaa1d2c885a2b8aeaa5672a6ef477d', 'amethyst/Natives/ControllerInput.m': '31810e437cf9ef278fb79d7d5525583d930353555f920f5b4cc8f56bf24b61d1'}

def f32(x):return struct.unpack('f',struct.pack('f',x))[0]
def cases():
 rng=random.Random(0x504f4a4156)
 # Boundary probes on both sides of every direction and dead-zone boundary.
 for w,h in [(100.,100.),(160.,100.),(100.,160.)]:
  travel=min(w,h)*.375
  for angle in [0,45,90,135,180,225,270,315]+[22.5+45*i+e for i in range(8) for e in [-.001,0,.001]]:
   for radius in [0.,travel*.35-.0001,travel*.35,travel*.35+.0001,travel,travel*3]:
    a=math.radians(angle);x=f32(w/2+math.cos(a)*radius);y=f32(h/2-math.sin(a)*radius)
    yield {'mode':'stick','width':w,'height':h,'grabbed':True,'shift':bool(rng.randrange(2)),'events':[{'op':'move','x':x,'y':y},{'op':'move','x':w/2,'y':h/2},{'op':'end'}]}
 for grabbed in [False,True]:
  for _ in range(70):
   w,h=rng.choice([(100.,100.),(160.,100.),(100.,160.)]);events=[{'op':'move','x':f32(rng.uniform(-w,w*2)),'y':f32(rng.uniform(-h,h*2))} for _ in range(18)]+[{'op':'end'}]
   yield {'mode':'stick','width':w,'height':h,'grabbed':grabbed,'events':events}
 for grabbed in [False,True]:
  for virtual in [False,True]:
   for speed in [.25,1.,2.5]:
    for scale,resolution in [(1.,1.),(2.,.5),(3.,.75)]:
     events=[{'op':'down','x':20.,'y':30.}]
     events += [{'op':rng.choice(['move','relative']),'x':f32(rng.uniform(-200,400)),'y':f32(rng.uniform(-200,400))} for _ in range(22)]
     events += [{'op':'up','x':10.,'y':20.}]
     yield {'mode':'cursor','width':300.,'height':200.,'grabbed':grabbed,'virtual':virtual,'speed':speed,'scale':scale,'resolution':resolution,'events':events}
 # Frame-time scaled joystick cursor movement at 30/60/120Hz and variable cadence.
 for fps in [30,60,120]:
  for speed in [.25,1.,2.5]:
   for grabbed in [False,True]:
    events=[];time=0.
    for i in range(90):
     time+=rng.choice([.5,1.,1.5])/fps
     vx,vy=rng.choice([(0.,0.),(.01,.02),(.75,0.),(-.5,.5),(1.,1.)])
     events.append({'time':time,'vx':vx,'vy':vy})
    yield {'mode':'tick','width':300.,'height':200.,'grabbed':grabbed,'speed':speed,'virtual':True,'events':events}
 # Valid native button callback sequences, including simultaneous left/right holds.
 for events in [[(-3,1),(-3,0)],[(-4,1),(-4,0)],[(-3,1),(-4,1),(-3,0),(-4,0)],[(-4,1),(-3,1),(-4,0),(-3,0)]]:
  yield {'mode':'buttons','width':100.,'height':100.,'grabbed':False,'events':[{'op':'down' if down else 'up','codes':[code,0,0,0]} for code,down in events]}
 for mode in ['world_buttons','menu_buttons']:
  for code in [87,65,83,68,32,340,294,84,256,69,-3,-4]:
   for outside in [False,True]:
    events=[{'op':'down','code':code,'id':1},{'op':'move','code':code,'id':1,'outside':outside},{'op':'up','code':code,'id':1,'outside':outside}]*3
    yield {'mode':mode,'width':1000.,'height':600.,'grabbed':mode=='world_buttons','events':events}
  # Walk, jump, sneak and click may be held simultaneously and released in any order.
  for _ in range(40):
   codes=rng.sample([87,65,83,68,32,340,-3,-4],rng.randrange(2,8));events=[{'op':'down','code':code,'id':i}for i,code in enumerate(codes)]
   rng.shuffle(codes)
   events += [{'op':'up','code':code,'id':next(e['id']for e in events if e['code']==code)}for code in codes]
   yield {'mode':mode,'width':1000.,'height':600.,'grabbed':mode=='world_buttons','events':events}
 for _ in range(100):
  events=[{'op':'down','x':f32(rng.uniform(0,100)),'y':f32(rng.uniform(0,100))}]
  events += [{'op':'move','x':f32(rng.uniform(-100,200)),'y':f32(rng.uniform(-100,200))} for _ in range(25)]
  events += [{'op':'end'}]
  yield {'mode':'world_stick','width':100.,'height':100.,'grabbed':True,'events':events}
 for scale in [1.,2.,3.,4.]:
  for _ in range(30):
   events=[{'op':'down','id':1,'x':100.,'y':100.}, {'op':'down','id':2,'x':200.,'y':200.}]
   for n in range(24):
    events.append({'op':'move','id':rng.choice([1,2]) if n<=8 else 2,'x':rng.randrange(50,1000)/4,'y':rng.randrange(50,1000)/4})
    if n==8:events.append({'op':'up','id':1,'x':100.,'y':100.})
   events.append({'op':'cancel','id':2,'x':200.,'y':200.})
   yield {'mode':'world_camera','width':1000.,'height':600.,'scale':scale,'grabbed':True,'events':events}
 for grabbed in [False,True]:
  for mouse in [False,True]:
   for slot in [-1,0,4,8]:
    for kind in [1,2,3,4,5]:
     phases=[3] if kind in [1,3] else [1,2,3,4,5]
     yield {'mode':'gestures','width':1000.,'height':600.,'grabbed':grabbed,'mouse':mouse,'events':[{'kind':kind,'phase':phase,'slot':slot,'x':.5,'y':-.25}for phase in phases]}
 for virtual in [False,True]:
  for _ in range(30):
   events=[{'op':'down','id':1,'x':100.,'y':100.},{'op':'down','id':2,'x':200.,'y':200.}]
   for n in range(20):
    events.append({'op':'move','id':rng.choice([1,2]) if n<=7 else 2,'x':rng.randrange(-100,2400)/4,'y':rng.randrange(-100,1600)/4})
    if n==7:events.append({'op':'up','id':1,'x':100.,'y':100.})
   events.append({'op':'up','id':2,'x':100.,'y':100.})
   yield {'mode':'menu_surface','width':1000.,'height':600.,'virtual':virtual,'grabbed':False,'events':events}

def run(binary,case,cwd):
 p=subprocess.run([str(binary)],input=json.dumps(case),capture_output=True,text=True,cwd=cwd,timeout=10)
 if p.returncode:raise RuntimeError(p.stderr)
 return json.loads(p.stdout)
def equal(a,b):
 if isinstance(a,(int,float)) and isinstance(b,(int,float)):return math.isclose(a,b,rel_tol=1e-12,abs_tol=1e-9)
 if type(a)!=type(b):return False
 if isinstance(a,dict):return a.keys()==b.keys() and all(equal(a[k],b[k])for k in a)
 if isinstance(a,list):return len(a)==len(b) and all(equal(x,y)for x,y in zip(a,b))
 return a==b

def rust_method(source,name):
 start=source.index('fn '+name+'(');opening=source.index('{',start);depth=0
 for end in range(opening,len(source)):
  if source[end]=='{':depth+=1
  elif source[end]=='}':
   depth-=1
   if depth==0:return source[start:end+1]
 raise ValueError(name)

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--reference-dir',type=Path,required=True);p.add_argument('--report',type=Path,required=True);args=p.parse_args()
 ref=args.reference_dir.resolve()
 if ref.is_relative_to(ROOT):raise SystemExit('Keep GPL reference source outside the repository')
 work=Path(tempfile.mkdtemp(prefix='horizon-pojav-parity-'));hashes={}
 for name in PINS:
  base=ref if name=='pojav' else ref/'amethyst'
  for rel in ['LICENSE','Natives/customcontrols/ControlJoystick.m','Natives/SurfaceViewController.m', 'Natives/input/ControllerInput.m' if name=='pojav' else 'Natives/ControllerInput.m']:
   file=base/rel
   if not file.is_file():raise SystemExit('Missing explicit reference source: '+str(file))
   key=name+'/'+rel;hashes[key]=hashlib.sha256(file.read_bytes()).hexdigest()
   if hashes[key]!=SOURCE_HASHES[key]:raise SystemExit('Reference hash does not match pinned commit: '+key)
 subprocess.run([sys.executable,str(ROOT/'tools/pojav_parity/build_reference.py'),str(ref)],check=True,cwd=work)
 project=work/'rust';(project/'src').mkdir(parents=True)
 (project/'Cargo.toml').write_text('[package]\nname="horizon-pojav-parity-probe"\nversion="0.0.0"\nedition="2024"\n[dependencies]\nserde_json="=1.0.151"\n[features]\nhorizon-parity-ios=[]\n[workspace]\n')
 game=(ROOT/'crates/horizon-client/src/game/touch.rs').read_text()
 game_probe=(ROOT/'tools/pojav_parity/game_probe.rs').read_text().replace('GAME_KEY_OF',rust_method(game,'key_of')).replace('GAME_TOUCH_ACT',rust_method(game,'touch_act')).replace('GAME_TOUCH_RAW_KEY',rust_method(game,'touch_raw_key')).replace('GAME_TOUCH_RAW_BUTTON',rust_method(game,'touch_raw_button'))
 game_probe=game_probe.replace('target_os = "ios"','feature = "horizon-parity-ios"')
 (project/'src/game.rs').write_text(game_probe)
 (project/'src/main.rs').write_text((ROOT/'tools/pojav_parity/probe.rs').read_text().replace('TOUCH_SOURCE',str(ROOT/'crates/horizon-client/src/touch.rs')))
 env=os.environ.copy();env['CARGO_TARGET_DIR']=str(work/'target');env['CARGO_BUILD_JOBS']='1';env['CARGO_INCREMENTAL']='0'
 for key in ['HORIZON_HOME','HORIZON_LAUNCHER_HOME','XDG_DATA_HOME','XDG_CONFIG_HOME']:env[key]=str(work/key)
 env=lab.game_env(env)
 with (work/'rust-build.log').open('w') as log:
  subprocess.run(['cargo','build','--offline','--features','horizon-parity-ios','--manifest-path',str(project/'Cargo.toml')],env=env,cwd=work,stdout=log,stderr=subprocess.STDOUT,check=True)
 binaries=[ref/'pojav-oracle',ref/'amethyst-oracle',work/'target/debug/horizon-pojav-parity-probe']
 mismatches=[];count=0;samples=0;groups={}
 for index,case in enumerate(cases()):
  outputs=[run(b,case,work) for b in binaries];count+=1;samples+=len(case['events']);groups[case['mode']]=groups.get(case['mode'],0)+1
  if not equal(outputs[0],outputs[1]) or not equal(outputs[0],outputs[2]):
   mismatches.append({'case':index,'input':case,'pojav':outputs[0],'amethyst':outputs[1],'horizon':outputs[2]})
  if count%200==0:print(f'{count} traces compared; {len(mismatches)} mismatches',flush=True)
 report={'format':'horizon-pojav-input-parity','status':'fail' if mismatches else 'pass','references':PINS,'reference_hashes':hashes,'cases':count,'samples':samples,'groups':groups,'mismatches':mismatches,'scratch':str(work),'boundary':'Native control processing to callback-bridge entry; actual pinned ObjC method bodies, Foundation geometry stubs. No UIKit hit-testing, JVM, Minecraft, rendering or physical-device claim. Forward-lock disabled; valid one-contact-per-button sequences.','tolerance':{'relative':1e-12,'absolute':1e-9}}
 args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2));print(json.dumps({k:report[k]for k in ['status','cases','samples','groups']},indent=2));raise SystemExit(bool(mismatches))
if __name__=='__main__':main()
