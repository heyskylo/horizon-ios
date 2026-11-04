//! PvP and HUD options (`docs/pvp.md`): a custom crosshair, block outline, hit colour and HUD
//! widgets (`pvp_hud`). Kept in
//! `horizon-extras.txt` with the other extras ([`crate::extras::Extras::pvp`]) and shown on
//! Horizon Settings > PvP & HUD. Every option is off by default, so the game looks like
//! vanilla until the player changes one, and none of them shows anything vanilla doesn't
//! already tell the player.

/// Colours the PvP options pick from. `Inverted` is vanilla's crosshair blend: the colour
/// under it, inverted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colour {
    Inverted,
    White,
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Magenta,
    Black,
}
impl Colour {
    pub const ALL: [Colour; 10] = [
        Colour::Inverted,
        Colour::White,
        Colour::Red,
        Colour::Orange,
        Colour::Yellow,
        Colour::Green,
        Colour::Cyan,
        Colour::Blue,
        Colour::Magenta,
        Colour::Black,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Colour::Inverted => "inverted",
            Colour::White => "white",
            Colour::Red => "red",
            Colour::Orange => "orange",
            Colour::Yellow => "yellow",
            Colour::Green => "green",
            Colour::Cyan => "cyan",
            Colour::Blue => "blue",
            Colour::Magenta => "magenta",
            Colour::Black => "black",
        }
    }
    fn english(self) -> &'static str {
        match self {
            Colour::Inverted => "Inverted",
            Colour::White => "White",
            Colour::Red => "Red",
            Colour::Orange => "Orange",
            Colour::Yellow => "Yellow",
            Colour::Green => "Green",
            Colour::Cyan => "Cyan",
            Colour::Blue => "Blue",
            Colour::Magenta => "Magenta",
            Colour::Black => "Black",
        }
    }
    /// The name in the current language.
    pub fn label(self) -> String {
        crate::lang::get_or(
            &format!("horizon.pvp.colour.{}", self.name()),
            self.english(),
        )
    }
    /// Opaque ARGB; white for `Inverted` (the blend makes it the inverse).
    pub fn argb(self) -> u32 {
        match self {
            Colour::Inverted | Colour::White => 0xFFFF_FFFF,
            Colour::Red => 0xFFFF_0000,
            Colour::Orange => 0xFFFF_A020,
            Colour::Yellow => 0xFFFF_FF40,
            Colour::Green => 0xFF40_FF40,
            Colour::Cyan => 0xFF40_FFFF,
            Colour::Blue => 0xFF40_80FF,
            Colour::Magenta => 0xFFFF_40FF,
            Colour::Black => 0xFF00_0000,
        }
    }
    fn parse(s: &str) -> Option<Colour> {
        Self::ALL.into_iter().find(|c| c.name() == s)
    }
    fn step(self, back: bool) -> Colour {
        step(&Self::ALL, self, back)
    }
    /// The next solid colour (`Inverted` left out).
    fn step_solid(self, back: bool) -> Colour {
        step(&Self::ALL[1..], self, back)
    }
    /// The colour at `percent` opacity, as ARGB.
    pub fn with_opacity(self, percent: u8) -> u32 {
        self.argb() & 0x00FF_FFFF | alpha(percent) << 24
    }
}
/// `percent` of 255, rounded.
fn alpha(percent: u8) -> u32 {
    (u32::from(percent.min(100)) * 255 + 50) / 100
}

/// The next (or previous) value of `all` after `v`, wrapping.
fn step<T: Copy + PartialEq>(all: &[T], v: T, back: bool) -> T {
    let i = all.iter().position(|&x| x == v).unwrap_or(0);
    let n = all.len();
    all[if back { (i + n - 1) % n } else { (i + 1) % n }]
}
/// `v` one step up or down within `lo..=hi`, wrapping.
fn step_num(v: u8, lo: u8, hi: u8, back: bool) -> u8 {
    step_by(v, (lo, hi), 1, back)
}
/// `v` `by` up or down within `lo..=hi`, wrapping.
fn step_by(v: u8, (lo, hi): (u8, u8), by: u8, back: bool) -> u8 {
    match (back, v) {
        (false, v) if v >= hi => lo,
        (false, v) => (v + by).min(hi),
        (true, v) if v <= lo => hi,
        (true, v) => v.saturating_sub(by).max(lo),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Four arms, as vanilla's.
    Cross,
    /// No arm above.
    T,
    /// Four diagonal arms.
    X,
    Circle,
    Square,
    /// Only the dot.
    Dot,
}
impl Shape {
    pub const ALL: [Shape; 6] = [
        Shape::Cross,
        Shape::T,
        Shape::X,
        Shape::Circle,
        Shape::Square,
        Shape::Dot,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Shape::Cross => "cross",
            Shape::T => "t",
            Shape::X => "x",
            Shape::Circle => "circle",
            Shape::Square => "square",
            Shape::Dot => "dot",
        }
    }
    fn english(self) -> &'static str {
        match self {
            Shape::Cross => "Cross",
            Shape::T => "T",
            Shape::X => "X",
            Shape::Circle => "Circle",
            Shape::Square => "Square",
            Shape::Dot => "Dot",
        }
    }
    pub fn label(self) -> String {
        crate::lang::get_or(
            &format!("horizon.pvp.shape.{}", self.name()),
            self.english(),
        )
    }
    fn parse(s: &str) -> Option<Shape> {
        Self::ALL.into_iter().find(|c| c.name() == s)
    }
}

/// A time of day this player's view can stay at (`ViewTime::day_time` in vanilla's ticks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewTime {
    Morning,
    Noon,
    Sunset,
    Midnight,
}
impl ViewTime {
    pub const ALL: [ViewTime; 4] = [
        ViewTime::Morning,
        ViewTime::Noon,
        ViewTime::Sunset,
        ViewTime::Midnight,
    ];
    pub fn name(self) -> &'static str {
        match self {
            ViewTime::Morning => "morning",
            ViewTime::Noon => "noon",
            ViewTime::Sunset => "sunset",
            ViewTime::Midnight => "midnight",
        }
    }
    fn english(self) -> &'static str {
        match self {
            ViewTime::Morning => "Morning",
            ViewTime::Noon => "Noon",
            ViewTime::Sunset => "Sunset",
            ViewTime::Midnight => "Midnight",
        }
    }
    /// The time of day it shows, as `/time set` takes it.
    pub fn day_time(self) -> i64 {
        match self {
            ViewTime::Morning => 1000,
            ViewTime::Noon => 6000,
            ViewTime::Sunset => 12500,
            ViewTime::Midnight => 18000,
        }
    }
    pub fn label(self) -> String {
        crate::lang::get_or(&format!("horizon.pvp.time.{}", self.name()), self.english())
    }
    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == s)
    }
}
/// The weather this player's view can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewWeather {
    Clear,
    Rain,
    Thunder,
}
impl ViewWeather {
    pub const ALL: [ViewWeather; 3] = [ViewWeather::Clear, ViewWeather::Rain, ViewWeather::Thunder];
    pub fn name(self) -> &'static str {
        match self {
            ViewWeather::Clear => "clear",
            ViewWeather::Rain => "rain",
            ViewWeather::Thunder => "thunder",
        }
    }
    fn english(self) -> &'static str {
        match self {
            ViewWeather::Clear => "Clear",
            ViewWeather::Rain => "Rain",
            ViewWeather::Thunder => "Thunder",
        }
    }
    /// `Level.getRainLevel` and `getThunderLevel` as `/weather` leaves them.
    pub fn levels(self) -> (f32, f32) {
        match self {
            ViewWeather::Clear => (0.0, 0.0),
            ViewWeather::Rain => (1.0, 0.0),
            ViewWeather::Thunder => (1.0, 1.0),
        }
    }
    pub fn label(self) -> String {
        crate::lang::get_or(
            &format!("horizon.pvp.weather.{}", self.name()),
            self.english(),
        )
    }
    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == s)
    }
}
/// The day time this player sees with `fixed` set: the same day (the moon's phase follows the
/// world's), at that time.
pub fn view_day_time(world: i64, fixed: Option<ViewTime>) -> i64 {
    match fixed {
        Some(t) => world.div_euclid(24000) * 24000 + t.day_time(),
        None => world,
    }
}

