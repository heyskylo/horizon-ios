//! Custom skies: the layers a pack draws over the sky of a world, from
//! `optifine/sky/world<N>/sky<M>.properties` (`world0` the Overworld, `world1` the End; the Nether
//! draws no sky), and the `sun` and `moon_phases` textures that replace the vanilla ones.
//!
//! A layer is a skybox (one texture, six faces in a 3x2 grid, [`Face`]) drawn on top of the
//! previous layer (or the vanilla sky) with a blend ([`Blend`]), at a brightness its fade times
//! give for the time of day ([`Fade`]), on the days, in the weather, biomes and heights it names
//! ([`SkyLayer::shows`]), and turning with the time of day around its axis ([`SkyLayer::turns`]).
//! Everything here is read once at load; a frame evaluates a few numbers per layer.
//!
//! Written from OptiFine's public documentation of the format (`OptiFineDoc/doc/sky.properties`
//! and the Custom Sky page of OptiDocs, with its skybox template); no OptiFine code was read.

use std::collections::BTreeMap;
use std::path::Path;

use crate::matchers::{IdList, RangeList};
use crate::paths::{properties_files, resolve};
use crate::properties::Properties;

/// Ticks in a day.
const DAY: i64 = 24_000;

/// The custom sky of every world a pack gives one, by world number (`0` the Overworld, `1` the
/// End).
#[derive(Debug, Default, PartialEq)]
pub struct Skies {
    pub worlds: BTreeMap<i32, WorldSky>,
}

/// One world's layers, drawn in order, and its sun and moon.
#[derive(Debug, Default, PartialEq)]
pub struct WorldSky {
    pub layers: Vec<SkyLayer>,
    /// Replaces `sun.png`: a texture laid out like it, and the blend it draws with.
    pub sun: Option<Body>,
    /// Replaces `moon_phases.png`: four columns and two rows of phases, as that sheet.
    pub moon: Option<Body>,
}

/// The texture and blend of a replaced sun or moon.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub source: String,
    pub blend: Blend,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkyLayer {
    /// The properties file, a resource path (for messages).
    pub file: String,
    /// The skybox texture, a resource path ending in `.png`.
    pub source: String,
    /// `None`: always at full brightness.
    pub fade: Option<Fade>,
    pub blend: Blend,
    pub rotate: bool,
    /// Turns per day.
    pub speed: f32,
    /// The axis it turns around, normalised; south (`0 0 1`) by default, as the sun.
    pub axis: [f32; 3],
    pub days: Option<RangeList>,
    pub days_loop: i32,
    pub weather: Weather,
    pub biomes: Option<IdList>,
    pub heights: Option<RangeList>,
    /// Seconds the brightness takes to follow a change of the conditions ([`Transition`]).
    pub transition: f32,
}

/// How a layer mixes with what is under it (`blend=`). `add` by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Blend {
    #[default]
    Add,
    Subtract,
    Multiply,
    Dodge,
    Burn,
    Screen,
    /// The layer's pixels wherever its brightness is above 0, unfaded.
    Replace,
    Overlay,
    /// A weighted average by the layer's alpha.
    Alpha,
}

impl Blend {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "add" => Self::Add,
            "subtract" => Self::Subtract,
            "multiply" => Self::Multiply,
            "dodge" => Self::Dodge,
            "burn" => Self::Burn,
            "screen" => Self::Screen,
            "replace" => Self::Replace,
            "overlay" => Self::Overlay,
            "alpha" => Self::Alpha,
            _ => return None,
        })
    }
}

/// The weathers a layer shows in (`weather=clear rain thunder`); `clear` alone by default, so a
/// layer without the key hides in the rain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weather {
    pub clear: bool,
    pub rain: bool,
    pub thunder: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Self {
            clear: true,
            rain: false,
            thunder: false,
        }
    }
}

/// The weather of a frame: thunder is a storm's rain, so it is only `Thunder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Now {
    Clear,
    Rain,
    Thunder,
}

impl Weather {
    fn parse(value: &str) -> Self {
        let mut out = Self {
            clear: false,
            rain: false,
            thunder: false,
        };
        for word in value.split_whitespace() {
            match word.to_ascii_lowercase().as_str() {
                "clear" => out.clear = true,
                "rain" => out.rain = true,
                "thunder" => out.thunder = true,
                _ => {}
            }
        }
        out
    }

    pub fn admits(self, now: Now) -> bool {
        match now {
            Now::Clear => self.clear,
            Now::Rain => self.rain,
            Now::Thunder => self.thunder,
        }
    }
}

