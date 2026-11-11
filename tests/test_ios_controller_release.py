"""Production UIKit pad selection and menu release cannot click on a device handoff."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_ios_controller_routing import program as routing_program
from test_mobile_command_keyboard import method
from test_mobile_multiplayer_keyboard import block
ROOT=Path(__file__).resolve().parents[1]


def program(source=None):
    source=source or (ROOT/'crates/horizon-client/src/game/pad/ios.rs').read_text()
    menu=(ROOT/'crates/horizon-client/src/game/pad_menu.rs').read_text()
    release=method(menu,'pad_menu_button')
    release=block(release,release.index('        if !down {'))
    code=routing_program().replace('mod horizon_gamecontroller;', 'mod native_bridge;')
    # Reuse production snapshot/router methods, with a real menu rectangle and
    # controllable native bridge for this fixture. Routing's standalone cases
    # have their own menu stub and run in test_ios_controller_routing instead.
    snapshot=method(code,'snapshot')
    code=code[:code.index('\nfn snapshot(')]
    begin=code.index('#[derive(Default)]struct Menu')
    end=code.index('impl Pads {',begin)
    code=code[:begin]+TYPES+code[end:]
    code += '\nimpl Pads{'+method(source,'read')+'}\n'+method(menu,'armed_click')+'\n'+menu[menu.index('const SAME_RECT:'):menu.index(';',menu.index('const SAME_RECT:'))+1]
    code += '\nimpl Game{fn release(&mut self,button:Button)->Result<(),()>{let down=false;let event_loop=&();'+release+'Ok(())}}\n'
    return code+'\n'+snapshot+'\nfn main(){device_handoff_does_not_click();}\n'+CASES


TYPES=r'''
extern crate self as log;
#[macro_export]macro_rules!info{($($arg:tt)*)=>{let _=format_args!($($arg)*);};}
mod horizon_gamecontroller{
 pub use crate::native_bridge::{Controller,Snapshot,ConnectedController};
 thread_local!{pub static ACTIVE:std::cell::RefCell<Option<ConnectedController>>=const{std::cell::RefCell::new(None)};}
 pub fn active_controller()->Option<ConnectedController>{ACTIVE.with_borrow(Clone::clone)}
 pub fn active()->Option<Snapshot>{active_controller().map(|p|p.state)}
 pub fn snapshots()->Vec<ConnectedController>{active_controller().into_iter().collect()}
 pub fn controllers()->Vec<Controller>{snapshots().into_iter().map(|p|p.controller).collect()}
}
type Rect=[f32;4];
#[derive(Clone,Copy)]struct Target{rect:Rect}impl Target{fn center(&self)->(f32,f32){(self.rect[0]+self.rect[2]/2.,self.rect[1]+self.rect[3]/2.)}}
#[derive(Default)]struct HeldTrigger{pressed_a:bool}
#[derive(Default)]struct Menu{trigger:Option<HeldTrigger>,armed:Option<Rect>,focus:Option<Target>,held:[bool;2],repeat:Option<(Button,usize,bool)>}
#[derive(Default)]struct Pads{peaks:[[f32;2];2],owner_epoch:u64,devices:Devices,menu:Menu,style:GlyphStyle,device:Option<Device>,connections:Vec<Connection>,feed_source:Option<PadId>}
#[derive(Debug,PartialEq)]enum Connection{Connected(GlyphStyle),Disconnected{active:bool}}
#[derive(Default)]struct Game{pad:Pads,clicks:usize}
impl Game{fn pad_recipe_book_showing(&self)->bool{false}fn pad_tab(&mut self,_:&(),_:bool){panic!("recipe book must not be reached by device-handoff probe");}fn pad_click_at(&mut self,_:&(),_:(f32,f32),_:bool){self.clicks+=1;}fn pad_pointer_button(&mut self,_:&(),_:bool,_:bool){}}
'''
CASES=r'''
fn choose(id:Option<u64>,pressed:bool){horizon_gamecontroller::ACTIVE.with_borrow_mut(|p|*p=id.map(|id|snapshot(id,pressed,false,[0.,0.],0.)));}
fn arm(g:&mut Game){let rect=[0.,0.,100.,20.];g.pad.menu.armed=Some(rect);g.pad.menu.focus=Some(Target{rect});}
fn device_handoff_does_not_click(){
 let mut g=Game::default();choose(Some(1),true);g.pad.read();assert_eq!(g.pad.device.as_ref().map(|d|d.name.as_str()),Some("pad1"));assert!(g.pad.connections.is_empty(),"initial connected pad is not a new connection");arm(&mut g);
 choose(Some(1),false);g.pad.read();g.release(Button::South).unwrap();assert_eq!(g.clicks,1,"same-pad release activates");
 choose(Some(1),true);g.pad.read();arm(&mut g);
 choose(None,false);g.pad.read();g.release(Button::South).unwrap();assert_eq!(g.clicks,1,"disconnect is not a release-click");assert!(g.pad.device.is_none());assert_eq!(g.pad.connections,vec![Connection::Disconnected{active:true}]);
 choose(Some(2),true);g.pad.read();arm(&mut g);
 choose(Some(3),false);g.pad.read();g.release(Button::South).unwrap();assert_eq!(g.clicks,1,"same-model controller switch is not a release-click");assert_eq!(g.pad.device.as_ref().map(|d|d.name.as_str()),Some("pad3"));
 // Routing adopts the normal poll's held A, then hands P1 a different pad.
 // Exercise the real transfer methods and the actual menu release branch together.
 choose(Some(3),true);g.pad.read();arm(&mut g);
 let mut router=PadRouter::default();router.adopt(&mut g.pad);
 assert!(router.update(vec![snapshot(3,true,false,[0.,0.],0.)]).is_empty());
 assert_eq!(router.update(vec![snapshot(4,false,false,[0.,0.],0.)]),vec![PadEvent::Gone(3),PadEvent::Back(4)]);
 g.pad.feed_source=Some(4);g.pad.release_hardware();router.hand_back(&mut g.pad);
 g.release(Button::South).unwrap();assert_eq!(g.clicks,1,"routed device handoff is not a release-click");
 assert_eq!(g.pad.device.as_ref().map(|d|d.name.as_str()),Some("pad4"));
 // Keeping the same controller across routing still permits its legitimate release.
 choose(Some(4),true);g.pad.read();arm(&mut g);router.adopt(&mut g.pad);
 assert!(router.update(vec![snapshot(4,false,false,[0.,0.],0.)]).is_empty());
 g.pad.release_hardware();router.hand_back(&mut g.pad);
 g.release(Button::South).unwrap();assert_eq!(g.clicks,2,"same-pad release after routing activates");
}
'''


class IOSControllerReleaseTests(unittest.TestCase):
    def test_active_controller_changes_do_not_click_armed_menu_button(self):
        if not shutil.which('rustc'): self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-controller-release-') as directory:
            source=Path(directory)/'probe.rs';binary=Path(directory)/'probe';source.write_text(program())
            built=subprocess.run(['rustc','--edition=2024',str(source),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(built.returncode,0,built.stdout+built.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
