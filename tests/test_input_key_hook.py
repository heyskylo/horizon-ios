"""Run the actual window-key callback with fake normal/early-return handlers."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_world_input_capture import method

ROOT = Path(__file__).resolve().parents[1]


class InputKeyHookTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("rustc"), "requires Rust")
    def test_original_event_after_handling_and_source_gates(self):
        source = "".join((ROOT / "crates/horizon-client/src" / path).read_text(encoding="utf-8")
                         for path in ("game/window_handler.rs", "game.rs"))
        actual = method(source, "window_event")
        fixture = r"""
type WindowId = i32;
struct ActiveEventLoop;
struct Window;
impl Window { fn id(&self) -> i32 { 7 } }
mod input_trace {
    #[derive(Clone, Copy)]
    pub enum InputOrigin { User, Agent, Game }
}
#[derive(Clone, Debug, PartialEq)]
struct KeyEvent { physical_key: i32, logical_key: &'static str, location: i32,
                  text: &'static str, pressed: bool, repeat: bool }
#[derive(Clone)] enum WindowEvent { KeyboardInput { event: KeyEvent, is_synthetic: bool }, Other }
mod splitscreen {
    pub fn guest_input(event: &super::WindowEvent) -> bool { matches!(event,super::WindowEvent::KeyboardInput{..}) }
    pub fn shared_event(_: &super::WindowEvent) -> bool { false }
}
fn glfw_modifiers(bits: u32) -> u32 { bits }
struct Game { splitscreen_guest: Option<Box<Game>>, window: Option<Window>, modifiers: u32, early: bool, scripted: bool,
              calls: Vec<&'static str>, observed: Vec<(KeyEvent, u32)> }
impl Game {
    fn scripted_run(&self) -> bool { self.scripted }
    fn handle_window_event(&mut self, _: &ActiveEventLoop, _: WindowId, mut event: WindowEvent,
                           _: input_trace::InputOrigin) {
        self.calls.push("handle"); self.modifiers = 0;
        if let WindowEvent::KeyboardInput { event, .. } = &mut event { event.physical_key = 99; event.text = ""; }
        if self.early { return; }
        self.calls.push("finish");
    }
    fn native_input_key(&mut self, event: &KeyEvent, modifiers: u32) {
        self.calls.push("hook"); self.observed.push((event.clone(), modifiers));
    }
    METHOD
}
#[test]
fn all_actions_consumption_and_window_gates() {
    for (pressed, repeat) in [(true, false), (true, true), (false, false)] {
        for early in [false, true] {
            let original = KeyEvent { physical_key: -1, logical_key: "logical", location: 2,
                text: "z", pressed, repeat };
            for (window, id, synthetic, fires) in [(true,7,false,true), (true,8,false,false),
                (true,7,true,false), (false,7,false,false)] {
                let mut game = Game { splitscreen_guest: None, window: window.then_some(Window), modifiers: 5, early,
                    scripted: false, calls: vec![], observed: vec![] };
                game.window_event(&ActiveEventLoop, id, WindowEvent::KeyboardInput { event: original.clone(), is_synthetic: synthetic });
                let mut expected = vec![];
                if window && id == 7 { expected.push("handle"); if !early { expected.push("finish"); } }
                if fires { expected.push("hook"); }
                assert_eq!(game.calls, expected);
                assert_eq!(game.observed, if fires { vec![(original.clone(),5)] } else { vec![] });
            }
        }
    }
    let mut game = Game { splitscreen_guest: None, window: Some(Window), modifiers: 5, early: false, scripted: false, calls: vec![], observed: vec![] };
    game.window_event(&ActiveEventLoop, 7, WindowEvent::Other);
    assert!(game.observed.is_empty());
    game.calls.clear();
    game.splitscreen_guest=Some(Box::new(Game {splitscreen_guest:None, window:Some(Window),modifiers:9,early:true,scripted:false,calls:vec![],observed:vec![]}));
    let key=KeyEvent{physical_key:1,logical_key:"a",location:0,text:"a",pressed:true,repeat:false};
    game.window_event(&ActiveEventLoop,7,WindowEvent::KeyboardInput{event:key.clone(),is_synthetic:false});
    assert!(game.calls.is_empty(),"guest input must not invoke host handlers or hooks");
    let guest=game.splitscreen_guest.as_ref().unwrap();
    assert_eq!(guest.calls,["handle","hook"]);assert_eq!(guest.observed,[(key.clone(),9)]);
    let guest=game.splitscreen_guest.as_mut().unwrap();
    guest.scripted=true;guest.calls.clear();guest.observed.clear();
    game.window_event(&ActiveEventLoop,7,WindowEvent::KeyboardInput{event:key,is_synthetic:false});
    assert!(game.calls.is_empty(),"scripted guest input must not reach host handlers");
    let guest=game.splitscreen_guest.as_ref().unwrap();
    assert_eq!(guest.calls,["handle"]);assert!(guest.observed.is_empty(),"scripted guest input reaches no native key hook");
    // A person's key during a scripted run reaches no mod either.
    let mut game = Game { splitscreen_guest: None, window: Some(Window), modifiers: 5, early: false, scripted: true,
        calls: vec![], observed: vec![] };
    let key = KeyEvent { physical_key: -1, logical_key: "logical", location: 2, text: "z",
        pressed: true, repeat: false };
    game.window_event(&ActiveEventLoop, 7, WindowEvent::KeyboardInput { event: key, is_synthetic: false });
    assert!(game.observed.is_empty());
}
""".replace("METHOD", actual)
        with tempfile.TemporaryDirectory() as folder:
            src, exe = Path(folder) / "key.rs", Path(folder) / "key.exe"
            src.write_text(fixture, encoding="utf-8")
            subprocess.run(["rustc", "--edition=2024", "--test", str(src), "-o", str(exe)], check=True)
            subprocess.run([str(exe)], check=True)
