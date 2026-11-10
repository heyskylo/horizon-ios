"""Recognized UIKit inputs run through the production Game frame adapter."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


class NativeGestureDispatchTests(unittest.TestCase):
    def test_timed_releases_context_changes_and_menu_button_dispatch(self):
        if not shutil.which("rustc"):
            self.skipTest("rustc required")
        source = (ROOT / "crates/horizon-client/src/game/touch.rs").read_text()
        methods = "\n".join(method(source, name) for name in ["touch_native_reset", "touch_native_input", "touch_native_frame"])
        key = method(source, "gesture_key") + method(source, "gesture_modifiers")
        start = source.index("struct ClickContext {")
        context = "#[derive(Clone,Debug,PartialEq)]" + source[start:source.index("\n}", start) + 2]
        fixture = r'''
#![allow(dead_code,unused_variables)]
extern crate self as log;
#[macro_export]macro_rules! warn {($($arg:tt)*)=>{()}}
#[path="TOUCH_SOURCE"]mod actual;
// The player's cursor-mode setting comes from the iOS launcher; the test chooses it.
mod touch{pub use crate::actual::*;pub static CURSOR:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
 pub fn cursor_mode()->bool{CURSOR.load(std::sync::atomic::Ordering::Relaxed)}}
use touch::desktop::Physical;
mod winit{pub mod keyboard{
 #[derive(Clone,Copy,Debug,Default,PartialEq)]pub struct ModifiersState(pub u32);
 impl ModifiersState{pub const SHIFT:Self=Self(1);pub const CONTROL:Self=Self(2);pub const ALT:Self=Self(4);pub const SUPER:Self=Self(8);
  pub fn empty()->Self{Self(0)}pub fn set(&mut self,flag:Self,on:bool){if on{self.0|=flag.0}else{self.0&=!flag.0}}}
}}
mod horizon_client{pub mod widgets{
 #[derive(Clone,Debug,PartialEq)]pub struct PointerGuard(pub (f32,f32));
 pub fn capture_pointer(at:(f32,f32))->PointerGuard{PointerGuard(at)}
}}
#[derive(Clone,Copy,Debug,PartialEq)]enum KeyCode{Digit1,Digit2,Digit3,Digit4,Digit5,Digit6,Digit7,Digit8,Digit9,KeyF,KeyQ}
#[derive(Clone,Copy,Debug,PartialEq)]enum MouseButton{Left,Right,Middle}
#[derive(Clone,Debug,PartialEq)]enum Event{Key(KeyCode,bool),Button(MouseButton,bool),Menu(MouseButton,bool),Scroll(f32,f32),Keyboard}
trait EventLoopCtl{}struct Loop;impl EventLoopCtl for Loop{}
mod horizon_ios_launcher{
 use std::sync::Mutex;
 #[derive(Clone,Copy)]pub struct Gesture{pub kind:u8,pub phase:u8,pub slot:i8,pub point:(f64,f64),pub age:f64,pub context:u64,pub modifiers:u32,pub touch:u64}
 pub static QUEUE:Mutex<Vec<Gesture>>=Mutex::new(vec![]);
 pub fn covered()->bool{false}
 pub fn touch_gestures(_:u64,_:u32,_:(f64,f64),_:f64)->Vec<Gesture>{std::mem::take(&mut*QUEUE.lock().unwrap())}
}
CONTEXT
#[derive(Default)]struct Input{click_contexts:Vec<(u64,ClickContext)>,cursor_delivery:Option<ClickContext>,native_pending:Vec<(Physical,f64,u64)>,native_keys:Vec<KeyCode>,native_mouse:[bool;2],native_screen:Option<u64>,cursor_screen:Option<u64>}
/// `seen` is the (modifiers, pointer) each mouse edge was delivered with.
#[derive(Default)]struct Game{touch:Input,key:u64,frames:u64,time:f64,playing:bool,events:Vec<Event>,modifiers:winit::keyboard::ModifiersState,seen:Vec<(u32,Option<(f32,f32)>)>}
struct Batch{width:f32,height:f32,scale:f32}
KEY
impl Game{
 fn key_screen(&self)->u64{self.key}fn touch_now(&mut self)->f64{self.time}fn touch_playing(&self)->bool{self.playing}
 fn touch_batch(&self)->Option<Batch>{Some(Batch{width:600.,height:300.,scale:2.})}
 fn touch_raw_key(&mut self,key:KeyCode,down:bool)->Result<(),&'static str>{self.events.push(Event::Key(key,down));Ok(())}
 fn note(&mut self){self.seen.push((self.modifiers.0,self.touch.cursor_delivery.as_ref().map(|c|c.at)));}
 fn screen_point(&self,at:(f32,f32))->(f32,f32){at}
 fn touch_record_click_context(&mut self){}
 fn touch_click_context(&self)->Option<ClickContext>{Some(self.context((100.,50.),None))}
 fn context(&self,at:(f32,f32),surface:Option<(u64,(f32,f32))>)->ClickContext{ClickContext{at,screen_at:at,size:(600.,300.),screen:self.key,modifiers:self.modifiers,touch_shift:false,guard:horizon_client::widgets::capture_pointer(at),surface}}
 fn touch_raw_button(&mut self,b:MouseButton,down:bool)->Result<(),&'static str>{self.note();self.events.push(Event::Button(b,down));Ok(())}
 fn touch_grab(&mut self){}
 fn touch_queue_cursor_button(&mut self,b:MouseButton,down:bool){self.note();self.events.push(Event::Menu(b,down));}
 fn touch_scroll(&mut self,_:&dyn EventLoopCtl,x:f32,y:f32){self.events.push(Event::Scroll(x,y));}
 fn toggle_system_keyboard(&mut self){self.events.push(Event::Keyboard);}
 fn sample(&mut self,kind:u8,phase:u8,slot:i8,age:f64){horizon_ios_launcher::QUEUE.lock().unwrap().push(horizon_ios_launcher::Gesture{kind,phase,slot,point:(0.5,-0.25),age,context:0,modifiers:0,touch:0});self.touch_native_frame(&Loop);}
 fn tap(&mut self,point:(f64,f64),context:u64,modifiers:u32,touch:u64){horizon_ios_launcher::QUEUE.lock().unwrap().push(horizon_ios_launcher::Gesture{kind:1,phase:3,slot:-1,point,age:0.,context,modifiers,touch});self.touch_native_frame(&Loop);}
 fn frame(&mut self,time:f64){self.frames+=1;self.time=time;self.touch_native_frame(&Loop);}
 METHODS
}
fn main(){
 use Event::*;use MouseButton::*;
 let mut g=Game{playing:true,..Default::default()};g.sample(1,3,-1,0.);
 assert_eq!(g.events,[Button(Right,true)]);assert!(g.touch.native_mouse[1]);
 g.frame(0.02);assert_eq!(g.events.len(),1);g.frame(0.032);assert_eq!(g.events.len(),1);
 g.frame(0.033);assert_eq!(g.events,[Button(Right,true),Button(Right,false)]);assert_eq!(g.touch.native_mouse,[false,false]);
 // Poll latency is subtracted from the source's 33ms release deadline.
 let mut g=Game{playing:true,..Default::default()};g.sample(1,3,-1,0.02);g.frame(0.012);assert_eq!(g.events.len(),1);g.frame(0.014);assert_eq!(g.events.len(),2);
 // A screen transition releases actual holds and discards the stale timer.
 let mut g=Game{playing:true,..Default::default()};g.sample(1,3,-1,0.);g.key+=1;g.playing=false;g.frame(0.01);
 assert_eq!(g.events,[Button(Right,true),Button(Right,false)]);assert!(g.touch.native_pending.is_empty());g.frame(1.);assert_eq!(g.events.len(),2);
 // Keyboard/mouse takeover uses this same reset and cannot leave Q held.
 let mut g=Game{playing:true,..Default::default()};g.sample(2,1,4,0.);assert_eq!(g.events,[Key(KeyCode::KeyQ,true)]);
 g.touch_native_reset();assert_eq!(g.events,[Key(KeyCode::KeyQ,true),Key(KeyCode::KeyQ,false)]);assert!(g.touch.native_keys.is_empty());
 // A menu tap uses the existing rendered-frame mouse edge queue.
 let mut g=Game::default();g.sample(1,3,-1,0.);g.frame(0.034);
 assert_eq!(g.events,[Menu(Left,true),Menu(Left,false)]);
 g.sample(4,2,-1,0.);g.sample(5,1,-1,0.);assert_eq!(&g.events[2..],&[Scroll(0.5,-0.25),Keyboard]);
 let mut g=Game{playing:true,..Default::default()};g.sample(1,3,8,0.);g.sample(3,3,8,0.);
 assert_eq!(g.events,[Key(KeyCode::Digit9,true),Key(KeyCode::Digit9,false),Key(KeyCode::KeyF,true),Key(KeyCode::KeyF,false)]);
 // Native queue overflow/cancellation releases each hold once.
 g.sample(2,1,-1,0.);g.sample(0,4,-1,0.);assert_eq!(&g.events[4..],&[Button(Left,true),Button(Left,false)]);
 // A menu tap carries the modifiers UIKit reported for it, and they do not outlive its delivery.
 let mut g=Game::default();g.tap((0.,0.),0,1|4,0);assert_eq!(g.events,[Menu(Left,true)]);assert_eq!(g.seen[0].0,5);assert_eq!(g.modifiers.0,0);assert!(g.touch.cursor_delivery.is_none());
 // A tap made against a recorded context is delivered with that context: a cursor dragged by the
 // same surface finger (cursor mode) lands at its recorded place plus the finger's travel, kept on the screen.
 let mut g=Game::default();let c=g.context((300.,150.),Some((7,(40.,40.))));g.touch.click_contexts.push((9,c));
 // Direct mode: the tap clicks under the finger.
 g.tap((50.,30.),9,0,7);assert_eq!(g.seen[0].1,Some((50.,30.)));g.seen.clear();
 touch::CURSOR.store(true,std::sync::atomic::Ordering::Relaxed);
 g.tap((50.,30.),9,0,7);assert_eq!(g.seen[0].1,Some((310.,140.)));
 g.tap((5000.,30.),9,0,7);assert_eq!(g.seen[1].1,Some((600.,140.)));
 // Another finger's tap clicks where the cursor was, unmoved.
 g.tap((50.,30.),9,0,8);assert_eq!(g.seen[2].1,Some((300.,150.)));
 // A tap whose context is no longer known is dropped rather than clicking at today's cursor.
 let before=g.events.len();g.tap((50.,30.),4242,0,7);assert_eq!(g.events.len(),before);
}
'''.replace("TOUCH_SOURCE", str(ROOT / "crates/horizon-client/src/touch.rs")).replace("METHODS", methods).replace("KEY", key).replace("CONTEXT", context)
        with tempfile.TemporaryDirectory(prefix="horizon-native-gesture-dispatch-") as directory:
            folder = Path(directory)
            (folder / "probe.rs").write_text(fixture)
            built = subprocess.run(["rustc", "--edition=2024", str(folder / "probe.rs"), "-o", str(folder / "probe")], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            ran = subprocess.run([str(folder / "probe")], capture_output=True, text=True)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)
