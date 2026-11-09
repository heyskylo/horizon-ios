"""Run real reload sampling/cancellation against production pad and touch reducers."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method
ROOT = Path(__file__).resolve().parents[1]

def program():
    game = (ROOT / 'crates/horizon-client/src/game/play_input.rs').read_text()
    pad = (ROOT / 'crates/horizon-client/src/game/pad.rs').read_text()
    touch = (ROOT / 'crates/horizon-client/src/game/touch.rs').read_text()
    buttons = (ROOT / 'crates/horizon-client-input/src/gamepad.rs').read_text()
    # Actual hardware state and edge/suppression reducer, without unrelated bindings.
    buttons = buttons[:buttons.index('/// Intentional activity')]
    buttons += '\nenum GlyphStyle{Xbox,Auto,PlayStation,Nintendo} struct PadSettings{move_deadzone:f32,look_deadzone:f32,trigger_threshold:f32}\n'
    radial = method((ROOT / 'crates/horizon-client-input/src/gamepad.rs').read_text(), 'radial')
    length = method((ROOT / 'crates/horizon-client-input/src/gamepad.rs').read_text(), 'length')
    start=pad.index('        for (i, (stick, deadzone))',pad.index('let activity ='))
    end=pad.index('        for button in std::mem::take',start)
    sticks='fn apply_sticks(&mut self,state:&mut PadState){'+pad[start:end]+'}'
    methods = sticks + method(game,'reload_blocks_input') + '\n' + method(touch,'cancel_touch_for_reload')
    methods += '\n' + method((ROOT / 'crates/horizon-client/src/game/presentation.rs').read_text(), 'menu_input_blocked')
    methods += '\n' + method((ROOT / 'crates/horizon-client/src/game/presentation.rs').read_text(), 'presentation_stale')
    for name in ['pad_reload_sample','finish_reload_input_disarm','pad_cancel_inputs','pad_button']:
        methods += '\n' + method(pad,name)
    return STUBS + '\nmod gamepad {\n' + buttons + '\npub '+radial+'\n'+length+'\n}\n' + f'#[path="{ROOT / "crates/horizon-client/src/touch.rs"}"]mod touch;\n' + '\nimpl Game {\n'+methods+'\n}\n'+CASES

STUBS = r'''
#![allow(dead_code,unused_imports)]
use gamepad::{Button,PadState,Edges};
type Result<T> = std::result::Result<T,String>;
#[derive(PartialEq)]enum Screen{Playing,EmoteWheel}
trait EventLoopCtl {} struct Loop; impl EventLoopCtl for Loop {}
#[macro_export] macro_rules! warn { ($($arg:tt)*) => {{ let _ = format_args!($($arg)*); }}; }
mod log {pub(crate) use crate::warn;}
#[derive(Default)]struct Menu{armed:Option<()>,osk:Option<()>,trigger:Option<()>}
struct Pad{reload_pending:bool,edges:Edges,state:PadState,menu:Menu,blocked_sticks:[bool;2],late_releases:Vec<Button>,actions:[Vec<&'static str>;16],held_drop:Option<()>,drop_stack:bool,in_use:bool}
impl Default for Pad {fn default()->Self{Self{reload_pending:false,edges:Edges::default(),state:PadState::default(),menu:Menu::default(),blocked_sticks:[false;2],late_releases:vec![],actions:std::array::from_fn(|_|vec![]),held_drop:None,drop_stack:false,in_use:false}}}
#[derive(Default)]struct Touch{menu:Option<()>,fingers:touch::Touches,late:Vec<()>,holding:Option<()>}
#[derive(Default)]struct Keys{touch:bool} enum Source{Touch}
impl Keys{fn release_source(&mut self,_:Source){self.touch=false;}}
struct Controller{move_deadzone:f32,look_deadzone:f32,trigger_threshold:f32}
impl Controller{fn actions_of(&self,_:Button)->Vec<&'static str>{vec!["key.jump"]}}
struct Settings{controller:Controller}
struct Renderer{current:bool} impl Renderer{fn presentation_current(&self)->bool{self.current}}
struct Game{renderer:Option<Renderer>,screen:Screen,captured:bool,focused:bool,menu_calls:usize,pad:Pad,touch:Touch,keys:Keys,pack_reload:Option<()>,transaction:bool,settings:Settings,releases:Vec<Button>,actions:Vec<(&'static str,bool)>,ime:bool,emote:bool}
impl Game{
 fn new()->Self{Self{renderer:None,screen:Screen::Playing,captured:false,focused:true,menu_calls:0,pad:Pad::default(),touch:Touch::default(),keys:Keys::default(),pack_reload:None,transaction:false,settings:Settings{controller:Controller{move_deadzone:0.2,look_deadzone:0.2,trigger_threshold:0.5}},releases:vec![],actions:vec![],ime:true,emote:true}}
 fn pad_focused(&self)->bool{true}
 fn pad_in_menu(&self)->bool{true}
 fn capture(&mut self,on:bool){self.captured=on;}
 fn pad_menu_button(&mut self,_:&dyn EventLoopCtl,_:Button,_:bool)->Result<()>{self.menu_calls+=1;Ok(())}
 fn pad_back(&mut self)->Result<()>{panic!("overlay must block Back")}
 fn emote_controller_opened(&mut self,_:Button){panic!("overlay must block emote")}
 fn content_reload_busy(&self)->bool{self.transaction}
 fn cancel_emote_controller(&mut self){self.emote=false;}
 fn suspend_system_keyboard(&mut self){self.ime=false;}
 fn pad_output_off(&mut self){}
 fn direct_craft_stop(&mut self){}
 fn pad_cancel_menu(&mut self,_:&dyn EventLoopCtl){assert!(self.pad.menu.armed.is_none());}
 fn pad_button_logged(&mut self,_:&dyn EventLoopCtl,b:Button,down:bool){assert!(!down);assert!(self.pad.menu.armed.is_none());self.releases.push(b);}
 fn pad_action(&mut self,a:&'static str,down:bool)->Result<()>{self.actions.push((a,down));Ok(())}
}
'''
CASES = r'''
#[test]fn all_reload_phases_disarm_real_holds_and_require_release(){
 for phase in 0..4 {
 let mut g=Game::new();match phase{0=>g.pack_reload=Some(()),1=>g.transaction=true,2=>g.pad.reload_pending=true,_=>g.renderer=Some(Renderer{current:false})};
 g.pad_button(&Loop,Button::Start,true).unwrap();g.pad_button(&Loop,Button::South,true).unwrap();
 assert!(!g.captured);assert_eq!(g.menu_calls,0);assert!(g.actions.is_empty());
 g.pad.actions[Button::South as usize].push("key.use");g.pad_button(&Loop,Button::South,false).unwrap();
 assert_eq!(g.menu_calls,0);assert_eq!(g.actions,vec![("key.use",false)]);g.actions.clear();
 let mut state=PadState::default();state.set(Button::South,true);state.left=[0.,1.];state.right=[1.,0.];state.triggers=[1.,0.];
 g.pad.menu.armed=Some(());g.pad.menu.osk=Some(());g.pad.actions[0].push("key.jump");g.pad.late_releases.push(Button::South);g.pad.held_drop=Some(());
 g.touch.menu=Some(());g.touch.late.push(());g.touch.holding=Some(());g.keys.touch=true;
 let r=touch::layout(1000.,600.).into_iter().find(|(c,_)|*c==touch::Control::Forward).unwrap().1;
 g.touch.fingers.start(1,(r[0]+r[2]/2.,r[1]+r[3]/2.),0.,&touch::layout(1000.,600.));
 assert!(g.pad_reload_sample(&Loop,state));assert!(!g.pad.reload_pending);
 assert!(g.pad.blocked_sticks.into_iter().all(|v|v));assert!(g.pad.late_releases.is_empty());assert!(g.pad.held_drop.is_none());
 assert_eq!(g.actions,vec![("key.jump",false)]);assert!(!g.ime);assert!(!g.emote);assert!(!g.keys.touch);assert!(g.touch.holding.is_none());assert!(g.touch.late.is_empty());assert!(!g.touch.fingers.held(touch::Control::Forward));
 assert!(g.touch.fingers.frame(20.).is_empty());assert!(g.touch.fingers.end(1).is_empty());
 assert!(!g.touch.fingers.start(2,(r[0]+r[2]/2.,r[1]+r[3]/2.),21.,&touch::layout(1000.,600.)).is_empty());
 g.pack_reload=None;g.transaction=false;g.renderer=None;assert!(!g.pad_reload_sample(&Loop,state));
 let mut held=state;g.apply_sticks(&mut held);assert_eq!(held.left,[0.;2]);assert_eq!(held.right,[0.;2]);
 let mut neutral=PadState::default();g.apply_sticks(&mut neutral);assert_eq!(g.pad.blocked_sticks,[false;2]);
 let mut fresh=state;g.apply_sticks(&mut fresh);assert_eq!(fresh.left,state.left);assert_eq!(fresh.right,state.right);
 assert!(g.pad.edges.update(&state,0.5).is_empty(),"held button cannot resume after overlay");
 assert!(g.pad.edges.update(&PadState::default(),0.5).is_empty());
 assert!(g.pad.edges.update(&state,0.5).contains(&(Button::South,true)),"fresh press re-arms");
 }
}
'''

class ReloadInputFenceTests(unittest.TestCase):
    def test_actual_pad_sample_disarm_and_touch_retirement(self):
        compiler=shutil.which('rustc')
        if not compiler: self.skipTest('rustc required')
        with tempfile.TemporaryDirectory(prefix='horizon-reload-input-') as folder:
            source=Path(folder)/'probe.rs';binary=Path(folder)/'probe';source.write_text(program())
            build=subprocess.run([compiler,'--edition=2024','--test',str(source),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(build.returncode,0,build.stdout+build.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=15)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
