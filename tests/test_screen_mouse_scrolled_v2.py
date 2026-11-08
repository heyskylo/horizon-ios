"""Run the window's actual wheel dispatch to the screen hooks with a fake mod."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def block(source, start_text):
    """The `if` statement starting at `start_text`, through its closing brace."""
    start = source.index(start_text)
    opening = source.index("{", start)
    depth, end = 1, opening + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


class ScreenMouseScrolledV2Tests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("rustc"), "requires Rust")
    def test_v2_before_v1_cancellation_and_mutable_game(self):
        source = (ROOT / "crates/horizon-client/src/game.rs").read_text(encoding="utf-8")
        dispatch = block(source, "if !self.pad_synthetic_input()\n                        && let Some((id, mouse))")
        self.assertIn("native_screen_mouse_scrolled_pre_v2", dispatch)
        fixture = r"""
fn glfw_modifiers(bits: u32) -> u32 { bits }
struct View { id: &'static str, mouse: [f32; 2] }
struct Game { modifiers: u32, synthetic: bool, open: bool, cancel_v1: bool, cancel_v2: bool,
    close_in_v2: bool, calls: Vec<&'static str>, scrolled: usize }
impl Game {
    fn pad_synthetic_input(&self) -> bool { self.synthetic }
    fn screen_view_now(&self) -> Option<View> { self.open.then_some(View { id: "inventory", mouse: [7.0, 9.0] }) }
    fn native_screen_mouse_scrolled_pre_v2(&mut self, screen: &str, mouse: [f32; 2], scroll: [f64; 2], modifiers: u32, canceled: &mut bool) {
        assert_eq!((screen, mouse, scroll, modifiers), ("inventory", [7.0, 9.0], [0.0, -2.0], 5));
        self.calls.push("v2");
        if self.close_in_v2 { self.open = false; }
        *canceled = self.cancel_v2;
    }
    fn native_screen_mouse_scrolled_pre(&self, screen: &View, scroll: [f64; 2], modifiers: u32, canceled: &mut bool) {
        assert_eq!((screen.id, scroll, modifiers), ("inventory", [0.0, -2.0], 5));
        *canceled = self.cancel_v1;
    }
    fn wheel(&mut self, raw_x: f64, raw_y: f64) -> Result<(), ()> {
        let scaled = |v: f64| v * 2.0;
        DISPATCH
        self.scrolled += 1;
        Ok(())
    }
}
fn game(cancel_v1: bool, cancel_v2: bool, close_in_v2: bool) -> Game {
    Game { modifiers: 5, synthetic: false, open: true, cancel_v1, cancel_v2, close_in_v2, calls: vec![], scrolled: 0 }
}
#[test]
fn order_and_cancellation() {
    // (cancel v1, cancel v2, v2 closes the screen) -> the screen scrolls
    for (cancel_v1, cancel_v2, close, scrolls) in [
        (false, false, false, true), (true, false, false, false),
        (false, true, false, false), (true, true, false, false),
        (true, false, true, true)] {
        let mut g = game(cancel_v1, cancel_v2, close);
        g.wheel(0.0, -1.0).unwrap();
        assert_eq!(g.calls, ["v2"]);
        assert_eq!(g.scrolled == 1, scrolls, "{cancel_v1} {cancel_v2} {close}");
    }
    let mut g = game(true, true, false);
    g.synthetic = true;
    g.wheel(0.0, -1.0).unwrap();
    assert!(g.calls.is_empty() && g.scrolled == 1);
    let mut g = game(true, true, false);
    g.open = false;
    g.wheel(0.0, -1.0).unwrap();
    assert!(g.calls.is_empty() && g.scrolled == 1);
}
""".replace("DISPATCH", dispatch)
        with tempfile.TemporaryDirectory() as folder:
            src, exe = Path(folder) / "scroll.rs", Path(folder) / "scroll.exe"
            src.write_text(fixture, encoding="utf-8")
            subprocess.run(["rustc", "--edition=2024", "--test", str(src), "-o", str(exe)], check=True)
            subprocess.run([str(exe)], check=True)


if __name__ == "__main__":
    unittest.main()
