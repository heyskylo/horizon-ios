"""Exercise touch-generated pointer events through keyboard-focus handoff.

Uses the production conversion and handoff methods; window/GPU endpoints are
small test doubles. This covers ordering, not a physical UIKit interaction.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    touch = (ROOT / 'crates/horizon-client/src/game/touch.rs').read_text()
    pad = (ROOT / 'crates/horizon-client/src/game/pad_menu.rs').read_text()
    methods = '\n'.join(method(touch, n) for n in ('touch_pointer', 'touch_button', 'touch_mouse_button', 'touch_wheel', 'touch_scroll', 'touch_window_event', 'touch_synthetic_input'))
    methods += '\n' + '\n'.join('pub(crate) ' + method(pad, n) for n in ('key_focus_mouse', 'key_focus_to_mouse'))
    controller = (ROOT / 'crates/horizon-client/src/game/pad.rs').read_text()
    methods += '\n' + method(controller, 'pad_yield_input')
    emote = (ROOT / 'crates/horizon-client/src/game/cosmetics/emote_wheel.rs').read_text().replace('self.cosmetics.wheel', 'self.emote_wheel').replace('self.cosmetics.emotes', 'self.emotes').replace('pub(in crate::game)', 'pub(super)')
    methods += '\n' + method(emote, 'cancel_emote_controller')
    return STUBS + '\nmod production { use super::*; impl Game {\n' + methods + '\n}\n' + CASES + '\n}\n'


STUBS = r'''
#![allow(dead_code)]
#[derive(Clone,Copy,Debug,PartialEq)]enum InputOrigin{Game}
mod input_trace{pub(crate) use super::InputOrigin;}
trait EventLoopCtl {} struct Loop; impl EventLoopCtl for Loop {}
#[derive(Clone, Copy, Debug, PartialEq)] struct PhysicalPosition<T>{x:T,y:T}
impl<T> PhysicalPosition<T>{fn new(x:T,y:T)->Self{Self{x,y}}}
struct DeviceId; impl DeviceId {fn dummy()->Self{Self}}
#[derive(Clone, Copy, PartialEq)] enum ElementState {Pressed,Released}
enum MouseButton {Left}
struct KeyEvent{state:ElementState}
enum WindowEvent {KeyboardInput{event:KeyEvent},CursorMoved{device_id:DeviceId,position:PhysicalPosition<f64>},MouseInput{device_id:DeviceId,state:ElementState,button:MouseButton},MouseWheel{device_id:DeviceId,delta:MouseScrollDelta,phase:TouchPhase}}
enum MouseScrollDelta{LineDelta(f32,f32)}enum TouchPhase{Moved}
struct Window;impl Window{fn id(&self)->u32{1}}
struct Batch{scale:f32}
struct Target{rect:[f32;4]}
#[derive(Default)]struct Menu{keyboard:bool,synthetic:bool,origin:Option<[f32;4]>,focus:Option<Target>,pos:Option<(f32,f32)>,pending:Option<()>,nudge:Option<()>,real_pointer:Option<PhysicalPosition<f64>>}
#[derive(Default)]struct Pad{menu:Menu,in_use:bool,held_actions:Vec<&'static str>,released_actions:Vec<&'static str>}
#[derive(Default)]struct TouchInput{synthetic:bool}
type WindowId=u32;
#[derive(Default)]struct Wheel{controller:Option<()>}
struct Game{emote_wheel:Wheel,touch:TouchInput,window:Option<Window>,pad:Pad,cursor:PhysicalPosition<f64>,clicks:Vec<PhysicalPosition<f64>>,scale:f32,airplay_scale:f32}
impl Game{fn pad_radial_captures(&self)->bool{false}
 fn screen_batch(&self)->Option<Batch>{Some(Batch{scale:self.scale})}
 fn screen_point(&self,p:(f32,f32))->(f32,f32){(p.0*self.airplay_scale,p.1*self.airplay_scale)}
 fn handle_window_event(&mut self,l:&dyn EventLoopCtl,_:u32,e:WindowEvent,origin:InputOrigin){
  assert_eq!(origin,InputOrigin::Game);
  if !self.pad.menu.synthetic { assert!(self.touch.synthetic); }
  self.key_focus_mouse(l,&e);
  match e {WindowEvent::CursorMoved{position,..}=>self.cursor=position,WindowEvent::MouseInput{state:ElementState::Pressed,..}=>self.clicks.push(self.cursor),_=>{}}
 }
 // The action dispatcher endpoint records releases rather than running a
 // world. Input ownership itself comes from the production pad_yield_input.
 fn pad_cancel_inputs(&mut self,_:&dyn EventLoopCtl){self.pad.released_actions.append(&mut self.pad.held_actions);}
 fn controller(&mut self){self.emote_wheel.controller=Some(());self.pad.in_use=true;self.pad.held_actions=vec!["key.forward","key.attack"];}
 fn pad_synthetic_event(&mut self,l:&dyn EventLoopCtl,id:u32,e:WindowEvent){self.pad.menu.synthetic=true;self.handle_window_event(l,id,e,InputOrigin::Game);self.pad.menu.synthetic=false;}
 fn new()->Self{Self{emote_wheel:Wheel::default(),touch:TouchInput::default(),window:Some(Window),pad:Pad::default(),cursor:PhysicalPosition::new(0.,0.),clicks:vec![],scale:2.,airplay_scale:1.}}
 fn keyboard(&mut self){self.pad.menu.keyboard=true;self.pad.menu.focus=Some(Target{rect:[1.,1.,20.,20.]});self.pad.menu.pos=Some((1.,1.));self.cursor=PhysicalPosition::new(1.,1.);}
}
'''
CASES = r'''
#[test]fn first_touch_after_keyboard_uses_finger_not_stale_mouse(){
 let mut g=Game::new();g.controller();g.pad.menu.real_pointer=Some(PhysicalPosition::new(900.,800.));g.keyboard();
 g.touch_pointer(&Loop,(32.,47.));g.touch_button(&Loop,true);
 assert_eq!(g.clicks,vec![PhysicalPosition::new(64.,94.)]);assert!(g.emote_wheel.controller.is_none());assert!(!g.pad.menu.keyboard);
 assert!(!g.pad.in_use);assert!(g.pad.held_actions.is_empty());assert_eq!(g.pad.released_actions,vec!["key.forward","key.attack"]);
}
#[test]fn repeated_keyboard_touch_switches_preserve_each_new_location(){
 let mut g=Game::new();
 for n in 1..6 {g.keyboard();g.touch_pointer(&Loop,(n as f32*20.,n as f32*11.));g.touch_button(&Loop,true);g.touch_button(&Loop,false);}
 assert_eq!(g.clicks.len(),5);assert_eq!(g.clicks[4],PhysicalPosition::new(200.,110.));
}
#[test]fn touch_scroll_releases_keyboard_focus_at_the_finger(){let mut g=Game::new();g.keyboard();g.touch_pointer(&Loop,(20.,30.));g.touch_wheel(&Loop,1.);assert!(!g.pad.menu.keyboard);assert_eq!(g.cursor,PhysicalPosition::new(40.,60.));}
#[test]fn airplay_controller_maps_before_focus_handoff(){
 let mut g=Game::new();g.airplay_scale=3.;g.keyboard();g.touch_pointer(&Loop,(10.,20.));g.touch_button(&Loop,true);
 assert_eq!(g.clicks,vec![PhysicalPosition::new(60.,120.)]);
}
#[test]fn keyboard_press_yields_controller_but_release_does_not(){
 let mut g=Game::new();g.controller();
 g.key_focus_mouse(&Loop,&WindowEvent::KeyboardInput{event:KeyEvent{state:ElementState::Released}});
 assert!(g.pad.in_use);assert!(g.pad.released_actions.is_empty());
 g.key_focus_mouse(&Loop,&WindowEvent::KeyboardInput{event:KeyEvent{state:ElementState::Pressed}});
 assert!(!g.pad.in_use);assert_eq!(g.pad.released_actions,vec!["key.forward","key.attack"]);
 g.key_focus_mouse(&Loop,&WindowEvent::KeyboardInput{event:KeyEvent{state:ElementState::Pressed}});
 assert_eq!(g.pad.released_actions.len(),2);
}
#[test]fn synthetic_pad_cursor_cannot_yield_its_own_controller(){
 let mut g=Game::new();g.controller();
 g.pad_synthetic_event(&Loop,1,WindowEvent::CursorMoved{device_id:DeviceId::dummy(),position:PhysicalPosition::new(40.,60.)});
 assert!(g.pad.in_use);assert!(g.pad.released_actions.is_empty());assert_eq!(g.pad.menu.real_pointer,None);
 g.touch_pointer(&Loop,(30.,40.));
 assert!(!g.pad.in_use);assert_eq!(g.pad.released_actions,vec!["key.forward","key.attack"]);
 assert_eq!(g.pad.menu.real_pointer,Some(PhysicalPosition::new(60.,80.)));
}
'''


class MobileTouchHandoffTests(unittest.TestCase):
    def test_production_touch_keyboard_pointer_handoff(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required for the production-method probe')
        with tempfile.TemporaryDirectory(prefix='horizon-touch-handoff-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', '--test', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn('6 passed', run.stdout)
