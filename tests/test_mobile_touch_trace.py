"""Original touch trace and replay dispatch retain finger identity and phase."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method
from test_mobile_multiplayer_keyboard import block
ROOT=Path(__file__).resolve().parents[1]


def program():
    trace=(ROOT/'crates/horizon-client/src/game/input_trace.rs').read_text()
    replay=(ROOT/'crates/horizon-client/src/game/repro.rs').read_text()
    actual=method(trace,'trace_window_event')
    gate=actual[actual.index('        if origin'):actual.index('        match event')]
    start=trace.index('            WindowEvent::Touch(touch) =>')
    arm=trace[start:trace.index('=>',start)+2]+block(trace,trace.index('{',trace.index('=>',start)))
    start=replay.index('            TraceKind::Touch {',replay.index('fn replay_event'))
    body=replay.index('{',replay.index('=>',start))
    replay_arm=replay[start:body]+block(replay,body)
    return STUBS+'\nimpl Game{fn trace(&mut self,origin:InputOrigin,event:&WindowEvent){'+gate+'match event{'+arm+',_=>{}}}\nfn replay(&mut self,event:TraceEvent)->bool{let id=7;let device_id=0;let event_loop=&();match event.kind{'+replay_arm+'}}}\n'+CASES


STUBS=r'''
#![allow(dead_code)]
#[derive(Clone,Copy,Debug,PartialEq)]enum InputOrigin{User,Agent,Game}
#[derive(Clone,Copy,Debug,PartialEq)]struct PhysicalPosition{x:f64,y:f64}impl PhysicalPosition{fn new(x:f64,y:f64)->Self{Self{x,y}}}
#[derive(Clone,Copy,Debug,PartialEq)]enum TouchPhase{Started,Moved,Ended,Cancelled}
#[derive(Clone,Copy,Debug,PartialEq)]struct Touch{device_id:u32,phase:TouchPhase,location:PhysicalPosition,force:Option<()>,id:u64}
mod winit{pub mod event{pub(crate)use crate::{TouchPhase,Touch};}}
#[derive(Clone,Copy)]enum WindowEvent{Touch(Touch),Other}
#[derive(Clone,Debug,PartialEq)]enum TraceKind{Touch{id:u64,phase:String,x:f64,y:f64}}
struct TraceEvent{kind:TraceKind}
#[derive(Default)]struct Game{synthetic:bool,notes:Vec<(InputOrigin,TraceKind)>,delivered:Vec<(InputOrigin,Touch)>}
impl Game{
 fn touch_synthetic_input(&self)->bool{self.synthetic}fn tracing_input(&self)->bool{true}
 fn note_input(&mut self,origin:InputOrigin,kind:TraceKind){self.notes.push((origin,kind));}
 fn handle_window_event(&mut self,_:&(),id:u32,event:WindowEvent,origin:InputOrigin){assert_eq!(id,7);self.trace(origin,&event);if let WindowEvent::Touch(touch)=event{self.delivered.push((origin,touch));}}
}
'''
CASES=r'''
fn main(){
 for (phase,name) in [(TouchPhase::Started,"started"),(TouchPhase::Moved,"moved"),(TouchPhase::Ended,"ended"),(TouchPhase::Cancelled,"cancelled")]{
  let mut g=Game::default();let touch=Touch{device_id:0,phase,location:PhysicalPosition::new(12.5,99.25),force:None,id:42};
  g.trace(InputOrigin::User,&WindowEvent::Touch(touch));
  assert_eq!(g.notes,[(InputOrigin::User,TraceKind::Touch{id:42,phase:name.into(),x:12.5,y:99.25})]);
  let kind=g.notes[0].1.clone();assert!(g.replay(TraceEvent{kind}));assert_eq!(g.delivered,[(InputOrigin::Agent,touch)]);
  assert_eq!(g.notes.len(),1,"replaying is not a second physical user intrusion");
  g.synthetic=true;g.trace(InputOrigin::Game,&WindowEvent::Touch(touch));assert_eq!(g.notes.len(),1,"derived events must not duplicate the recording");
 }
 let mut g=Game::default();
 for (phase,x) in [("invalid",1.),("started",f64::NAN),("cancelled",f64::INFINITY)]{assert!(!g.replay(TraceEvent{kind:TraceKind::Touch{id:1,phase:phase.into(),x,y:0.}}));}
 assert!(g.delivered.is_empty());
}
'''


class MobileTouchTraceTests(unittest.TestCase):
    def test_touch_trace_and_replay_origin_phase_and_coordinates(self):
        if not shutil.which('rustc'):
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-touch-trace-') as directory:
            source=Path(directory)/'probe.rs';binary=Path(directory)/'probe'
            source.write_text(program())
            built=subprocess.run(['rustc','--edition=2024',str(source),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(built.returncode,0,built.stdout+built.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
