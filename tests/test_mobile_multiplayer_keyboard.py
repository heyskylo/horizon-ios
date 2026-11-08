"""UIKit characters through the game's real title dispatch and native text hooks.

Only unrelated game/window services and the final text field are test doubles;
text extraction, key handling, title editing and hook routing are production.
"""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def block(source, start):
    opening = source.index('{', start)
    end, depth = opening + 1, 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]


def program(game=None, control=None):
    game = game or (ROOT / 'crates/horizon-client/src/game.rs').read_text()
    control = control or (ROOT / 'crates/horizon-client/src/game/control.rs').read_text()
    hooks = (ROOT / 'crates/horizon-client/src/game/native_hooks.rs').read_text()
    clip = (ROOT / 'crates/horizon-client/src/game/clipboard.rs').read_text()
    uikit = (ROOT / 'third_party/winit/src/platform_impl/ios/hardware_keyboard.rs').read_text()
    prefix = game[game.index('        let mut typed: Option<String> = None;'):game.index('        // Screen.keyPressed: Tab, the arrows and Enter')]
    # Its pattern contains braces: find the arm body's closing brace.
    start = game.index('                WindowEvent::KeyboardInput { event, .. } if self.title.is_some() =>')
    body = game.index('{', game.index('=>', start))
    arm = game[start:body] + block(game, body)
    callbacks = '\n'.join(method(uikit,n) for n in ['key_text','insert_text_keys','text_key_code','logical_key'])
    methods = '\n'.join(method(hooks,n) for n in ['screen_chars_typed','screen_text_typed'])
    methods += '\n'+method(control,'type_chars_into_screen')+'\n'+method(clip,'title_edit')
    helpers = '\n'.join(method(hooks,n) for n in ['screen_text_hooked','chars_to_type'])
    owns = method(control,'title_owns_text') if 'fn title_owns_text(' in control else 'fn title_owns_text(_: &Screen,_:bool)->bool{false}'
    keys = sorted(set(re.findall(r'KeyCode::(\w+)', callbacks+arm)) | {'KeyA'})
    names = sorted(set(re.findall(r'NamedKey::(\w+)', callbacks)))
    code = STUBS.replace('KEYS', ','.join(keys)).replace('NAMES', ','.join(names))
    code += '\n'+callbacks+'\n'+helpers+'\n'+owns+'\nmod control { use super::*; pub(crate) '+owns+'}\n'
    code += 'mod production { use super::*; impl Game {\n'+methods+'\nfn dispatch(&mut self, mut event:WindowEvent) { let event_loop=&();\n'+prefix
    code += '\nlet result:Result<()>=(|| { match event { '+arm+',_=>{} } Ok(())})(); result.unwrap(); self.screen_chars_typed(typed.as_deref());\n}\n}\n'+CASES+'\n}'
    return code


