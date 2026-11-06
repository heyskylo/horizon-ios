//! Brewing parity (tools/test_brewing_parity.py): reads "input reagent
//! candidate" lines of item SNBT and prints the three brewing-slot filters,
//! the matching recipe and its assembled result.
//!
//! The first input line is `#resources RESOURCES`; usage: brewing_fixture INPUT OUTPUT
use horizon_client::{
    brewing::{Brewing, brewing_fuel},
    item_data::{canon_stack, parse_stack},
    items::Items,
};
use horizon_world::loot::PackOverlay;
use std::{fmt::Write as _, fs};

pub fn main() -> horizon_client::Result<()> {
    let args: Vec<String> = crate::args().collect();
    let input = fs::read_to_string(args.get(1).ok_or("missing INPUT")?)?;
    let root = input
        .lines()
        .find_map(|line| line.strip_prefix("#resources "))
        .map(std::path::PathBuf::from)
        .ok_or("no #resources line")?;
    let items = Items::catalog(&root, &PackOverlay::NONE)?;
    let brewing = Brewing::load(&root, &items, &PackOverlay::NONE);
    let mut out = String::new();
    for (n, line) in input
        .lines()
        .filter(|line| !line.starts_with('#'))
        .enumerate()
    {
        let f: Vec<&str> = line.split(' ').collect();
        let input = parse_stack(f[0])?;
        let reagent = parse_stack(f[1])?;
        let candidate = parse_stack(f[2])?;
        let potion_slot = candidate.as_ref().is_some_and(|s| brewing.is_bottle(s));
        let reagent_slot = candidate.as_ref().is_some_and(|s| brewing.is_reagent(s));
        let fuel_slot = candidate
            .as_ref()
            .is_some_and(|s| brewing_fuel(&items, s).is_some());
        let found = match (input.as_ref(), reagent.as_ref()) {
            (Some(input), Some(reagent)) => brewing.find(input, reagent),
            _ => None,
        };
        let result = found.map_or_else(|| "-".to_owned(), |r| canon_stack(Some(&r.output)));
        let id = found.map_or_else(|| "-".to_owned(), |r| r.id.clone());
        writeln!(
            out,
            "{n} {potion_slot} {reagent_slot} {fuel_slot} {result} {id}"
        )?;
    }
    fs::write(args.get(2).ok_or("missing OUTPUT")?, out)?;
    Ok(())
}
