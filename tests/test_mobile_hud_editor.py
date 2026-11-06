"""Production touch classification, gesture reducer and HUD mutation handlers."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method
ROOT=Path(__file__).resolve().parents[1]


def program(touch_source=None):
    source=(ROOT/'crates/horizon-client/src/game/hud_editor.rs').read_text()
    touch=touch_source or (ROOT/'crates/horizon-client/src/game/touch.rs').read_text()
    pad=(ROOT/'crates/horizon-client/src/game/pad_menu.rs').read_text()
    layout=(ROOT/'crates/horizon-client-render/src/pvp_hud.rs').read_text()
    struct=source[source.index('#[derive(Default)]'):source.index('/// The style buttons')].replace('pub(super)', 'pub(crate)')
    helpers='\n'.join(method(source,n) for n in ['buttons','selection_buttons','control_at'])
    methods='\n'.join(method(source,n) for n in ['widget_under_cursor','hud_editor_drag_target','hud_editor_press','hud_editor_drag','hud_editor_release','hud_editor_scroll','resize_hud_widget'])
    methods+='\n'+method(touch,'touch_menu_mode')+'\n'+method(pad,'pad_pointer_screen')
    pure='\n'.join('pub(crate) '+method(layout,n) for n in ['to_fractions','snap'])
    code=STUBS.replace('MENU_SOURCE',str(ROOT/'crates/horizon-client/src/touch/menu.rs')).replace('PURE_LAYOUT',pure)
    return code+struct+helpers+'\nimpl Game{'+methods+'}\n'+CASES


STUBS=r'''
#![allow(dead_code)]
extern crate self as horizon_client;
#[path="MENU_SOURCE"]mod menu;use menu::{MenuMode,MenuGesture};
#[derive(Clone,Copy,Debug,PartialEq)]enum WidgetId{First,Second}
#[derive(Clone,Copy,Debug,PartialEq)]struct Place{at:Option<(f32,f32)>,scale:u8}impl Default for Place{fn default()->Self{Self{at:None,scale:100}}}
#[derive(Default)]struct Layout{places:[Place;2]}impl Layout{fn get_mut(&mut self,id:WidgetId)->&mut Place{&mut self.places[id as usize]}}
mod pvp_hud{pub const SNAP:f32=4.;pub const SCALES:(u8,u8)=(50,200);PURE_LAYOUT}
mod ui{pub fn hovered(at:(f32,f32),x:f32,y:f32,w:f32,h:f32)->bool{at.0>=x&&at.0<x+w&&at.1>=y&&at.1<y+h}}
mod widgets{pub enum Kind{Slider{value:f32},Field,Slot,Button}pub struct Target{pub kind:Kind,pub rect:[f32;4]}pub fn last_targets()->Vec<Target>{vec![]}}
mod touch{pub fn hit_rect(r:[f32;4],p:(f32,f32))->bool{crate::ui::hovered(p,r[0],r[1],r[2],r[3])}}
#[derive(Clone,Copy,PartialEq)]enum Screen{Playing,HudEditor,Inventory,Creative,Stats,Advancements,Options}
#[derive(PartialEq)]enum Page{Packs,Other}struct Options;impl Options{fn page(&self)->Page{Page::Other}}struct UI{options:Options}
struct Pvp{boxes:Vec<(WidgetId,[f32;4])>,editor:Option<HudEditor>}struct Hud{background:bool}struct PvpSettings{layout:Layout,hud:Hud}struct Extras{pvp:PvpSettings}struct Settings{extras:Extras}impl Settings{fn save(&self,_:&std::path::Path){}}
struct Audio;impl Audio{fn play_button_click(&self){}}
#[derive(Clone,Copy)]struct Opt;impl Opt{fn cycle(&self,_:&mut PvpSettings,_:bool){}}
const STYLE:[Opt;4]=[Opt;4];const BUTTON_W:f32=150.;const BUTTON_H:f32=20.;const GAP:f32=4.;const TOOL_W:f32=(BUTTON_W*2.-GAP)/3.;
struct Game{screen:Screen,cursor:(f32,f32),size:(f32,f32),pvp:Pvp,settings:Settings,audio:Audio,book:Option<()>,ui:UI}
impl Game{fn new()->Self{Self{screen:Screen::HudEditor,cursor:(12.,12.),size:(480.,270.),pvp:Pvp{boxes:vec![(WidgetId::First,[10.,10.,40.,20.])],editor:Some(HudEditor::default())},settings:Settings{extras:Extras{pvp:PvpSettings{layout:Layout::default(),hud:Hud{background:true}}}},audio:Audio,book:None,ui:UI{options:Options}}}fn gui_size(&self)->(f32,f32){self.size}fn screen_point(&self,at:(f32,f32))->(f32,f32){at}fn has_shift_down(&self)->bool{false}fn options_path()->std::path::PathBuf{std::path::PathBuf::new()}fn close_hud_editor(&mut self){self.screen=Screen::Playing;}}
'''
CASES=r'''
fn tap(g:&mut Game,at:(f32,f32)){let mut gesture=MenuGesture::new(at,g.touch_menu_mode(at));for _ in 0..8{assert_eq!(gesture.frame().button,None);}gesture.end(at);assert_eq!(gesture.frame().button,None);assert_eq!(gesture.frame().button,Some(true));g.cursor=at;g.hud_editor_press(false);assert_eq!(gesture.frame().button,Some(false));g.hud_editor_release();}
#[test]fn touch_drags_real_hud_state_and_tools_resize_reset_selected_widget(){
 let mut g=Game::new();assert!(g.pad_pointer_screen(),"controller must point at freely positioned widgets");
 let at=(12.,12.);assert_eq!(g.touch_menu_mode(at),MenuMode::Drag);let mut gesture=MenuGesture::new(at,g.touch_menu_mode(at));assert_eq!(gesture.frame().button,None);assert_eq!(gesture.frame().button,Some(true));g.hud_editor_press(false);
 let to=(102.,62.);assert_eq!(gesture.moved(to).scroll,0.);g.cursor=to;g.hud_editor_drag();let place=g.settings.extras.pvp.layout.places[0];assert_eq!(place.at,Some((100./440.,60./250.)));
 gesture.end(to);assert_eq!(gesture.frame().button,Some(false));g.hud_editor_release();assert!(g.pvp.editor.as_ref().unwrap().drag.is_none());
 let controls=selection_buttons(g.size);tap(&mut g,(controls[1].0+TOOL_W/2.,controls[1].1+10.));assert_eq!(g.settings.extras.pvp.layout.places[0].scale,110);
 tap(&mut g,(controls[0].0+TOOL_W/2.,controls[0].1+10.));assert_eq!(g.settings.extras.pvp.layout.places[0].scale,100);
 tap(&mut g,(controls[2].0+TOOL_W/2.,controls[2].1+10.));assert_eq!(g.settings.extras.pvp.layout.places[0],Place::default());
}
#[test]fn panel_buttons_have_priority_over_a_hud_widget_underneath(){let mut g=Game::new();g.pvp.boxes[0].1=[0.,0.,480.,270.];for at in buttons(g.size).into_iter().chain(selection_buttons(g.size)){let point=(at.0+5.,at.1+5.);assert_eq!(g.touch_menu_mode(point),MenuMode::Tap);assert!(!g.hud_editor_drag_target(point));}}
'''


class MobileHudEditorTests(unittest.TestCase):
    def test_production_hud_touch_and_controller_manipulation(self):
        if not shutil.which('rustc'):self.skipTest('rustc required')
        with tempfile.TemporaryDirectory(prefix='horizon-hud-input-') as folder:
            source=Path(folder)/'probe.rs';binary=Path(folder)/'probe';source.write_text(program())
            built=subprocess.run(['rustc','--edition=2024','--test',str(source),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(built.returncode,0,built.stdout+built.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
