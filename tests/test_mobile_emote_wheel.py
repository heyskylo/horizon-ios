"""Exercise the production wheel draw/event/ownership methods with graphics endpoints recorded.

No game assets or account services: the renderer records actual widget geometry;
input events run the same wheel methods as touch, keyboard, and pad translation.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import re
from test_mobile_command_keyboard import method
ROOT=Path(__file__).resolve().parents[1]


def program(game_source=None):
    source=(ROOT/'crates/horizon-client/src/game/cosmetics/emote_wheel.rs').read_text().replace('self.cosmetics.wheel', 'self.emote_wheel').replace('self.cosmetics.emotes', 'self.emotes').replace('pub(in crate::game)', 'pub(super)')
    source = re.sub(r"self\s*\.cosmetics\s*\.emotes", "self.emotes", source)
    source = re.sub(r"self\s*\.cosmetics\s*\.wheel", "self.emote_wheel", source)
    # The separate actual-Game catalogue fixture is tested with the real game;
    # retain the complete production wheel and its original geometry unit tests.
    fixture_start=source.index('// Isolated actual-Game fixture:')
    fixture_end=source.index('#[cfg(test)]',fixture_start)
    source=source[:fixture_start]+source[fixture_end:]
    assert source.count("impl Game<'_>") == 1
    source=source.replace('use crate::game::{Game, Screen};', 'use crate::winit;\nuse super::{Game, Screen};')
    game_source=game_source or (ROOT/'crates/horizon-client/src/game/pointer_capture.rs').read_text()
    pause=method(game_source,'pauses_game')
    pause += '\n}\n' + method(game_source, 'top_screen_pauses') + "\nimpl Game<'_> {\n"
    return STUBS+'\nmod emote {\n'+source+"\nimpl Game<'_> {\n"+pause+'\n}\n'+CASES+'\n}\n'


STUBS=r'''
#![allow(dead_code,unused_imports)]
extern crate self as horizon_client;
mod winit{pub mod keyboard{#[derive(Clone,Copy,PartialEq,Eq,Hash)]pub enum KeyCode{KeyB,Other}pub enum PhysicalKey{Code(KeyCode),Unidentified}}
pub mod event{use super::keyboard::PhysicalKey;#[derive(Clone,Copy,PartialEq)]pub enum ElementState{Pressed,Released}#[derive(Clone,Copy,PartialEq)]pub enum MouseButton{Left,Right}pub struct KeyEvent{pub state:ElementState,pub physical_key:PhysicalKey}pub enum WindowEvent{KeyboardInput{event:KeyEvent},MouseInput{state:ElementState,button:MouseButton},Other}}}
mod gamepad{#[derive(Clone,Copy,PartialEq,Eq,Hash)]pub enum Button{South,East,LeftTrigger,RightTrigger}#[derive(Default)]pub struct PadState{pub buttons:Vec<Button>,pub triggers:[f32;2]}impl PadState{pub fn held(&self,b:Button)->bool{self.buttons.contains(&b)}}}
mod text{pub struct Text(String);impl Text{pub fn literal(s:impl AsRef<str>)->Self{Self(s.as_ref().into())}pub fn translate(s:&str,_:&[Self])->Self{Self(if s=="gui.cancel"{"Cancel"}else{s}.into())}pub fn plain(&self)->String{self.0.clone()}}}
mod widgets{#[derive(Clone,Copy)]pub enum Action{Back}pub struct ScreenInfo;impl ScreenInfo{pub fn new(_:&str,_:&str)->Self{Self}}#[derive(Clone,Debug)]pub struct Target{pub name:String,pub rect:[f32;4]}
thread_local!{pub static TARGETS:std::cell::RefCell<Vec<Target>>=const{std::cell::RefCell::new(Vec::new())};}
pub fn begin_screen(f:impl FnOnce()->ScreenInfo){f();TARGETS.with_borrow_mut(Vec::clear);}pub fn record(s:&str,r:[f32;4],_:bool){TARGETS.with_borrow_mut(|v|v.push(Target{name:s.into(),rect:r}));}pub fn pointer_binding(_:[f32;4],_:String){}
}
mod hud{use crate::text::Text;thread_local!{pub static TEXT:std::cell::RefCell<Vec<String>>=const{std::cell::RefCell::new(Vec::new())};}pub struct HudAtlas;pub struct HudBatch{pub width:f32,pub height:f32}impl HudBatch{pub fn fill_quad(&mut self,_:&HudAtlas,_:[[f32;2];4],_:u32){}pub fn text_width(_:&HudAtlas,s:&str)->f32{s.len() as f32*6.}pub fn text(&mut self,_:&HudAtlas,_:&str,_:f32,_:f32,_:u32,_:bool){}pub fn text_styled(&mut self,_:&HudAtlas,text:&Text,_:f32,_:f32,_:u32,_:bool){TEXT.with_borrow_mut(|v|v.push(text.plain()));}}}
mod ui{use crate::{hud::*,widgets,text::Text};pub fn hovered(p:(f32,f32),x:f32,y:f32,w:f32,h:f32)->bool{p.0>=x&&p.0<x+w&&p.1>=y&&p.1<y+h}pub fn action_button(_:&mut HudBatch,_:&HudAtlas,_:widgets::Action,x:f32,y:f32,w:f32,h:f32,t:&Text,on:bool,_:bool){widgets::record(&t.plain(),[x,y,w,h],on);}}
#[derive(Clone,Copy,Debug,PartialEq)]enum Screen{Playing,Paused,EmoteWheel,HudEditor,Options,Stats,Advancements,WorldOptions,Friends,Social,ServerLinks,ConfirmLink,DialogWarning,CommandBlock,DebugOptions}
struct Overlay;impl Overlay{fn pauses_world(&self)->bool{true}}
impl Screen{fn is_pause_screen(self)->bool{matches!(self,Self::Paused|Self::Options|Self::Stats|Self::Advancements|Self::WorldOptions|Self::HudEditor)}}
#[derive(Default)]struct Catalogue{reported:Option<&'static str>}impl Catalogue{fn status(&self)->Option<&'static str>{self.reported}}
#[derive(Default)]struct Emotes{catalogue:Catalogue}
struct Item{id:String}struct Gui{atlas:hud::HudAtlas}struct Pad{owner_epoch:u64}struct Controller{trigger_threshold:f32}struct Keys;impl Keys{fn translate(&self,k:winit::keyboard::KeyCode)->Option<Result<winit::keyboard::KeyCode,()>>{Some(Ok(k))}}struct Settings{keys:Keys,controller:Controller}
struct Game<'a>{emotes:Emotes,refresh_calls:usize,title:Option<()>,book:Option<Overlay>,dialogs:Overlay,singleplayer:bool,published:bool,split:bool,_marker:std::marker::PhantomData<&'a ()>,screen:Screen,emote_wheel:emote::Wheel,keys:std::collections::HashSet<winit::keyboard::KeyCode>,left:bool,right:bool,mining:Option<()>,captured:bool,using:bool,owned:Vec<Item>,played:Vec<String>,gui:Gui,cursor:(f32,f32),size:(f32,f32),pad:Pad,settings:Settings,focused:bool}
impl Game<'_>{fn new(screen:Screen,n:usize)->Self{Self{emotes:Emotes::default(),refresh_calls:0,title:None,book:None,dialogs:Overlay,singleplayer:true,published:false,split:false,_marker:std::marker::PhantomData,screen,emote_wheel:Default::default(),keys:std::collections::HashSet::new(),left:true,right:true,mining:Some(()),captured:true,using:true,owned:(0..n).map(|i|Item{id:format!("emote{i}")}).collect(),played:vec![],gui:Gui{atlas:hud::HudAtlas},cursor:(0.,0.),size:(320.,240.),pad:Pad{owner_epoch:7},settings:Settings{keys:Keys,controller:Controller{trigger_threshold:0.4}},focused:true}}
fn dialog_open(&self)->bool{false}fn has_singleplayer_server(&self)->bool{self.singleplayer}fn is_published(&self)->bool{self.published}fn has_split_players(&self)->bool{self.split}fn win_screen_pauses(&self)->bool{false}
fn refresh_owned_emotes(&mut self){self.refresh_calls+=1;}fn release_use_for_keybind_screen(&mut self){self.using=false;}fn capture(&mut self,on:bool){self.captured=on;}fn owned_emotes(&mut self)->&[Item]{&self.owned}fn play_emote(&mut self,id:&str)->bool{self.played.push(id.into());true}fn emote_name(&self,id:&str)->String{id.into()}fn gui_size(&self)->(f32,f32){self.size}fn pad_focused(&self)->bool{self.focused}}
'''
CASES=r'''
fn draw(g:&mut Game){crate::hud::TEXT.with_borrow_mut(Vec::clear);g.draw_emote_wheel(&mut HudBatch{width:g.size.0,height:g.size.1});}
fn press(g:&mut Game){assert!(g.emote_wheel_event(&WindowEvent::MouseInput{state:ElementState::Pressed,button:MouseButton::Left}));}
#[test]fn draw_refreshes_catalogue_and_displays_loading_retry_and_empty_status(){
 let mut g=Game::new(Screen::Paused,0);g.open_emote_wheel();let before=g.refresh_calls;
 for status in ["horizon.emote_wheel.loading","horizon.emote_wheel.retry"] {
  g.emotes.catalogue.reported=Some(status);draw(&mut g);
  let text=crate::hud::TEXT.with_borrow(Clone::clone);
  assert!(text.iter().any(|s|s==status));assert!(!text.iter().any(|s|s=="horizon.emote_wheel.empty"));
  let targets=crate::widgets::TARGETS.with_borrow(Clone::clone);assert_eq!(targets.len(),1);assert_eq!(targets[0].name,"Cancel");assert!(g.pauses_game());
 }
 assert_eq!(g.refresh_calls,before+2,"paused frames must still refresh catalogue");
 g.emotes.catalogue.reported=None;draw(&mut g);assert!(crate::hud::TEXT.with_borrow(|v|v.iter().any(|s|s=="horizon.emote_wheel.empty")));
 let mut g=Game::new(Screen::Playing,2);g.open_emote_wheel();g.emotes.catalogue.reported=Some("horizon.emote_wheel.retry");draw(&mut g);
 assert_eq!(g.emote_wheel.displayed,["emote0","emote1"]);assert!(crate::hud::TEXT.with_borrow(|v|v.iter().any(|s|s=="horizon.emote_wheel.retry")));assert_eq!(crate::widgets::TARGETS.with_borrow(|v|v.len()),3);
}
#[test]fn pause_parent_keeps_simulation_paused_until_selection(){
 let mut g=Game::new(Screen::Paused,1);assert!(g.pauses_game());g.open_emote_wheel();draw(&mut g);assert!(g.pauses_game());
 g.close_emote_wheel(None);assert_eq!(g.screen,Screen::Paused);assert!(g.pauses_game());
 g.open_emote_wheel();draw(&mut g);g.close_emote_wheel(Some(0));assert_eq!(g.screen,Screen::Playing);assert!(!g.pauses_game());
 let mut g=Game::new(Screen::Playing,1);g.open_emote_wheel();draw(&mut g);assert!(!g.pauses_game());g.close_emote_wheel(None);assert!(!g.pauses_game());
 for shared in 0..3{let mut g=Game::new(Screen::Paused,0);if shared==0{g.published=true}else if shared==1{g.split=true}else{g.singleplayer=false};g.open_emote_wheel();assert!(!g.pauses_game(),"multiplayer/split policy must stay unchanged");}
}
#[test]fn drawn_sector_targets_activate_even_when_hover_cache_is_stale(){
 for size in [(320.,240.),(480.,262.),(800.,450.)]{
 let mut g=Game::new(Screen::Paused,8);g.size=size;g.open_emote_wheel();draw(&mut g);
 let targets=crate::widgets::TARGETS.with_borrow(Clone::clone);assert_eq!(targets.len(),9);
 for (i,t) in targets[..8].iter().enumerate(){let [x,y,w,h]=t.rect;assert!(x>=0.&&y>=0.&&x+w<=size.0&&y+h<=size.1);for other in &targets[i+1..]{let [a,b,c,d]=other.rect;assert!(!(x<a+c&&a<x+w&&y<b+d&&b<y+h));}}
 let target=&targets[3];g.cursor=(target.rect[0]+target.rect[2]/2.,target.rect[1]+target.rect[3]/2.);g.emote_wheel.hovered=Some(0);press(&mut g);assert_eq!(g.played,["emote3"]);assert_eq!(g.screen,Screen::Playing);assert!(g.captured);
 }
}
#[test]fn empty_wheel_has_drawn_cancel_and_returns_to_pause(){let mut g=Game::new(Screen::Paused,0);g.open_emote_wheel();assert!(!g.using&&!g.left&&!g.right&&g.mining.is_none());draw(&mut g);let targets=crate::widgets::TARGETS.with_borrow(Clone::clone);assert_eq!(targets.len(),1);assert_eq!(targets[0].name,"Cancel");g.cursor=(g.size.0/2.,g.size.1/2.);press(&mut g);assert_eq!(g.screen,Screen::Paused);assert!(!g.captured);assert!(g.played.is_empty());}
#[test]fn real_keyboard_release_keeps_normal_release_bookkeeping(){let mut g=Game::new(Screen::Playing,1);g.open_emote_wheel();draw(&mut g);g.cursor=(160.,120.-61.*wheel_scale(320.,240.));let consumed=g.emote_wheel_event(&WindowEvent::KeyboardInput{event:winit::event::KeyEvent{state:ElementState::Released,physical_key:PhysicalKey::Code(KeyCode::KeyB)}});assert!(!consumed);assert_eq!(g.played,["emote0"]);}
#[test]fn controller_release_survives_gameplay_mapping_cancel_but_not_owner_change(){
 let mut g=Game::new(Screen::Playing,1);g.open_emote_wheel();draw(&mut g);g.emote_controller_opened(Button::South);g.cursor=(160.,120.-61.*wheel_scale(320.,240.));
 let held=PadState{buttons:vec![Button::South],..Default::default()};g.poll_emote_controller(&held);assert!(g.played.is_empty());g.poll_emote_controller(&PadState::default());assert_eq!(g.played,["emote0"]);
 let mut g=Game::new(Screen::Playing,1);g.open_emote_wheel();draw(&mut g);g.emote_controller_opened(Button::South);g.cursor=(160.,60.);g.pad.owner_epoch+=1;g.poll_emote_controller(&PadState::default());assert!(g.played.is_empty());assert_eq!(g.screen,Screen::EmoteWheel);
 g.emote_controller_opened(Button::South);g.focused=false;g.poll_emote_controller(&PadState::default());assert!(g.played.is_empty());
}
'''


class MobileEmoteWheelTests(unittest.TestCase):
    def test_production_wheel_targets_events_and_controller_ownership(self):
        if not shutil.which('rustc'): self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-mobile-emote-') as folder:
            source=Path(folder)/'probe.rs';binary=Path(folder)/'probe';source.write_text(program())
            build=subprocess.run(['rustc','--edition=2024','--test',str(source),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(build.returncode,0,build.stdout+build.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
