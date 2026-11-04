//! PvP and HUD options (`docs/pvp.md`): a custom crosshair. Kept in
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
            Colour::Red => 0xFFFF_3030,
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
}

/// The next (or previous) value of `all` after `v`, wrapping.
fn step<T: Copy + PartialEq>(all: &[T], v: T, back: bool) -> T {
    let i = all.iter().position(|&x| x == v).unwrap_or(0);
    let n = all.len();
    all[if back { (i + n - 1) % n } else { (i + 1) % n }]
}
/// `v` one step up or down within `lo..=hi`, wrapping.
fn step_num(v: u8, lo: u8, hi: u8, back: bool) -> u8 {
    match (back, v) {
        (false, v) if v >= hi => lo,
        (false, v) => v + 1,
        (true, v) if v <= lo => hi,
        (true, v) => v - 1,
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pvp {
    pub crosshair: Crosshair,
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
    rows
}
impl Opt {
    /// The switch this option belongs to.
    pub fn parent(self) -> Option<Opt> {
        match self {
            Opt::Crosshair => None,
            _ => Some(Opt::Crosshair),
        }
    }
    pub fn switch(self, p: &Pvp) -> Option<bool> {
        let c = &p.crosshair;
        Some(match self {
            Opt::Crosshair => c.custom,
            Opt::CrosshairOutline => c.outline,
            Opt::CrosshairDot => c.dot,
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
            _ => return None,
        })
    }
    /// A click; `back` (right button, wheel up) goes the other way.
    pub fn cycle(self, p: &mut Pvp, back: bool) {
        let c = &mut p.crosshair;
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
        let names = Colour::ALL.map(|c| (format!("horizon.pvp.colour.{}", c.name()), c.english()));
        let shapes = Shape::ALL.map(|c| (format!("horizon.pvp.shape.{}", c.name()), c.english()));
        for (key, english) in names.iter().chain(&shapes) {
            assert_eq!(en.get(key), Some(*english), "{key}");
        }
    }
}
