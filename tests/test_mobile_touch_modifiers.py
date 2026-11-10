"""Touch Shift is a live physical modifier, independent of crouch latches."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


class TouchModifierTests(unittest.TestCase):
    def test_menu_and_world_shift_click_modifier_releases_without_latching(self):
        if not shutil.which("rustc"):
            self.skipTest("rustc required")
        source = (ROOT / "crates/horizon-client/src/game/touch.rs").read_text()
        inventory = (ROOT / "crates/horizon-client/src/game/inventory_input.rs").read_text()
        methods = (method(source, "touch_shift_held") + method(inventory, "has_shift_down")).replace('target_os = "ios"', 'feature = "ios-fixture"')
        program = r'''
#![allow(dead_code)]
#[path="SOURCE"]mod touch;
use touch::Control;
#[derive(Default)]struct Modifiers(bool);impl Modifiers{fn shift_key(&self)->bool{self.0}}
struct ClickContext{touch_shift:bool}
#[derive(Default)]struct Input{fingers:touch::Touches,cursor_keys:Vec<Control>,cursor_delivery:Option<ClickContext>}
#[derive(Default)]struct Game{touch:Input,modifiers:Modifiers}
impl Game{METHODS}
fn main(){
 let mut game=Game::default();let layout=[(Control::Sneak,[0.,0.,40.,40.])];
 assert!(!game.has_shift_down());
 game.touch.fingers.start(1,(20.,20.),0.,&layout);assert!(game.has_shift_down());
 game.touch.fingers.end(1);assert!(!game.has_shift_down());
 game.touch.cursor_keys.push(Control::Sneak);assert!(game.has_shift_down());
 game.touch.cursor_keys.clear();assert!(!game.has_shift_down());
 // A touch release does not clear the hardware keyboard's own live modifier.
 game.modifiers.0=true;game.touch.fingers.start(2,(20.,20.),0.,&layout);
 game.touch.fingers.cancel(2);assert!(game.has_shift_down());
 game.modifiers.0=false;assert!(!game.has_shift_down());
 // A delayed click is delivered with the Shift state captured when it was made,
 // whatever the Shift button is doing by the time it arrives.
 game.touch.cursor_delivery=Some(ClickContext{touch_shift:true});assert!(game.has_shift_down());
 game.touch.cursor_keys.push(Control::Sneak);
 game.touch.cursor_delivery=Some(ClickContext{touch_shift:false});assert!(!game.has_shift_down());
 game.touch.cursor_delivery=None;assert!(game.has_shift_down());
}
'''.replace("SOURCE", str(ROOT / "crates/horizon-client/src/touch.rs")).replace("METHODS", methods)
        with tempfile.TemporaryDirectory(prefix="horizon-touch-shift-") as directory:
            folder = Path(directory)
            (folder / "probe.rs").write_text(program)
            built = subprocess.run(["rustc", "--edition=2024", "--cfg", 'feature="ios-fixture"', str(folder / "probe.rs"), "-o", str(folder / "probe")], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            ran = subprocess.run([str(folder / "probe")], capture_output=True, text=True)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)
