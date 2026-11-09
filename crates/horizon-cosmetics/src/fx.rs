//! The worn kinds that are not on a bone: a companion floating beside the player, a trail of
//! particles left while moving, an aura around the feet or the body. Their blocks in the item
//! file and the limits they are held to; `follow` and `trail` run them.

use serde::Deserialize;

/// How far a companion's vertices may reach from its own origin (pixels), animations included:
/// a small creature or object.
pub const MAX_COMPANION_REACH: f32 = 8.0;
/// How far a companion's anchor may be from the neck (pixels).
pub const MAX_COMPANION_ANCHOR: f32 = 28.0;
/// The centre of the head in the player model (pixels, y down): what a companion keeps clear of.
pub const HEAD: [f32; 3] = [0.0, -4.0, 0.0];
/// The head's radius (the 8-pixel cube's half diagonal, rounded up) plus the gap kept between the
/// head and the nearest point a companion may reach (pixels).
pub const HEAD_CLEARANCE: f32 = 7.0 + 4.0;
/// The longest smoothing a companion's follow may have (ticks).
pub const MAX_LAG: f32 = 40.0;
/// Particles a trail may leave per block moved.
pub const MAX_TRAIL_RATE: f32 = 4.0;
/// The longest a trail's particle lives (ticks): two seconds.
pub const MAX_TRAIL_LIFETIME: u32 = 40;
/// The largest side of a particle (pixels).
pub const MAX_PARTICLE_SIZE: f32 = 4.0;
/// Sprites a trail's texture may hold (square, side by side).
pub const MAX_TRAIL_SPRITES: u32 = 8;
/// The largest sprite side (texels).
pub const MAX_SPRITE: u32 = 16;
/// How far from the middle of the feet an aura may reach sideways (pixels): about the width of
/// the hitbox plus half a block, so it frames the player without hiding anyone near them.
pub const MAX_AURA_RADIUS: f32 = 14.0;

/// What a companion does: where it floats and how it follows.
#[derive(Clone, Debug, PartialEq)]
pub struct Companion {
    /// Where it floats at rest, in the wearer's body space (pixels from the neck, y down, +x the
    /// wearer's left, +z behind).
    pub anchor: [f32; 3],
    /// How slowly it catches up (ticks): the time constant of its follow.
    pub lag: f32,
    /// A part that turns to look where the wearer looks.
    pub look: Option<String>,
    /// How far its vertices reach from its origin (pixels), found when read.
    pub reach: f32,
    /// Drawn at full light (a firefly's lantern).
    pub glow: bool,
}

/// Where a trail's particles start, or an aura sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum From {
    #[default]
    Feet,
    Body,
}

/// What a trail leaves: particles drawn from the item's texture, a row of square sprites.
#[derive(Clone, Debug, PartialEq)]
pub struct Trail {
    pub sprites: u32,
    /// Particles per block moved.
    pub rate: f32,
    /// Ticks each lives.
    pub lifetime: u32,
    /// The smallest and largest side (pixels).
    pub size: [f32; 2],
    /// Blocks per tick², down (negative: rises, as embers).
    pub gravity: f32,
    /// How far each sways sideways (pixels).
    pub drift: f32,
    /// Degrees per tick it turns.
    pub spin: f32,
    pub from: From,
    /// Fades out over its last third.
    pub fade: bool,
    /// Drawn at full light.
    pub glow: bool,
}

/// What an aura is: the item's model around the feet (or the body), see-through.
#[derive(Clone, Debug, PartialEq)]
pub struct Aura {
    pub at: From,
    /// Its opacity, at most `max_alpha`.
    pub alpha: f32,
    /// Drawn at full light.
    pub glow: bool,
}

impl Aura {
    /// The most opaque an aura may be: a ring at the feet hides nothing; one around the body
    /// stays faint so the player inside is as easy to see and hit.
    pub fn max_alpha(at: From) -> f32 {
        match at {
            From::Feet => 0.6,
            From::Body => 0.35,
        }
    }

