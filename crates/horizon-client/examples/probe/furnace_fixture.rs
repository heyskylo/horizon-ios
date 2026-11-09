//! Furnace parity (tools/test_furnace_parity.py): reads "furnace KIND TICKS
//! INPUT FUEL OUTPUT TIMER TOTAL LIT LITTOTAL SPEED CHANGE_AT NEW_INPUT"
//! lines, runs the furnace model and prints the slots, timers, LIT state,
//! RecipesUsed counts and each used recipe's experience.
//!
//! The first input line is `#resources RESOURCES`; usage: furnace_fixture INPUT OUTPUT
use horizon_client::{
    crafting::Recipes,
    furnace::FurnaceCooking,
    item_data::{canon_stack, parse_stack},
    items::Items,
};
use horizon_client_render::{
    chunk::block_entity::{Furnace, FurnaceKind},
    providers::Providers,
};
use horizon_world::loot::PackOverlay;
use std::{collections::HashMap, fmt::Write as _, fs};

pub fn main() -> horizon_client::Result<()> {
    let args: Vec<String> = crate::args().collect();
    let input = fs::read_to_string(args.get(1).ok_or("missing INPUT")?)?;
    let root = input
        .lines()
        .find_map(|line| line.strip_prefix("#resources "))
        .map(std::path::PathBuf::from)
        .ok_or("no #resources line")?;
    let items = Items::catalog(&root, &PackOverlay::NONE)?;
    let recipes = Recipes::load(&root, &items, &PackOverlay::NONE)?;
    let providers = Providers::load(&root);
    let mut out = String::new();
    for (n, line) in input
        .lines()
        .filter(|line| !line.starts_with('#'))
        .enumerate()
    {
        let f: Vec<&str> = line.split(' ').collect();
        let kind = match f[1] {
            "furnace" => FurnaceKind::Furnace,
            "blast_furnace" => FurnaceKind::BlastFurnace,
            "smoker" => FurnaceKind::Smoker,
            _ => return Err("unknown furnace kind".into()),
        };
        let ticks: u32 = f[2].parse()?;
        let mut items3 = [None, None, None];
        for i in 0..3 {
            items3[i] = parse_stack(f[3 + i])?;
        }
        let mut furnace = Furnace {
            kind,
            items: items3,
            lit_remaining: f[8].parse()?,
            lit_total: f[9].parse()?,
            cooking_timer: f[6].parse()?,
            cooking_total: f[7].parse()?,
            speed: f[10].parse()?,
            recipes_used: HashMap::new(),
        };
        let change_at: i32 = f[11].parse()?;
        let new_input = parse_stack(f[12])?;
        let mut lit_state = false;
        let mut drops = Vec::new();
        for t in 0..ticks {
            if t as i32 == change_at {
                furnace.set_input(&recipes, new_input.clone());
            }
            if let Some(lit) = furnace.tick(&recipes, &items, &providers, &mut drops) {
                lit_state = lit;
            }
        }
        if change_at == ticks as i32 {
            furnace.set_input(&recipes, new_input.clone());
        }
        write!(out, "{n} kind={} ", f[1])?;
        for i in 0..3 {
            write!(out, "s{i}={} ", canon_stack(furnace.items[i].as_ref()))?;
        }
        write!(
            out,
            "timer={} total={} lit={} littotal={} speed={:.3} state={lit_state} used=",
            furnace.cooking_timer,
            furnace.cooking_total,
            furnace.lit_remaining,
            furnace.lit_total,
            furnace.speed
        )?;
        let mut ids: Vec<&String> = furnace.recipes_used.keys().collect();
        ids.sort();
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write!(out, "{id}={}", furnace.recipes_used[*id])?;
        }
        out.push_str(" xp=");
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write!(out, "{id}={}", recipes.cooking_experience(id))?;
        }
        writeln!(out)?;
    }
    fs::write(args.get(2).ok_or("missing OUTPUT")?, out)?;
    Ok(())
}
