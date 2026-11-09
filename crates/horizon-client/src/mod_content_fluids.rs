//! The content mods' fluids: their registry order and the table the world
//! reads them from (`horizon_world::fluid::MOD_FLUIDS_FILE`).
use super::{ContentMod, Overlay, Result};
use horizon_mods::content::Fluid;

/// The mods' fluids in registry order, by id as effects are. Each takes two
/// network ids after vanilla's, its source's then its flowing form's.
pub fn fluids(mods: &[ContentMod]) -> Vec<&Fluid> {
    let mut fluids: Vec<&Fluid> = mods.iter().flat_map(|m| &m.content.fluids).collect();
    fluids.sort_by(|a, b| a.id.cmp(&b.id));
    fluids
}

/// Writes the fluid table's mod rows, in network-id order, when there are
/// any: the world reads them when it loads the content root
/// (`horizon_world::fluid::load_mod_fluids`).
pub(super) fn write_table(overlay: &Overlay<'_>, fluids: &[&Fluid]) -> Result<()> {
    if fluids.is_empty() {
        return Ok(());
    }
    let rows: Vec<horizon_world::fluid::ModFluid> = fluids
        .iter()
        .map(|f| horizon_world::fluid::ModFluid {
            id: f.id.clone(),
            flowing: f.flowing_id(),
            template: f.template.clone(),
            block: f.block.clone(),
            tick_delay: f.tick_rate.map(|t| t as i32),
            drop_off: f.level_decrease_per_block.map(|d| d as i32),
            slope_find_distance: f.slope_find_distance.map(|d| d as i32),
            convert_to_source: f.can_convert_to_source,
        })
        .collect();
    overlay.write(
        horizon_world::fluid::MOD_FLUIDS_FILE,
        &serde_json::to_vec(&rows)?,
    )
}