/// The fade times of a layer, in ticks of the day (`0` sunrise, 6:00). Between `start_in` and
/// `end_in` the brightness rises from 0 to 1, stays 1 until `start_out`, falls to 0 at `end_out`,
/// and stays 0 until `start_in` again; each span may cross midnight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fade {
    pub start_in: i64,
    pub end_in: i64,
    pub start_out: i64,
    pub end_out: i64,
}

impl Fade {
    /// `startFadeOut` is optional: the fade out lasts as long as the fade in.
    fn parse(p: &Properties) -> Option<Self> {
        let start_in = time(p.get("startFadeIn")?)?;
        let end_in = time(p.get("endFadeIn")?)?;
        let end_out = time(p.get("endFadeOut")?)?;
        let start_out = match p.get("startFadeOut") {
            Some(v) => time(v)?,
            None => (end_out - (end_in - start_in).rem_euclid(DAY)).rem_euclid(DAY),
        };
        Some(Self {
            start_in,
            end_in,
            start_out,
            end_out,
        })
    }

    /// The brightness at `day_time` (ticks; any day).
    pub fn brightness(&self, day_time: i64) -> f32 {
        let since = |t: i64| (t - self.start_in).rem_euclid(DAY);
        let now = since(day_time);
        let (end_in, start_out) = (since(self.end_in), since(self.start_out));
        // A fade out that ends where the fade in starts ends a whole day later, not at once.
        let end_out = match since(self.end_out) {
            0 => DAY,
            end_out => end_out,
        };
        if now < end_in {
            now as f32 / end_in as f32
        } else if now < start_out {
            1.0
        } else if now < end_out {
            1.0 - (now - start_out) as f32 / (end_out - start_out) as f32
        } else {
            0.0
        }
    }
}

/// `hh:mm` on a 24-hour clock, as ticks of the day: 6:00 is `0` (sunrise), 12:00 `6000`.
pub fn time(value: &str) -> Option<i64> {
    let (h, m) = value.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some((h * 1000 + m * 1000 / 60 - 6000).rem_euclid(DAY))
}

/// What a frame's layers are tested against.
#[derive(Clone, Copy, Debug)]
pub struct Conditions<'a> {
    pub day_time: i64,
    pub weather: Now,
    /// The camera's biome (`minecraft:plains`).
    pub biome: &'a str,
    /// The camera's block y.
    pub y: i32,
}

impl SkyLayer {
    /// The layer of the properties file at resource path `file`; `None` when it names a blend
    /// that does not exist or a source path outside the pack.
    pub fn parse(file: &str, p: &Properties) -> Option<Self> {
        let source = match p.get("source") {
            Some(source) => with_png(&resolve(file, source)?),
            None => format!("{}.png", file.strip_suffix(".properties")?),
        };
        let blend = match p.get("blend") {
            Some(v) => Blend::parse(v)?,
            None => Blend::Add,
        };
        let fade = Fade::parse(p);
        if fade.is_none() && p.get("startFadeIn").is_some() {
            log::warn!("{file}: fade times missing or not hh:mm; the layer always shows");
        }
        let axis = p.get("axis").and_then(axis).unwrap_or([0.0, 0.0, 1.0]);
        Some(Self {
            file: file.to_owned(),
            source,
            fade,
            blend,
            rotate: p
                .get("rotate")
                .is_none_or(|v| !v.eq_ignore_ascii_case("false")),
            speed: p.get("speed").and_then(|v| v.parse().ok()).unwrap_or(1.0),
            axis,
            days: p.get("days").and_then(RangeList::parse),
            days_loop: p
                .get("daysLoop")
                .and_then(|v| v.parse().ok())
                .filter(|&n: &i32| n > 0)
                .unwrap_or(8),
            weather: p.get("weather").map(Weather::parse).unwrap_or_default(),
            biomes: p.get("biomes").map(IdList::parse),
            heights: p.get("heights").and_then(RangeList::parse),
            transition: p
                .get("transition")
                .and_then(|v| v.parse().ok())
                .filter(|&t: &f32| t >= 0.0)
                .unwrap_or(1.0),
        })
    }

    /// Whether the day, weather, biome and height admit the layer; the brightness it then
    /// shows at is [`SkyLayer::brightness`], smoothed by a [`Transition`].
    pub fn shows(&self, c: &Conditions) -> bool {
        let day = c
            .day_time
            .div_euclid(DAY)
            .rem_euclid(i64::from(self.days_loop)) as i32;
        self.days.as_ref().is_none_or(|d| d.contains(day))
            && self.weather.admits(c.weather)
            && self.biomes.as_ref().is_none_or(|b| b.matches(c.biome))
            && self.heights.as_ref().is_none_or(|h| h.contains(c.y))
    }

