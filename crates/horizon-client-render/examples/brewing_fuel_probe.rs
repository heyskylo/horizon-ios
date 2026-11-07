//! Headless production brewing-fuel component fixture.
use horizon_client_render::{
    brewing::{Brewing, BrewingStand, brewing_fuel},
    providers::Providers,
};
use horizon_items::{
    item_data::decode_components,
    items::{ItemStack, Items},
};
use horizon_world::loot::PackOverlay;
use std::{
    io::{BufRead, Write},
    path::Path,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(&args[1]);
    let items = Items::catalog(root, &PackOverlay::NONE)?;
    let providers = Providers::load(root);
    let brewing = Brewing {
        recipes: vec![],
        reagents: Default::default(),
        inputs: Default::default(),
    };
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[3])?);
    for line in std::io::BufReader::new(std::fs::File::open(&args[2])?).lines() {
        let input: serde_json::Value = serde_json::from_str(&line?)?;
        let mut stack = ItemStack::new(
            input["item"].as_str().unwrap(),
            input["count"].as_u64().unwrap() as u32,
        );
        let components = horizon_client_core::scoreboard::json_to_tag(&input["components"]);
        let (_, extra) = decode_components(Some(&components));
        stack.extra = extra;
        let accepted = brewing_fuel(&items, &stack).is_some();
        let mut stand = BrewingStand::new();
        // A newly constructed vanilla block entity has no saved total yet.
        stand.total_fuel = 0;
        stand.items[4] = Some(stack);
        for _ in 0..2 {
            stand.tick(&brewing, &items, &providers);
            writeln!(
                out,
                "{} {} {} {} {}",
                accepted,
                stand.fuel,
                stand.total_fuel,
                stand.speed.to_bits(),
                stand.items[4].as_ref().map_or(0, |s| s.count)
            )?;
        }
    }
    Ok(())
}
