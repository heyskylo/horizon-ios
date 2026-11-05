"""Actual remote clock tick compared with pinned 26.3 ClientClockManager results."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    source = (ROOT / 'crates/horizon-client/src/game/remote.rs').read_text()
    return STUBS + '\nimpl Game {\n' + method(source, 'remote_clock_tick') + '\n}\n' + CASES


STUBS = r'''
struct World{game_time:i64,day_time:i64}
struct Remote{clock_game_time:i64,clock_partial:f32,clock_rate:f32}
struct Game{world:World,remote:Option<Remote>}
'''

# Expected day ticks and raw partial-tick bits came from the actual shipped
# ClientClockManager.tick using its real ClientClockInstance, not Rust's math.
CASES = r'''
fn main(){
 let cases:[(i64,i64,i64,f32,f32,i64,u32);10]=[
  (10,0,100,0.0,1.0,110,0),
  (1,0,i64::MAX,0.0,1.0,i64::MIN,0),
  (i64::MIN,i64::MAX,100,0.0,1.0,101,0),
  (i64::MAX,i64::MIN,100,0.0,1.0,99,0),
  (5_000_000_000,0,100,0.0,1.0,2_147_483_747,0x4f2a05f2),
  (-5_000_000_000,0,100,0.0,1.0,-2_147_483_548,0xcf2a05f2),
  (10,0,100,0.5,0.25,103,0),
  (1,0,100,f32::NAN,1.0,100,0x7fc00000),
  (1,0,100,f32::INFINITY,1.0,2_147_483_747,0x7f800000),
  (1,0,100,f32::NEG_INFINITY,1.0,-2_147_483_548,0xff800000),
 ];
 let mut failures=0;
 for(i,(now,last,day,partial,rate,want_day,want_partial))in cases.into_iter().enumerate(){
  let mut g=Game{world:World{game_time:now,day_time:day},remote:Some(Remote{clock_game_time:last,clock_partial:partial,clock_rate:rate})};
  if std::panic::catch_unwind(std::panic::AssertUnwindSafe(||g.remote_clock_tick())).is_err(){eprintln!("case {i} panicked");failures+=1;continue;}
  let r=g.remote.unwrap();
  if (g.world.day_time,r.clock_partial.to_bits(),r.clock_game_time)!=(want_day,want_partial,now){eprintln!("case {i}: got ({},{:08x},{}), expected ({},{:08x},{})",g.world.day_time,r.clock_partial.to_bits(),r.clock_game_time,want_day,want_partial,now);failures+=1;}
 }
 assert_eq!(failures,0,"clock differs from native 26.3");
 let mut g=Game{world:World{game_time:20,day_time:30},remote:None};g.remote_clock_tick();assert_eq!(g.world.day_time,30);
}
'''


class RemoteClockTickTests(unittest.TestCase):
    def test_remote_tick_matches_native_clock_arithmetic(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-remote-clock-') as folder:
            source, binary = Path(folder) / 'probe.rs', Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
