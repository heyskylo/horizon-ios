"""Exercise the actual menu cursor dispatcher and delayed mouse edges."""
from pathlib import Path
import tempfile,subprocess,shutil,unittest
from test_mobile_command_keyboard import method
ROOT=Path(__file__).resolve().parents[1]
def struct(source,name):
 start=source.index('struct '+name+' {');end=source.index('\n}',start)+2
 return source[start:end]
class VirtualCursorTests(unittest.TestCase):
 def test_menu_drag_buttons_screen_change_and_cancel(self):
  if not shutil.which('rustc'): self.skipTest('requires Rust')
  code=(ROOT/'crates/horizon-client/src/game/touch.rs').read_text()
  methods='\n'.join(method(code,n) for n in ['touch_menu_cursor_mode','visible_touch_layout','touch_render_point','touch_click_context','touch_record_click_context','touch_cursor_flush','touch_cursor_reset','touch_cursor_motion','touch_cursor_event','touch_cursor_frame','touch_menu_mouse_held','touch_queue_cursor_button'])
  window=(ROOT/'crates/horizon-client/src/game/window_handler.rs').read_text()
  a=window.index('        if let WindowEvent::MouseInput { state, button, .. } = &event {')
  b=window.index('        let mouse =',a)
  ledger=window[a:b]
  methods += method((ROOT/'crates/horizon-client/src/game/hud_editor.rs').read_text(),'hud_editor_release')
  hud = method((ROOT/'crates/horizon-client/src/game.rs').read_text(), 'hud')
  # Execute the production final composition wrapper, with unrelated renderers stubbed.
  # Its touch overlay acknowledgement drives the real deferred click dispatcher below.
  methods += hud
  acknowledgement = code[code.index('            self.touch.cursor_drawn = Some('):].split('\n', 1)[0]
  acknowledgement = acknowledgement.replace('self.cursor', 'self.point')
  types='#[derive(Clone,Debug,PartialEq)]'+struct(code,'ClickContext')+struct(code,'CursorEdge')
  fixture=r'''
#![allow(dead_code)]
#[path="SOURCE"] mod actual;
mod touch {pub use crate::actual::{layout,desktop,Rect}; pub fn cursor_mode()->bool{true}}
mod winit{pub mod keyboard{#[derive(Clone,Copy,Debug,Default,PartialEq)]pub struct ModifiersState(pub u8);}}
// What is under a point: the test moves widgets by changing WIDGET, as a redraw would.
mod horizon_client{pub mod hud_kit{pub mod room{pub use crate::Room;}} pub mod widgets{
 use std::sync::atomic::{AtomicU32,Ordering};
 pub static WIDGET:AtomicU32=AtomicU32::new(0);
 #[derive(Clone,Debug,PartialEq)]pub struct PointerGuard(u32);
 pub fn capture_pointer(_:(f32,f32))->PointerGuard{PointerGuard(WIDGET.load(Ordering::Relaxed))}
}}
mod horizon_ios_launcher{
 use std::sync::atomic::{AtomicU64,Ordering};
 static NEXT:AtomicU64=AtomicU64::new(1);pub static SET:AtomicU64=AtomicU64::new(0);
 pub fn next_touch_context()->u64{NEXT.fetch_add(1,Ordering::Relaxed)}
 pub fn set_touch_context(id:u64){SET.store(id,Ordering::Relaxed)}
}
use actual::Control;
#[derive(Clone,Copy,Debug,PartialEq,Eq,Hash)]enum MouseButton{Left,Right,Middle}
#[derive(Clone,Copy,PartialEq)]enum TouchPhase{Started,Moved,Ended,Cancelled}
#[derive(Clone,Copy,Debug,PartialEq)] enum KeyCode{KeyW,KeyS,KeyA,KeyD,Space,ShiftLeft,F5,KeyT,Escape,KeyE,ControlLeft,F3,Tab}
KEY_OF
#[derive(Clone,Copy,PartialEq)]enum ElementState{Pressed,Released}
enum WindowEvent{MouseInput{state:ElementState,button:MouseButton},Other}
#[derive(Default)]struct Editor{drag:Option<()>,guides:(Option<f32>,Option<f32>)}
#[derive(Default)]struct Pvp{editor:Option<Editor>}
#[derive(Default)]struct Settings{horizon:Horizon,saved:std::cell::Cell<usize>}
impl Settings{fn save(&self,_:&str){self.saved.set(self.saved.get()+1);}}
struct RenderBatch{scale:f32}
trait EventLoopCtl{} struct Loop;impl EventLoopCtl for Loop{}
struct Batch{width:f32,height:f32}
type HudBatch=Batch;
impl Batch{fn new(size:[u32;2])->Self{Self{width:size[0] as f32,height:size[1] as f32}}}
#[derive(Default)]pub struct Room;
impl Room{fn new(_:f32,_:f32)->Self{Self}}
#[derive(Default)]struct Ui{room:Room}
#[derive(Default)]struct Packs{prompt:Option<()>}
#[derive(Default)]struct Gui{atlas:()}
#[derive(Default)]struct Graph;
impl Graph{fn draw(&self,_:(),_:&mut Batch,_:&()){}}
#[derive(Default)]struct Horizon{overlay:()}

TYPES
#[derive(Default)]struct Input{more:bool,hidden:bool,cursor:actual::desktop::Cursor,cursor_screen:Option<u64>,cursor_pending:Vec<CursorEdge>,fingers:actual::Touches,
 cursor_down:[bool;3],native_mouse:[bool;3],custom_mouse:[bool;3],cursor_cancelled:[bool;3],cursor_delivery:Option<ClickContext>,cursor_drawn:Option<(u64,(f32,f32))>,
 click_contexts:Vec<(u64,ClickContext)>,native_surface:Option<(u64,(f32,f32))>,cursor_keys:Vec<Control>}
/// `clicks` records (right, down, pointer, modifiers) as the screen receives each mouse edge.
#[derive(Default)]struct Game{render_scale:f32,title:Option<()>,ui:Ui,server_packs:Packs,code_of_conduct:Option<()>,command_prompt:Option<()>,player:Option<()>,gui:Gui,frame_graph:Graph,rendered:usize,pvp:Pvp,settings:Settings,touch:Input,frames:u64,key:u64,point:(f32,f32),clicks:Vec<(bool,bool,(f32,f32),u8)>,switch_on_down:bool,
 keys:Vec<(KeyCode,bool)>,modifiers:winit::keyboard::ModifiersState,physical_buttons:std::collections::HashSet<MouseButton>,size:(f32,f32)}
impl Game{
 fn advance_skin_frame(&mut self){}
 fn dialog_open(&self)->bool{false}
 fn hud_layers(&mut self,size:[u32;2])->Batch{Batch::new(size)}
 fn draw_dialog(&mut self,_:&mut Batch){}
 fn draw_pack_prompt(&mut self,_:&mut Batch){}
 fn draw_code_of_conduct(&mut self,_:&mut Batch){}
 fn draw_link_prompt(&mut self,_:&mut Batch){}
 fn draw_command_prompt(&mut self,_:&mut Batch){}
 fn draw_narrator_toast(&mut self,_:&mut Batch){}
 fn draw_debug_overlay(&mut self,_:&mut Batch,_:[u32;2]){}
 fn draw_pad_over_hud(&mut self,_:&mut Batch){}
 fn draw_touch_close(&mut self,_:&mut Batch){ACKNOWLEDGEMENT self.rendered+=1;}
 fn options_path()->&'static str{"unused-test-path"}
 fn new()->Self{Game{player:Some(()),size:(512.,300.),..Default::default()}}
 fn touch_raw_key(&mut self,key:KeyCode,down:bool)->Result<(),()>{self.keys.push((key,down));Ok(())}
 fn touch_now(&mut self)->f64{self.frames as f64/60.0}
 fn key_screen(&self)->u64{self.key}
 fn touch_batch(&self)->Option<Batch>{Some(Batch{width:self.size.0,height:self.size.1})}
 fn screen_batch(&self)->Option<RenderBatch>{Some(RenderBatch{scale:self.render_scale.max(1.)})}
 fn touch_effective_modifiers(&self)->winit::keyboard::ModifiersState{self.modifiers}
 fn screen_point(&self,at:(f32,f32))->(f32,f32){at}
 fn hardware_hides_touch(&self)->bool{false}
 fn touch_pointer(&mut self,_:&dyn EventLoopCtl,at:(f32,f32)){let s=self.render_scale.max(1.);self.point=((at.0*s)/s,(at.1*s)/s);}
 fn touch_mouse_button(&mut self,_:&dyn EventLoopCtl,b:MouseButton,down:bool){
  let event=WindowEvent::MouseInput{state:if down{ElementState::Pressed}else{ElementState::Released},button:b};
LEDGER
  if !down && b==MouseButton::Left{self.hud_editor_release();}
  self.clicks.push((b==MouseButton::Right,down,self.point,self.modifiers.0));if down&&self.switch_on_down{self.key+=1;}}
 fn input(&mut self,id:u64,phase:TouchPhase,at:(f32,f32)){let size=self.size;self.touch_cursor_event(&Loop,id,phase,at,size);}
 /// One rendered frame: the screen is drawn with the pointer where it is, then input runs.
 fn frame(&mut self){self.hud([self.size.0 as u32,self.size.1 as u32]);self.frames+=1;self.touch_cursor_frame(&Loop,false);}
 fn edges(&self)->Vec<(bool,bool)>{self.clicks.iter().map(|c|(c.0,c.1)).collect()}
METHODS
}
fn main(){
 // An idle touch frame must not warp the pointer back and cancel a virtual
 // Tab/arrow key's screen focus. Only a delivered queued pointer needs restore.
 let mut idle=Game::new();idle.touch.cursor.position=Some((100.,100.));idle.point=(300.,150.);idle.frame();assert_eq!(idle.point,(300.,150.));

 for (index,button,control) in [(0,MouseButton::Left,Control::LeftClick),(1,MouseButton::Right,Control::RightClick),(2,MouseButton::Middle,Control::MiddleClick)] {
  let layout=actual::layout(512.,300.);let r=layout.iter().find(|(c,_)|*c==control).unwrap().1;let point=(r[0]+r[2]/2.,r[1]+r[3]/2.);
  for custom_first in [false,true] {
   let mut g=Game::new();g.touch.cursor_screen=Some(g.key);
   if custom_first {g.touch.custom_mouse[index]=true;g.touch_queue_cursor_button(button,true);}else{g.input(1,TouchPhase::Started,point);}
   for _ in 0..3{g.frame();}assert!(g.physical_buttons.contains(&button));
   if custom_first {g.input(1,TouchPhase::Started,point);g.touch.custom_mouse[index]=false;g.touch_queue_cursor_button(button,false);}else{g.touch.custom_mouse[index]=true;g.touch_queue_cursor_button(button,true);g.input(1,TouchPhase::Ended,point);}
   for _ in 0..3{g.frame();}assert!(g.physical_buttons.contains(&button));assert_eq!(g.clicks.len(),1);
   if custom_first {g.input(1,TouchPhase::Ended,point);}else{g.touch.custom_mouse[index]=false;g.touch_queue_cursor_button(button,false);}for _ in 0..3{g.frame();}
   assert!(!g.physical_buttons.contains(&button));assert_eq!(g.clicks.len(),2);
  }
 }

 // Fractional positions must survive the same physical-pixel round trip as
 // the real renderer; otherwise the pending press never acknowledges a draw.
 let mut fractional=Game::new();fractional.render_scale=5.;
 fractional.input(99,TouchPhase::Started,(200.,100.));
 fractional.input(99,TouchPhase::Moved,(364.0010070800781,100.));
 let left=actual::layout(512.,300.).into_iter().find(|(c,_)|*c==Control::LeftClick).unwrap().1;
 let at=(left[0]+left[2]/2.,left[1]+left[3]/2.);
 fractional.input(100,TouchPhase::Started,at);fractional.input(100,TouchPhase::Ended,at);
 for _ in 0..6 {fractional.frame();}
 assert_eq!(fractional.edges(),[(false,true),(false,false)]);assert!(fractional.touch.cursor_pending.is_empty());

 use horizon_client::widgets::WIDGET;use std::sync::atomic::Ordering::Relaxed;
 let layout=actual::layout(512.,300.);let centre=|c|{let r=layout.iter().find(|(b,_)|*b==c).unwrap().1;(r[0]+r[2]/2.,r[1]+r[3]/2.)};
 assert!(!layout.iter().any(|(c,_)|*c==Control::Runtime));
 // Every screen reaches the final overlay, even when layers were replaced by a prompt.
 for prompt in [false,true] {
  let mut g=Game::new();g.server_packs.prompt=prompt.then_some(());
  g.input(1,TouchPhase::Started,centre(Control::LeftClick));g.frame();g.frame();
  assert_eq!(g.rendered,2);assert!(g.edges().contains(&(false,true)));
 }
 // Title/disconnected screens have no overlay hitboxes and use direct touch.
 for title in [None,Some(())] {
  let mut g=Game::new();g.player=None;g.title=title;g.touch.more=true;
  assert!(g.visible_touch_layout(512.,300.).is_empty());assert!(!g.touch_menu_cursor_mode());
  g.input(1,TouchPhase::Started,centre(Control::Forward));
  assert_eq!(g.point,centre(Control::Forward));
  g.input(1,TouchPhase::Ended,centre(Control::Forward));g.frame();g.frame();
  assert!(g.keys.is_empty());
  g.touch.hidden=true;assert!(g.visible_touch_layout(512.,300.).is_empty());
 }
 // Returning to the title hides controls even if player teardown is not finished.
 let mut title=Game::new();title.title=Some(());
 assert!(!title.visible_touch_layout(512.,300.).iter().any(|(c,_)|*c==Control::Forward));
 // World controls are restored without changing the saved layout.
 title.title=None;
 assert!(title.visible_touch_layout(512.,300.).iter().any(|(c,_)|*c==Control::Forward));
 // A surface finger drags the cursor relatively, from the middle of the screen.
 let mut g=Game::new();g.input(1,TouchPhase::Started,(200.,100.));g.input(1,TouchPhase::Moved,(220.,110.));assert_eq!(g.point,(276.,160.));assert!(g.clicks.is_empty());
 // A tap on the button is a press on one drawn frame and a release on the next, both at the cursor.
 g.input(2,TouchPhase::Started,centre(Control::LeftClick));g.input(2,TouchPhase::Ended,centre(Control::LeftClick));assert!(g.clicks.is_empty());
 g.frame();assert_eq!(g.clicks,[(false,true,(276.,160.),0)]);
 g.frame();assert_eq!(g.edges(),[(false,true),(false,false)]);assert_eq!(g.clicks[1].2,(276.,160.));
 // The middle button owns its own ledger entry and cancellation sends its paired up.
 let mut middle=Game::new();middle.input(88,TouchPhase::Started,centre(Control::MiddleClick));middle.frame();middle.frame();
 assert!(middle.physical_buttons.contains(&MouseButton::Middle));assert!(middle.touch.cursor_down[2]);
 middle.touch_cursor_reset();middle.touch_cursor_flush(&Loop);assert!(middle.physical_buttons.is_empty());assert!(!middle.touch.cursor_down[2]);
 // A held button stays down while the surface finger drags.
 g.input(3,TouchPhase::Started,centre(Control::RightClick));g.frame();assert_eq!(g.edges().last(),Some(&(true,true)));
 g.input(1,TouchPhase::Moved,(240.,110.));assert_eq!(g.point,(296.,160.));assert!(g.touch.cursor_down[1]);
 // A new screen releases it exactly once, and nothing is left queued or held.
 g.key+=1;g.frame();assert_eq!(g.edges().last(),Some(&(true,false)));assert_eq!(g.clicks.len(),4);
 assert!(g.touch.cursor_pending.is_empty());assert_eq!(g.touch.cursor_down,[false,false,false]);assert!(g.physical_buttons.is_empty());
 g.frame();g.frame();assert_eq!(g.clicks.len(),4);

 // Reload can reset without an event loop: it clears the ledger immediately but must
 // subsequently deliver a real window up to end the actual HUD drag and save once.
 let mut drag=Game::new();drag.input(99,TouchPhase::Started,centre(Control::LeftClick));drag.frame();drag.frame();
 assert!(drag.physical_buttons.contains(&MouseButton::Left));
 drag.pvp.editor=Some(Editor{drag:Some(()),guides:(Some(1.),Some(2.))});
 drag.touch_cursor_reset();assert!(drag.physical_buttons.is_empty());assert!(drag.pvp.editor.as_ref().unwrap().drag.is_some());
 drag.touch_cursor_flush(&Loop);assert!(drag.pvp.editor.as_ref().unwrap().drag.is_none());assert_eq!(drag.settings.saved.get(),1);
 drag.touch_cursor_flush(&Loop);assert_eq!(drag.settings.saved.get(),1);
 // The press is not delivered until a frame has been drawn with the pointer at its place.
 let mut g=Game::new();g.input(1,TouchPhase::Started,centre(Control::LeftClick));
 g.touch.cursor_drawn=None;g.frames+=1;g.touch_cursor_frame(&Loop,false);assert!(g.clicks.is_empty());
 g.frame();assert_eq!(g.edges(),[(false,true)]);

 // Moving while a press is queued does not move that press: it lands where the tap was made,
 // and the release follows at the later position.
 let mut g=Game::new();g.input(1,TouchPhase::Started,(200.,100.));g.input(1,TouchPhase::Moved,(220.,110.));g.frame();
 g.input(2,TouchPhase::Started,centre(Control::LeftClick));g.input(1,TouchPhase::Moved,(250.,110.));
 g.input(2,TouchPhase::Ended,centre(Control::LeftClick));
 for _ in 0..4{g.frame();}
 assert_eq!(g.edges(),[(false,true),(false,false)]);assert_eq!(g.clicks[0].2,(276.,160.));assert_eq!(g.clicks[1].2,(306.,160.));

 // Modifiers are the ones held when the tap was made, restored after delivery.
 let mut g=Game::new();g.modifiers.0=1;g.input(1,TouchPhase::Started,centre(Control::LeftClick));g.modifiers.0=0;
 g.frame();g.frame();assert_eq!(g.clicks.last().unwrap().3,1);assert_eq!(g.modifiers.0,0);

 // What was under the pointer changed before the press arrived: no click at all, on anything.
 let mut g=Game::new();g.input(1,TouchPhase::Started,centre(Control::LeftClick));g.input(1,TouchPhase::Ended,centre(Control::LeftClick));
 WIDGET.store(7,Relaxed);for _ in 0..3{g.frame();}assert!(g.clicks.is_empty());assert!(g.touch.cursor_pending.is_empty());WIDGET.store(0,Relaxed);
 // The screen was resized under a queued press: likewise dropped.
 let mut g=Game::new();g.input(1,TouchPhase::Started,centre(Control::LeftClick));g.size=(600.,300.);
 for _ in 0..3{g.frame();}assert!(g.clicks.is_empty());assert!(g.touch.cursor_pending.is_empty());

 // Shift is a held key in menus too, and screen reset always releases it.
 let mut k=Game::new();k.input(1,TouchPhase::Started,centre(Control::Sneak));assert_eq!(k.keys,[(KeyCode::ShiftLeft,true)]);
 k.input(1,TouchPhase::Moved,(500.,100.));assert_eq!(k.keys.len(),1);
 k.input(1,TouchPhase::Ended,(500.,100.));assert_eq!(k.keys,[(KeyCode::ShiftLeft,true),(KeyCode::ShiftLeft,false)]);
 k.input(2,TouchPhase::Started,centre(Control::Forward));k.key+=1;k.frame();
 assert_eq!(&k.keys[2..],&[(KeyCode::KeyW,true),(KeyCode::KeyW,false)]);
 // A native two-finger pan cancels surface touches, not an independently held Shift button.
 let mut k=Game::new();k.input(10,TouchPhase::Started,(200.,100.));k.input(11,TouchPhase::Started,centre(Control::Sneak));
 k.input(10,TouchPhase::Cancelled,(200.,100.));assert_eq!(k.keys,[(KeyCode::ShiftLeft,true)]);
 k.input(11,TouchPhase::Ended,centre(Control::Sneak));assert_eq!(k.keys,[(KeyCode::ShiftLeft,true),(KeyCode::ShiftLeft,false)]);
 // Cancelling a tap before its deferred press never creates a click.
 let mut g=Game::new();g.input(1,TouchPhase::Started,centre(Control::LeftClick));g.input(1,TouchPhase::Cancelled,(0.,0.));for _ in 0..10{g.frame();}assert!(g.clicks.is_empty());
 // A pressed button that changes screens aborts the remaining old-screen edges, and is itself released once.
 let mut g=Game::new();g.switch_on_down=true;g.input(1,TouchPhase::Started,centre(Control::LeftClick));g.input(2,TouchPhase::Started,centre(Control::RightClick));
 g.frame();assert!(g.clicks.is_empty());g.frame();assert_eq!(g.edges(),[(false,true),(false,false)]);assert!(g.touch.cursor_pending.is_empty());
 g.switch_on_down=false;for _ in 0..3{g.frame();}assert_eq!(g.clicks.len(),2);assert!(g.physical_buttons.is_empty());
}
'''.replace('SOURCE',str(ROOT/'crates/horizon-client/src/touch.rs')).replace('METHODS',methods).replace('KEY_OF',method(code,'key_of')).replace('TYPES',types).replace('LEDGER',ledger).replace('ACKNOWLEDGEMENT',acknowledgement)
  with tempfile.TemporaryDirectory(prefix='horizon-virtual-cursor-') as d:
   p=Path(d);(p/'probe.rs').write_text(fixture)
   b=subprocess.run(['rustc','--edition=2024',str(p/'probe.rs'),'-o',str(p/'probe')],capture_output=True,text=True,cwd=p);self.assertEqual(b.returncode,0,b.stdout+b.stderr)
   b=subprocess.run([str(p/'probe')],capture_output=True,text=True,cwd=p);self.assertEqual(b.returncode,0,b.stdout+b.stderr)
