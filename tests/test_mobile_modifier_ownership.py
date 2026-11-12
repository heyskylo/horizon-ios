"""Real modifier-latch healing must preserve touch/controller-owned actions.

The probe uses production healing, logical held-source tracking and toggle
state. Only physical modifier reports and key-binding lookup are endpoints.
"""
from pathlib import Path
import shutil
import re
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method
from test_mobile_multiplayer_keyboard import block

ROOT = Path(__file__).resolve().parents[1]


def program():
    game = (ROOT / 'crates/horizon-client/src/game/play_input.rs').read_text()
    modules = '\n'.join(f'#[path="{ROOT / path}"] mod {name};' for name, path in (
        ('held', 'crates/horizon-client-input/src/held.rs'),
        ('actions', 'crates/horizon-client-input/src/actions.rs'),
        ('keymapping', 'crates/horizon-client-input/src/keymapping.rs'),
    ))
    methods = '\n'.join(method(game, name) for name in ('sync_toggle', 'heal_hold_latches', 'mapped_key', 'toggle_held_elsewhere', 'input_holds'))
    helpers = '\n'.join(method(game, name) for name in ('studio_replay_shortcut', 'studio_freecam_movement_key'))
    methods += '\nfn touch_sneak(&mut self, down:bool)->Result<()> { self.mapped_key(KeyCode::ShiftLeft,down,false,KeyCode::ShiftLeft,Source::Touch) }'
    actions = (ROOT / 'crates/horizon-client-input/src/actions.rs').read_text()
    codes = sorted(set(re.findall(r'KeyCode::(\w+)', actions + methods + helpers + STUBS + CASES)))
    stubs = STUBS.replace('enum KeyCode{KEY_CODES}', 'enum KeyCode{' + ','.join(codes) + '}')
    methods = methods.replace('self.replay.playing()', 'self.studio_replay.is_some()').replace('self.studio.flying()', 'self.studio_freecam.active')
    return stubs + modules + helpers + '\nimpl Game {\n' + methods + '\n}\n' + CASES


STUBS = r'''
#![allow(dead_code)]
extern crate self as winit;
extern crate self as horizon_client;
pub mod keyboard {#[derive(Clone,Copy,Debug,Hash,PartialEq,Eq)]pub enum KeyCode{KEY_CODES}}
use keyboard::KeyCode;use actions::Action;use held::Source;use keymapping::{KeyAction,SNEAK};
type Result<T>=std::result::Result<T,String>;
#[derive(Clone,Copy,PartialEq)]enum Screen{Playing,Menu}
#[derive(Clone,Copy,PartialEq)]enum Control{Sneak,Sprint}
#[derive(Default)]struct TouchInput{toggle_keys:Vec<KeyCode>}
fn gameplay_has_keyboard(screen:Screen,title:bool,dialog:bool)->bool{matches!(screen,Screen::Playing)&&!title&&!dialog}
use std::collections::HashSet;
pub mod keybinds{
 use super::KeyCode;
 pub fn key_code(name:&str)->Option<KeyCode>{Some(match name{"shift"=>KeyCode::ShiftLeft,"control"=>KeyCode::ControlLeft,"a"=>KeyCode::KeyA,_=>return None})}
}
struct Bindings{rebound:bool}
impl Bindings{fn translate_all(&self,key:KeyCode)->Vec<std::result::Result<KeyCode,()>>{vec![Ok(key)]}fn get(&self,mapping:&str)->&'static str{if self.rebound{"a"}else if mapping=="key.sneak"{"shift"}else{"control"}}}
#[derive(Default)]struct Controller{toggle_sneak:bool}
#[derive(Default)]struct Freecam{active:bool}
struct Settings{toggle_crouch:bool,toggle_sprint:bool,keys:Bindings,controller:Controller}
struct Modifiers{shift:bool,control:bool}
impl Modifiers{fn shift_key(&self)->bool{self.shift}fn control_key(&self)->bool{self.control}fn alt_key(&self)->bool{false}fn super_key(&self)->bool{false}}
struct Pad{held:Vec<&'static str>}
impl Pad{fn holds_action(&self,name:&str)->bool{self.held.contains(&name)}}
struct Game{touch:TouchInput,studio_replay:Option<()>,studio_freecam:Freecam,screen:Screen,title:Option<()>,dialog:bool,dispatched:Vec<KeyCode>,keys:held::HeldKeys,physical_keys:HashSet<KeyCode>,toggles:keymapping::ToggleKeys,modifiers:Modifiers,settings:Settings,pad:Pad}
impl Game{
 fn new()->Self{Self{touch:TouchInput::default(),studio_replay:None,studio_freecam:Freecam::default(),screen:Screen::Playing,title:None,dialog:false,dispatched:vec![],keys:held::HeldKeys::default(),physical_keys:HashSet::new(),toggles:keymapping::ToggleKeys::default(),modifiers:Modifiers{shift:false,control:false},settings:Settings{toggle_crouch:false,toggle_sprint:false,keys:Bindings{rebound:false},controller:Controller::default()},pad:Pad{held:vec![]}}}
 fn dialog_open(&self)->bool{self.dialog}fn screen_over_playing(&self)->bool{self.dialog}
 fn key_pressed(&mut self,key:KeyCode,_:KeyCode)->Result<()>{self.dispatched.push(key);Ok(())}
 fn press(&mut self,slot:usize,source:Source){self.toggles.key(slot,keymapping::KeyAction::Press,false,false);self.keys.press([KeyCode::ShiftLeft,KeyCode::ControlLeft][slot],source);}
}
'''

