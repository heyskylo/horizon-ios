"""Actual HUD cache and game reload/visibility methods, with resource and drawing endpoints isolated."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program(case):
    pvp = (ROOT / 'crates/horizon-client/src/game/extras/pvp.rs').read_text()
    options = (ROOT / 'crates/horizon-client/src/game/options.rs').read_text()
    hud = (ROOT / 'crates/horizon-client-render/src/pvp_hud.rs').read_text().split('#[cfg(test)]')[0]
    states = []
    for name in ('PvpState', 'Keystrokes'):
        declaration = pvp.index(f'struct {name} {{')
        start = pvp.rfind('#[derive(Default)]', 0, declaration)
        end = pvp.index('\n}', declaration) + 2
        states.append(pvp[start:end].replace('pub(super) ', '').replace('pub(in crate::game) ', '').replace('crate::game::hud_editor::HudEditor', 'HudEditor').replace('super::hud_editor::HudEditor', 'HudEditor'))
    state = '\n'.join(states)
    methods = method(options, 'load_language') + method(pvp, 'pvp_effects_shown')
    methods += method(pvp, 'invalidate_pvp_hud')
    return STUBS + '\nmod pvp_hud {\n' + hud + '}\nuse pvp_hud::{Combo, Widget, Line, Icon};\nuse text::Text;\n' + state + '\nimpl Game {\n' + methods + '}\n' + HELPERS + case


STUBS = r'''
#![allow(dead_code)]
use std::{path::PathBuf,sync::{Arc,Mutex,atomic::{AtomicU32,Ordering}}};
static LANGUAGE:Mutex<String>=Mutex::new(String::new());
static WIDTH:AtomicU32=AtomicU32::new(1);
mod text{#[derive(Clone,Debug,PartialEq)]pub struct Text(pub String);impl Text{pub fn literal(s:impl Into<String>)->Self{Self(s.into())}pub fn plain(&self)->String{self.0.clone()}}}
mod lang{pub struct Language(String);impl Language{pub fn load(_:&[&std::path::PathBuf],code:&str)->Self{Self(code.into())}}pub fn set(l:Language){*crate::LANGUAGE.lock().unwrap()=l.0;}}
#[derive(Default)]struct CrosshairCache;
struct HudEditor{session:u32}
#[derive(Clone,Copy)]enum Corner{TopRight}
#[derive(Clone,Copy)]struct Hud{effects:bool,effects_corner:Corner}
struct PvpOptions{hud:Hud}struct Extras{pvp:PvpOptions}struct Settings{extras:Extras}
struct Options{resources:PathBuf}
struct Chat;impl Chat{fn rescale(&mut self,_:&()){}}
struct Ui{chat:Chat}struct Gui{atlas:()}
struct Creative;impl Creative{fn forget_items(&mut self){}}
struct Game{pvp:PvpState,options:Options,ui:Ui,gui:Gui,creative_screen:Creative,settings:Settings,corner_visible:bool}
impl Game{
 fn new()->Self{Self{pvp:PvpState::default(),options:Options{resources:PathBuf::new()},ui:Ui{chat:Chat},gui:Gui{atlas:()},creative_screen:Creative,settings:Settings{extras:Extras{pvp:PvpOptions{hud:Hud{effects:true,effects_corner:Corner::TopRight}}}},corner_visible:true}}
 fn refresh_remote_language(&mut self){}
 fn native_corner_shown(&self,_:Corner)->bool{self.corner_visible}
}
'''

HELPERS = r'''
fn refresh<K:PartialEq+Default>(w:&mut Widget<K>)->bool{w.refresh(|_,lines|{let label=LANGUAGE.lock().unwrap().clone();lines.push(Line::new(Icon::None,label,&|s|s.len() as f32*WIDTH.load(Ordering::SeqCst)as f32));})}
fn seed(s:&mut PvpState){refresh(&mut s.armour);refresh(&mut s.effects);refresh(&mut s.held);refresh(&mut s.ping);refresh(&mut s.coords);refresh(&mut s.server);refresh(&mut s.combo_count);refresh(&mut s.fps);refresh(&mut s.cps);refresh(&mut s.keys.cps);refresh(&mut s.clock);refresh(&mut s.reach_shown);refresh(&mut s.speed_shown);}
fn verify(s:&mut PvpState){
 assert!(refresh(&mut s.armour));assert!(refresh(&mut s.effects));assert!(refresh(&mut s.held));assert!(refresh(&mut s.ping));assert!(refresh(&mut s.coords));assert!(refresh(&mut s.server));assert!(refresh(&mut s.combo_count));
 assert!(refresh(&mut s.fps));assert!(refresh(&mut s.cps));assert!(refresh(&mut s.keys.cps));assert!(refresh(&mut s.clock));assert!(refresh(&mut s.reach_shown));assert!(refresh(&mut s.speed_shown));
 assert!(s.keys.labels.is_empty());assert_eq!(s.keys.held,[true;7]);assert_eq!(s.keys.bound[0],"key.keyboard.w");
 assert_eq!(s.editor.as_ref().unwrap().session,7,"a reload must preserve the active editor session");
 assert_eq!(s.effects.lines[0].text.plain(),"de");
 assert!(s.chat_search);assert_eq!(s.chat_search_aside,"/warp");assert_eq!(s.combo.count(0.1),1);
}
'''

CACHE_CASE = r'''
fn main(){
 let mut g=Game::new();*LANGUAGE.lock().unwrap()="en".into();seed(&mut g.pvp);
 g.pvp.editor=Some(HudEditor{session:7});
 g.pvp.keys.held=[true;7];g.pvp.keys.bound[0]="key.keyboard.w".into();g.pvp.keys.labels.push((Text::literal("Attack"),6.0));
 g.pvp.chat_search=true;g.pvp.chat_search_aside="/warp".into();g.pvp.combo.hurt(7,Some(true),0.0);
 g.load_language("de");verify(&mut g.pvp);
 assert_eq!(g.pvp.effects.lines[0].text_width,2.0);
 // A resource-pack font change must rebuild widths even with unchanged widget keys.
 WIDTH.store(4,Ordering::SeqCst);g.invalidate_pvp_hud();verify(&mut g.pvp);
 assert_eq!(g.pvp.effects.lines[0].text_width,8.0);
 // Ordinary unchanged frames still keep their allocation-free caches.
 assert!(!refresh(&mut g.pvp.effects));
}
'''

VISIBILITY_CASE = r'''
fn main(){let mut g=Game::new();assert!(g.pvp_effects_shown());g.corner_visible=false;
 assert!(!g.pvp_effects_shown(),"vanilla icons were hidden although the debug screen suppressed the custom widget");
 g.corner_visible=true;assert!(g.pvp_effects_shown());g.settings.extras.pvp.hud.effects=false;assert!(!g.pvp_effects_shown());}
'''


class PvpHudReloadTests(unittest.TestCase):
    def run_case(self, case):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-pvp-hud-reload-') as folder:
            source, binary = Path(folder) / 'probe.rs', Path(folder) / 'probe'
            source.write_text(program(case))
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)

    def test_language_reload_rebuilds_labels_and_pack_reload_rebuilds_widths(self):
        self.run_case(CACHE_CASE)

    def test_hidden_effect_widget_restores_vanilla_icons(self):
        self.run_case(VISIBILITY_CASE)
