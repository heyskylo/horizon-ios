"""Virtual raw keys reach a real EditBox via the window keyboard transport."""
from pathlib import Path
import subprocess,tempfile,shutil,unittest
from test_mobile_command_keyboard import method
ROOT=Path(__file__).resolve().parents[1]
class CustomKeyboardTests(unittest.TestCase):
 def test_virtual_keys_edit_and_paste_without_typing_characters(self):
  if not shutil.which('rustc'):self.skipTest('rustc required')
  touch=(ROOT/'crates/horizon-client/src/game/touch.rs').read_text()
  ui=(ROOT/'crates/horizon-client-render/src/ui.rs').read_text();impl=ui[ui.index('impl EditBox {'):]
  clip=(ROOT/'crates/horizon-client-input/src/clipboard.rs').read_text()
  methods='\n'.join(method(impl,n) for n in ['reset_blink','selected','selected_text','insert_text','delete_chars','word_position','word_position_from','set_cursor','move_cursor_to_end','scroll_to','key','shortcut'])
  helpers='\n'.join(method(ui,n) for n in ['byte_index','char_len','word_position','multiline_word_position','java_whitespace'])
  clipboard='\n'.join('pub '+method(clip,n) for n in ['shortcut_letter','edit_shortcut','is_allowed_chat_character','filter_text','utf16_prefix','apply'])
  begin=ui.index('impl crate::clipboard::Editable for EditBox');end=ui.index('\nimpl EditBox {',begin)
  source=CODE.replace('TRANSPORT',method(touch,'touch_custom_keyboard')+method(touch,'touch_window_event').replace('super::input_trace::','input_trace::'))
  source=source.replace('CONSTRUCTOR','pub '+method((ROOT/'third_party/winit/src/event.rs').read_text(),'from_virtual_key'))
  source=source.replace('EDIT_METHODS',methods).replace('HELPERS',helpers).replace('CLIPBOARD',clipboard).replace('EDITABLE_IMPL',ui[begin:end])
  with tempfile.TemporaryDirectory(prefix='horizon-custom-keyboard-') as d:
   p=Path(d);(p/'probe.rs').write_text(source)
   r=subprocess.run(['rustc','--edition=2024',str(p/'probe.rs'),'-o',str(p/'probe')],capture_output=True,text=True);self.assertEqual(r.returncode,0,r.stdout+r.stderr)
   r=subprocess.run([str(p/'probe')],capture_output=True,text=True);self.assertEqual(r.returncode,0,r.stdout+r.stderr)
CODE=r'''
#![allow(dead_code)]
extern crate self as winit;
use std::time::Instant;
pub mod keyboard{
 #[derive(Clone,Copy,PartialEq,Debug)]pub enum KeyCode{Backspace,Delete,ArrowLeft,ArrowRight,Home,End,KeyA,KeyC,KeyV,KeyX,Enter}
 #[derive(Clone,Copy)]pub enum PhysicalKey{Code(KeyCode),Unidentified}
 pub enum NativeKey{Unidentified}pub enum Key{Unidentified(NativeKey),Character(String)}pub enum KeyLocation{Standard}
}
use keyboard::*;
#[derive(Clone,Copy,PartialEq)]pub enum ElementState{Pressed,Released}
mod platform_impl{pub struct KeyEventExtra{}}
pub mod event{
 use super::*;
 pub struct KeyEvent{pub physical_key:PhysicalKey,pub logical_key:Key,pub text:Option<String>,pub location:KeyLocation,pub state:ElementState,pub repeat:bool,platform_specific:platform_impl::KeyEventExtra}
 impl KeyEvent{CONSTRUCTOR}
}
use event::KeyEvent;
mod clipboard{
 use super::*;
 #[derive(Clone,Copy)]pub enum EditShortcut{SelectAll,Copy,Paste,Cut}
 pub trait Clipboard{fn get(&mut self)->String;fn set(&mut self,text:&str);}
 pub trait Editable{fn select_all(&mut self);fn selected_text(&self)->String;fn insert(&mut self,text:&str)->bool;}
 CLIPBOARD
}
HELPERS
struct EditBox{value:String,cursor:usize,highlight:usize,display:usize,max_length:usize,multiline:bool,editable:bool,focused_at:Instant}
impl EditBox{EDIT_METHODS}
EDITABLE_IMPL
struct Board;impl clipboard::Clipboard for Board{fn get(&mut self)->String{"fixture paste".into()}fn set(&mut self,_:&str){}}
struct Window;impl Window{fn id(&self)->u32{1}}
type WindowId=u32;struct DeviceId;impl DeviceId{fn dummy()->Self{Self}}
enum WindowEvent{KeyboardInput{device_id:DeviceId,event:KeyEvent,is_synthetic:bool}}
mod input_trace{pub enum InputOrigin{Game}}
trait EventLoopCtl{}struct Loop;impl EventLoopCtl for Loop{}
struct Touch{synthetic:bool}
struct Game{window:Option<Window>,touch:Touch,field:EditBox,command:bool,submitted:bool,events:usize}
impl Game{
 fn touch_custom_activity(&mut self){}
 TRANSPORT
 fn handle_window_event(&mut self,_:&dyn EventLoopCtl,_:u32,event:WindowEvent,_:input_trace::InputOrigin){
  assert!(self.touch.synthetic);let WindowEvent::KeyboardInput{event,is_synthetic,..}=event;assert!(is_synthetic);assert!(event.text.is_none());self.events+=1;
  if event.state==ElementState::Released{return;}
  if let Some(shortcut)=clipboard::edit_shortcut(clipboard::shortcut_letter(&event.logical_key,event.physical_key),self.command,false,false){self.field.shortcut(shortcut,&mut Board);return;}
  if let PhysicalKey::Code(key)=event.physical_key{if key==KeyCode::Enter{self.submitted=true;}else{self.field.key(key,false,false);}}
 }
}
fn main(){
 let mut g=Game{window:Some(Window),touch:Touch{synthetic:false},field:EditBox{value:"abc".into(),cursor:3,highlight:3,display:0,max_length:256,multiline:false,editable:true,focused_at:Instant::now()},command:false,submitted:false,events:0};
 g.touch_custom_keyboard(&Loop,KeyCode::Backspace,true);g.touch_custom_keyboard(&Loop,KeyCode::Backspace,false);assert_eq!(g.field.value,"ab");
 g.touch_custom_keyboard(&Loop,KeyCode::ArrowLeft,true);assert_eq!(g.field.cursor,1);
 g.command=true;g.touch_custom_keyboard(&Loop,KeyCode::KeyV,true);assert_eq!(g.field.value,"afixture pasteb");
 g.command=false;g.touch_custom_keyboard(&Loop,KeyCode::KeyA,true);assert_eq!(g.field.value,"afixture pasteb");
 g.touch_custom_keyboard(&Loop,KeyCode::Enter,true);assert!(g.submitted);assert!(!g.touch.synthetic);assert_eq!(g.events,6);
}
'''