/// The crosshair's look. Its defaults draw vanilla's: a 15-pixel cross, one pixel thick,
/// inverting what is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crosshair {
    /// Off: vanilla's crosshair sprite (a resource pack's included).
    pub custom: bool,
    pub shape: Shape,
    /// Arm length in GUI pixels (the circle's and square's radius past the gap).
    pub size: u8,
    pub gap: u8,
    pub thickness: u8,
    pub colour: Colour,
    /// A one-pixel black rim around every part.
    pub outline: bool,
    /// A dot in the middle.
    pub dot: bool,
    /// The colour while the crosshair is over a living entity; `None` keeps `colour`.
    pub hover: Option<Colour>,
}
pub const SIZES: (u8, u8) = (1, 12);
pub const GAPS: (u8, u8) = (0, 8);
pub const THICKNESSES: (u8, u8) = (1, 4);
impl Default for Crosshair {
    fn default() -> Self {
        Self {
            custom: false,
            shape: Shape::Cross,
            size: 7,
            gap: 0,
            thickness: 1,
            colour: Colour::Inverted,
            outline: false,
            dot: false,
            hover: None,
        }
    }
}

/// The crosshair's pixels on a 64x64 grid centred on bit 32 of row 32 (the middle pixel of
/// vanilla's 15x15 sprite): bit `x` of row `y` is the pixel `x - 32`, `y - 32` from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mask(pub [u64; 64]);
pub const MASK_CENTRE: i32 = 32;
impl Mask {
    fn set(&mut self, x: i32, y: i32) {
        let (gx, gy) = (x + MASK_CENTRE, y + MASK_CENTRE);
        if (0..64).contains(&gx) && (0..64).contains(&gy) {
            self.0[gy as usize] |= 1 << gx;
        }
    }
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y);
            }
        }
    }
    pub fn get(&self, x: i32, y: i32) -> bool {
        let (gx, gy) = (x + MASK_CENTRE, y + MASK_CENTRE);
        (0..64).contains(&gx) && (0..64).contains(&gy) && self.0[gy as usize] >> gx & 1 == 1
    }
    /// The pixels around this mask's (eight neighbours) that are not in it.
    pub fn rim(&self) -> Mask {
        let mut grown = [0u64; 64];
        for (y, &row) in self.0.iter().enumerate() {
            let spread = row | row << 1 | row >> 1;
            for dy in [y.wrapping_sub(1), y, y + 1] {
                if let Some(g) = grown.get_mut(dy) {
                    *g |= spread;
                }
            }
        }
        for (g, &row) in grown.iter_mut().zip(&self.0) {
            *g &= !row;
        }
        Mask(grown)
    }
    /// Each row's runs of set pixels as (x0, y, x1) offsets from the centre, `x1` exclusive.
    pub fn runs(&self) -> impl Iterator<Item = (i32, i32, i32)> + '_ {
        self.0.iter().enumerate().flat_map(|(y, &row)| {
            let mut bits = row;
            std::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let start = bits.trailing_zeros();
                let len = (bits >> start).trailing_ones();
                bits &= !((((1u128 << len) - 1) as u64) << start);
                let y = y as i32 - MASK_CENTRE;
                let x0 = start as i32 - MASK_CENTRE;
                Some((x0, y, x0 + len as i32))
            })
        })
    }
}
impl Crosshair {
    /// The pixels of the shape and its dot.
    pub fn mask(&self) -> Mask {
        let mut m = Mask([0; 64]);
        let t = i32::from(self.thickness.clamp(THICKNESSES.0, THICKNESSES.1));
        // A thickness band centred on the middle pixel (the extra pixel of an even one
        // below and right).
        let (lo, hi) = (-(t - 1) / 2, -(t - 1) / 2 + t - 1);
        let size = i32::from(self.size.clamp(SIZES.0, SIZES.1));
        let gap = i32::from(self.gap.min(GAPS.1));
        let (near, far) = (hi + 1 + gap, hi + gap + size);
        let (near_neg, far_neg) = (lo - 1 - gap, lo - gap - size);
        match self.shape {
            Shape::Cross | Shape::T => {
                m.rect(near, lo, far, hi);
                m.rect(far_neg, lo, near_neg, hi);
                m.rect(lo, near, hi, far);
                if self.shape == Shape::Cross {
                    m.rect(lo, far_neg, hi, near_neg);
                }
                if gap == 0 {
                    m.rect(lo, lo, hi, hi);
                }
            }
            Shape::X => {
                for d in gap + 1..=gap + size {
                    for (sx, sy) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
                        let (x, y) = (sx * d, sy * d);
                        m.rect(x + lo, y + lo, x + hi, y + hi);
                    }
                }
                if gap == 0 {
                    m.rect(lo, lo, hi, hi);
                }
            }
            Shape::Circle | Shape::Square => {
                let outer = gap + size;
                let inner = outer - t;
                for y in -outer..=outer {
                    for x in -outer..=outer {
                        let inside = |r: i32| match self.shape {
                            Shape::Circle => {
                                let d = ((x * x + y * y) as f32).sqrt();
                                d <= r as f32 + 0.5
                            }
                            _ => x.abs().max(y.abs()) <= r,
                        };
                        if inside(outer) && !inside(inner) {
                            m.set(x, y);
                        }
                    }
                }
            }
            Shape::Dot => {}
        }
        if self.dot || self.shape == Shape::Dot {
            m.rect(lo, lo, hi, hi);
        }
        m
    }
}