    /// How high above the feet an aura may reach (pixels), and how far below.
    pub fn height(at: From) -> (f32, f32) {
        match at {
            From::Feet => (6.0, 1.0),
            From::Body => (30.0, 1.0),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompanionFile {
    anchor: [f32; 3],
    #[serde(default = "default_lag")]
    lag: f32,
    #[serde(default)]
    look: Option<String>,
    #[serde(default)]
    glow: bool,
}

fn default_lag() -> f32 {
    6.0
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrailFile {
    #[serde(default = "one")]
    sprites: u32,
    rate: f32,
    lifetime: u32,
    size: [f32; 2],
    #[serde(default)]
    gravity: f32,
    #[serde(default)]
    drift: f32,
    #[serde(default)]
    spin: f32,
    #[serde(default)]
    from: From,
    #[serde(default)]
    fade: bool,
    #[serde(default)]
    glow: bool,
}

fn one() -> u32 {
    1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuraFile {
    #[serde(default)]
    at: From,
    alpha: f32,
    #[serde(default)]
    glow: bool,
}

fn invalid(text: impl Into<String>) -> crate::Error {
    crate::Error::Invalid(text.into())
}

fn finite(values: &[f32], what: &str) -> Result<(), crate::Error> {
    match values.iter().all(|v| v.is_finite()) {
        true => Ok(()),
        false => Err(invalid(format!("{what} is not a number"))),
    }
}

impl CompanionFile {
    /// Checks the block of an item whose parts are `names`; `reach` is how far its vertices get
    /// from its origin, animations included.
    pub(crate) fn check(self, names: &[String], reach: f32) -> Result<Companion, crate::Error> {
        finite(&self.anchor, "companion.anchor")?;
        finite(&[self.lag], "companion.lag")?;
        if reach > MAX_COMPANION_REACH {
            return Err(invalid(format!(
                "a companion reaches {reach:.1} pixels from its origin, at most {MAX_COMPANION_REACH}"
            )));
        }
        let anchor = glam::Vec3::from(self.anchor);
        if anchor.length() > MAX_COMPANION_ANCHOR {
            return Err(invalid(format!(
                "companion.anchor is at most {MAX_COMPANION_ANCHOR} pixels from the neck"
            )));
        }
        if anchor.distance(glam::Vec3::from(HEAD)) < HEAD_CLEARANCE + reach {
            return Err(invalid(format!(
                "companion.anchor is {:.1} pixels or more from the head's centre",
                HEAD_CLEARANCE + reach
            )));
        }
        if !(1.0..=MAX_LAG).contains(&self.lag) {
            return Err(invalid(format!("companion.lag is 1 to {MAX_LAG} ticks")));
        }
        if let Some(look) = &self.look
            && !names.contains(look)
        {
            return Err(invalid(format!("companion.look names no part {look:?}")));
        }
        Ok(Companion {
            anchor: self.anchor,
            lag: self.lag,
            look: self.look,
            reach,
            glow: self.glow,
        })
    }
}

impl TrailFile {
    /// Checks the block against the item's texture (`width`×`height`).
    pub(crate) fn check(self, width: u32, height: u32) -> Result<Trail, crate::Error> {
        let t = self;
        finite(
            &[t.rate, t.size[0], t.size[1], t.gravity, t.drift, t.spin],
            "the trail",
        )?;
        if !(1..=MAX_TRAIL_SPRITES).contains(&t.sprites)
            || height > MAX_SPRITE
            || width != t.sprites * height
        {
            return Err(crate::Error::Texture(format!(
                "a trail's texture is a row of 1 to {MAX_TRAIL_SPRITES} square sprites of at most \
                 {MAX_SPRITE} texels"
            )));
        }
        if !(t.rate > 0.0 && t.rate <= MAX_TRAIL_RATE) {
            return Err(invalid(format!(
                "trail.rate is above 0 and at most {MAX_TRAIL_RATE} per block"
            )));
        }
        if !(1..=MAX_TRAIL_LIFETIME).contains(&t.lifetime) {
            return Err(invalid(format!(
                "trail.lifetime is 1 to {MAX_TRAIL_LIFETIME} ticks"
            )));
        }
        if !(t.size[0] > 0.0 && t.size[0] <= t.size[1] && t.size[1] <= MAX_PARTICLE_SIZE) {
            return Err(invalid(format!(
                "trail.size is [smallest, largest], above 0 and at most {MAX_PARTICLE_SIZE} pixels"
            )));
        }
        if t.gravity.abs() > 0.04
            || !(0.0..=4.0).contains(&t.drift)
            || !(0.0..=20.0).contains(&t.spin)
        {
            return Err(invalid(
                "trail.gravity is within 0.04, drift 0 to 4 pixels, spin 0 to 20 degrees a tick",
            ));
        }
        Ok(Trail {
            sprites: t.sprites,
            rate: t.rate,
            lifetime: t.lifetime,
            size: t.size,
            gravity: t.gravity,
            drift: t.drift,
            spin: t.spin,
            from: t.from,
            fade: t.fade,
            glow: t.glow,
        })
    }
}

impl AuraFile {
    /// Checks the block of an item whose vertices span `low` to `high` (pixels, from the middle of
    /// the feet, y down) over its animations.
    pub(crate) fn check(self, low: glam::Vec3, high: glam::Vec3) -> Result<Aura, crate::Error> {
        finite(&[self.alpha], "aura.alpha")?;
        let max = Aura::max_alpha(self.at);
        if !(self.alpha > 0.0 && self.alpha <= max) {
            return Err(invalid(format!("aura.alpha is above 0 and at most {max}")));
        }
        let radius = [low.x, high.x, low.z, high.z]
            .iter()
            .fold(0.0_f32, |r, v| r.max(v.abs()));
        let (up, down) = Aura::height(self.at);
        if radius > MAX_AURA_RADIUS || -low.y > up || high.y > down {
            return Err(invalid(format!(
                "an aura stays within {MAX_AURA_RADIUS} pixels of the feet's middle, \
                 {up} above them and {down} below"
            )));
        }
        Ok(Aura {
            at: self.at,
            alpha: self.alpha,
            glow: self.glow,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::pose::tests::png;
    use crate::{Error, From, Item, Slot};

    /// A firefly: a 4-pixel cube round its origin that bobs 2 pixels over 40 ticks, a "wing"
    /// child for `look`, floating at `anchor`.
    fn companion(anchor: [f32; 3], extra: &str) -> String {
        format!(
            r#"{{"format": 1, "id": "firefly", "slot": "companion", "texture": {{"png": "{}"}},
              "parts": [{{"name": "body", "cubes": [{{"origin": [-2, -2, -2], "size": [4, 4, 4], "uv": [0, 0]}}],
                "children": [{{"name": "wing", "cubes": [{{"origin": [-1, -2, 0], "size": [2, 0, 2], "uv": [0, 8]}}]}}]}}],
              "animations": [{{"part": "body", "motion": "bob", "axis": "y", "amplitude": 2, "period": 40}}],
              "companion": {{"anchor": {anchor:?}, "lag": 8{extra}}}}}"#,
            png(16, 16)
        )
    }

    fn refused(file: &str) -> String {
        match Item::parse(file.as_bytes()) {
            Err(Error::Invalid(e)) | Err(Error::Texture(e)) => e,
            other => panic!("read: {other:?}"),
        }
    }

    #[test]
    fn a_companion_floats_at_its_anchor_and_measures_its_reach() {
        let item =
            Item::parse(companion([-14.0, -6.0, 8.0], r#", "look": "wing""#).as_bytes()).unwrap();
        assert_eq!(item.slot, Slot::Companion);
        let c = item.companion.unwrap();
        assert_eq!(
            (c.anchor, c.lag, c.look.as_deref()),
            ([-14.0, -6.0, 8.0], 8.0, Some("wing"))
        );
        // The cube's corner at the bob's top: (2, -2 - 2, 2) from the origin.
        assert!((c.reach - 24.0_f32.sqrt()).abs() < 1e-3, "{}", c.reach);
        // Beside the player, not on the head: the name tag stays where it is.
        assert_eq!(item.top, 0.0);
        assert!(item.trail.is_none() && item.aura.is_none() && item.tag.is_none());
    }

    #[test]
    fn a_companion_keeps_clear_of_the_head_and_stays_small_and_near() {
        // 11 pixels of clearance plus its reach of 4.9 from the head's centre (0, -4, 0).
        assert!(refused(&companion([0.0, -4.0, 15.0], "")).contains("from the head"));
        assert!(Item::parse(companion([0.0, -4.0, 16.0], "").as_bytes()).is_ok());
        // At most 28 pixels from the neck.
        assert!(refused(&companion([0.0, 0.0, 29.0], "")).contains("from the neck"));
        assert!(refused(&companion([-14.0, -6.0, 8.0], r#", "look": "tail""#)).contains("tail"));
        let slow = companion([-14.0, -6.0, 8.0], "").replace(r#""lag": 8"#, r#""lag": 41"#);
        assert!(refused(&slow).contains("lag"));
        // A part 9 pixels out: past the 8 a companion may reach.
        let big = companion([-20.0, -6.0, 8.0], "").replace("[-1, -2, 0]", "[-1, -2, 7]");
        assert!(refused(&big).contains("reaches"));
        // No bone, for the item or a part.
        let boned = companion([-14.0, -6.0, 8.0], "").replace(
            r#""slot": "companion","#,
            r#""slot": "companion", "bone": "head","#,
        );
        assert!(refused(&boned).contains("no bone"));
        let missing = companion([-14.0, -6.0, 8.0], "")
            .replace(r#""slot": "companion""#, r#""slot": "head""#);
        assert!(refused(&missing).contains("only a companion"));
    }

    fn trail(sprites: u32, side: u32, block: &str, parts: &str) -> String {
        format!(
            r#"{{"format": 1, "id": "leaves", "slot": "trail", "texture": {{"png": "{}"}},
              "parts": [{parts}], "trail": {{"sprites": {sprites}, {block}}}}}"#,
            png(sprites * side, side)
        )
    }

    const FALLING: &str = r#""rate": 2, "lifetime": 30, "size": [1.5, 3], "gravity": 0.01, "drift": 1.5, "spin": 8, "fade": true"#;

    #[test]
    fn a_trail_is_a_row_of_sprites_and_its_particles_rules() {
        let item = Item::parse(trail(4, 8, FALLING, "").as_bytes()).unwrap();
        let t = item.trail.unwrap();
        assert_eq!((t.sprites, t.rate, t.lifetime), (4, 2.0, 30));
        assert_eq!(
            (t.size, t.from, t.fade, t.glow),
            ([1.5, 3.0], From::Feet, true, false)
        );
        assert_eq!((item.vertices, item.top), (0, 0.0));
        // The texture must hold exactly the sprites, square, at most 16 texels.
        let wide = trail(4, 8, FALLING, "").replace(r#""sprites": 4"#, r#""sprites": 3"#);
        assert!(refused(&wide).contains("square sprites"));
        assert!(refused(&trail(2, 32, FALLING, "")).contains("square sprites"));
        // A trail has no model.
        let part = r#"{"name": "leaf", "cubes": [{"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}]}"#;
        assert!(refused(&trail(4, 8, FALLING, part)).contains("no parts"));
        for (from, to) in [
            (r#""rate": 2"#, r#""rate": 4.5"#),
            (r#""lifetime": 30"#, r#""lifetime": 41"#),
            (r#""size": [1.5, 3]"#, r#""size": [1.5, 5]"#),
            (r#""size": [1.5, 3]"#, r#""size": [3, 1.5]"#),
            (r#""gravity": 0.01"#, r#""gravity": 0.05"#),
            (r#""spin": 8"#, r#""spin": 30"#),
        ] {
            let file = trail(4, 8, &FALLING.replace(from, to), "");
            assert!(Item::parse(file.as_bytes()).is_err(), "{to} was read");
        }
    }

    /// A ring of four leaves `radius` pixels out, `lift` pixels up, at `alpha`, `at` feet or body.
    fn aura(radius: f32, lift: f32, alpha: f32, at: &str) -> String {
        let cube = |x: f32, z: f32| {
            format!(
                r#"{{"origin": [{}, {}, {}], "size": [2, 1, 2], "uv": [0, 0]}}"#,
                x - 1.0,
                -lift,
                z - 1.0
            )
        };
        let r = radius - 1.0;
        format!(
            r#"{{"format": 1, "id": "ring", "slot": "aura", "texture": {{"png": "{}"}},
              "parts": [{{"name": "ring", "cubes": [{}, {}, {}, {}]}}],
              "animations": [{{"part": "ring", "motion": "spin", "axis": "y", "period": 80}}],
              "aura": {{"at": "{at}", "alpha": {alpha}, "glow": true}}}}"#,
            png(16, 16),
            cube(r, 0.0),
            cube(-r, 0.0),
            cube(0.0, r),
            cube(0.0, -r)
        )
    }

    #[test]
    fn an_aura_stays_faint_and_near_the_feet() {
        let item = Item::parse(aura(12.0, 1.0, 0.5, "feet").as_bytes()).unwrap();
        let a = item.aura.unwrap();
        assert_eq!((a.at, a.alpha, a.glow), (From::Feet, 0.5, true));
        // Spun about y it stays within 12.1 of the middle; tipped 45° about x it rises 8.5
        // pixels: past the 6 a feet aura may.
        assert!(
            refused(
                &aura(12.0, 1.0, 0.5, "feet")
                    .replace(r#""motion": "spin""#, r#""motion": "swing""#)
                    .replace(r#""axis": "y","#, r#""axis": "x", "amplitude": 45,"#)
            )
            .contains("within")
        );
        assert!(refused(&aura(15.0, 1.0, 0.5, "feet")).contains("within"));
        // A feet aura 6 pixels high at most; a body aura 30, fainter.
        assert!(refused(&aura(12.0, 7.0, 0.5, "feet")).contains("within"));
        assert!(refused(&aura(12.0, 1.0, 0.65, "feet")).contains("alpha"));
        assert!(Item::parse(aura(12.0, 20.0, 0.3, "body").as_bytes()).is_ok());
        assert!(refused(&aura(12.0, 20.0, 0.4, "body")).contains("alpha"));
        assert!(
            refused(
                &aura(12.0, 1.0, 0.5, "feet").replace(r#""slot": "aura""#, r#""slot": "back""#)
            )
            .contains("an aura an aura")
        );
    }
}
