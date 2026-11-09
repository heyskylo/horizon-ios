"""Execute the shared mouse mapping and real touch-hold inventory across input sources."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method
ROOT=Path(__file__).resolve().parents[1]


def program():
    game=(ROOT/'crates/horizon-client/src/game/play_input.rs').read_text()
    touch=(ROOT/'crates/horizon-client/src/game/touch.rs').read_text()
    methods='\n'.join(method(game,n) for n in ['mouse_mapping','mouse_mapping_held_elsewhere'])+'\n'+method(touch,'touch_holds_mouse_mapping')
    return STUBS+'\nimpl Game{'+methods+'}\n'+CASES


STUBS=r'''
#![allow(dead_code)]
use std::collections::HashSet;
type Result<T>=std::result::Result<T,()>;
#[derive(Clone,Copy,PartialEq,Eq,Hash,Debug)]enum MouseButton{Left,Right,Middle}
#[derive(Clone,Copy,PartialEq)]enum Source{Keyboard,Pad,Touch,Agent}
#[derive(Default)]struct Touch{holding:Option<MouseButton>,late:Vec<(MouseButton,u64)>}
#[derive(Default)]struct Pad{held:bool}impl Pad{fn holds_action(&self,_:&(),_:&str)->bool{self.held}}
#[derive(Default)]struct Keys;impl Keys{fn translate_mouse_all(&self,b:MouseButton)->Vec<std::result::Result<i32,MouseButton>>{vec![Err(b)]}fn translate_all(&self,_:i32)->Vec<std::result::Result<i32,MouseButton>>{vec![Err(MouseButton::Right)]}}
#[derive(Default)]struct Settings{controller:(),keys:Keys}
#[derive(Default)]struct Game{touch:Touch,pad:Pad,settings:Settings,physical_buttons:HashSet<MouseButton>,physical_keys:HashSet<i32>,events:Vec<(MouseButton,bool)>}
impl Game{fn play_mouse(&mut self,b:MouseButton,pressed:bool)->Result<()>{self.events.push((b,pressed));Ok(())}}
'''
CASES=r'''
fn main(){
 let mut g=Game::default();g.physical_buttons.insert(MouseButton::Right);
 g.mouse_mapping(MouseButton::Right,false,Source::Touch).unwrap();assert!(g.events.is_empty());
 g.physical_buttons.clear();g.physical_keys.insert(1);g.mouse_mapping(MouseButton::Right,false,Source::Touch).unwrap();assert!(g.events.is_empty());
 g.physical_keys.clear();g.pad.held=true;g.mouse_mapping(MouseButton::Right,false,Source::Touch).unwrap();assert!(g.events.is_empty());
 g.pad.held=false;g.mouse_mapping(MouseButton::Right,false,Source::Touch).unwrap();assert_eq!(g.events,[(MouseButton::Right,false)]);g.events.clear();
 g.touch.holding=Some(MouseButton::Right);for source in [Source::Keyboard,Source::Pad,Source::Agent]{g.mouse_mapping(MouseButton::Right,false,source).unwrap();}assert!(g.events.is_empty());
 g.touch.holding=None;g.touch.late.push((MouseButton::Left,1));g.mouse_mapping(MouseButton::Left,false,Source::Keyboard).unwrap();assert!(g.events.is_empty());
 g.touch.late.clear();g.mouse_mapping(MouseButton::Left,false,Source::Keyboard).unwrap();assert_eq!(g.events,[(MouseButton::Left,false)]);
}
'''


class MobileMouseOwnershipTests(unittest.TestCase):
    def test_touch_and_other_devices_do_not_release_each_others_mouse_mapping(self):
        if not shutil.which('rustc'):self.skipTest('rustc required')
        with tempfile.TemporaryDirectory(prefix='horizon-touch-mouse-') as folder:
            source=Path(folder)/'probe.rs';binary=Path(folder)/'probe';source.write_text(program())
            built=subprocess.run(['rustc','--edition=2024',str(source),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(built.returncode,0,built.stdout+built.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
