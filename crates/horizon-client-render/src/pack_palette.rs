//! The colours a pack replaces where vanilla hardcodes them (Polytone's `colors.json`, read by the
//! platform's pack formats, [`crate::pack_hooks`]): dye, sheep and redstone colours, for drawing
//! only (the pack recolours collars, tropical fish, banners, wool, sign text and redstone wire and
//! dust, while gameplay keeps vanilla's `DyeColor`), mob effect colours, the experience number's,
//! entity shadows, the effect fogs' settings and durability bar colours. Lock-free: the draw paths
//! and the mesher's workers read the dye, sheep and redstone tables per face.
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, RwLock};

use crate::block_entity_render::DYES;
use crate::signs::DYE_TEXT;

/// The fog settings vanilla hardcodes, as the packs set them (vanilla's by default).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Environment {
    /// The fog colour inside powder snow (RGB).
    pub powder_snow_fog: Option<u32>,
    /// The fog ends of Blindness (5), Darkness (15) and bosses that add world fog (96).
    pub blindness_fog_distance: f32,
    pub darkness_fog_distance: f32,
    pub boss_fog_distance: f32,
    /// How much the underwater fog colour washes out (1 vanilla, 0 none).
    pub water_fog_brightening: f32,
    /// The ticks under water it takes (vanilla's 600).
    pub water_fog_brightening_time: f32,
    /// Below it the sky draws its dark disc (vanilla's 63; flat worlds keep their floor).
    pub horizon_height: f32,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            powder_snow_fog: None,
            blindness_fog_distance: 5.0,
            darkness_fog_distance: 15.0,
            boss_fog_distance: 96.0,
            water_fog_brightening: 1.0,
            water_fog_brightening_time: 600.0,
            horizon_height: 63.0,
        }
    }
}

/// The packs' colours; `None` (and the defaults) where they keep vanilla's.
#[derive(Clone, Debug, PartialEq)]
pub struct PackColors {
    /// By dye id (`DyeColor` order): `getTextureDiffuseColor`, `getTextColor`, the sheep's wool.
    pub dye_diffuse: [Option<u32>; 16],
    pub dye_text: [Option<u32>; 16],
    pub sheep: [Option<u32>; 16],
    /// `RedStoneWireBlock.COLORS` by power.
    pub redstone: [Option<u32>; 16],
    /// Mob effects' colours, by effect id without vanilla's namespace.
    pub effects: Vec<(String, u32)>,
    /// False hides every entity's round shadow, whatever the video option says.
    pub entity_shadows: bool,
    /// The experience level number's colour and outline.
    pub xp_bar: [Option<u32>; 2],
    /// The effect fogs' settings: the fog reads them every frame.
    pub env: Environment,
}

impl Default for PackColors {
    fn default() -> Self {
        Self {
            dye_diffuse: [None; 16],
            dye_text: [None; 16],
            sheep: [None; 16],
            redstone: [None; 16],
            effects: Vec::new(),
            entity_shadows: true,
            xp_bar: [None; 2],
            env: Environment::default(),
        }
    }
}

impl PackColors {
    /// Whether entities draw their round shadow, as far as the packs say.
    pub fn entity_shadows(&self) -> bool {
        self.entity_shadows
    }
}

/// 0 is unset, else `1 << 32 | rgb`.
static DIFFUSE: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static TEXT: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static SHEEP: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static REDSTONE: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];

/// The colours of the pack overlay at `root`, from the installed pack formats; their dye, sheep
/// and redstone colours go to the draw paths' tables ([`set`]).
pub fn read(root: &Path) -> PackColors {
    let formats = crate::pack_hooks::formats();
    let colors = formats.colors(root).unwrap_or_default();
    set(&colors);
    *ITEM_BARS.write().unwrap_or_else(|e| e.into_inner()) = formats.item_bars(root);
    colors
}

/// The packs' durability bar colours by item (Polytone's item modifiers' `bar_color`).
pub trait ItemBars: Send + Sync {
    /// The bar colour (RGB) of `item` at this damage (0 to 1), when a pack sets one.
    fn color(&self, item: &str, damage: f32) -> Option<u32>;
}

static ITEM_BARS: RwLock<Option<Arc<dyn ItemBars>>> = RwLock::new(None);

/// A pack's durability bar colour (RGB) for an item at this damage (0 to 1).
pub fn item_bar_color(item: &str, damage: f32) -> Option<u32> {
    let bars = ITEM_BARS.read().unwrap_or_else(|e| e.into_inner());
    bars.as_ref()?.color(item, damage)
}

/// Takes the colours of the packs just loaded (vanilla's where they set none).
pub fn set(colors: &PackColors) {
    let store = |table: &[AtomicU64; 16], i: usize, color: Option<u32>| {
        table[i].store(
            color.map_or(0, |c| 1 << 32 | u64::from(c & 0xFF_FFFF)),
            Relaxed,
        );
    };
    for i in 0..16 {
        store(&DIFFUSE, i, colors.dye_diffuse[i]);
        store(&TEXT, i, colors.dye_text[i]);
        store(&SHEEP, i, colors.sheep[i]);
        store(&REDSTONE, i, colors.redstone[i]);
    }
}

fn get(table: &[AtomicU64; 16], i: usize) -> Option<u32> {
    let v = table[i & 15].load(Relaxed);
    (v != 0).then_some(v as u32)
}

/// `DyeColor.getTextureDiffuseColor` (RGB) as the packs draw it.
pub fn diffuse(dye: usize) -> u32 {
    get(&DIFFUSE, dye).unwrap_or(DYES[dye & 15].1)
}

/// `DyeColor.getTextColor` (RGB) as the packs draw it.
pub fn text(dye: usize) -> u32 {
    get(&TEXT, dye).unwrap_or(DYE_TEXT[dye & 15].1)
}

/// A pack's `RedStoneWireBlock.COLORS` entry for this power (RGB), when it sets one.
pub fn redstone(power: u8) -> Option<u32> {
    get(&REDSTONE, usize::from(power))
}

/// `ColorLerper.Type.SHEEP` wool colour: a pack's `sheep.<name>`, else white 0xE6E6E6 and the
/// others 75% of vanilla's dye (a pack's `dye.*.diffuse` doesn't reach wool).
pub fn sheep_wool(dye: u8) -> u32 {
    let i = usize::from(dye & 15);
    if let Some(color) = get(&SHEEP, i) {
        return color;
    }
    if i == 0 {
        return 0xE6E6E6;
    }
    let src = DYES[i].1;
    let f = |c: u32| ((c as f32 * 0.75).floor() as u32).min(255);
    f(src >> 16 & 255) << 16 | f(src >> 8 & 255) << 8 | f(src & 255)
}
