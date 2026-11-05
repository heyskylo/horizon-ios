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
    code=routing_program().replace('mod horizon_gamecontroller;', 'mod native_bridge;').replace('struct Pads;', TYPES)
    code += '\nimpl Pads{'+method(source,'read')+'}\n'+method(menu,'armed_click')
    code += '\nimpl Game{fn release(&mut self,button:Button)->Result<(),()>{let down=false;let event_loop=&();'+release+'Ok(())}}\n'
    return code.replace('fn main(){','fn main(){\n device_handoff_does_not_click();')+CASES


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
#[derive(Default)]struct Devices{connected:usize,active:Option<u64>}
#[derive(Default)]struct Menu{armed:Option<Rect>,focus:Option<Target>,held:[bool;2],repeat:Option<(Button,usize,bool)>}
#[derive(Default)]struct Pads{devices:Devices,menu:Menu,style:GlyphStyle}
#[derive(Default)]struct Game{pad:Pads,clicks:usize}
impl Game{fn pad_click_at(&mut self,_:&(),_:(f32,f32),_:bool){self.clicks+=1;}fn pad_pointer_button(&mut self,_:&(),_:bool,_:bool){}}
'''
CASES=r'''
fn choose(id:Option<u64>,pressed:bool){horizon_gamecontroller::ACTIVE.with_borrow_mut(|p|*p=id.map(|id|snapshot(id,pressed,false,[0.,0.],0.)));}
fn arm(g:&mut Game){let rect=[0.,0.,100.,20.];g.pad.menu.armed=Some(rect);g.pad.menu.focus=Some(Target{rect});}
fn device_handoff_does_not_click(){
 let mut g=Game::default();choose(Some(1),true);g.pad.read();arm(&mut g);
 choose(Some(1),false);g.pad.read();g.release(Button::South).unwrap();assert_eq!(g.clicks,1,"same-pad release activates");
 choose(Some(1),true);g.pad.read();arm(&mut g);
 choose(None,false);g.pad.read();g.release(Button::South).unwrap();assert_eq!(g.clicks,1,"disconnect is not a release-click");
 choose(Some(2),true);g.pad.read();arm(&mut g);
 choose(Some(3),false);g.pad.read();g.release(Button::South).unwrap();assert_eq!(g.clicks,1,"same-model controller switch is not a release-click");
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
