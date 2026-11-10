"""Actual touch toggle dispatch retains independent hardware key ownership."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class TouchToggleTests(unittest.TestCase):
    def test_toggle_options_and_independent_sources(self):
        if not shutil.which('rustc'):
            self.skipTest('requires Rust')
        source = (ROOT / 'crates/horizon-client/src/game/touch.rs').read_text()
        begin = source.index('                Out::Press(control @ (Control::Sprint | Control::Sneak)) => {')
        end = source.index('                #[cfg(target_os = "ios")]', begin)
        arms = source[begin:end]
        fixture = r'''
#![allow(dead_code)]
extern crate self as winit;
pub mod keyboard{#[derive(Clone,Copy,Debug,PartialEq,Eq,Hash)]pub enum KeyCode{ShiftLeft,ControlLeft}}
mod actions{pub struct Action;impl Action{pub fn key(self)->Option<crate::keyboard::KeyCode>{None}}}
#[path="HELD"]mod held;
use keyboard::KeyCode;use held::{HeldKeys,Source};
#[derive(Clone,Copy,PartialEq)]enum Control{Sprint,Sneak}
enum Out{Press(Control),Release(Control)}
#[derive(Default)]struct Input{toggle_keys:Vec<KeyCode>}
#[derive(Default)]struct Settings{toggle_crouch:bool,toggle_sprint:bool}
#[derive(Default)]struct Game{touch:Input,settings:Settings,keys:HeldKeys,edges:Vec<(KeyCode,bool,Source)>}
impl Game{
 fn mapped_key(&mut self,k:KeyCode,down:bool,_repeat:bool,_raw:KeyCode,source:Source)->Result<(),()>{
  self.edges.push((k,down,source));if down{self.keys.press(k,source);}else{self.keys.release(&k,source);}Ok(())
 }
 fn input(&mut self,out:Vec<Out>)->Result<(),()>{for o in out{match o{ARMS}}Ok(())}
}
fn main(){
 for (control,key) in [(Control::Sneak,KeyCode::ShiftLeft),(Control::Sprint,KeyCode::ControlLeft)]{
  let mut g=Game::default();g.keys.press(key,Source::Keyboard);
  g.input(vec![Out::Press(control),Out::Release(control)]).unwrap();
  assert_eq!(g.edges,[(key,true,Source::Touch)]);assert!(g.keys.held_by(&key,Source::Touch));
  g.input(vec![Out::Press(control),Out::Release(control)]).unwrap();
  assert_eq!(g.edges,[(key,true,Source::Touch),(key,false,Source::Touch)]);
  assert!(!g.keys.held_by(&key,Source::Touch));assert!(g.keys.held_by(&key,Source::Keyboard));
  for option in [false,true]{
   let mut g=Game::default();g.settings.toggle_crouch=option;g.settings.toggle_sprint=option;
   for _ in 0..2{g.input(vec![Out::Press(control),Out::Release(control)]).unwrap();}
   if option{assert_eq!(g.edges,[(key,true,Source::Touch),(key,false,Source::Touch),(key,true,Source::Touch),(key,false,Source::Touch)]);assert!(g.touch.toggle_keys.is_empty());}
   else{assert_eq!(g.edges,[(key,true,Source::Touch),(key,false,Source::Touch)]);}
  }
 }
}
'''.replace('ARMS', arms).replace('HELD', str(ROOT / 'crates/horizon-client-input/src/held.rs'))
        with tempfile.TemporaryDirectory(prefix='horizon-touch-toggle-') as directory:
            folder = Path(directory)
            (folder/'probe.rs').write_text(fixture)
            built = subprocess.run(['rustc', '--edition=2024', str(folder/'probe.rs'), '-o', str(folder/'probe')], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            ran = subprocess.run([str(folder/'probe')], capture_output=True, text=True)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)