STUBS = r'''
#![allow(dead_code)]
use std::cell::RefCell;
type Result<T> = std::result::Result<T,String>;
#[derive(Clone,Copy,PartialEq)]enum KeyCode{KEYS}
#[derive(Clone,Copy,PartialEq)]enum NamedKey{NAMES}
#[derive(Clone,Copy,PartialEq)]enum NativeKeyCode{Unidentified}
#[derive(Clone,Copy,PartialEq)]enum PhysicalKey{Code(KeyCode),Unidentified(NativeKeyCode)}
#[derive(Clone,PartialEq)]enum NativeKey{Unidentified}
#[derive(Clone,PartialEq)]enum Key{Character(String),Named(NamedKey),Unidentified(NativeKey)}
mod smol_str { pub type SmolStr=String; }
#[derive(Clone,Copy,PartialEq)]enum ElementState{Pressed,Released}
struct KeyEvent{physical_key:PhysicalKey,state:ElementState,text:Option<String>,repeat:bool}
enum WindowEvent{KeyboardInput{event:KeyEvent},Other}
#[derive(Clone,Copy,PartialEq)]enum Screen{Playing,Options,Chat,CommandBlock,Social,BlockEditor,DebugOptions,WorldOptions,Inventory,Creative}
enum Action{None}
struct Title{value:String,focused_button:Option<u8>}
impl Title {
 fn type_text(&mut self,s:&str){self.value.push_str(s)}
 fn backspace(&mut self){self.value.pop();}
 fn has_caret_field(&mut self)->bool{true}
 fn focused_field(&mut self)->Option<(u8,&mut String,usize)>{Some((0,&mut self.value,128))}
 fn search_changed(&mut self){}
 fn tab_bar_key(&mut self,_:KeyCode,_:bool,_:bool)->Option<bool>{None}
 fn list_focused(&self)->bool{false}
 fn confirm_key_size(&mut self,_:(u32,u32))->(Action,bool){(Action::None,false)}
 fn field_key(&mut self,key:KeyCode,_:bool,_:bool){if key==KeyCode::Backspace{self.backspace()}}
 fn arrow_key(&mut self,_:KeyCode,_:(u32,u32),_:bool){}
 fn tab_dir(&mut self,_:bool,_:(u32,u32)){}
 fn escape(&mut self){} fn refresh_servers(&mut self){}
}
struct Selection; impl Selection{fn take(&mut self,_:u8,_:&mut String)->bool{false}}
struct Audio;impl Audio{fn play_button_click(&mut self){}}
struct Options;impl Options{fn type_text(&mut self,_:&(),_:&str){}}
struct UI{options:Options}struct GUI{atlas:()}
struct Creative{search:bool}impl Creative{fn tab(&self)->&Self{self}}
struct Game{screen:Screen,title:Option<Title>,modifiers:u32,book:Option<()>,sign_edit:Option<()>,ui:UI,gui:GUI,creative_screen:Creative,text_selection:Selection,audio:Audio,cancel:bool,cancel_key:bool,blocked:bool,hooked:RefCell<String>}
fn glfw_modifiers(m:u32)->u32{m}
impl Game {
 fn new()->Self{Self{screen:Screen::Playing,title:Some(Title{value:String::new(),focused_button:None}),modifiers:0,book:None,sign_edit:None,ui:UI{options:Options},gui:GUI{atlas:()},creative_screen:Creative{search:false},text_selection:Selection,audio:Audio,cancel:false,cancel_key:false,blocked:false,hooked:RefCell::new(String::new())}}
 fn screen_hooks_open(&self)->bool{true}fn screen_view_now(&self)->Option<()>{Some(())}
 fn types_text(&self)->bool{!self.blocked}
 fn screen_key_pressed(&self,_:KeyCode,_:bool)->bool{self.cancel_key}
 fn native_screen_char_typed_pre(&self,_:&(),c:char,_:u32,cancel:&mut bool){self.hooked.borrow_mut().push(c);*cancel=self.cancel;}
 fn edit_shortcut(&self,_:&KeyEvent)->Option<()>{None}fn title_shortcut(&mut self,_:()){}
 fn has_control_down_with_quirk(&self)->bool{false}fn has_control_down(&self)->bool{false}fn has_shift_down(&self)->bool{false}
 fn gui_size(&self)->(u32,u32){(400,300)}fn save_title_options(&mut self){}
 fn title_action(&mut self,_:&(),_:Action)->Result<()>{Ok(())}fn reopen_data_packs(&mut self,_:&())->Result<()>{Ok(())}
 fn book_type(&mut self,_:&str){}fn sign_input(&mut self,_:Option<KeyCode>,_:Option<&str>){}
 fn chat_type(&mut self,_:&str){}fn command_block_type(&mut self,_:&str){}fn social_type(&mut self,_:&str){}fn block_editor_type(&mut self,_:&str){}fn debug_options_type(&mut self,_:&str){}fn world_options_text(&mut self,_:Option<&str>){}
 fn mod_browser_typing(&self)->bool{false}fn recipe_search_focused(&self)->bool{false}fn station_typing(&self)->bool{false}
 fn mod_browser_text(&mut self,_:Option<&str>,_:bool){}fn recipe_text(&mut self,_:Option<&str>,_:bool){}fn station_text(&mut self,_:Option<&str>,_:bool){}fn creative_search_edit(&mut self,_:Option<&str>){}
}
'''
CASES = r'''
fn type_uikit(g:&mut Game,text:&str){for (physical_key,logical) in insert_text_keys(text){let text=if let Key::Character(s)=logical{Some(s)}else{None};g.dispatch(WindowEvent::KeyboardInput{event:KeyEvent{physical_key,state:ElementState::Pressed,text,repeat:false}});g.dispatch(WindowEvent::KeyboardInput{event:KeyEvent{physical_key,state:ElementState::Released,text:None,repeat:false}});}}
#[test]fn server_address_characters_and_backspace(){let mut g=Game::new();type_uikit(&mut g,"mc.hypixel.net:25565");assert_eq!(g.title.as_ref().unwrap().value,"mc.hypixel.net:25565");type_uikit(&mut g,"\u{8}");assert_eq!(g.title.as_ref().unwrap().value,"mc.hypixel.net:2556");}
#[test]fn editing_modifier_blocks_plain_characters(){let mut g=Game::new();g.blocked=true;type_uikit(&mut g,"v");assert_eq!(g.title.as_ref().unwrap().value,"");}
#[test]fn canceled_physical_key_still_delivers_its_separate_character_callback(){let mut g=Game::new();g.cancel_key=true;g.dispatch(WindowEvent::KeyboardInput{event:KeyEvent{physical_key:PhysicalKey::Code(KeyCode::KeyA),state:ElementState::Pressed,text:Some("a".into()),repeat:false}});assert_eq!(g.title.as_ref().unwrap().value,"a");assert_eq!(*g.hooked.borrow(),"a");}
#[test]fn names_unicode_and_hook_cancellation(){let mut g=Game::new();type_uikit(&mut g,"日本 🦀");assert_eq!(g.title.as_ref().unwrap().value,"日本 🦀");assert_eq!(*g.hooked.borrow(),"日本 🦀");g.cancel=true;type_uikit(&mut g,"blocked");assert_eq!(g.title.as_ref().unwrap().value,"日本 🦀");}
'''


class MobileMultiplayerKeyboardTests(unittest.TestCase):
    def test_production_title_dispatch_and_uikit_characters(self):
        compiler = shutil.which('rustc')
        if not compiler:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-server-keyboard-') as directory:
            source=Path(directory)/'probe.rs';binary=Path(directory)/'probe'
            source.write_text(program())
            build=subprocess.run([compiler,'--edition=2024','--test',str(source),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(build.returncode,0,build.stdout+build.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
