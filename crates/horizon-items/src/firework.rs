//! Firework explosions (the `firework_explosion` component and its shapes) and
//! the dye colours fireworks use.

/// FireworkExplosion.Shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FireworkShape {
    #[default]
    SmallBall,
    LargeBall,
    Star,
    Creeper,
    Burst,
}
impl FireworkShape {
    pub fn from_name(name: &str) -> Self {
        match name {
            "large_ball" => Self::LargeBall,
            "star" => Self::Star,
            "creeper" => Self::Creeper,
            "burst" => Self::Burst,
            _ => Self::SmallBall,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::SmallBall => "small_ball",
            Self::LargeBall => "large_ball",
            Self::Star => "star",
            Self::Creeper => "creeper",
            Self::Burst => "burst",
        }
    }
}
/// `DyeColor` names and `getFireworkColor`, by dye id (white to black).
pub const DYE_FIREWORK_COLORS: [(&str, u32); 16] = [
    ("white", 0xF0F0F0),
    ("orange", 0xEB8844),
    ("magenta", 0xC354CD),
    ("light_blue", 0x6689D3),
    ("yellow", 0xDECF2A),
    ("lime", 0x41CD34),
    ("pink", 0xD88198),
    ("gray", 0x434343),
    ("light_gray", 0xABABAB),
    ("cyan", 0x287697),
    ("purple", 0x7B2FBE),
    ("blue", 0x253192),
    ("brown", 0x51301A),
    ("green", 0x3B511A),
    ("red", 0xB3312C),
    ("black", 0x1E1B1B),
];
/// The `firework_explosion` component.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FireworkExplosion {
    pub shape: FireworkShape,
    pub colors: Vec<u32>,
    pub fade: Vec<u32>,
    pub trail: bool,
    pub twinkle: bool,
}
