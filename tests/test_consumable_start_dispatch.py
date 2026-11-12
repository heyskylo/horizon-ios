"""Exercise the real game consumable dispatch, not only the world component kernel.

Vanilla 26.3: Consumable.startConsuming bytecode 13..77 consumes immediately
when consumeTicks <= 0. ItemStack.use (72..77), finishUsingItem (7..21), and
applyAfterUseComponentSideEffects (0..70) order consumption/remainder before
cooldown and obtain both after-use components from the original stack.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def method(source, name):
    position = source.index("fn " + name + "(")
    start = source.rfind("\n", 0, position) + 1
    opening = source.index("{", position)
    end, depth = opening + 1, 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end].replace("pub(super)", "pub(crate)")


@unittest.skipUnless(shutil.which("rustc"), "requires Rust")
class ConsumableDispatchTests(unittest.TestCase):
    def test_instant_and_timed_completion_use_original_components_in_order(self):
        consume = (ROOT / "crates/horizon-client/src/game/consume.rs").read_text()
        game = (ROOT / "crates/horizon-client/src/game/using_item.rs").read_text()
        tick = method(game, "tick_used_item")
        completion = tick[tick.rindex("        self.item_on_use_tick(&stack, ticks);"): ]
        completion = completion[:completion.rfind("\n    }")]
        item_use = (ROOT / "crates/horizon-client/src/game/item_use.rs").read_text()
        fixture = r'''
#![allow(dead_code)]
mod item_use { HELPERS }
mod fixture { use super::*;
mod item_sync {
 pub fn food(_: &super::ItemStack, prototype: Option<()>) -> Option<()> {prototype}
 pub fn can_always_eat(_: &super::ItemStack, prototype: bool) -> bool {prototype}
}
#[derive(Clone)] struct ItemStack { item: &'static str, cooldown: &'static str }
#[derive(Clone)] struct ItemConsumable { ticks: i32 }
impl ItemConsumable { fn ticks(&self) -> i32 { self.ticks } }
struct ItemInfo { food: Option<()>, can_always_eat: bool, consumable: Option<ItemConsumable> }
struct Food { food: i32 }
struct Health { food: Food }
struct Player { health: Health }
struct Game {
 player: Option<Player>, creative: bool, using: Option<(usize,u32)>, slot: usize,
 held: Option<ItemStack>, events: Vec<String>, remote: Option<()>, use_duration: i32,
}
impl Game {
 fn new(slot: usize) -> Self {
  Self { player: Some(Player { health: Health { food: Food { food: 19 } } }),
   creative: false, using: None, slot, remote: None, use_duration: 2,
   held: Some(ItemStack { item: "original", cooldown: "original-group" }), events: vec![] }
 }
 fn consumable(&self,_:&ItemStack)->Option<ItemConsumable>{Some(ItemConsumable{ticks:self.use_duration})}
 fn can_eat(&self,_:&ItemStack,always:bool)->bool{always||self.creative||self.player.as_ref().is_some_and(|p|p.health.food.food<20)}
 fn start_using_item(&mut self){if self.using.is_none(){self.using=Some((self.slot,0));}}
 fn item_on_use_tick(&mut self,_:&ItemStack,_:u32){}
 fn finish_drinking(&mut self,_:usize,_:&ItemStack)->bool{false}
 fn stop_spyglass(&mut self){}
 fn use_vibration(&mut self,stack:&ItemStack,event:&'static str){assert_eq!(stack.item,"original");assert_eq!(stack.cooldown,"original-group");assert_eq!(event,"minecraft:item_interact_finish");self.events.push(format!("vibration:{event}"));}
 fn use_item_finished(&mut self,stack:&ItemStack){assert_eq!(stack.item,"original");assert_eq!(stack.cooldown,"original-group");assert_eq!(self.held.as_ref().unwrap().item,"bowl");self.events.push("finish".into());}
 fn held(&self) -> Option<&ItemStack> {self.held.as_ref()}
 fn held_slot(&self) -> usize {self.slot}
 fn send_use_item_complete(&mut self) {self.events.push("event".into());}
 fn finish_consuming(&mut self, _: &ItemInfo, _: &ItemConsumable, stack: &ItemStack) {
  self.events.push(format!("consume:{}",stack.item));
  self.held=Some(ItemStack {item:"bowl",cooldown:"wrong-bowl-group"});
  self.events.push("remainder:bowl".into());
 }
 fn apply_use_cooldown(&mut self, stack: &ItemStack) {
  self.events.push(format!("cooldown:{}",stack.cooldown));
 }
METHODS
 fn finish_tick(&mut self, info: ItemInfo, stack: ItemStack, ticks:u32,duration:u32) {
  self.use_duration=duration as i32;
  let slot=self.slot;
TICK_COMPLETION
 }
}
fn info(ticks:i32) -> ItemInfo {
 ItemInfo {food:Some(()),can_always_eat:false,consumable:Some(ItemConsumable {ticks})}
}
pub fn run() {
 for slot in [0,40] {
  let mut g=Game::new(slot);g.use_duration=0;
  assert!(g.start_eating(&info(0)));
  assert!(g.using.is_none(),"instant use must not wait for a tick/release");
  assert_eq!(g.events,vec!["consume:original","remainder:bowl","cooldown:original-group"]);
  assert_eq!(g.held.as_ref().unwrap().item,"bowl");
  let mut timed=Game::new(slot);
  assert!(timed.start_eating(&info(2)));
  assert_eq!(timed.using,Some((slot,0))); assert!(timed.events.is_empty());
  let stack=timed.held.as_ref().unwrap().clone();
  timed.finish_tick(info(2),stack.clone(),0,2);
  assert!(timed.events.is_empty(),"not completed before duration");
  timed.finish_tick(info(2),stack,1,2);
  assert!(timed.using.is_none());
  assert_eq!(timed.events,vec!["event","consume:original","remainder:bowl","cooldown:original-group","finish","vibration:minecraft:item_interact_finish"]);
 }
 for ticks in [0,1] {
  let mut full=Game::new(0);full.use_duration=ticks;full.player.as_mut().unwrap().health.food.food=20;
  assert!(!full.start_eating(&info(ticks)));assert!(full.events.is_empty());assert!(full.using.is_none());
 }
 // A stack patch overrides the prototype's positive duration.
 let mut patched=Game::new(0);patched.use_duration=0;assert!(patched.start_eating(&info(2)));
 assert!(patched.using.is_none());assert_eq!(patched.events,vec!["consume:original","remainder:bowl","cooldown:original-group"]);
 let mut always=Game::new(0);always.use_duration=0;always.player.as_mut().unwrap().health.food.food=20;
 let mut edible=info(0);edible.can_always_eat=true;
 assert!(always.start_eating(&edible));assert_eq!(always.events.len(),3);
 let mut remote=Game::new(0);remote.use_duration=0;remote.remote=Some(());assert!(remote.start_eating(&info(0)));assert!(remote.events.is_empty());
}
}
fn main(){fixture::run();}
'''
        fixture = fixture.replace("METHODS", method(game, "start_eating").replace("horizon_client::items::ItemInfo", "ItemInfo").replace("horizon_client::item_sync::", "item_sync::") + "\n" + method(consume, "complete_consuming") + "\n" + method(item_use,"complete_using_item")).replace("TICK_COMPLETION", completion).replace("HELPERS", "\n".join(method(item_use,n) for n in ("consumes_at_once", "count_down")))
        with tempfile.TemporaryDirectory(prefix="horizon-consumable-dispatch-") as directory:
            directory = Path(directory)
            (directory / "probe.rs").write_text(fixture)
            built = subprocess.run(["rustc", "--edition=2024", str(directory / "probe.rs"),
                                    "-o", str(directory / "probe")], capture_output=True, text=True, timeout=60)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            run = subprocess.run([str(directory / "probe")], cwd=directory, capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)


if __name__ == "__main__":
    unittest.main()
