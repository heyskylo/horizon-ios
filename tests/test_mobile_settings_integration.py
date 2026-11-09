"""Execute the mobile settings facade and focused-field contract without a GPU."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]
RENDER = ROOT / 'crates/horizon-client-render/src'


def program():
    extras = (RENDER / 'extras.rs').read_text()
    mobile = (RENDER / 'extras/mobile_ui.rs').read_text()
    options = (RENDER / 'options_ui.rs').read_text()
    desktop_start = extras.index('mod desktop_ui {')
    desktop_enum = extras[extras.index('#[derive(Clone, Copy, Debug', desktop_start):extras.index('/// The Extras page:', desktop_start)]
    mobile_enum = mobile[mobile.index('#[derive(Clone, Copy, Debug'):mobile.index('/// The Extras page:')]
    code = '#![allow(dead_code)]\n' + STUBS
    code += 'mod extras {use crate::{pvp,horizon_cosmetics};\npub mod desktop_ui {\n' + desktop_enum
    code += 'impl Item {\n' + '\n'.join("pub(crate) " + method(extras, n) for n in ['description_key', 'description', 'description_text']) + '}\n}\n'
    code += 'pub mod mobile_ui {use super::*;\n' + mobile_enum + "pub(crate) " + method(mobile, 'rows')
    code += '\nimpl Item {\n' + '\n'.join("pub(crate) " + method(mobile, n) for n in ['shared', 'description_key', 'description', 'description_text']) + '}\n}}\n'
    # Graphics/controller input moved to the shared settings shell. Execute its
    # production Hosted methods rather than the removed per-screen Ui methods.
    hosted = (RENDER / 'settings_shell.rs').read_text()
    hosted = hosted[hosted.index('impl<S: Screen + Default> Hosted for Shell<S> {'):]
    code += 'mod shell {use crate::EditBox;\n'
    code += '#[derive(Clone,Copy,PartialEq)]pub enum Target{Search,Done}\n'
    code += 'pub trait Hosted{fn typing(&self)->bool;fn focused_rect(&self)->Option<[f32;4]>;}\n'
    code += '#[derive(Default)]pub struct Ui{pub search:EditBox,pub focus:Option<Target>}\nimpl Hosted for Ui{'
    code += method(hosted, 'typing') + method(hosted, 'focused_rect') + '} }\n'
    for module in ['graphics_ui', 'controller_ui']:
        code += 'mod ' + module + '{pub use crate::shell::{Ui,Target};#[derive(Clone,Copy)]pub enum Page{General,Advanced,Hdr,Bindings}}\n'
    code += 'mod shader_ui{pub enum Tab{Packs,Screen(usize)}}\n'
    code += OPTIONS_TYPES
    code += 'impl OptionsUi {\n' + '\n'.join(method(options, n) for n in ['page', 'settings_search_active', 'typing', 'focused_rect', 'graphics_page', 'controller_page', 'controller_open', 'video_open', 'shell', 'shader_page']) + '}\n'
    # Exercise the native-only match arm on the host; iOS compilation checks its cfg.
    code += 'const SORT_KEY:&str="horizon.sort";const ZOOM_KEY:&str="horizon.zoom";\n#[derive(Default)]struct ExtraKeys{sort_key:String,zoom_key:String,pvp:pvp::Keys}\nimpl ExtraKeys{'
    code += method(extras, 'set_key').replace('#[cfg(target_os = "ios")]', '') + '}\n'
    return code + CASES


STUBS = r'''
mod horizon_cosmetics {pub fn enabled()->bool{false}}
mod lang {
 pub fn get_or(key:&str,fallback:&str)->String {
  match key {
   "horizon.extras.fullbright.tooltip"=>"Shared translated lighting".into(),
   "horizon.extras.zoom.tooltip"=>"Native translated zoom".into(),
   _=>fallback.into(),
  }
 }
}
mod pvp {
 pub const HUD_EDITOR_KEY:&str="horizon.hudEditor";
 #[derive(Default)]pub struct Keys{pub editor_key:String}
 #[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Opt{Hud}
 impl Opt{pub fn description_key(self)->&'static str{"horizon.pvp.hud.tooltip"}
 pub fn description(self)->&'static str{"HUD fallback"}}
}
#[derive(Default)]struct EditBox{x:f32,y:f32,width:f32,height:f32,focused:bool}
#[derive(Default)]struct Packs{focused:bool}
impl Packs{fn search_focused(&self)->bool{self.focused}fn search_rect(&self)->Option<[f32;4]>{self.focused.then_some([1.,2.,3.,4.])}}
'''

OPTIONS_TYPES = r'''
#[derive(Clone,Copy,PartialEq,Default)]enum Page{
 #[default]Main,Language,Packs,Video,Controls,KeyBinds,ModBrowser,Graphics,GraphicsAdvanced,GraphicsHdr,Controller,PadBinds,Extras,ShaderPacks,ShaderOptions,
}
#[derive(Default)]struct OptionsUi{
 stack:Vec<Page>,resource_error:Option<String>,settings_search:EditBox,language_search:EditBox,
 pack_screen:Option<Packs>,graphics:graphics_ui::Ui,controller:controller_ui::Ui,extras:shell::Ui,shader:shell::Ui,
}
'''

CASES = r'''
fn main(){
 let mut keys=ExtraKeys::default();
 assert!(keys.set_key("horizon.zoom","key.keyboard.c".into()));assert_eq!(keys.zoom_key,"key.keyboard.c");
 assert!(keys.set_key("horizon.sort","key.keyboard.r".into()));assert_eq!(keys.sort_key,"key.keyboard.r");
 assert!(keys.set_key(pvp::HUD_EDITOR_KEY,"key.keyboard.h".into()));assert_eq!(keys.pvp.editor_key,"key.keyboard.h");
 assert!(!keys.set_key("key.jump","key.keyboard.space".into()));assert_eq!(keys.zoom_key,"key.keyboard.c");
 use extras::mobile_ui::{Item,rows};
 let rows:Vec<_>=rows().into_iter().filter_map(Result::ok).collect();
 for item in [Item::Zoom,Item::ZoomKey,Item::ZoomLevel,Item::ZoomSmooth,Item::ZoomSensitivity,
 Item::Fps,Item::FpsCorner,Item::FpsLows,Item::Keystrokes,Item::KeystrokesCorner,
 Item::KeystrokesCps,Item::Minimap,Item::MinimapCorner,Item::MinimapSize,
 Item::MinimapShape,Item::MinimapZoom,Item::MinimapRotate,Item::OwnNameTag] {
  assert_eq!(rows.iter().filter(|&&row|row==item).count(),1,"{item:?}");
 }
 for item in [Item::Fullbright,Item::DynamicLights,Item::HideBlockParticles]{assert!(!rows.contains(&item));}
 assert_eq!(Item::Fullbright.description_key(),"horizon.extras.fullbright.tooltip");
 assert_eq!(Item::Fullbright.description_text(),"Shared translated lighting");
 assert_eq!(Item::Zoom.description_text(),"Native translated zoom");
 assert_eq!(Item::ZoomLevel.description_text(),"The zoom the key starts at.");
 for item in rows {assert!(item.description_key().starts_with("horizon."));assert!(!item.description_text().is_empty());}
 let mut ui=OptionsUi::default();
 for page in [Page::Graphics,Page::GraphicsAdvanced,Page::GraphicsHdr]{
  ui.stack=vec![page];assert!(ui.video_open());assert!(!ui.typing());assert_eq!(ui.focused_rect(),None);
  ui.graphics.focus=Some(graphics_ui::Target::Search);
  ui.graphics.search=EditBox{x:15.,y:25.,width:140.,height:18.,focused:false};
  assert!(ui.typing());assert_eq!(ui.focused_rect(),Some([15.,25.,140.,18.]));
  ui.resource_error=Some("bad pack".into());assert!(!ui.typing());assert_eq!(ui.focused_rect(),None);
  ui.resource_error=None;ui.graphics.focus=None;assert_eq!(ui.focused_rect(),None);
 }
 for page in [Page::Controller,Page::PadBinds]{
  ui.stack=vec![page];assert!(!ui.video_open());ui.controller.focus=Some(controller_ui::Target::Search);
  ui.controller.search=EditBox{x:20.,y:40.,width:180.,height:20.,focused:false};
  assert!(ui.typing());assert_eq!(ui.focused_rect(),Some([20.,40.,180.,20.]));
 }
 ui.stack=vec![Page::Language];assert!(!ui.typing());assert_eq!(ui.focused_rect(),None);
 ui.language_search=EditBox{x:10.,y:12.,width:100.,height:20.,focused:true};
 assert_eq!(ui.focused_rect(),Some([10.,12.,100.,20.]));
 ui.stack=vec![Page::Packs];ui.pack_screen=Some(Packs{focused:true});assert_eq!(ui.focused_rect(),Some([1.,2.,3.,4.]));
 ui.stack=vec![Page::Main];assert!(!ui.typing());assert_eq!(ui.focused_rect(),None);
}
'''


class MobileSettingsIntegrationTests(unittest.TestCase):
    def test_mobile_facade_and_keyboard_hit_rectangles(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-mobile-settings-') as directory:
            source = Path(directory) / 'probe.rs'
            binary = Path(directory) / 'probe'
            source.write_text(program())
            built = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
