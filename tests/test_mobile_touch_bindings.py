"""Virtual physical inputs use the real options key-binding translation.

Only window key/button types and final game consumers are isolated. The binding
implementation and touch adapter are the current production sources.
"""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


class TouchBindingTests(unittest.TestCase):
    def test_physical_virtual_keys_and_mouse_follow_rebound_options(self):
        if not shutil.which("rustc"):
            self.skipTest("rustc required")
        bindings = (ROOT / "crates/horizon-client-input/src/keybinds.rs").read_text()
        variants = sorted(set(re.findall(r"KeyCode::(\w+)", bindings)))
        # Layout learning/display labels are separate platform functions; keep
        # all of KeyBinds and its input-name tables unchanged.
        start = bindings.index("pub use horizon_client_core::key_display::display_name;")
        end = bindings.index("/// Current bindings by mapping name.", start)
        bindings = bindings[:start] + bindings[end:]
        bindings = bindings.replace("use winit::{", "use crate::winit::{")
        touch = (ROOT / "crates/horizon-client/src/game/touch.rs").read_text()
        methods = "\n".join(method(touch, name) for name in ["touch_raw_key", "touch_key_held", "touch_raw_button", "touch_raw_holds_mouse_mapping"])
        program = r'''
#![allow(dead_code)]
mod winit {
 pub mod keyboard {#[derive(Clone,Copy,Debug,Eq,PartialEq)] pub enum KeyCode {VARIANTS}}
 pub mod event {#[derive(Clone,Copy,Debug,Eq,PartialEq)] pub enum MouseButton {Left,Right,Middle,Back,Forward,Other(u16)}}
}
mod horizon_client {pub mod debug_keys {#[derive(Clone,Copy,Debug,PartialEq)]pub enum KeyAction{Press,Release}} pub mod keybinds {BINDINGS}}
#[path="TOUCH_SOURCE"]mod actual_touch;
use actual_touch::Control;
use winit::{keyboard::KeyCode,event::MouseButton};
KEY_OF
#[derive(Default)]struct Touch{fingers:actual_touch::Touches,cursor:actual_touch::desktop::Cursor,cursor_down:[bool;3],cursor_keys:Vec<Control>,native_mouse:[bool;3],native_keys:Vec<KeyCode>,custom_keys:Vec<KeyCode>,custom_mouse:[bool;3]}
use horizon_client::keybinds::KeyBinds;
type Result<T>=std::result::Result<T,()>;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]enum Source{Touch}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]enum Event{Key(KeyCode,bool,KeyCode,Source),Mouse(MouseButton,bool,Source)}
#[derive(Default)]struct Settings{keys:KeyBinds}
#[derive(Default)]struct Game{touch:Touch,settings:Settings,events:Vec<Event>,another_touch_hold:bool,debug_edges:Vec<horizon_client::debug_keys::KeyAction>}
impl Game{
 fn debug_key(&mut self,key:KeyCode,action:horizon_client::debug_keys::KeyAction)->bool{if key==KeyCode::F3{self.debug_edges.push(action);true}else{false}}
 fn mapped_key(&mut self,key:KeyCode,pressed:bool,_:bool,raw:KeyCode,source:Source)->Result<()>{self.events.push(Event::Key(key,pressed,raw,source));Ok(())}
 fn mouse_mapping(&mut self,button:MouseButton,pressed:bool,source:Source)->Result<()>{self.events.push(Event::Mouse(button,pressed,source));Ok(())}
 fn touch_holds_mouse_mapping(&self,b:MouseButton)->bool{self.another_touch_hold || self.touch_raw_holds_mouse_mapping(b)}
 fn shared_default_acts(&self,_:MouseButton,_:Option<&str>)->bool{true}
 METHODS
}
fn key(key:KeyCode,down:bool,raw:KeyCode)->Event{Event::Key(key,down,raw,Source::Touch)}
fn mouse(button:MouseButton,down:bool)->Event{Event::Mouse(button,down,Source::Touch)}
fn main(){
 use KeyCode::*;use MouseButton::*;
 for (control,binding) in [(Control::Sprint,"key.keyboard.left.control"),(Control::MiddleClick,"key.mouse.middle"),(Control::PlayerList,"key.keyboard.tab"),(Control::Debug,"key.keyboard.f3")] {
  let mut g=Game::default();g.settings.keys.set("key.attack",binding);
  g.touch.fingers.start(1,(10.,10.),0.,&[(control,[0.,0.,20.,20.])]);
  assert!(g.touch_raw_holds_mouse_mapping(Left),"new control lost ownership");
  g.touch.fingers.end(1);assert!(!g.touch_raw_holds_mouse_mapping(Left));
 }
 let mut debug=Game::default();debug.touch_raw_key(F3,true).unwrap();debug.touch_raw_key(F3,false).unwrap();
 assert_eq!(debug.debug_edges,[horizon_client::debug_keys::KeyAction::Press,horizon_client::debug_keys::KeyAction::Release]);assert!(debug.events.is_empty());
 let mut g=Game::default();g.touch_raw_key(KeyW,true).unwrap();g.touch_raw_key(KeyW,false).unwrap();
 assert_eq!(g.events,[key(KeyW,true,KeyW),key(KeyW,false,KeyW)]);
 g.events.clear();g.settings.keys.set("key.forward","key.keyboard.r");g.settings.keys.set("key.back","key.keyboard.w");
 g.touch_raw_key(KeyW,true).unwrap();g.touch_raw_key(KeyW,false).unwrap();g.touch_raw_key(KeyR,true).unwrap();
 assert_eq!(g.events,[key(KeyS,true,KeyW),key(KeyS,false,KeyW),key(KeyW,true,KeyR)]);
 g.events.clear();g.settings.keys.set("key.jump","key.keyboard.w");g.touch_raw_key(KeyW,true).unwrap();
 assert_eq!(g.events,[key(KeyS,true,KeyW),key(Space,true,KeyW)]);
 let mut g=Game::default();g.settings.keys.set("key.forward","key.keyboard.r");g.touch_raw_key(KeyW,true).unwrap();assert!(g.events.is_empty());
 let mut g=Game::default();g.settings.keys.set("key.attack","key.keyboard.w");
 g.touch_raw_key(KeyW,true).unwrap();g.touch_raw_key(KeyW,false).unwrap();
 assert_eq!(g.events,[mouse(Left,true),key(KeyW,true,KeyW),mouse(Left,false),key(KeyW,false,KeyW)]);
 g.events.clear();g.touch_raw_button(Left,true).unwrap();assert!(g.events.is_empty(),"old attack binding must become inert");
 let mut g=Game::default();g.settings.keys.set("key.attack","key.mouse.right");g.settings.keys.set("key.use","key.mouse.left");
 g.touch_raw_button(Left,true).unwrap();g.touch_raw_button(Left,false).unwrap();g.touch_raw_button(Right,true).unwrap();
 assert_eq!(g.events,[mouse(Right,true),mouse(Right,false),mouse(Left,true)]);
 let mut g=Game::default();g.settings.keys.set("key.attack","key.keyboard.g");g.settings.keys.set("key.inventory","key.mouse.left");
 g.touch_raw_button(Left,true).unwrap();g.touch_raw_button(Left,false).unwrap();g.touch_raw_key(KeyG,true).unwrap();
 assert_eq!(g.events,[key(KeyE,true,KeyE),key(KeyE,false,KeyE),mouse(Left,true),key(KeyG,true,KeyG)]);
 // A gesture release cannot end a regular touch hold of the same mapping.
 g.events.clear();g.another_touch_hold=true;g.touch_raw_key(KeyG,false).unwrap();assert_eq!(g.events,[key(KeyG,false,KeyG)]);g.events.clear();
 g.another_touch_hold=false;g.touch_raw_key(KeyG,false).unwrap();assert_eq!(g.events,[mouse(Left,false),key(KeyG,false,KeyG)]);
}
'''.replace("VARIANTS", ",".join(variants)).replace("BINDINGS", bindings).replace("METHODS", methods).replace("TOUCH_SOURCE",str(ROOT/"crates/horizon-client/src/touch.rs")).replace("KEY_OF",method(touch,"key_of"))
        with tempfile.TemporaryDirectory(prefix="horizon-touch-bindings-") as directory:
            folder = Path(directory)
            (folder / "probe.rs").write_text(program)
            built = subprocess.run(["rustc", "--edition=2024", str(folder / "probe.rs"), "-o", str(folder / "probe")], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            ran = subprocess.run([str(folder / "probe")], capture_output=True, text=True)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)
