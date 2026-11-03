"""The actual entity-use dispatch honors the click's secondary-action snapshot."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class EntityInteractionSecondaryTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which('rustc'), 'requires Rust')
    def test_click_snapshot_wins_over_newer_movement_input(self):
        source = (ROOT / 'crates/horizon-client/src/game/lan_actions.rs').read_text()
        marker = source.index('g.guest_use = Some(GuestUse::Entity(hand, target));')
        start = source.rfind('let swung = self.as_guest(host, conn, |g| {', 0, marker)
        start = source.index('{', start) + 1
        body = source[start:source.index('});', marker)]
        fixture = r'''
use std::collections::HashSet;
#[derive(Eq, PartialEq, Hash)] enum KeyCode { ShiftLeft, KeyW }
enum GuestUse { Entity((), ()) }
struct Packet { secondary: bool }
struct Game { keys: HashSet<KeyCode>, guest_use: Option<GuestUse>, swings_started: u32, expected: bool }
impl Game {
    fn use_item(&mut self) -> Result<(), ()> {
        assert_eq!(self.keys.contains(&KeyCode::ShiftLeft), self.expected);
        self.swings_started += 1; Ok(())
    }
}
#[test]
fn click_snapshot() {
    for movement in [false, true] {
        for secondary in [false, true] {
            let p = Packet { secondary };
            let (hand, target) = ((), ());
            let mut g = Game { keys: HashSet::from([KeyCode::KeyW]), guest_use: None,
                               swings_started: 0, expected: secondary };
            if movement { g.keys.insert(KeyCode::ShiftLeft); }
            let swung = (|g: &mut Game| { BODY })(&mut g);
            assert!(swung && g.guest_use.is_none());
            assert!(g.keys.contains(&KeyCode::KeyW));
        }
    }
}
'''.replace('BODY', body)
        with tempfile.TemporaryDirectory() as directory:
            src, binary = Path(directory) / 'dispatch.rs', Path(directory) / 'dispatch'
            src.write_text(fixture)
            subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)