/// The crosshair's draw list, cached: rebuilt only when its settings change.
#[derive(Default)]
pub struct CrosshairCache {
    key: Option<Crosshair>,
    shape: Vec<(i32, i32, i32)>,
    rim: Vec<(i32, i32, i32)>,
}
impl CrosshairCache {
    /// The shape's runs and its outline's (empty without one), as [`Mask::runs`].
    pub fn runs(&mut self, c: &Crosshair) -> (&[(i32, i32, i32)], &[(i32, i32, i32)]) {
        if self.key != Some(*c) {
            let mask = c.mask();
            self.shape.clear();
            self.shape.extend(mask.runs());
            self.rim.clear();
            if c.outline {
                self.rim.extend(mask.rim().runs());
            }
            self.key = Some(*c);
        }
        (&self.shape, &self.rim)
    }
}

/// The block outline (`LevelRenderer.submitBlockOutline`). Off, vanilla's: black at 40%, or the
/// accessibility option's high contrast one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outline {
    pub custom: bool,
    /// A solid colour, at `opacity` percent.
    pub colour: Colour,
    pub opacity: u8,
    /// Times vanilla's width (`Window.getAppropriateLineWidth`).
    pub thickness: u8,
    /// The block's faces tinted with this colour, lightly.
    pub fill: Option<Colour>,
}
pub const OPACITIES: (u8, u8) = (10, 100);
pub const OUTLINE_THICKNESSES: (u8, u8) = (1, 4);
/// How much the fill covers the block's faces, in percent.
pub const FILL_OPACITY: u8 = 25;
impl Default for Outline {
    fn default() -> Self {
        Self {
            custom: false,
            colour: Colour::Black,
            opacity: 40,
            thickness: 1,
            fill: None,
        }
    }
}
impl Outline {
    /// The stroke's ARGB and its width as a multiple of vanilla's; none for vanilla's.
    pub fn stroke(&self) -> Option<(u32, f32)> {
        self.custom.then(|| {
            (
                self.colour.with_opacity(self.opacity),
                f32::from(self.thickness),
            )
        })
    }
    /// The fill's ARGB, when there is one.
    pub fn fill(&self) -> Option<u32> {
        self.fill
            .filter(|_| self.custom)
            .map(|c| c.with_opacity(FILL_OPACITY))
    }
}
/// The tint a hurt entity flashes (`OverlayTexture`'s red rows). Off, vanilla's: red over 30%.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub custom: bool,
    pub colour: Colour,
    /// How much of the colour covers the entity, in percent.
    pub opacity: u8,
}
pub const HIT_OPACITIES: (u8, u8) = (0, 100);
impl Default for Hit {
    fn default() -> Self {
        Self {
            custom: false,
            colour: Colour::Red,
            opacity: 30,
        }
    }
}
impl Hit {
    /// The overlay texel as RGBA bytes in a little-endian u32 (WGSL's `unpack4x8unorm`), its alpha
    /// what of the entity's own colour stays (`entity.fsh` mixes by it); 0 for vanilla's texel.
    pub fn texel(&self) -> u32 {
        if !self.custom {
            return 0;
        }
        let [_, r, g, b] = self.colour.argb().to_be_bytes();
        u32::from_le_bytes([r, g, b, (255 - alpha(self.opacity)) as u8])
    }
}

/// What the renderer reads of these options each frame (`FrameInputs::pvp`); the default is
/// vanilla's look.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    /// The block outline's stroke ([`Outline::stroke`]).
    pub outline: Option<(u32, f32)>,
    /// The hurt overlay texel ([`Hit::texel`]), 0 for vanilla's.
    pub hurt_texel: u32,
    /// The glint's colour ([`Tweaks::glint_texel`]), 0 for vanilla's.
    pub glint: u32,
}
impl Pvp {
    pub fn frame(&self) -> Frame {
        Frame {
            outline: self.outline.stroke(),
            hurt_texel: self.hit.texel(),
            glint: self.tweaks.glint_texel(),
        }
    }
}

/// The HUD widgets (`pvp_hud`), each in a corner of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hud {
    /// The armour worn, head first, with each piece's durability.
    pub armour: bool,
    pub armour_corner: Corner,
    pub armour_percent: bool,
    /// The active effects: icon, name and level, time left. They replace vanilla's icons.
    pub effects: bool,
    pub effects_corner: Corner,
    /// How many of the held item the inventory holds (arrows with a bow or crossbow).
    pub held: bool,
    pub held_corner: Corner,
    /// The latency the server reports for this player (the tab list's bars), in multiplayer.
    pub ping: bool,
    pub ping_corner: Corner,
    /// The block position and the direction faced, as F3 shows them; hidden when F3 is
    /// reduced (`reducedDebugInfo`).
    pub coords: bool,
    pub coords_corner: Corner,
    /// The server's address, in multiplayer.
    pub server: bool,
    pub server_corner: Corner,
    /// Hits in a row the server confirmed, until this player is hurt or 3 seconds pass.
    pub combo: bool,
    pub combo_corner: Corner,
    /// A dark box behind each widget.
    pub background: bool,
}
impl Default for Hud {
    fn default() -> Self {
        Self {
            armour: false,
            armour_corner: Corner::BottomRight,
            armour_percent: false,
            effects: false,
            effects_corner: Corner::TopRight,
            held: false,
            held_corner: Corner::BottomRight,
            ping: false,
            ping_corner: Corner::TopLeft,
            coords: false,
            coords_corner: Corner::TopLeft,
            server: false,
            server_corner: Corner::TopLeft,
            combo: false,
            combo_corner: Corner::TopLeft,
            background: true,
        }
    }
}
pub use crate::extras::Corner;
/// The value of the view options while they follow the world.
fn world_label() -> String {
    crate::lang::get_or("horizon.pvp.view.world", "World's")
}
fn corner_label(c: Corner) -> String {
    crate::lang::get_or(&format!("horizon.pvp.corner.{}", c.name()), c.label())
}

