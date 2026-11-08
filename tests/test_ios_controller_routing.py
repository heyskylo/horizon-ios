"""Production iOS snapshot-to-seat routing without requiring a physical controller.

GameController API calls are covered by the separate iOS compile. This probe
uses real gamepad edges and the production router with supplied snapshots.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    source = (ROOT / 'crates/horizon-client/src/game/pad/ios.rs').read_text()
    router = source[source.index('pub(in crate::game) type PadId'):source.index('\nimpl Pads {', source.index('struct RoutedPad'))]
    router = router.replace('pub(in crate::game)', 'pub(crate)')
    code = '#![allow(dead_code)]\n'
    code += f'#[path={str(ROOT / "crates/horizon-gamecontroller/src/lib.rs")!r}] mod horizon_gamecontroller;\n'.replace("'", '"')
    code += f'#[path={str(ROOT / "crates/horizon-client-input/src/gamepad.rs")!r}] mod gamepad;\n'.replace("'", '"')
    code += 'use gamepad::{Button,PadState,Edges,GlyphStyle};\nstruct Pads;\n'
    code += '#[derive(Debug,PartialEq)]enum PadEvent{Press(PadId,Button),Gone(PadId),Back(PadId)}\n'
    return code + method(source, 'style') + '\n' + router + '\n' + CASES


CASES = r'''
fn snapshot(id:u64, a:bool,b:bool,left:[f32;2],trigger:f32)->horizon_gamecontroller::ConnectedController {
 horizon_gamecontroller::ConnectedController { id,
  controller:horizon_gamecontroller::Controller{name:Some(format!("pad{id}")),category:"DualSense".into(),gamepad:true},
  state:horizon_gamecontroller::Snapshot{category:"DualSense".into(),a,b,left_stick:left,triggers:[trigger,0.],..Default::default()}
 }
}
fn main(){
 let mut router=PadRouter::default();
 let one=snapshot(1,true,false,[0.4,-0.6],0.8);let two=snapshot(2,false,true,[-0.1,0.7],0.);
 assert_eq!(router.update(vec![one.clone(),two.clone()]),vec![PadEvent::Back(1),PadEvent::Press(1,Button::South),PadEvent::Press(1,Button::LeftTrigger),PadEvent::Back(2),PadEvent::Press(2,Button::East)]);
 // Reordering the OS list or changing its active controller cannot switch seats.
 assert!(router.update(vec![two.clone(),one.clone()]).is_empty());
 assert!(router.state(1).held(Button::South));assert!(!router.state(1).held(Button::East));
 assert_eq!(router.state(2).left,[-0.1,0.7]);assert_eq!(router.name(2),"pad2");assert_eq!(router.style(1),GlyphStyle::PlayStation);
 assert_eq!(router.update(vec![two.clone()]),vec![PadEvent::Gone(1)]);assert!(!router.connected(1));assert_eq!(router.state(1),PadState::default());
 // A reconnected native controller has a new ID; another identical model
 // must not inherit the vanished controller's held state or ownership.
 let reconnected=snapshot(3,true,false,[0.,0.],0.);
 assert_eq!(router.update(vec![two.clone(),reconnected]),vec![PadEvent::Back(3),PadEvent::Press(3,Button::South)]);
 assert_eq!(router.uuid(3),[0;16]);assert!(!router.connected(1));
 assert_eq!(router.update(vec![]),vec![PadEvent::Gone(2),PadEvent::Gone(3)]);
}
'''


class IOSControllerRoutingTests(unittest.TestCase):
    def test_native_snapshot_seats_edges_and_disconnects(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-ios-pads-') as directory:
            source = Path(directory) / 'probe.rs'
            binary = Path(directory) / 'probe'
            source.write_text(program())
            built = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