CASES = r'''
fn main(){
 // Direct touch sneak/sprint have no hardware Shift/Control modifier bit.
 // Repeated latch healing must not cancel those actions while still owned.
 for slot in 0..2 {
  let key=[KeyCode::ShiftLeft,KeyCode::ControlLeft][slot];let mut g=Game::new();
  g.press(slot,Source::Touch);g.physical_keys.insert(key);
  for _ in 0..3 {g.heal_hold_latches();assert!(g.keys.held_by(&key,Source::Touch));assert!(g.toggles.is_down(slot));}
  assert!(!g.physical_keys.contains(&key));
  // Releasing touch really releases its modifier action; it is not a latch
  // that remains merely because it was once protected during healing.
  g.keys.release(&key,Source::Touch);g.toggles.key(slot,keymapping::KeyAction::Release,false,false);g.heal_hold_latches();
  assert!(!g.keys.contains(&key));assert!(!g.toggles.is_down(slot));
 }
 // Exercise the actual default-action dispatcher, not only HeldKeys:
 // releasing either physical or touch Shift must keep the other's sneak
 // action down, while the final release really ends the action.
 for (key,slot) in [(KeyCode::ShiftLeft,0),(KeyCode::ControlLeft,1)] {
  for (first,second) in [(Source::Touch,Source::Keyboard),(Source::Keyboard,Source::Touch),(Source::Pad,Source::Touch)] {
   let mut g=Game::new();g.mapped_key(key,true,false,key,first).unwrap();
   assert!(g.keys.held_by(&key,first));
   if first!=Source::Keyboard {assert!(!g.keys.held_by(&key,Source::Keyboard),"sync must not invent a physical keyboard hold");}
   g.mapped_key(key,true,false,key,second).unwrap();g.mapped_key(key,false,false,key,first).unwrap();
   assert!(g.toggles.is_down(slot));assert!(g.keys.held_by(&key,second));assert!(!g.keys.held_by(&key,first));
   g.mapped_key(key,false,false,key,second).unwrap();assert!(!g.toggles.is_down(slot));assert!(!g.keys.contains(&key));
   assert!(g.toggles.0[slot].consume_click());assert!(g.toggles.0[slot].consume_click());assert!(!g.toggles.0[slot].consume_click());
  }
 }
 // An OS repeat remains a click of ToggleKeyMapping but is not a second
 // physical hold that needs an extra release.
 let mut g=Game::new();let key=KeyCode::ShiftLeft;
 g.mapped_key(key,true,false,key,Source::Touch).unwrap();g.mapped_key(key,true,true,key,Source::Touch).unwrap();g.mapped_key(key,false,false,key,Source::Touch).unwrap();
 assert!(!g.toggles.is_down(0));assert!(!g.keys.contains(&key));assert_eq!(g.dispatched,vec![key]);
 // A touch Shift hold and release preserve independent hardware ownership.
 let mut g=Game::new();g.mapped_key(key,true,false,key,Source::Keyboard).unwrap();
 g.touch_sneak(true).unwrap();assert!(g.keys.held_by(&key,Source::Touch));assert!(g.keys.held_by(&key,Source::Keyboard));
 g.touch_sneak(false).unwrap();assert!(!g.keys.held_by(&key,Source::Touch));assert!(g.toggles.is_down(0));
 g.mapped_key(key,false,false,key,Source::Keyboard).unwrap();assert!(!g.toggles.is_down(0));
 // Healing a stale physical hold while touch remains down removes only the
 // stale keyboard ownership. The subsequent touch release ends the action.
 let mut g=Game::new();g.mapped_key(key,true,false,key,Source::Keyboard).unwrap();g.mapped_key(key,true,false,key,Source::Touch).unwrap();g.heal_hold_latches();
 assert!(!g.keys.held_by(&key,Source::Keyboard));assert!(g.keys.held_by(&key,Source::Touch));assert!(g.toggles.is_down(0));
 g.mapped_key(key,false,false,key,Source::Touch).unwrap();assert!(!g.toggles.is_down(0));
 // Vanilla toggle-crouch still flips on each press, from any input device,
 // and keeps its latch after releases. A menu blocks new toggle presses.
 let mut g=Game::new();g.settings.toggle_crouch=true;
 g.mapped_key(key,true,false,key,Source::Touch).unwrap();g.mapped_key(key,false,false,key,Source::Touch).unwrap();assert!(g.toggles.is_down(0));
 g.mapped_key(key,true,false,key,Source::Keyboard).unwrap();g.mapped_key(key,false,false,key,Source::Keyboard).unwrap();assert!(!g.toggles.is_down(0));
 g.screen=Screen::Menu;g.mapped_key(key,true,false,key,Source::Touch).unwrap();assert!(!g.toggles.is_down(0));assert!(!g.keys.contains(&key));
 // The existing keyboard stale-modifier repair remains effective.
 let mut g=Game::new();g.press(0,Source::Keyboard);g.physical_keys.insert(KeyCode::ShiftLeft);g.physical_keys.insert(KeyCode::ShiftRight);g.heal_hold_latches();
 assert!(!g.keys.contains(&KeyCode::ShiftLeft));assert!(!g.toggles.is_down(0));assert!(g.physical_keys.is_empty());
 // A controller action still protects its held mapping independently.
 let mut g=Game::new();g.press(0,Source::Pad);g.pad.held.push("key.sneak");g.heal_hold_latches();assert!(g.keys.held_by(&KeyCode::ShiftLeft,Source::Pad));
 // A real modifier still protects the keyboard mapping.
 let mut g=Game::new();g.press(1,Source::Keyboard);g.modifiers.control=true;g.heal_hold_latches();assert!(g.keys.held_by(&KeyCode::ControlLeft,Source::Keyboard));
 // Shift's OS modifier report says nothing about a rebound sneak key.
 let mut g=Game::new();g.settings.keys.rebound=true;g.press(0,Source::Keyboard);g.heal_hold_latches();assert!(g.keys.contains(&KeyCode::ShiftLeft));
}
'''


class MobileModifierOwnershipTests(unittest.TestCase):
    def test_production_modifier_healing_respects_input_owners(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-modifier-ownership-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