/// Smaller changes to what vanilla shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tweaks {
    /// Enchantment levels as numbers in tooltips ("Sharpness 5").
    pub numeric_enchants: bool,
    /// How far down the first-person fire overlay moves, in percent of its height (0: vanilla).
    pub fire_lower: u8,
    /// The first-person shield's size, in percent (100: vanilla).
    pub shield_size: u8,
    /// The totem animation's size, in percent (100: vanilla).
    pub totem_size: u8,
    /// The enchantment glint's colour; `None` is vanilla's purple.
    pub glint: Option<Colour>,
    /// Water fog as after 30 seconds under water (vanilla's clearest), from the first moment.
    pub clear_water: bool,
    /// Lava fog as with Fire Resistance (vanilla's clearest), without the effect.
    pub clear_lava: bool,
    /// The time of day this player sees, `None` for the world's. Never sent to the server.
    pub time: Option<ViewTime>,
    /// The weather this player sees, `None` for the world's. Never sent to the server.
    pub weather: Option<ViewWeather>,
}
impl Default for Tweaks {
    fn default() -> Self {
        Self {
            numeric_enchants: false,
            fire_lower: 0,
            shield_size: 100,
            totem_size: 100,
            glint: None,
            clear_water: false,
            clear_lava: false,
            time: None,
            weather: None,
        }
    }
}
pub const FIRE_LOWER: (u8, u8) = (0, 80);
pub const SHIELD_SIZES: (u8, u8) = (50, 100);
pub const TOTEM_SIZES: (u8, u8) = (25, 100);
impl Tweaks {
    /// `colour` as the glint shaders read it: `0` for vanilla's, else `0xFF` alpha over RGB in
    /// little-endian bytes (`unpack4x8unorm`'s order).
    pub fn glint_texel(&self) -> u32 {
        self.glint.map_or(0, |c| {
            let [_, r, g, b] = c.argb().to_be_bytes();
            u32::from_le_bytes([r, g, b, 255])
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pvp {
    pub crosshair: Crosshair,
    pub outline: Outline,
    pub hit: Hit,
    pub hud: Hud,
    pub tweaks: Tweaks,
}
fn flag(value: &str, d: bool) -> bool {
    match value {
        "true" => true,
        "false" => false,
        _ => d,
    }
}
fn num(value: &str, (lo, hi): (u8, u8), d: u8) -> u8 {
    value.parse::<u8>().map_or(d, |v| v.clamp(lo, hi))
}
impl Pvp {
    /// One `key:value` line of `horizon-extras.txt`; false when the key isn't a PvP one.
    pub fn parse_line(&mut self, key: &str, value: &str) -> bool {
        let c = &mut self.crosshair;
        let (o, h, u) = (&mut self.outline, &mut self.hit, &mut self.hud);
        let t = &mut self.tweaks;
        match key {
            "crosshair" => c.custom = flag(value, c.custom),
            "crosshairShape" => c.shape = Shape::parse(value).unwrap_or(c.shape),
            "crosshairSize" => c.size = num(value, SIZES, c.size),
            "crosshairGap" => c.gap = num(value, GAPS, c.gap),
            "crosshairThickness" => c.thickness = num(value, THICKNESSES, c.thickness),
            "crosshairColour" => c.colour = Colour::parse(value).unwrap_or(c.colour),
            "crosshairOutline" => c.outline = flag(value, c.outline),
            "crosshairDot" => c.dot = flag(value, c.dot),
            "crosshairHover" => {
                c.hover = match value {
                    "off" => None,
                    v => Colour::parse(v).or(c.hover),
                }
            }
            "outline" => o.custom = flag(value, o.custom),
            "outlineColour" => o.colour = Colour::parse(value).unwrap_or(o.colour),
            "outlineOpacity" => o.opacity = num(value, OPACITIES, o.opacity),
            "outlineThickness" => o.thickness = num(value, OUTLINE_THICKNESSES, o.thickness),
            "outlineFill" => {
                o.fill = match value {
                    "off" => None,
                    v => Colour::parse(v).or(o.fill),
                }
            }
            "hit" => h.custom = flag(value, h.custom),
            "hitColour" => h.colour = Colour::parse(value).unwrap_or(h.colour),
            "hitOpacity" => h.opacity = num(value, HIT_OPACITIES, h.opacity),
            "hudArmour" => u.armour = flag(value, u.armour),
            "hudArmourCorner" => u.armour_corner = Corner::parse(value, u.armour_corner),
            "hudArmourPercent" => u.armour_percent = flag(value, u.armour_percent),
            "hudEffects" => u.effects = flag(value, u.effects),
            "hudEffectsCorner" => u.effects_corner = Corner::parse(value, u.effects_corner),
            "hudHeld" => u.held = flag(value, u.held),
            "hudHeldCorner" => u.held_corner = Corner::parse(value, u.held_corner),
            "hudBackground" => u.background = flag(value, u.background),
            "hudPing" => u.ping = flag(value, u.ping),
            "hudPingCorner" => u.ping_corner = Corner::parse(value, u.ping_corner),
            "hudCoords" => u.coords = flag(value, u.coords),
            "hudCoordsCorner" => u.coords_corner = Corner::parse(value, u.coords_corner),
            "hudServer" => u.server = flag(value, u.server),
            "hudServerCorner" => u.server_corner = Corner::parse(value, u.server_corner),
            "hudCombo" => u.combo = flag(value, u.combo),
            "hudComboCorner" => u.combo_corner = Corner::parse(value, u.combo_corner),
            "numericEnchants" => t.numeric_enchants = flag(value, t.numeric_enchants),
            "lowFire" => t.fire_lower = num(value, FIRE_LOWER, t.fire_lower),
            "shieldSize" => t.shield_size = num(value, SHIELD_SIZES, t.shield_size),
            "totemSize" => t.totem_size = num(value, TOTEM_SIZES, t.totem_size),
            "glintColour" => {
                t.glint = match value {
                    "off" => None,
                    v => Colour::parse(v).or(t.glint),
                }
            }
            "clearWater" => t.clear_water = flag(value, t.clear_water),
            "clearLava" => t.clear_lava = flag(value, t.clear_lava),
            "viewTime" => {
                t.time = match value {
                    "off" => None,
                    v => ViewTime::parse(v).or(t.time),
                }
            }
            "viewWeather" => {
                t.weather = match value {
                    "off" => None,
                    v => ViewWeather::parse(v).or(t.weather),
                }
            }
            _ => return false,
        }
        true
    }
    /// The PvP lines of `horizon-extras.txt`.
    pub fn serialize(&self, out: &mut String) {
        use std::fmt::Write;
        let c = &self.crosshair;
        let _ = write!(
            out,
            "crosshair:{}\ncrosshairShape:{}\ncrosshairSize:{}\ncrosshairGap:{}\ncrosshairThickness:{}\ncrosshairColour:{}\ncrosshairOutline:{}\ncrosshairDot:{}\ncrosshairHover:{}\n",
            c.custom,
            c.shape.name(),
            c.size,
            c.gap,
            c.thickness,
            c.colour.name(),
            c.outline,
            c.dot,
            c.hover.map_or("off", Colour::name),
        );
        let (o, h) = (&self.outline, &self.hit);
        let _ = write!(
            out,
            "outline:{}\noutlineColour:{}\noutlineOpacity:{}\noutlineThickness:{}\noutlineFill:{}\nhit:{}\nhitColour:{}\nhitOpacity:{}\n",
            o.custom,
            o.colour.name(),
            o.opacity,
            o.thickness,
            o.fill.map_or("off", Colour::name),
            h.custom,
            h.colour.name(),
            h.opacity,
        );
        let u = &self.hud;
        let _ = write!(
            out,
            "hudArmour:{}\nhudArmourCorner:{}\nhudArmourPercent:{}\nhudEffects:{}\nhudEffectsCorner:{}\nhudHeld:{}\nhudHeldCorner:{}\nhudBackground:{}\n",
            u.armour,
            u.armour_corner.name(),
            u.armour_percent,
            u.effects,
            u.effects_corner.name(),
            u.held,
            u.held_corner.name(),
            u.background,
        );
        let _ = write!(
            out,
            "hudPing:{}\nhudPingCorner:{}\nhudCoords:{}\nhudCoordsCorner:{}\nhudServer:{}\nhudServerCorner:{}\nhudCombo:{}\nhudComboCorner:{}\n",
            u.ping,
            u.ping_corner.name(),
            u.coords,
            u.coords_corner.name(),
            u.server,
            u.server_corner.name(),
            u.combo,
            u.combo_corner.name(),
        );
        let t = &self.tweaks;
        let _ = write!(
            out,
            "numericEnchants:{}\nlowFire:{}\nshieldSize:{}\ntotemSize:{}\nglintColour:{}\nclearWater:{}\nclearLava:{}\n",
            t.numeric_enchants,
            t.fire_lower,
            t.shield_size,
            t.totem_size,
            t.glint.map_or("off", Colour::name),
            t.clear_water,
            t.clear_lava,
        );
        let _ = write!(
            out,
            "viewTime:{}\nviewWeather:{}\n",
            t.time.map_or("off", ViewTime::name),
            t.weather.map_or("off", ViewWeather::name),
        );
    }
}

/// The options of the PvP & HUD page: each one's id (its language keys are `horizon.pvp.<id>`
/// and `horizon.pvp.<id>.tooltip`), English caption and description (en_us.json has the same
/// text; a test keeps them equal).
macro_rules! opts {
    ($($name:ident, $id:literal, $caption:literal, $description:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Opt { $($name),* }
        impl Opt {
            pub const ALL: &'static [Opt] = &[$(Opt::$name),*];
            pub fn caption_key(self) -> &'static str {
                match self { $(Opt::$name => concat!("horizon.pvp", ".", $id)),* }
            }
            pub fn description_key(self) -> &'static str {
                match self { $(Opt::$name => concat!("horizon.pvp", ".", $id, ".tooltip")),* }
            }
            pub fn caption(self) -> &'static str {
                match self { $(Opt::$name => $caption),* }
            }
            pub fn description(self) -> &'static str {
                match self { $(Opt::$name => $description),* }
            }
        }
    };
}
opts! {
    Crosshair, "crosshair", "Custom crosshair",
        "Draws your own crosshair instead of vanilla's. Its first settings look like vanilla's.";
    CrosshairShape, "crosshair_shape", "Shape", "A cross, a T, an X, a circle, a square or only a dot.";
    CrosshairSize, "crosshair_size", "Size", "The length of each arm, in GUI pixels.";
    CrosshairGap, "crosshair_gap", "Gap", "The empty space in the middle.";
    CrosshairThickness, "crosshair_thickness", "Thickness", "How wide each line is.";
    CrosshairColour, "crosshair_colour", "Colour",
        "Inverted shows the colour under it inverted, as vanilla's crosshair does.";
    CrosshairOutline, "crosshair_outline", "Outline", "A thin black edge, so it shows on any background.";
    CrosshairDot, "crosshair_dot", "Centre dot", "A dot in the middle of the crosshair.";
    CrosshairHover, "crosshair_hover", "Colour on entities",
        "Another colour while the crosshair is over a mob or a player you could hit, as vanilla's attack indicator knows it.";
    Outline, "outline", "Custom block outline", "Changes the outline around the block you look at.";
    OutlineColour, "outline_colour", "Colour", "The outline's colour. Black at 40% is vanilla's.";
    OutlineOpacity, "outline_opacity", "Opacity", "How solid the outline is.";
    OutlineThickness, "outline_thickness", "Thickness", "The outline's width, in times vanilla's.";
    OutlineFill, "outline_fill", "Fill", "Tints the faces of the block you look at, lightly.";
    Hit, "hit", "Custom hit colour",
        "Changes the colour mobs and players flash when they are hurt. Red at 30% is vanilla's.";
    HitColour, "hit_colour", "Colour", "The colour hurt mobs and players flash.";
    HitOpacity, "hit_opacity", "Opacity", "How much of the colour covers them.";
    HudArmour, "hud_armour", "Armour",
        "The armour you wear, head first, with how many uses each piece has left.";
    HudArmourCorner, "hud_armour_corner", "Armour corner", "Where the armour shows.";
    HudArmourPercent, "hud_armour_percent", "Durability in %",
        "Uses left as a percent of the piece's maximum instead of a number.";
    HudEffects, "hud_effects", "Effects",
        "Your effects with their names, levels and time left, in place of the icons at the top right.";
    HudEffectsCorner, "hud_effects_corner", "Effects corner", "Where the effects show.";
    HudHeld, "hud_held", "Held item count",
        "How many of the item in your hand your inventory holds; arrows with a bow or crossbow.";
    HudHeldCorner, "hud_held_corner", "Held count corner", "Where the count shows.";
    HudPing, "hud_ping", "Ping", "The latency the server reports for you, in milliseconds. Multiplayer only.";
    HudPingCorner, "hud_ping_corner", "Ping corner", "Where the ping shows.";
    HudCoords, "hud_coords", "Coordinates",
        "Your block position and the direction you face, as F3 shows them. Hidden when the world or server reduces F3.";
    HudCoordsCorner, "hud_coords_corner", "Coordinates corner", "Where the coordinates show.";
    HudServer, "hud_server", "Server address", "The address of the server you play on. Multiplayer only.";
    HudServerCorner, "hud_server_corner", "Server corner", "Where the address shows.";
    HudCombo, "hud_combo", "Combo counter",
        "Hits in a row the server confirmed, until you are hurt or 3 seconds pass without one.";
    HudComboCorner, "hud_combo_corner", "Combo corner", "Where the combo shows.";
    HudBackground, "hud_background", "Widget backgrounds", "A dark box behind each widget.";
    NumericEnchants, "numeric_enchants", "Enchantment levels as numbers",
        "Tooltips and the enchanting table show Sharpness 5 instead of Sharpness V.";
    FireLower, "fire_lower", "Lower fire",
        "Moves the flames you see while burning down, so they cover less of the screen.";
    ShieldSize, "shield_size", "Shield size", "The size of the shield in your hand, in first person.";
    TotemSize, "totem_size", "Totem animation size",
        "The size of the totem that fills the screen when it saves you.";
    GlintColour, "glint_colour", "Enchantment glint colour",
        "The colour of the shimmer on enchanted items and armour. Off is vanilla's purple.";
    ClearWater, "clear_water", "Clear water",
        "Under water, see as far as vanilla lets you after 30 seconds there, from the start.";
    ClearLava, "clear_lava", "Clear lava",
        "In lava, see as far as vanilla's Fire Resistance lets you, without the effect.";
    ViewTime, "view_time", "Time of day",
        "The time of day you see: the sky and its light only. The world's time goes on and nothing is sent to the server.";
    ViewWeather, "view_weather", "Weather",
        "The weather you see: rain, its light and fog only. The world's weather goes on and nothing is sent to the server.";
}
/// The PvP & HUD page: headers (language keys) and options.
pub fn rows() -> Vec<Result<Opt, &'static str>> {
    use Opt::*;
    let mut rows = vec![Err("horizon.pvp.header.crosshair")];
    rows.extend(
        [
            Crosshair,
            CrosshairShape,
            CrosshairSize,
            CrosshairGap,
            CrosshairThickness,
            CrosshairColour,
            CrosshairOutline,
            CrosshairDot,
            CrosshairHover,
        ]
        .map(Ok),
    );
    rows.push(Err("horizon.pvp.header.outline"));
    rows.extend(
        [
            Outline,
            OutlineColour,
            OutlineOpacity,
            OutlineThickness,
            OutlineFill,
        ]
        .map(Ok),
    );
    rows.push(Err("horizon.pvp.header.hit"));
    rows.extend([Hit, HitColour, HitOpacity].map(Ok));
    rows.push(Err("horizon.pvp.header.hud"));
    rows.extend(
        [
            HudArmour,
            HudArmourCorner,
            HudArmourPercent,
            HudEffects,
            HudEffectsCorner,
            HudHeld,
            HudHeldCorner,
            HudPing,
            HudPingCorner,
            HudCoords,
            HudCoordsCorner,
            HudServer,
            HudServerCorner,
            HudCombo,
            HudComboCorner,
            HudBackground,
        ]
        .map(Ok),
    );
    rows.push(Err("horizon.pvp.header.items"));
    rows.push(Ok(NumericEnchants));
    rows.push(Ok(GlintColour));
    rows.push(Err("horizon.pvp.header.visual"));
    rows.extend([FireLower, ShieldSize, TotemSize, ClearWater, ClearLava].map(Ok));
    rows.push(Err("horizon.pvp.header.view"));
    rows.extend([ViewTime, ViewWeather].map(Ok));
    rows
}
impl Opt {
    /// The switch this option belongs to.
    pub fn parent(self) -> Option<Opt> {
        use Opt::*;
        match self {
            Crosshair | Outline | Hit => None,
            OutlineColour | OutlineOpacity | OutlineThickness | OutlineFill => Some(Outline),
            HitColour | HitOpacity => Some(Hit),
            HudArmour | HudEffects | HudHeld | HudPing | HudCoords | HudServer | HudCombo
            | HudBackground | NumericEnchants => None,
            FireLower | ShieldSize | TotemSize | GlintColour | ClearWater | ClearLava => None,
            ViewTime | ViewWeather => None,
            HudPingCorner => Some(HudPing),
            HudCoordsCorner => Some(HudCoords),
            HudServerCorner => Some(HudServer),
            HudComboCorner => Some(HudCombo),
            HudArmourCorner | HudArmourPercent => Some(HudArmour),
            HudEffectsCorner => Some(HudEffects),
            HudHeldCorner => Some(HudHeld),
            _ => Some(Crosshair),
        }
    }
    pub fn switch(self, p: &Pvp) -> Option<bool> {
        let c = &p.crosshair;
        Some(match self {
            Opt::Crosshair => c.custom,
            Opt::CrosshairOutline => c.outline,
            Opt::CrosshairDot => c.dot,
            Opt::Outline => p.outline.custom,
            Opt::Hit => p.hit.custom,
            Opt::HudArmour => p.hud.armour,
            Opt::HudArmourPercent => p.hud.armour_percent,
            Opt::HudEffects => p.hud.effects,
            Opt::HudHeld => p.hud.held,
            Opt::HudBackground => p.hud.background,
            Opt::HudPing => p.hud.ping,
            Opt::HudCoords => p.hud.coords,
            Opt::HudServer => p.hud.server,
            Opt::HudCombo => p.hud.combo,
            Opt::NumericEnchants => p.tweaks.numeric_enchants,
            Opt::ClearWater => p.tweaks.clear_water,
            Opt::ClearLava => p.tweaks.clear_lava,
            _ => return None,
        })
    }
    /// The value its button shows, for the options that aren't switches.
    pub fn value(self, p: &Pvp) -> Option<String> {
        let c = &p.crosshair;
        Some(match self {
            Opt::CrosshairShape => c.shape.label(),
            Opt::CrosshairSize => c.size.to_string(),
            Opt::CrosshairGap => c.gap.to_string(),
            Opt::CrosshairThickness => c.thickness.to_string(),
            Opt::CrosshairColour => c.colour.label(),
            Opt::CrosshairHover => match c.hover {
                Some(colour) => colour.label(),
                None => crate::lang::get_or("Off", "Off"),
            },
            Opt::OutlineColour => p.outline.colour.label(),
            Opt::OutlineOpacity => format!("{}%", p.outline.opacity),
            Opt::OutlineThickness => format!("{}x", p.outline.thickness),
            Opt::OutlineFill => match p.outline.fill {
                Some(colour) => colour.label(),
                None => crate::lang::get_or("Off", "Off"),
            },
            Opt::HitColour => p.hit.colour.label(),
            Opt::HitOpacity => format!("{}%", p.hit.opacity),
            Opt::HudArmourCorner => corner_label(p.hud.armour_corner),
            Opt::HudEffectsCorner => corner_label(p.hud.effects_corner),
            Opt::HudHeldCorner => corner_label(p.hud.held_corner),
            Opt::HudPingCorner => corner_label(p.hud.ping_corner),
            Opt::HudCoordsCorner => corner_label(p.hud.coords_corner),
            Opt::HudServerCorner => corner_label(p.hud.server_corner),
            Opt::HudComboCorner => corner_label(p.hud.combo_corner),
            Opt::FireLower => match p.tweaks.fire_lower {
                0 => crate::lang::get_or("Off", "Off"),
                v => format!("{v}%"),
            },
            Opt::ShieldSize => format!("{}%", p.tweaks.shield_size),
            Opt::TotemSize => format!("{}%", p.tweaks.totem_size),
            Opt::GlintColour => match p.tweaks.glint {
                Some(colour) => colour.label(),
                None => crate::lang::get_or("Off", "Off"),
            },
            Opt::ViewTime => p.tweaks.time.map_or_else(world_label, ViewTime::label),
            Opt::ViewWeather => p
                .tweaks
                .weather
                .map_or_else(world_label, ViewWeather::label),
            _ => return None,
        })
    }
    /// A click; `back` (right button, wheel up) goes the other way.
    pub fn cycle(self, p: &mut Pvp, back: bool) {
        let c = &mut p.crosshair;
        let (o, h, u) = (&mut p.outline, &mut p.hit, &mut p.hud);
        let t = &mut p.tweaks;
        let corner = |c: Corner| step(&Corner::ALL, c, back);
        // Off, then every solid colour.
        let solid_or_off = |v: Option<Colour>| {
            let all = [None]
                .into_iter()
                .chain(Colour::ALL[1..].iter().copied().map(Some));
            step(&all.collect::<Vec<_>>(), v, back)
        };
        match self {
            Opt::Crosshair => c.custom = !c.custom,
            Opt::CrosshairShape => c.shape = step(&Shape::ALL, c.shape, back),
            Opt::CrosshairSize => c.size = step_num(c.size, SIZES.0, SIZES.1, back),
            Opt::CrosshairGap => c.gap = step_num(c.gap, GAPS.0, GAPS.1, back),
            Opt::CrosshairThickness => {
                c.thickness = step_num(c.thickness, THICKNESSES.0, THICKNESSES.1, back)
            }
            Opt::CrosshairColour => c.colour = c.colour.step(back),
            Opt::CrosshairOutline => c.outline = !c.outline,
            Opt::CrosshairDot => c.dot = !c.dot,
            Opt::CrosshairHover => {
                // Off, then every colour.
                let all: Vec<Option<Colour>> = std::iter::once(None)
                    .chain(Colour::ALL.into_iter().map(Some))
                    .collect();
                c.hover = step(&all, c.hover, back);
            }
            Opt::Outline => o.custom = !o.custom,
            Opt::OutlineColour => o.colour = o.colour.step_solid(back),
            Opt::OutlineOpacity => o.opacity = step_by(o.opacity, OPACITIES, 10, back),
            Opt::OutlineThickness => {
                o.thickness = step_by(o.thickness, OUTLINE_THICKNESSES, 1, back)
            }
            Opt::OutlineFill => o.fill = solid_or_off(o.fill),
            Opt::Hit => h.custom = !h.custom,
            Opt::HitColour => h.colour = h.colour.step_solid(back),
            Opt::HitOpacity => h.opacity = step_by(h.opacity, HIT_OPACITIES, 10, back),
            Opt::HudArmour => u.armour = !u.armour,
            Opt::HudArmourCorner => u.armour_corner = corner(u.armour_corner),
            Opt::HudArmourPercent => u.armour_percent = !u.armour_percent,
            Opt::HudEffects => u.effects = !u.effects,
            Opt::HudEffectsCorner => u.effects_corner = corner(u.effects_corner),
            Opt::HudHeld => u.held = !u.held,
            Opt::HudHeldCorner => u.held_corner = corner(u.held_corner),
            Opt::HudBackground => u.background = !u.background,
            Opt::HudPing => u.ping = !u.ping,
            Opt::HudPingCorner => u.ping_corner = corner(u.ping_corner),
            Opt::HudCoords => u.coords = !u.coords,
            Opt::HudCoordsCorner => u.coords_corner = corner(u.coords_corner),
            Opt::HudServer => u.server = !u.server,
            Opt::HudServerCorner => u.server_corner = corner(u.server_corner),
            Opt::HudCombo => u.combo = !u.combo,
            Opt::HudComboCorner => u.combo_corner = corner(u.combo_corner),
            Opt::NumericEnchants => p.tweaks.numeric_enchants = !p.tweaks.numeric_enchants,
            Opt::FireLower => t.fire_lower = step_by(t.fire_lower, FIRE_LOWER, 10, back),
            Opt::ShieldSize => t.shield_size = step_by(t.shield_size, SHIELD_SIZES, 10, back),
            Opt::TotemSize => t.totem_size = step_by(t.totem_size, TOTEM_SIZES, 25, back),
            Opt::GlintColour => t.glint = solid_or_off(t.glint),
            Opt::ClearWater => t.clear_water = !t.clear_water,
            Opt::ClearLava => t.clear_lava = !t.clear_lava,
            Opt::ViewTime => {
                let all: Vec<_> = std::iter::once(None)
                    .chain(ViewTime::ALL.map(Some))
                    .collect();
                t.time = step(&all, t.time, back);
            }
            Opt::ViewWeather => {
                let all: Vec<_> = std::iter::once(None)
                    .chain(ViewWeather::ALL.map(Some))
                    .collect();
                t.weather = step(&all, t.weather, back);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mask drawn as rows of `#` and `.` over `-r..=r`.
    fn picture(m: &Mask, r: i32) -> Vec<String> {
        (-r..=r)
            .map(|y| {
                (-r..=r)
                    .map(|x| if m.get(x, y) { '#' } else { '.' })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_default_crosshair_covers_vanillas_sprite() {
        // hud/crosshair.png: a 15x15 plus sign, one pixel wide, through the middle.
        let m = Crosshair::default().mask();
        for y in -7..=7 {
            for x in -7..=7 {
                assert_eq!(m.get(x, y), x == 0 || y == 0, "({x}, {y})");
            }
        }
        assert!(!m.get(8, 0) && !m.get(0, -8));
        assert_eq!(
            m.runs().count(),
            15,
            "one run per row: 14 single pixels and the bar"
        );
    }

    #[test]
    fn gap_thickness_shape_and_dot_change_the_pixels() {
        let c = Crosshair {
            size: 2,
            gap: 1,
            ..Default::default()
        };
        assert_eq!(
            picture(&c.mask(), 3),
            [
                "...#...", "...#...", ".......", "##...##", ".......", "...#...", "...#...",
            ]
        );
        let dotted = Crosshair { dot: true, ..c };
        assert!(dotted.mask().get(0, 0));
        let t = Crosshair {
            shape: Shape::T,
            thickness: 3,
            size: 1,
            ..Default::default()
        };
        assert_eq!(
            picture(&t.mask(), 2),
            [".....", "#####", "#####", "#####", ".###."]
        );
        let x = Crosshair {
            shape: Shape::X,
            size: 2,
            ..Default::default()
        };
        assert_eq!(
            picture(&x.mask(), 2),
            ["#...#", ".#.#.", "..#..", ".#.#.", "#...#"]
        );
        let square = Crosshair {
            shape: Shape::Square,
            size: 2,
            ..Default::default()
        };
        assert_eq!(
            picture(&square.mask(), 2),
            ["#####", "#...#", "#...#", "#...#", "#####"]
        );
        let ring = Crosshair {
            shape: Shape::Circle,
            size: 3,
            ..Default::default()
        };
        let p = picture(&ring.mask(), 3);
        assert_eq!(p[3], "#.....#", "{p:?}");
        assert_eq!(p[0], "..###..", "{p:?}");
        assert!(!ring.mask().get(0, 0));
        let dot = Crosshair {
            shape: Shape::Dot,
            thickness: 2,
            ..Default::default()
        };
        assert_eq!(picture(&dot.mask(), 1), ["...", ".##", ".##"]);
    }

    #[test]
    fn the_outline_rings_the_shape_without_covering_it() {
        let dot = Crosshair {
            shape: Shape::Dot,
            ..Default::default()
        };
        assert_eq!(
            picture(&dot.mask().rim(), 2),
            [".....", ".###.", ".#.#.", ".###.", "....."]
        );
        let mut cache = CrosshairCache::default();
        let (shape, rim) = cache.runs(&Crosshair {
            outline: true,
            ..Crosshair::default()
        });
        assert_eq!(shape.len(), 15);
        // Rows -8 and 8 (one run each), and the rows beside the bar: three runs each
        // (left of it, between, right of it) except where the arms meet it.
        assert!(rim.iter().all(|&(x0, _, x1)| x1 > x0));
        assert!(rim.iter().any(|&(_, y, _)| y == -8) && rim.iter().any(|&(_, y, _)| y == 8));
    }

    #[test]
    fn options_round_trip_and_cycle_both_ways() {
        let d = Pvp::default();
        let mut text = String::new();
        d.serialize(&mut text);
        let mut back = Pvp::default();
        for line in text.lines() {
            let (k, v) = line.split_once(':').unwrap();
            assert!(back.parse_line(k, v), "{k}");
        }
        assert_eq!(back, d);
        let mut p = Pvp::default();
        for opt in rows().into_iter().flatten() {
            opt.cycle(&mut p, false);
        }
        assert!(p.crosshair.custom && p.crosshair.outline && p.crosshair.dot);
        assert_eq!(p.crosshair.shape, Shape::T);
        assert_eq!(p.crosshair.size, 8);
        assert_eq!(p.crosshair.hover, Some(Colour::Inverted));
        // From vanilla's values, one click: the overlays' sizes wrap to their smallest.
        let t = p.tweaks;
        assert_eq!((t.fire_lower, t.shield_size, t.totem_size), (10, 50, 25));
        assert_eq!(t.glint, Some(Colour::White));
        assert!(t.clear_water && t.clear_lava);
        assert_eq!(
            (t.time, t.weather),
            (Some(ViewTime::Morning), Some(ViewWeather::Clear))
        );
        let mut text = String::new();
        p.serialize(&mut text);
        let mut read = Pvp::default();
        text.lines().for_each(|l| {
            let (k, v) = l.split_once(':').unwrap();
            read.parse_line(k, v);
        });
        assert_eq!(read, p);
        // Down from the lowest wraps to the highest; bad values keep what was there.
        Opt::CrosshairThickness.cycle(&mut p, true);
        Opt::CrosshairThickness.cycle(&mut p, true);
        assert_eq!(p.crosshair.thickness, THICKNESSES.1);
        Opt::CrosshairHover.cycle(&mut p, true);
        assert_eq!(p.crosshair.hover, None);
        assert!(p.parse_line("crosshairSize", "99") && p.crosshair.size == SIZES.1);
        assert!(p.parse_line("crosshairColour", "plaid") && p.crosshair.colour == Colour::White);
        assert!(!p.parse_line("fullbright", "true"));
        assert!(p.parse_line("totemSize", "5") && p.tweaks.totem_size == TOTEM_SIZES.0);
    }

    #[test]
    fn a_fixed_time_keeps_the_worlds_day() {
        // Day 9 at 5000: the moon's phase (the day number) stays, the time of day is the chosen.
        let world = 9 * 24000 + 5000;
        assert_eq!(view_day_time(world, None), world);
        assert_eq!(
            view_day_time(world, Some(ViewTime::Midnight)),
            9 * 24000 + 18000
        );
        assert_eq!(view_day_time(-100, Some(ViewTime::Noon)), -24000 + 6000);
    }

    #[test]
    fn the_glint_colour_reaches_the_shaders_as_rgb_bytes() {
        let mut t = Tweaks::default();
        assert_eq!(t.glint_texel(), 0, "vanilla's glint");
        t.glint = Some(Colour::Red);
        // unpack4x8unorm reads the low byte first: r = 1, g = 0, b = 0, a = 1.
        assert_eq!(t.glint_texel().to_le_bytes(), [255, 0, 0, 255]);
    }

    #[test]
    fn outline_and_hit_colours_default_to_vanillas() {
        // Turned on with their first settings, they give vanilla's colours: the outline's
        // 0x66000000 (LevelRenderer) and OverlayTexture's hurt texel 0xB2FF0000.
        let o = Outline {
            custom: true,
            ..Default::default()
        };
        assert_eq!(o.stroke(), Some((0x6600_0000, 1.0)));
        assert_eq!(o.fill(), None);
        let [r, g, b, a] = crate::overlay::HURT_TEXEL;
        let hit = Hit {
            custom: true,
            ..Default::default()
        };
        assert_eq!(hit.texel(), u32::from_le_bytes([r, g, b, a]));
        assert_eq!(
            (Outline::default().stroke(), Hit::default().texel()),
            (None, 0)
        );
        // Fully opaque cyan covers the entity entirely: none of its own colour stays.
        let cyan = Hit {
            custom: true,
            colour: Colour::Cyan,
            opacity: 100,
        };
        assert_eq!(cyan.texel().to_le_bytes(), [0x40, 0xFF, 0xFF, 0]);
        let mut p = Pvp::default();
        Opt::Outline.cycle(&mut p, false);
        Opt::OutlineFill.cycle(&mut p, false);
        assert_eq!(p.outline.fill(), Some(0x40FF_FFFF), "white at 25%");
        Opt::OutlineOpacity.cycle(&mut p, true);
        Opt::OutlineOpacity.cycle(&mut p, true);
        Opt::OutlineOpacity.cycle(&mut p, true);
        Opt::OutlineOpacity.cycle(&mut p, true);
        assert_eq!(p.outline.opacity, 100, "down from 10% wraps to 100%");
        Opt::OutlineColour.cycle(&mut p, true);
        assert_eq!(p.outline.colour, Colour::Magenta, "never Inverted");
        let mut text = String::new();
        p.serialize(&mut text);
        let mut read = Pvp::default();
        for line in text.lines() {
            let (k, v) = line.split_once(':').unwrap();
            assert!(read.parse_line(k, v), "{k}");
        }
        assert_eq!(read, p);
    }

    #[test]
    fn every_option_and_value_has_its_english_text() {
        let en = crate::lang::Language::load(&[], "en_us");
        for opt in Opt::ALL {
            assert_eq!(en.get(opt.caption_key()), Some(opt.caption()), "{opt:?}");
            assert_eq!(
                en.get(opt.description_key()),
                Some(opt.description()),
                "{opt:?}"
            );
        }
        for header in rows().into_iter().filter_map(Result::err) {
            assert!(en.get(header).is_some(), "{header}");
        }
        for c in Corner::ALL {
            let key = format!("horizon.pvp.corner.{}", c.name());
            assert_eq!(en.get(&key), Some(c.label()), "{key}");
        }
        let names = Colour::ALL.map(|c| (format!("horizon.pvp.colour.{}", c.name()), c.english()));
        let shapes = Shape::ALL.map(|c| (format!("horizon.pvp.shape.{}", c.name()), c.english()));
        let times = ViewTime::ALL.map(|c| (format!("horizon.pvp.time.{}", c.name()), c.english()));
        let weathers =
            ViewWeather::ALL.map(|c| (format!("horizon.pvp.weather.{}", c.name()), c.english()));
        assert_eq!(en.get("horizon.pvp.view.world"), Some("World's"));
        for (key, english) in names.iter().chain(&shapes).chain(&times).chain(&weathers) {
            assert_eq!(en.get(key), Some(*english), "{key}");
        }
    }
}
