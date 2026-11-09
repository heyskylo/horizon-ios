"""Exercise the actual touch dispatcher with an interrupted menu gesture."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method as production_method

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("rustc"), "requires Rust")
class TouchHandoffTests(unittest.TestCase):
    def test_real_dispatcher_recovers_and_cancels_without_ghost_clicks(self):
        source = (ROOT / "crates/horizon-client/src/game/touch.rs").read_text()

        def method(name):
            start = source.index("    ", source.index("impl Game<'_> {"))
            start = source.rfind("\n", start, source.index(f"fn {name}(", start)) + 1
            end = source.index("\n    }", start) + len("\n    }")
            return source[start:end].replace("pub(super)", "pub(crate)")

        fixture = r'''
#![allow(dead_code)]
use std::time::Instant;
#[path = "TOUCH_SOURCE"] mod touch;
#[path = "MODE_SOURCE"] mod input_mode;
use touch::{Control, Out, Touches, MenuGesture, MenuMode};
type Result<T> = std::result::Result<T, std::convert::Infallible>;
trait EventLoopCtl {}
struct ActiveEventLoop; impl EventLoopCtl for ActiveEventLoop {}
#[derive(Clone, Copy, PartialEq, Eq)]
enum TouchPhase { Started, Moved, Ended, Cancelled }
struct Point { x: f64, y: f64 }
struct Touch { id: u64, phase: TouchPhase, location: Point }
struct Batch { scale: f32, width: f32, height: f32 }
#[derive(Clone,Copy,PartialEq,Eq)]enum MenuTarget { Pointer }
struct MenuFinger { id: u64, gesture: MenuGesture, target: MenuTarget }
#[derive(Default)]
struct TouchInput { seen: bool, hardware: input_mode::InputMode,
    menu: Option<MenuFinger>, fingers: Touches }
#[derive(Default)]struct Pad{in_use:bool}
#[derive(Default)]struct EmoteWheel{controller:Option<()>}
struct Game { emote_wheel:EmoteWheel,pad:Pad,touch: TouchInput, last_input: Instant, playing: bool,
    actions: Vec<Out>, buttons: Vec<bool>, menu_calls: usize }
mod horizon_gamecontroller { pub fn keyboard_connected() -> bool { true } }
impl Game {
 fn menu_input_blocked(&self)->bool{false}
 fn cancel_touch_for_reload(&mut self){panic!("no reload in handoff fixture");}
    fn new(playing: bool, pressed: bool) -> Self {
        let mut gesture=MenuGesture::new((0.0,0.0),MenuMode::Drag);
        if pressed { gesture.frame(); gesture.frame(); }
        Self { emote_wheel:EmoteWheel::default(),pad:Pad::default(),touch: TouchInput { menu: Some(MenuFinger { id: 1, gesture, target: MenuTarget::Pointer }),
            ..TouchInput::default() }, last_input: Instant::now(), playing,
            actions: vec![], buttons: vec![], menu_calls: 0 }
    }
    fn pad_cancel_inputs(&mut self,_:&dyn EventLoopCtl){panic!("no controller was active in this menu-gesture probe");}
    fn touch_batch(&self) -> Option<Batch> { Some(Batch { scale: 2.0, width: 500.0, height: 300.0 }) }
    fn touch_now(&mut self) -> f64 { 0.0 }
    fn touch_playing(&self) -> bool { self.playing }
    fn touch_menu(&mut self, _: &dyn EventLoopCtl, _: u64, _: TouchPhase, _: (f32,f32)) { self.menu_calls += 1; }
    fn touch_button(&mut self, _: &dyn EventLoopCtl, down: bool) { self.buttons.push(down); }
    fn touch_act(&mut self, actions: Vec<Out>, _: f32) -> Result<()> { self.actions.extend(actions); Ok(()) }
METHODS
}
fn event(id: u64, phase: TouchPhase, at: (f32,f32)) -> Touch {
    Touch { id, phase, location: Point { x: (at.0 * 2.0).into(), y: (at.1 * 2.0).into() } }
}
fn forward() -> (f32,f32) {
    let r = touch::layout(500.0,300.0).into_iter().find(|(c,_)| *c == Control::Forward).unwrap().1;
    (r[0]+r[2]/2.0, r[1]+r[3]/2.0)
}
#[test]
fn lost_menu_release_does_not_swallow_world_movement() {
    let mut g=Game::new(true,true);
    g.touch_event(&ActiveEventLoop,event(2,TouchPhase::Started,forward())).unwrap();
    assert!(g.touch.menu.is_none());
    assert_eq!(g.buttons,vec![false]);
    assert_eq!(g.menu_calls,0);
    assert_eq!(g.actions,vec![Out::Press(Control::Forward)]);
    assert!(g.touch.fingers.held(Control::Forward));
    g.touch_event(&ActiveEventLoop,event(2,TouchPhase::Ended,forward())).unwrap();
    assert_eq!(g.actions.last(),Some(&Out::Release(Control::Forward)));
}
#[test]
fn keyboard_then_touch_restores_actions_not_only_overlay() {
    let mut g=Game::new(true,false);
    g.touch_keyboard_used(&ActiveEventLoop);
    assert!(g.touch.menu.is_none());
    assert!(g.touch.hardware.hides_touch(true,true));
    g.touch_event(&ActiveEventLoop,event(2,TouchPhase::Started,forward())).unwrap();
    assert!(!g.touch.hardware.hides_touch(true,true));
    assert_eq!(g.actions,vec![Out::Press(Control::Forward)]);
    assert!(g.buttons.is_empty(),"an unpressed menu gesture must not create a click");
}
#[test]
fn cancelled_world_finger_never_becomes_a_tap() {
    let mut g=Game::new(true,false);
    g.touch_event(&ActiveEventLoop,event(3,TouchPhase::Started,(250.0,150.0))).unwrap();
    g.touch_event(&ActiveEventLoop,event(3,TouchPhase::Cancelled,(250.0,150.0))).unwrap();
    assert!(g.actions.is_empty());
}
#[test]
fn cancelled_menu_finger_releases_without_delayed_press() {
    for pressed in [false,true] {
        let mut g=Game::new(false,pressed);
        g.touch_event(&ActiveEventLoop,event(1,TouchPhase::Cancelled,(250.0,150.0))).unwrap();
        assert!(g.touch.menu.is_none()); assert_eq!(g.menu_calls,0);
        assert_eq!(g.buttons,if pressed {vec![false]} else {vec![]});
    }
}
'''
        fixture = fixture.replace("TOUCH_SOURCE", str(ROOT / "crates/horizon-client/src/touch.rs"))
        fixture = fixture.replace("MODE_SOURCE", str(ROOT / "crates/horizon-client/src/game/input_mode.rs"))
        fixture = fixture.replace("METHODS", "\n".join(method(name) for name in (
            "touch_event", "touch_release_menu", "touch_keyboard_used")) + "\n" + production_method(
                (ROOT / "crates/horizon-client/src/game/pad.rs").read_text(), "pad_yield_input") + "\n" + production_method(
                (ROOT / "crates/horizon-client/src/game/emote_wheel.rs").read_text(), "cancel_emote_controller"))
        with tempfile.TemporaryDirectory(prefix="horizon-touch-handoff-") as folder:
            folder = Path(folder)
            (folder / "fixture.rs").write_text(fixture)
            build = subprocess.run(["rustc", "--edition=2024", "--test", str(folder / "fixture.rs"),
                                    "-o", str(folder / "probe")], capture_output=True, text=True)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(folder / "probe")], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)


if __name__ == "__main__":
    unittest.main()
