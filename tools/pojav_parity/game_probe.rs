//! Test endpoints for the production touch-to-game adapter, not an alternative
//! implementation. The driver inserts the current Game::touch_act and key_of.
use crate::touch::{Control, Out};
use serde_json::{Value, json};
mod horizon_ios_launcher {pub fn native_gestures_available()->bool{true}}
mod horizon_client {pub mod debug_keys {pub enum KeyAction {Press,Release}} pub mod keybinds {pub fn mouse_name(button:super::super::MouseButton)->String {if matches!(button,super::super::MouseButton::Left){"key.mouse.left"}else{"key.mouse.right"}.into()}}}
#[derive(Default)]struct Bindings;
impl Bindings {
 fn translate_all(&self,key:KeyCode)->Vec<std::result::Result<KeyCode,MouseButton>>{vec![Ok(key)]}
 fn translate_mouse_all(&self,b:MouseButton)->Vec<std::result::Result<KeyCode,MouseButton>>{vec![Err(b)]}
}
#[derive(Default)]struct Settings{keys:Bindings}
type Result<T> = std::result::Result<T, ()>;
#[derive(Clone, Copy)]
#[repr(i32)]
pub enum KeyCode { KeyW=87, KeyA=65, KeyS=83, KeyD=68, Space=32, ShiftLeft=340,
    F5=294, F3=292, ControlLeft=341, Tab=258, KeyT=84, Escape=256, KeyE=69 }
#[derive(Clone, Copy)]
pub enum MouseButton {Left, Right, Middle}
#[derive(Clone, Copy)]
enum Source {Touch}
#[derive(Default)]
struct Mouse {motion:Vec<Value>}
impl Mouse {fn add_motion(&mut self,x:f64,y:f64) {self.motion.push(json!(["cursor",3,x,y]));}}
#[derive(Default)]
struct Touch {more:bool,hidden:bool,fingers:crate::touch::Touches,toggle_keys:Vec<KeyCode>,late:Vec<(MouseButton,u64)>,holding:Option<MouseButton>}
#[derive(Default)]
pub struct Game {pub events:Vec<Value>,selected:usize,ticks:u64,settings:Settings,touch:Touch,mouse:Mouse}
GAME_KEY_OF
impl Game {
    fn menu_input_blocked(&self)->bool {false}
    fn cancel_touch_for_reload(&mut self) {panic!("unexpected reload in control trace")}
    fn touch_grab(&mut self) {}
    fn entity_targeted(&self)->bool {false}
    fn touch_holds_use(&self)->bool {false}
    fn mapped_key(&mut self,key:KeyCode,down:bool,_repeat:bool,_raw:KeyCode,_source:Source)->Result<()> {
        self.events.push(json!(["key",key as i32,i32::from(down),0]));Ok(())
    }
    fn mouse_mapping(&mut self,b:MouseButton,down:bool,_source:Source)->Result<()> {
        self.events.push(json!(["button",if matches!(b,MouseButton::Right){1}else{0},i32::from(down),0]));Ok(())
    }
    fn shared_default_acts(&self,_:MouseButton,_:Option<&str>)->bool{true}
    fn touch_key_held(&self,_:KeyCode)->bool{false}
    fn debug_key(&mut self,_:KeyCode,_:horizon_client::debug_keys::KeyAction)->bool{false}
    fn touch_holds_mouse_mapping(&self,_:MouseButton)->bool{false}
GAME_TOUCH_RAW_KEY
GAME_TOUCH_RAW_BUTTON
GAME_TOUCH_ACT
    pub fn dispatch(&mut self,out:Vec<Out>,scale:f32)->Vec<Value> {
        self.events.clear();self.mouse.motion.clear();
        self.touch_act(out,scale).unwrap();
        self.events.append(&mut self.mouse.motion);
        std::mem::take(&mut self.events)
    }
}
pub fn control(code:i64)->Control {
    match code {87=>Control::Forward,65=>Control::Left,83=>Control::Back,68=>Control::Right,
    32=>Control::Jump,340=>Control::Sneak,294=>Control::Perspective,84=>Control::Chat,
    256=>Control::Pause,69=>Control::Inventory,-3=>Control::LeftClick,-4=>Control::RightClick,
    _=>panic!("unsupported trace control")}
}
pub fn key(control:Control)->i32 {key_of(control).expect("keyboard control") as i32}
