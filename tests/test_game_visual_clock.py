"""Deterministic frame clocks use world ticks without debug-only overflow."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method
ROOT=Path(__file__).resolve().parents[1]


def program():
    game=(ROOT/'crates/horizon-client/src/game.rs').read_text()
    return r'''
use std::time::Instant;
struct Clock{still:bool}impl Clock{fn still(&self)->bool{self.still}}
struct World{game_time:i64}struct Game{clock:Clock,world:World,ticks:u64,started:Instant}
impl Game{
''' + method(game,'visual_ticks') + method(game,'visual_millis') + r'''
}
fn main(){
 let mut g=Game{clock:Clock{still:true},world:World{game_time:7},ticks:130,started:Instant::now()};
 assert_eq!(g.visual_ticks(),7);assert_eq!(g.visual_millis(),350);
 g.world.game_time=-1;assert_eq!(g.visual_ticks(),0);assert_eq!(g.visual_millis(),0);
 g.world.game_time=i64::MAX;assert_eq!(g.visual_ticks(),9223372036854775807);assert_eq!(g.visual_millis(),-50);
 g.world.game_time=184467440737095517;assert_eq!(g.visual_millis(),-9223372036854775766);
 g.clock.still=false;assert_eq!(g.visual_ticks(),130);assert!(g.visual_millis()>=0&&g.visual_millis()<10000);
}
'''


class GameVisualClockTests(unittest.TestCase):
    def test_world_frame_clock_and_signed_millisecond_wrap(self):
        if not shutil.which('rustc'):self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-visual-clock-') as folder:
            source=Path(folder)/'probe.rs';binary=Path(folder)/'probe';source.write_text(program())
            built=subprocess.run(['rustc','--edition=2024',str(source),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(built.returncode,0,built.stdout+built.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
