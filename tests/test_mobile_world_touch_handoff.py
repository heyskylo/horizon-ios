"""World touches yield controller ownership before dispatching real touch outputs.

The native event/renderer endpoints are small doubles; touch routing, finger
state, control layout and the ownership handoff execute production code.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    touch = (ROOT / 'crates/horizon-client/src/game/touch.rs').read_text()
    pad = (ROOT / 'crates/horizon-client/src/game/pad.rs').read_text()
    emote = (ROOT / 'crates/horizon-client/src/game/emote_wheel.rs').read_text()
    modules = f'#[path="{ROOT / "crates/horizon-client/src/touch.rs"}"]mod touch;\n'
    return STUBS + modules + '\nimpl Game {\n' + method(touch, 'visible_touch_layout') + '\n' + method(touch, 'touch_event') + '\n' + method(pad, 'pad_yield_input') + '\n' + method(emote, 'cancel_emote_controller') + '\n}\n' + CASES


STUBS = r'''
#![allow(dead_code)]
use std::time::Instant;use touch::Control;
type Result<T>=std::result::Result<T,String>;
trait EventLoopCtl{}struct Loop;impl EventLoopCtl for Loop{}
#[derive(Clone,Copy,PartialEq)]enum TouchPhase{Started,Moved,Ended,Cancelled}
struct Position{x:f64,y:f64}struct Touch{id:u64,phase:TouchPhase,location:Position}
struct Batch{width:f32,height:f32,scale:f32}
struct Hardware;impl Hardware{fn touch(&mut self){}}
struct MenuFinger{id:u64}
struct TouchInput{more:bool,hidden:bool,seen:bool,hardware:Hardware,fingers:touch::Touches,menu:Option<MenuFinger>}
struct Pad{in_use:bool,held:Vec<&'static str>,released:Vec<&'static str>}
struct EmoteWheel{controller:Option<()>}
struct Game{emote_wheel:EmoteWheel,touch:TouchInput,pad:Pad,last_input:Instant,dispatched:Vec<(bool,Vec<touch::Out>)>}
impl Game{
 fn menu_input_blocked(&self)->bool{false}
 fn cancel_touch_for_reload(&mut self){panic!("no reload in handoff fixture");}
 fn new()->Self{Self{emote_wheel:EmoteWheel{controller:Some(())},touch:TouchInput{more:false,hidden:false,seen:false,hardware:Hardware,fingers:touch::Touches::default(),menu:None},pad:Pad{in_use:true,held:vec!["key.forward","key.attack"],released:vec![]},last_input:Instant::now(),dispatched:vec![]}}
 fn touch_batch(&self)->Option<Batch>{Some(Batch{width:1000.,height:600.,scale:2.})}
 fn touch_playing(&self)->bool{true}
 fn touch_release_menu(&mut self,_:&dyn EventLoopCtl){self.touch.menu=None;}
 fn touch_menu(&mut self,_:&dyn EventLoopCtl,_:u64,_:TouchPhase,_:(f32,f32)){panic!("world input routed to menu");}
 fn touch_now(&mut self)->f64{0.}
 fn touch_act(&mut self,out:Vec<touch::Out>,_:f32)->Result<()>{self.dispatched.push((self.pad.in_use,out));Ok(())}
 fn pad_cancel_inputs(&mut self,_:&dyn EventLoopCtl){self.pad.released.append(&mut self.pad.held);}
}
fn event(id:u64,phase:TouchPhase,x:f32,y:f32)->Touch{Touch{id,phase,location:Position{x:f64::from(x*2.),y:f64::from(y*2.)}}}
'''

CASES = r'''
fn main(){
 let layout=touch::layout(1000.,600.);let rect=layout.iter().find(|(c,_)|*c==touch::Control::Forward).unwrap().1;
 let (x,y)=(rect[0]+rect[2]/2.,rect[1]+rect[3]/2.);
 let mut g=Game::new();g.touch_event(&Loop,event(1,TouchPhase::Started,x,y)).unwrap();
 assert!(!g.pad.in_use);assert!(g.emote_wheel.controller.is_none());assert_eq!(g.pad.released,vec!["key.forward","key.attack"]);
 assert_eq!(g.dispatched[0],(false,vec![touch::Out::Press(touch::Control::Forward)]));
 // End/cancel release this real held finger. Neither is a fresh takeover
 // when the player has intentionally returned to a controller meanwhile.
 g.pad.in_use=true;g.emote_wheel.controller=Some(());g.touch_event(&Loop,event(1,TouchPhase::Cancelled,x,y)).unwrap();assert!(g.pad.in_use);assert!(g.emote_wheel.controller.is_some(),"cancelled old finger must not cancel newly owned controller");
 assert_eq!(g.dispatched[1],(true,vec![touch::Out::Release(touch::Control::Forward)]));
 assert!(!g.touch.fingers.held(touch::Control::Forward));
 g.touch_event(&Loop,event(2,TouchPhase::Started,x,y)).unwrap();g.pad.in_use=true;
 g.touch_event(&Loop,event(2,TouchPhase::Ended,x,y)).unwrap();assert!(g.pad.in_use);
 assert_eq!(g.dispatched.last().unwrap(),&(true,vec![touch::Out::Release(touch::Control::Forward)]));
 // An ongoing world-look drag intentionally takes input back again after
 // controller activity; no stale controller hold survives the new movement.
 let mut g=Game::new();g.touch_event(&Loop,event(3,TouchPhase::Started,500.,250.)).unwrap();
 g.pad.in_use=true;g.pad.held.push("key.use");g.touch_event(&Loop,event(3,TouchPhase::Moved,550.,300.)).unwrap();
 assert!(!g.pad.in_use);assert!(g.pad.held.is_empty());assert_eq!(g.pad.released,vec!["key.forward","key.attack","key.use"]);
 assert_eq!(g.dispatched.last().unwrap(),&(false,vec![touch::Out::Look(50.,50.)]));
}
'''


class MobileWorldTouchHandoffTests(unittest.TestCase):
    def test_world_touch_ownership_and_cancel_dispatch(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-world-touch-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
