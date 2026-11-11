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
    for name in ("radial", "input_source"):
        code += f'#[path="{ROOT / "crates/horizon-client-input/src" / (name + ".rs")}"] mod {name};\n'
    gamepad = (ROOT / 'crates/horizon-client-input/src/gamepad.rs').read_text()
    gamepad = gamepad.replace("mod radial_file;", f'#[path="{ROOT / "crates/horizon-client-input/src/gamepad/radial_file.rs"}"] mod radial_file;')
    # This probe owns native snapshot/seat/edge behavior, not the separate
    # keyboard binding dispatcher. Keep real production state, hysteresis,
    # cancellation and glyph code without inventing winit/keybind shims.
    begin = gamepad.index('/// How a bindable action reaches the game:')
    end = gamepad.index("/// An action's name in the binds list:", begin)
    code += 'mod gamepad {\n' + gamepad[:begin] + gamepad[gamepad.index('pub const RADIAL_CAPTION:'):gamepad.index(';', gamepad.index('pub const RADIAL_CAPTION:'))+1] + gamepad[end:] + '\n}\n'
    code += 'use gamepad::{Button,PadState,Edges,GlyphStyle};\n#[derive(Clone,Debug,Default,PartialEq)]struct Device{name:String,vendor:Option<u16>,product:Option<u16>}\n'
    # The JSON endpoint double retains every controller field, so the actual
    # report_status method can be checked without linking the whole renderer.
    code += r'''
#[derive(Clone,Debug,PartialEq)]enum Value { Controller{name:String,vendor:Option<u16>,product:Option<u16>,category:String,gamepad:bool,active:bool}, Status{backend:bool,controllers:Vec<Value>} }
macro_rules! json {
 ({"name": $name:expr, "vendor": $vendor:expr, "product": $product:expr, "category": $category:expr, "gamepad": $gamepad:expr, "active": $active:expr}) => { Value::Controller{name:$name.clone(),vendor:$vendor,product:$product,category:$category.clone(),gamepad:$gamepad,active:$active} };
 ({"backend": $backend:expr, "controllers": $controllers:expr}) => { Value::Status{backend:$backend,controllers:$controllers} };
}
'''
    code += source[source.index('#[derive(Default)]'):source.index('/// The glyphs')].replace('pub(super)', 'pub(crate)')
    code += r'''
#[derive(Default)]struct Menu { armed:Option<Button> }
#[derive(Default)]struct Pads {
    owner_epoch: u64, peaks: [[f32;2];2],
 devices:Devices,feed_source:Option<PadId>,device:Option<Device>,style:GlyphStyle,
 menu:Menu,connections:Vec<Connection>,
}
#[derive(Debug,PartialEq)]enum Connection {Connected(GlyphStyle),Disconnected{active:bool}}
// The production native poll only logs names. Keep logging out of this standalone fixture.
mod log { pub(crate) use crate::info; }
#[macro_export]macro_rules! info {($($arg:tt)*)=>{let _=format!($($arg)*);};}
'''
    code += 'impl Pads {\n' + method(source, 'read_snapshots') + '\n' + method(source, 'release_hardware') + '\n}\n'
    code += '#[derive(Debug,PartialEq)]enum PadEvent{Press(PadId,Button),Gone(PadId),Back(PadId)}\n'
    return code + method(source, 'style') + '\n' + method(source, 'device_of') + '\n' + router + '\n' + CASES


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
 assert_eq!(router.state(2).left,[-0.1,0.7]);assert_eq!(router.name(2),"pad2");assert_eq!(router.device(2),Some(Device{name:"pad2".into(),vendor:Some(0x054c),product:None}));assert_eq!(router.style(1),GlyphStyle::PlayStation);
 assert_eq!(router.update(vec![two.clone()]),vec![PadEvent::Gone(1)]);assert!(!router.connected(1));assert_eq!(router.state(1),PadState::default());
 // A reconnected native controller has a new ID; another identical model
 // must not inherit the vanished controller's held state or ownership.
 let reconnected=snapshot(3,true,false,[0.,0.],0.);
 assert_eq!(router.update(vec![two.clone(),reconnected]),vec![PadEvent::Back(3),PadEvent::Press(3,Button::South)]);
 assert_eq!(router.uuid(3),[0;16]);assert!(!router.connected(1));
 let mut reported=Pads::default();router.report_status(&mut reported,Some(3));
 let Some(Value::Status{backend,controllers})=reported.devices.routed else{panic!("router status missing")};assert!(backend);
 assert_eq!(controllers.iter().map(|v|match v{Value::Controller{active,..}=>*active,_=>panic!("not a controller")}).collect::<Vec<_>>(),[false,true]);
 assert_eq!(router.update(vec![]),vec![PadEvent::Gone(2),PadEvent::Gone(3)]);
 handoffs();
}
fn handoffs(){
 let mut pads=Pads::default();let mut router=PadRouter::default();
 let one=snapshot(41,true,false,[0.5,-0.3],0.8);
 let two=snapshot(42,false,true,[-0.4,0.2],0.);
 assert!(pads.read_snapshots(vec![one.clone(),two.clone()],Some(one.clone())).held(Button::South));
 pads.menu.armed=Some(Button::South);
 // An undelivered notice and the menu's press belong to P1, not to routing.
 pads.connections.push(Connection::Connected(GlyphStyle::Nintendo));
 router.adopt(&mut pads);
 assert_eq!(router.state(41).left,[0.5,-0.3]);
 assert_eq!(router.device(41),Some(Device{name:"pad41".into(),vendor:Some(0x054c),product:None}));
 assert_eq!(pads.menu.armed,Some(Button::South));
 assert_eq!(pads.connections,vec![Connection::Connected(GlyphStyle::Nintendo)]);
 // The existing A and trigger hold cannot create a new join offer on adoption.
 assert!(router.update(vec![two.clone(),one.clone()]).is_empty());
 // Split routing releases the normal poll cache every frame; adopting again must not
 // erase connected identities or turn that unchanged hold into another press.
 pads.release_hardware();router.adopt(&mut pads);
 assert!(router.update(vec![one.clone(),two.clone()]).is_empty());
 let released=snapshot(41,false,false,[0.,0.],0.);
 assert!(router.update(vec![released.clone(),two.clone()]).is_empty());
 assert_eq!(router.update(vec![one.clone(),two.clone()]),vec![PadEvent::Press(41,Button::South),PadEvent::Press(41,Button::LeftTrigger)]);
 // P1 was assigned pad 42 while routing. Its metadata and connection baseline return
 // together, and pad 41's armed confirm cannot fire on pad 42's eventual release.
 pads.feed_source=Some(42);router.hand_back(&mut pads);
 assert_eq!(pads.devices.active,Some(42));assert!(pads.devices.started);
 assert_eq!(pads.device.as_ref().map(|device|device.name.as_str()),Some("pad42"));
 assert_eq!(pads.style,GlyphStyle::PlayStation);assert_eq!(pads.menu.armed,None);
 assert_eq!(pads.connections,vec![Connection::Connected(GlyphStyle::Nintendo)]);
 assert!(pads.read_snapshots(vec![two.clone(),one.clone()],Some(two.clone())).held(Button::East));
 assert_eq!(pads.connections,vec![Connection::Connected(GlyphStyle::Nintendo)]);
 // Repeated hand-back is inert; a later adopt uses the returned snapshots to
 // suppress held inputs again, while new devices still announce and press.
 router.hand_back(&mut pads);router.adopt(&mut pads);
 assert!(router.update(vec![two.clone(),one.clone()]).is_empty());
 let three=snapshot(43,true,false,[0.,0.],0.);
 assert_eq!(router.update(vec![two.clone(),three.clone()]),vec![PadEvent::Gone(41),PadEvent::Back(43),PadEvent::Press(43,Button::South)]);
 pads.release_hardware();router.hand_back(&mut pads);
 pads.connections.clear();
 assert!(pads.read_snapshots(vec![three.clone()],Some(three.clone())).held(Button::South));
 assert_eq!(pads.connections,vec![Connection::Disconnected{active:true}]);
 // A disappearance while routed must not leave P1's device descriptor behind.
 router.adopt(&mut pads);assert_eq!(router.update(vec![]),vec![PadEvent::Gone(43)]);
 pads.release_hardware();router.hand_back(&mut pads);
 assert_eq!(pads.device,None);assert_eq!(pads.devices.active,None);
 pads.connections.clear();pads.read_snapshots(vec![],None);assert!(pads.connections.is_empty());
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
