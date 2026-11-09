//! A content mod's fluids (`data/<id>/horizon/fluid/`): the declaration and its own checks.
use serde::Deserialize;

/// The vanilla fluids a mod fluid copies: how it flows, what it does to
/// mobs and how it meets other fluids, as a content block copies a block.
pub const FLUID_TEMPLATES: [&str; 2] = ["minecraft:water", "minecraft:lava"];

/// A fluid the mod adds: NeoForge's `FluidType` and `BaseFlowingFluid.Properties`
/// over a copy of a [`FLUID_TEMPLATES`] fluid, drawn as its `FluidModel`. Like
/// vanilla's, it is two fluids: the source `<mod id>:<path>` and the flowing
/// [`Fluid::flowing_id`]. A number left out is the template's.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fluid {
    /// `<mod id>:<path>`, the source fluid.
    #[serde(skip)]
    pub id: String,
    pub template: String,
    /// `FluidType.Properties`.
    #[serde(default)]
    pub light_level: Option<u8>,
    #[serde(default)]
    pub density: Option<i32>,
    #[serde(default)]
    pub viscosity: Option<i32>,
    #[serde(default)]
    pub temperature: Option<i32>,
    #[serde(default)]
    pub motion_scale: Option<f64>,
    #[serde(default)]
    pub fall_distance_modifier: Option<f32>,
    #[serde(default)]
    pub can_push_entity: Option<bool>,
    #[serde(default)]
    pub can_swim: Option<bool>,
    #[serde(default)]
    pub can_drown: Option<bool>,
    #[serde(default)]
    pub can_extinguish: Option<bool>,
    #[serde(default)]
    pub can_convert_to_source: Option<bool>,
    #[serde(default)]
    pub supports_boating: Option<bool>,
    #[serde(default)]
    pub can_hydrate: Option<bool>,
    /// `BaseFlowingFluid.Properties`: ticks between spreads, how far it looks
    /// for a way down, and the levels it loses per block.
    #[serde(default)]
    pub tick_rate: Option<u32>,
    #[serde(default)]
    pub slope_find_distance: Option<u32>,
    #[serde(default)]
    pub level_decrease_per_block: Option<u32>,
    #[serde(default)]
    pub explosion_resistance: Option<f32>,
    /// The mod's item that is its bucket, and the mod's block that holds it in
    /// the world: neither is required.
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub block: Option<String>,
    /// `FluidModel`: the still, flowing and overlay sprites (texture ids in the
    /// block atlas, `<namespace>:block/...`), and an ARGB tint over them.
    pub still: String,
    pub flowing: String,
    #[serde(default)]
    pub overlay: Option<String>,
    #[serde(default)]
    pub tint: Option<u32>,
}

impl Fluid {
    /// The flowing fluid's id: `flowing_` before the path's last part, as
    /// vanilla's `minecraft:flowing_water` (`ruby:molten/flowing_iron`).
    pub fn flowing_id(&self) -> String {
        match self.id.rsplit_once('/') {
            Some((dir, last)) => format!("{dir}/flowing_{last}"),
            None => {
                let (ns, path) = self.id.split_once(':').unwrap_or(("", &self.id));
                format!("{ns}:flowing_{path}")
            }
        }
    }
}

/// A fluid's own fields; `bucket` and `block` are checked against the mod's
/// items and blocks by the caller.
pub(super) fn check_fluid(fluid: &Fluid) -> Result<(), String> {
    if !FLUID_TEMPLATES.contains(&fluid.template.as_str()) {
        return Err(format!(
            "template {:?} is not one of {FLUID_TEMPLATES:?}",
            fluid.template
        ));
    }
    if fluid.light_level.is_some_and(|l| l > 15) {
        return Err("light_level is 0 to 15".into());
    }
    if fluid.tick_rate == Some(0) {
        return Err("tick_rate is at least 1".into());
    }
    if fluid
        .slope_find_distance
        .is_some_and(|d| !(1..=8).contains(&d))
    {
        return Err("slope_find_distance is 1 to 8".into());
    }
    // A level runs 1 to 8: dropping 8 a block, a fluid could not leave its source.
    if fluid
        .level_decrease_per_block
        .is_some_and(|d| !(1..=7).contains(&d))
    {
        return Err("level_decrease_per_block is 1 to 7".into());
    }
    let non_negative = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && v >= 0.0);
    if !non_negative(fluid.motion_scale) {
        return Err("motion_scale is a number 0 or more".into());
    }
    if !non_negative(fluid.explosion_resistance.map(f64::from)) {
        return Err("explosion_resistance is a number 0 or more".into());
    }
    if fluid.fall_distance_modifier.is_some_and(|v| !v.is_finite()) {
        return Err("fall_distance_modifier is a number".into());
    }
    let id_like = |s: &str| {
        s.split_once(':')
            .is_some_and(|(n, p)| !n.is_empty() && !p.is_empty())
    };
    let textures = [
        ("still", Some(&fluid.still)),
        ("flowing", Some(&fluid.flowing)),
        ("overlay", fluid.overlay.as_ref()),
    ];
    for (key, texture) in textures {
        if texture.is_some_and(|t| !id_like(t)) {
            return Err(format!("{key} is a texture id (namespace:path)"));
        }
    }
    Ok(())
}
