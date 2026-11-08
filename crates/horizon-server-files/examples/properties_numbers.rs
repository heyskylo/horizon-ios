//! DedicatedServerProperties numeric parsing probe; no server or window.
use horizon_server_files::{
    java_properties::Properties,
    properties::{Entropy, ServerProperties},
};
struct Fixed;
impl Entropy for Fixed {
    fn next_long(&mut self) -> i64 {
        1
    }
    fn next_int(&mut self, _: u32) -> u32 {
        0
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::args().nth(1).ok_or("INPUT")?;
    for value in std::fs::read_to_string(input)?.lines() {
        let mut raw = Properties::default();
        for key in ["max-players", "op-permission-level", "max-tick-time"] {
            raw.put(key, value.into());
        }
        let p = ServerProperties::new(raw, &mut Fixed)?;
        println!(
            "RESULT {} {} {}",
            p.max_players, p.op_permission_level, p.max_tick_time
        );
        let mut extra = Properties::default();
        for key in ["difficulty", "gamemode", "max-world-size", "level-seed"] {
            extra.put(key, value.into());
        }
        let q = ServerProperties::new(extra, &mut Fixed)?;
        let seed = if q.random_seed {
            "<random>".into()
        } else {
            q.seed.to_string()
        };
        println!(
            "EXTRA {} {} {} {seed}",
            q.difficulty, q.game_mode, q.max_world_size
        );
        let mut broadcast = Properties::default();
        broadcast.put("entity-broadcast-range-percentage", value.into());
        match ServerProperties::new(broadcast, &mut Fixed) {
            Ok(q) => println!("BROADCAST {}", q.entity_broadcast_range_percentage),
            Err(_) => println!("BROADCAST ERROR"),
        }
    }
    Ok(())
}
