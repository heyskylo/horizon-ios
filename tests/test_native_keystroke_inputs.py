"""Native keystrokes use real logical ownership and the gameplay stick merger.

Only pad_move's already-gated analog result and physical-to-logical key binding
lookup are endpoints; the overlay method, Actions, HeldKeys and with_stick run
from production source.
"""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method
ROOT=Path(__file__).resolve().parents[1]


def program():
    actions=(ROOT/'crates/horizon-client-input/src/actions.rs').read_text()
    sprint=(ROOT/'crates/horizon-client/src/sprint.rs').read_text()
    overlay=(ROOT/'crates/horizon-client/src/game/extras_ios.rs').read_text()
    keys=sprint[sprint.index('#[derive(Clone, Copy, Debug, Default)]'):sprint.index('impl Keys')]
    codes=sorted(set(re.findall(r'KeyCode::(\w+)',actions)) | {'KeyZ'})
    code='#![allow(dead_code)]\nextern crate self as winit;\npub mod keyboard{#[derive(Clone,Copy,Debug,Hash,PartialEq,Eq)]pub enum KeyCode{'+','.join(codes)+'}}\n'
    for name,file in [('actions','actions.rs'),('held','held.rs')]:
        code+=f'#[path="{ROOT / "crates/horizon-client-input/src" / file}"]mod {name};\n'
    code+='use actions::Action;use held::{HeldKeys,Source};use keyboard::KeyCode;\n'+keys+'impl Keys{'+method(sprint,'with_stick')+'}\n'
    code+='#[derive(Default)]struct Game{keys:HeldKeys,stick:Option<[f32;2]>}\nimpl Game{fn pad_move(&self)->Option<[f32;2]>{self.stick}\n'+method(overlay,'native_keystroke_inputs')+'}\n'
    return code+CASES


CASES=r'''
fn flags(g:&Game)->[bool;5]{let k=g.native_keystroke_inputs();[k.forward,k.backward,k.left,k.right,k.jump]}
fn main(){
 for source in [Source::Keyboard,Source::Pad,Source::Touch,Source::Agent]{
  let mut g=Game::default();for action in [Action::Forward,Action::Left,Action::Jump]{g.keys.press(action.key().unwrap(),source);}
  assert_eq!(flags(&g),[true,false,true,false,true]);g.keys.release_source(source);assert_eq!(flags(&g),[false;5]);
 }
 // A rebound Z represents Forward only after the binding dispatcher translates
 // it to that logical action. A physical non-mapping key must not light WASD.
 let mut g=Game::default();g.keys.press(KeyCode::KeyZ,Source::Keyboard);assert_eq!(flags(&g),[false;5]);
 g.keys.press(Action::Forward.key().unwrap(),Source::Keyboard);assert!(g.native_keystroke_inputs().forward);
 g.keys.press(Action::Forward.key().unwrap(),Source::Touch);g.keys.release_source(Source::Keyboard);assert!(g.native_keystroke_inputs().forward);
 for (stick,wanted) in [
  (None,[false;5]),(Some([0.3,0.3]),[false;5]),
  (Some([0.31,1.0]),[true,false,true,false,false]),
  (Some([-0.31,-1.0]),[false,true,false,true,false]),
  (Some([0.0,0.30001]),[true,false,false,false,false]),
 ]{let g=Game{stick,..Game::default()};assert_eq!(flags(&g),wanted);}
 let mut g=Game{stick:Some([-1.0,-1.0]),..Game::default()};
 g.keys.press(Action::Forward.key().unwrap(),Source::Agent);g.keys.press(Action::Left.key().unwrap(),Source::Touch);
 assert_eq!(flags(&g),[true,true,true,true,false],"stick flags OR with held actions as gameplay does");
 assert_eq!(g.native_keystroke_inputs().analog,Some([-1.0,-1.0]));
}
'''


class NativeKeystrokeInputTests(unittest.TestCase):
    def test_production_action_ownership_and_gameplay_analog_flags(self):
        if not shutil.which('rustc'):self.skipTest('rustc required')
        with tempfile.TemporaryDirectory(prefix='horizon-native-keys-') as folder:
            source=Path(folder)/'probe.rs';binary=Path(folder)/'probe';source.write_text(program())
            built=subprocess.run(['rustc','--edition=2024',str(source),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(built.returncode,0,built.stdout+built.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
