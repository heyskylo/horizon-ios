"""Exercise production menu ownership across all guest menu storage paths."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_world_input_capture import method

ROOT = Path(__file__).resolve().parents[1]


class GuestMenuWindowTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which('rustc'), 'requires Rust')
    def test_only_the_open_menu_accepts_addressed_actions(self):
        source = (ROOT / 'crates/horizon-client/src/game/lan_actions.rs').read_text()
        window = method(source, 'guest_menu_window')
        start = source.index('        let addressed_window = match &packet')
        end = source.index('        if matches!(', start)
        gate = source[start:end].replace('return;', 'return false;')
        fixture = r'''
use std::collections::HashMap;
struct Menu { window: i32 }
struct Guest { menu: Option<Menu> }
struct Hub { g: Guest }
impl Hub { fn guest(&self, _: u32) -> Option<&Guest> { Some(&self.g) } }
#[derive(Default)] struct Crafting { table: Option<(i32,)> }
#[derive(Default)] struct Play { crafting: Crafting, station: Option<Menu>, merchant: Option<Menu>, lectern: Option<Menu>, mount_window: Option<Menu> }
struct LanHost { hub: Hub, play: HashMap<u32, Play> }
struct Packet { container: i32 }
enum S { ContainerClick(Packet), ContainerButtonClick(Packet), ContainerSlotStateChanged(Packet), PlaceRecipe(Packet), Other }
struct Game;
impl Game {
    WINDOW
    fn accepts(&self, host: &LanHost, conn: u32, packet: S) -> bool { GATE true }
}
#[test]
fn menu_ownership() {
    for owner in 0..7 {
        let mut p = Play::default();
        let mut g = Guest { menu: None };
        match owner {
            0 => {},
            1 => g.menu = Some(Menu { window: 7 }),
            2 => p.crafting.table = Some((7,)),
            3 => p.station = Some(Menu { window: 7 }),
            4 => p.merchant = Some(Menu { window: 7 }),
            5 => p.lectern = Some(Menu { window: 7 }),
            _ => p.mount_window = Some(Menu { window: 7 }),
        }
        let host = LanHost { hub: Hub { g }, play: HashMap::from([(42, p)]) };
        let expected = if owner == 0 { 0 } else { 7 };
        assert_eq!(Game.guest_menu_window(&host, 42), expected);
        for requested in [0, 7, 8, -1] {
            for kind in 0..4 {
                let p = Packet { container: requested };
                let packet = match kind { 0 => S::ContainerClick(p), 1 => S::ContainerButtonClick(p),
                    2 => S::ContainerSlotStateChanged(p), _ => S::PlaceRecipe(p) };
                assert_eq!(Game.accepts(&host, 42, packet), requested == expected,
                           "owner {owner} request {requested} kind {kind}");
            }
        }
        assert!(Game.accepts(&host, 42, S::Other));
    }
}
'''.replace('WINDOW', window).replace('GATE', gate)
        with tempfile.TemporaryDirectory() as directory:
            src, exe = Path(directory) / 'windows.rs', Path(directory) / 'windows'
            src.write_text(fixture)
            subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
            subprocess.run([str(exe)], check=True)
