//! A level's saved chunk tickets through `chunk_tickets.dat`, for VanillaPortalTicketsOracle:
//! each case's file is loaded, its `PORTAL` tickets ticked, refreshed and reloaded over a
//! restart, and every step prints what the file then holds and which of the case's chunks a
//! `PORTAL` ticket holds now.
use horizon_client::save::{LevelShape, SaveDir, read_dat, write_dat};
use horizon_client::ticket_levels::ChunkTickets;
use horizon_world::nbt::{Tag, parse_snbt};
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

fn chunk(pos: &Value) -> (i32, i32) {
    (
        pos[0].as_i64().unwrap() as i32,
        pos[1].as_i64().unwrap() as i32,
    )
}

/// Save the level's tickets, then list each saved one (type, level, chunk and ticks_left,
/// "-" where it is left out) and "held x z" for each of `chunks` a `PORTAL` ticket holds.
fn snapshot(
    (save, file): (&SaveDir, &Path),
    forced: &HashSet<(i32, i32)>,
    tickets: &ChunkTickets,
    case: &Value,
) -> Vec<String> {
    save.save_chunk_tickets(forced, tickets.portal_tickets())
        .unwrap();
    let data = read_dat(file).unwrap();
    let mut rows = Vec::new();
    if let Some(Tag::List(saved)) = data.get("data").and_then(|d| d.get("tickets")) {
        for t in saved {
            let (Some(Tag::String(kind)), Some(Tag::Int(level)), Some(Tag::IntArray(pos))) =
                (t.get("type"), t.get("level"), t.get("chunk_pos"))
            else {
                panic!("unexpected ticket {t:?}")
            };
            let left = match t.get("ticks_left") {
                Some(Tag::Long(n)) => n.to_string(),
                None => "-".into(),
                Some(other) => format!("{other:?}"),
            };
            let kind = kind.to_string_lossy();
            rows.push(format!("{kind} {level} {} {} {left}", pos[0], pos[1]));
        }
    }
    for (x, z) in case["chunks"].as_array().unwrap().iter().map(chunk) {
        if tickets.portal_tickets().iter().any(|t| t.chunk == (x, z)) {
            rows.push(format!("held {x} {z}"));
        }
    }
    rows.sort();
    rows
}

pub fn main() {
    let args: Vec<_> = super::args().collect();
    let input = std::fs::read_to_string(&args[1]).unwrap();
    let mut out = std::fs::File::create(&args[2]).unwrap();
    let root = std::env::temp_dir().join(format!("horizon-portal-tickets-{}", std::process::id()));
    for line in input.lines() {
        let _ = std::fs::remove_dir_all(&root);
        let case: Value = serde_json::from_str(line).unwrap();
        let (data, _) = parse_snbt(case["saved"].as_str().unwrap()).unwrap();
        let file = root.join("data/minecraft/chunk_tickets.dat");
        write_dat(&file, Tag::compound([("data", data)])).unwrap();
        // How the game opens a level (`world_loading::make_streaming`, `open_level`).
        let save = SaveDir::new(root.clone(), LevelShape::OVERWORLD);
        let mut forced = save.load_forced_chunks();
        let mut tickets = ChunkTickets::with_saved_portal(save.load_portal_tickets());
        let level = (&save, file.as_path());
        let mut trace = vec![snapshot(level, &forced, &tickets, &case)];
        for op in case["ops"].as_array().unwrap() {
            match op["op"].as_str().unwrap() {
                "tick" => {
                    let loading: HashSet<_> = op["loading"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(chunk)
                        .collect();
                    for _ in 0..op["times"].as_u64().unwrap_or(1) {
                        tickets.tick_portal(|t| !loading.contains(&t.chunk));
                    }
                }
                "portal" => tickets.add_portal(chunk(&op["pos"])),
                // The last snapshot saved them: the level opens again from the file.
                "restart" => {
                    forced = save.load_forced_chunks();
                    tickets = ChunkTickets::with_saved_portal(save.load_portal_tickets());
                }
                other => panic!("unknown operation {other}"),
            }
            trace.push(snapshot(level, &forced, &tickets, &case));
        }
        writeln!(out, "{}", serde_json::to_string(&trace).unwrap()).unwrap();
    }
    let _ = std::fs::remove_dir_all(&root);
}