    /// The brightness of the fade times at `day_time`; a `replace` layer is all or nothing.
    pub fn brightness(&self, day_time: i64) -> f32 {
        let b = self.fade.map_or(1.0, |f| f.brightness(day_time));
        if self.blend == Blend::Replace {
            if b > 0.0 { 1.0 } else { 0.0 }
        } else {
            b
        }
    }

    /// How far the layer has turned around its axis, in turns, when the sun stands at
    /// `sun_turns` (vanilla's sun angle over 360°: `0` at noon). A turn a day at speed 1, so the
    /// layer keeps pace with the sun; counted from noon of day 0, so a slower speed carries on
    /// across days. `0` for a layer that does not rotate.
    pub fn turns(&self, day_time: i64, sun_turns: f32) -> f32 {
        if !self.rotate {
            return 0.0;
        }
        let days = (day_time - 6000).div_euclid(DAY);
        // Whole turns only matter at a speed that is not whole; keep the f32 small.
        let days = (days as f64 * f64::from(self.speed)).rem_euclid(1.0);
        (days + f64::from(sun_turns) * f64::from(self.speed)).rem_euclid(1.0) as f32
    }
}

fn with_png(path: &str) -> String {
    if path.ends_with(".png") {
        path.to_owned()
    } else {
        format!("{path}.png")
    }
}

/// `x y z`, normalised; `None` for the zero vector or a malformed one.
fn axis(value: &str) -> Option<[f32; 3]> {
    let v: Vec<f32> = value
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [x, y, z] = v[..] else { return None };
    let len = (x * x + y * y + z * z).sqrt();
    (len > 1e-6 && len.is_finite()).then(|| [x / len, y / len, z / len])
}

/// A layer's brightness as it eases toward the target its conditions set: over `transition`
/// seconds from 0 to 1 (linearly), so it does not pop at a biome's border.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transition {
    pub value: f32,
}

impl Transition {
    /// Moves toward `target` by `seconds` of the transition and returns the new value.
    pub fn step(&mut self, target: f32, seconds: f32, transition: f32) -> f32 {
        if transition <= 0.0 {
            self.value = target;
        } else {
            let step = seconds / transition;
            self.value = if self.value < target {
                (self.value + step).min(target)
            } else {
                (self.value - step).max(target)
            };
        }
        self.value
    }
}

/// A face of the skybox, as the 3x2 grid of its texture holds them: `Bottom`, `Top`, `South` in
/// the top row, `West`, `North`, `East` below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Bottom,
    Top,
    South,
    West,
    North,
    East,
}

impl Face {
    pub const ALL: [Self; 6] = [
        Self::Bottom,
        Self::Top,
        Self::South,
        Self::West,
        Self::North,
        Self::East,
    ];

    /// The face's cell of the texture, `[u0, v0, u1, v1]` in 0..1.
    pub fn cell(self) -> [f32; 4] {
        let (col, row) = match self {
            Self::Bottom => (0, 0),
            Self::Top => (1, 0),
            Self::South => (2, 0),
            Self::West => (0, 1),
            Self::North => (1, 1),
            Self::East => (2, 1),
        };
        let (u, v) = (col as f32 / 3.0, row as f32 / 2.0);
        [u, v, u + 1.0 / 3.0, v + 0.5]
    }

    /// The face's corners (on the unit cube around the camera; x east, y up, z south) in the
    /// order of its cell's `(u0, v0)`, `(u1, v0)`, `(u1, v1)`, `(u0, v1)`. The side faces are
    /// seen from inside with up at the top of their cell; the top face has north at the bottom of
    /// its cell, the bottom face north at the top (the template's `N` marks).
    pub fn corners(self) -> [[f32; 3]; 4] {
        match self {
            // Seen looking north: west on the left.
            Self::North => [
                [-1., 1., -1.],
                [1., 1., -1.],
                [1., -1., -1.],
                [-1., -1., -1.],
            ],
            Self::South => [[1., 1., 1.], [-1., 1., 1.], [-1., -1., 1.], [1., -1., 1.]],
            Self::East => [[1., 1., -1.], [1., 1., 1.], [1., -1., 1.], [1., -1., -1.]],
            Self::West => [
                [-1., 1., 1.],
                [-1., 1., -1.],
                [-1., -1., -1.],
                [-1., -1., 1.],
            ],
            // Looking up with north toward the cell's bottom edge: east on the right.
            Self::Top => [[-1., 1., 1.], [1., 1., 1.], [1., 1., -1.], [-1., 1., -1.]],
            // Looking down with north toward the cell's top edge: east on the right.
            Self::Bottom => [
                [-1., -1., -1.],
                [1., -1., -1.],
                [1., -1., 1.],
                [-1., -1., 1.],
            ],
        }
    }
}

impl Skies {
    /// Every `sky/world<N>/` of the pack under `root`. Layers draw in the order of their number
    /// (`sky1` before `sky10`); a file that fails to parse is skipped with a warning.
    pub fn scan(root: &Path) -> Self {
        let mut out = Self::default();
        for ns in ["minecraft"] {
            for top in ["optifine", "mcpatcher"] {
                let base = format!("assets/{ns}/{top}/sky");
                let Ok(dirs) = std::fs::read_dir(root.join(&base)) else {
                    continue;
                };
                for dir in dirs.flatten() {
                    let name = dir.file_name().to_string_lossy().into_owned();
                    let Some(world) = name.strip_prefix("world").and_then(|n| n.parse().ok())
                    else {
                        continue;
                    };
                    if out.worlds.contains_key(&world) {
                        continue; // optifine/ before mcpatcher/
                    }
                    let sky = WorldSky::scan(root, &format!("{base}/{name}"));
                    if sky != WorldSky::default() {
                        out.worlds.insert(world, sky);
                    }
                }
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }
}

impl WorldSky {
    /// The layers and bodies of the folder at resource path `dir`.
    fn scan(root: &Path, dir: &str) -> Self {
        let read = |file: &str| {
            std::fs::read_to_string(root.join(file))
                .ok()
                .map(|t| Properties::parse(&t))
        };
        let feature = dir.split_once("/sky/").map_or("", |(_, w)| w);
        let mut layers: Vec<(u32, SkyLayer)> = Vec::new();
        for file in properties_files(root, &format!("sky/{feature}")) {
            if !file.starts_with(dir) {
                continue;
            }
            let stem = file.rsplit('/').next().unwrap_or("");
            let Some(n) = stem
                .strip_prefix("sky")
                .and_then(|s| s.strip_suffix(".properties"))
                .and_then(|n| n.parse().ok())
            else {
                continue;
            };
            let Some(p) = read(&file) else { continue };
            match SkyLayer::parse(&file, &p) {
                Some(layer) if root.join(&layer.source).is_file() => layers.push((n, layer)),
                Some(layer) => log::warn!("{file}: no texture {}", layer.source),
                None => log::warn!("{file}: unknown blend or bad source; skipped"),
            }
        }
        layers.sort_by_key(|(n, _)| *n);
        let body = |name: &str| {
            let props = format!("{dir}/{name}.properties");
            let p = read(&props).unwrap_or_default();
            let source = match p.get("source") {
                Some(s) => with_png(&resolve(&props, s)?),
                None => format!("{dir}/{name}.png"),
            };
            if !root.join(&source).is_file() {
                return None;
            }
            let blend = p
                .get("blend")
                .and_then(Blend::parse)
                .unwrap_or(Blend::Alpha);
            Some(Body { source, blend })
        };
        Self {
            layers: layers.into_iter().map(|(_, l)| l).collect(),
            sun: body("sun"),
            moon: body("moon_phases"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(text: &str) -> SkyLayer {
        SkyLayer::parse(
            "assets/minecraft/optifine/sky/world0/sky3.properties",
            &Properties::parse(text),
        )
        .unwrap()
    }

    fn at(day_time: i64) -> Conditions<'static> {
        Conditions {
            day_time,
            weather: Now::Clear,
            biome: "minecraft:plains",
            y: 64,
        }
    }

    #[test]
    fn clock_times_are_ticks_from_sunrise() {
        assert_eq!(time("6:00"), Some(0));
        assert_eq!(time("12:00"), Some(6000));
        assert_eq!(time("18:00"), Some(12000));
        assert_eq!(time("0:00"), Some(18000));
        assert_eq!(time("05:30"), Some(23500));
        assert_eq!(time("24:00"), None);
        assert_eq!(time("noon"), None);
    }

    #[test]
    fn a_night_layer_fades_across_midnight() {
        // In from 18:00 to 20:00, out by 6:00: the fade out starts at 4:00, two hours before.
        let l = layer("startFadeIn=18:00\nendFadeIn=20:00\nendFadeOut=6:00");
        let f = l.fade.unwrap();
        assert_eq!(f.start_out, 22000);
        assert_eq!(l.brightness(12000), 0.0); // 18:00
        assert_eq!(l.brightness(13000), 0.5); // 19:00
        assert_eq!(l.brightness(18000), 1.0); // midnight
        assert_eq!(l.brightness(23000), 0.5); // 5:00
        assert_eq!(l.brightness(0), 0.0); // 6:00
        assert_eq!(l.brightness(6000), 0.0); // noon
        assert_eq!(l.brightness(24000 * 3 + 13000), 0.5, "any day");
    }

    #[test]
    fn a_fade_out_ending_where_the_fade_in_starts_still_ramps_down() {
        let fade = Fade {
            start_in: 0,
            end_in: 1000,
            start_out: 10000,
            end_out: 0,
        };
        assert_eq!(fade.brightness(9999), 1.0);
        assert_eq!(fade.brightness(17000), 0.5);
        assert_eq!(fade.brightness(0), 0.0);
    }

    #[test]
    fn an_explicit_fade_out_start_is_kept() {
        let l = layer(
            "startFadeIn=6:00\nendFadeIn=7:00\nstartFadeOut=10:00\nendFadeOut=14:00\nblend=alpha",
        );
        assert_eq!(l.brightness(5000), 0.75); // 11:00, a quarter of the way out
    }

    #[test]
    fn no_fade_times_means_always_and_replace_is_unfaded() {
        assert_eq!(layer("").brightness(9999), 1.0);
        let l = layer("startFadeIn=6:00\nendFadeIn=8:00\nendFadeOut=20:00\nblend=replace");
        assert_eq!(l.brightness(500), 1.0, "half way in, yet full");
        assert_eq!(l.brightness(20000), 0.0);
    }

    #[test]
    fn defaults_and_paths() {
        let l = layer("");
        assert_eq!(l.source, "assets/minecraft/optifine/sky/world0/sky3.png");
        assert_eq!(l.blend, Blend::Add);
        assert!(l.rotate);
        assert_eq!(l.axis, [0.0, 0.0, 1.0]);
        assert_eq!(l.days_loop, 8);
        assert_eq!(l.transition, 1.0);
        let l = layer("source=./stars\naxis=0 2 0\nblend=Multiply");
        assert_eq!(l.source, "assets/minecraft/optifine/sky/world0/stars.png");
        assert_eq!(l.axis, [0.0, 1.0, 0.0]);
        assert_eq!(l.blend, Blend::Multiply);
        assert_eq!(
            layer("source=~/sky/clouds.png").source,
            "assets/minecraft/optifine/sky/clouds.png"
        );
        assert_eq!(layer("axis=0 0 0").axis, [0.0, 0.0, 1.0]);
        let p = Properties::parse("blend=lighten");
        assert!(
            SkyLayer::parse("assets/minecraft/optifine/sky/world0/sky1.properties", &p).is_none()
        );
        let p = Properties::parse("source=../../../../../etc/passwd");
        assert!(
            SkyLayer::parse("assets/minecraft/optifine/sky/world0/sky1.properties", &p).is_none()
        );
    }

    #[test]
    fn weather_defaults_to_clear_only() {
        let l = layer("");
        assert!(l.shows(&at(0)));
        assert!(!l.shows(&Conditions {
            weather: Now::Rain,
            ..at(0)
        }));
        let l = layer("weather=rain thunder");
        assert!(!l.shows(&at(0)));
        assert!(l.shows(&Conditions {
            weather: Now::Thunder,
            ..at(0)
        }));
    }

    #[test]
    fn days_biomes_and_heights() {
        let l = layer("days=0 2-3\ndaysLoop=4");
        let shows = |day: i64| l.shows(&at(day * 24000 + 100));
        assert_eq!(
            (0..8).map(shows).collect::<Vec<_>>(),
            [true, false, true, true, true, false, true, true]
        );
        let l = layer("biomes=desert !plains minecraft:badlands\nheights=(-64)-0 100-");
        let c = |biome, y| Conditions { biome, y, ..at(0) };
        assert!(l.shows(&c("minecraft:desert", -10)));
        assert!(l.shows(&c("minecraft:badlands", 300)));
        assert!(!l.shows(&c("minecraft:desert", 64)));
        assert!(!l.shows(&c("minecraft:plains", -10)));
        assert!(!l.shows(&c("minecraft:forest", -10)));
    }

    #[test]
    fn layers_turn_with_the_sun_and_slower_ones_carry_over_days() {
        let l = layer("");
        assert_eq!(l.turns(6000, 0.0), 0.0);
        assert_eq!(l.turns(12000, 0.25), 0.25);
        assert_eq!(layer("rotate=false").turns(12000, 0.25), 0.0);
        // Half speed: noon of day 1 is half a turn on, and a quarter day later 5/8.
        let l = layer("speed=0.5");
        assert_eq!(l.turns(6000, 0.0), 0.0);
        assert_eq!(l.turns(30000, 0.0), 0.5);
        assert_eq!(l.turns(36000, 0.25), 0.625);
        assert_eq!(l.turns(54000, 0.0), 0.0);
    }

    #[test]
    fn transitions_ease_linearly() {
        let mut t = Transition::default();
        assert_eq!(t.step(1.0, 0.5, 2.0), 0.25);
        assert_eq!(t.step(1.0, 10.0, 2.0), 1.0);
        assert_eq!(t.step(0.0, 1.0, 2.0), 0.5);
        assert_eq!(t.step(0.2, 0.0, 0.0), 0.2);
    }

    #[test]
    fn the_grid_holds_each_face_once_and_faces_point_out() {
        let mut cells: Vec<_> = Face::ALL
            .iter()
            .map(|f| f.cell().map(|x| (x * 6.0) as i32))
            .collect();
        cells.dedup();
        assert_eq!(cells.len(), 6);
        assert_eq!(Face::East.cell(), [2.0 / 3.0, 0.5, 1.0, 1.0]);
        for face in Face::ALL {
            let c = face.corners();
            let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
            let (a, b) = (sub(c[1], c[0]), sub(c[3], c[0]));
            let n = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            let centre: [f32; 3] =
                std::array::from_fn(|i| c.iter().map(|p| p[i]).sum::<f32>() / 4.0);
            // Seen from inside, u runs right and v down, so u x v points away from the camera.
            let dot: f32 = (0..3).map(|i| n[i] * centre[i]).sum();
            assert!(dot > 0.0, "{face:?} is mirrored");
        }
        // The top cell's bottom edge (v1) is north: its corners 2 and 3 have z = -1.
        assert_eq!(Face::Top.corners()[2][2], -1.0);
        assert_eq!(Face::Top.corners()[3][2], -1.0);
        // The bottom cell's top edge (v0) is north.
        assert_eq!(Face::Bottom.corners()[0][2], -1.0);
        assert_eq!(Face::Bottom.corners()[1][2], -1.0);
    }

    #[test]
    fn scan_reads_worlds_in_number_order_with_bodies() {
        let root = std::env::temp_dir().join(format!("horizon-sky-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("assets/minecraft/optifine/sky/world0");
        std::fs::create_dir_all(&dir).unwrap();
        let end = root.join("assets/minecraft/optifine/sky/world1");
        std::fs::create_dir_all(&end).unwrap();
        for (name, text) in [
            ("sky10.properties", "blend=alpha"),
            ("sky2.properties", "blend=screen"),
            ("sky3.properties", "blend=add"), // no sky3.png: skipped
            ("sun.properties", "blend=add\nsource=./bright_sun.png"),
        ] {
            std::fs::write(dir.join(name), text).unwrap();
        }
        for png in ["sky10.png", "sky2.png", "bright_sun.png", "moon_phases.png"] {
            std::fs::write(dir.join(png), b"png").unwrap();
        }
        std::fs::write(end.join("sky1.properties"), "").unwrap();
        std::fs::write(end.join("sky1.png"), b"png").unwrap();
        let skies = Skies::scan(&root);
        let _ = std::fs::remove_dir_all(&root);
        let o = &skies.worlds[&0];
        assert_eq!(
            o.layers.iter().map(|l| l.blend).collect::<Vec<_>>(),
            [Blend::Screen, Blend::Alpha]
        );
        let sun = o.sun.as_ref().unwrap();
        assert_eq!(
            sun.source,
            "assets/minecraft/optifine/sky/world0/bright_sun.png"
        );
        assert_eq!(sun.blend, Blend::Add);
        let moon = o.moon.as_ref().unwrap();
        assert_eq!(
            moon.source,
            "assets/minecraft/optifine/sky/world0/moon_phases.png"
        );
        assert_eq!(moon.blend, Blend::Alpha);
        assert_eq!(skies.worlds[&1].layers.len(), 1);
        assert_eq!(skies.worlds.len(), 2);
    }
}
