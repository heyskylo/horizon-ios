//! The player's own look, on top of Dark, Light or System: an accent colour and a picture behind
//! Home.
//!
//! An accent is a hue, and the player may pick any of them; seven hues have names, as shortcuts.
//! Horizon's violet is the default. Another hue turns the violet tokens (`horizon_theme::Blocks`'
//! accent and the main action's face, bevel and lip) to it, keeping their saturation and
//! lightness, then darkens or lightens each just enough to read as well as the violet does: white
//! on the face at 4.5:1, the accent at 3:1 on every background.
use horizon_theme::night::{Night, SkyArt};
use horizon_theme::{contrast, Blocks};
use std::path::{Path, PathBuf};

/// An accent: a hue in degrees, or none for Horizon's violet, whose tokens are the palette's own
/// and stay untouched; and for a colour typed as hex, that colour (the glass look wears it as
/// is, the light look takes its hue).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Accent {
    hue: Option<f32>,
    rgb: Option<u32>,
}

/// The accent a launcher theme brings: a hue in degrees (First Light) or one exact colour (Nord's
/// frost).
#[derive(Clone, Copy, Debug)]
pub enum ThemeAccent {
    Hue(f32),
    Colour(u32),
}

impl ThemeAccent {
    pub fn accent(self) -> Accent {
        match self {
            Self::Hue(hue) => Accent::custom(hue),
            Self::Colour(rgb) => Accent::from_rgb(rgb),
        }
    }
}

/// A launcher theme: free for everyone in Settings › Appearance, never sold or claimed. Each
/// has a sky (six stops, top to bottom, behind the light appearance) and the accent it brings.
/// A theme that brings a whole palette carries its dark and light [`Night`] and paints every
/// surface with it; the other themes only change the sky.
pub struct Theme {
    /// Its name in `Settings::theme`.
    pub id: &'static str,
    /// Its name in Settings, in English (through `tr`).
    pub name: &'static str,
    pub sky: [u32; 6],
    pub accent: ThemeAccent,
    /// The theme's own palette (dark, light), for a theme that is more than a sky.
    pub palette: Option<(&'static Night, &'static Night)>,
}

/// Nord's polar night and snow storm, with the frost accent.
pub const NORD_DARK: Night = Night {
    sky: [0x2e3440, 0x2e3440, 0x313845, 0x353d4b, 0x3b4252, 0x434c5e],
    page: 0x2e3440,
    side: 0x292e39,
    card: 0x3b4252,
    card_alpha: 0.88,
    card_hi: 0x434c5e,
    card_hi_alpha: 0.92,
    stroke: 0xd8dee9,
    stroke_alpha: 0.10,
    rule_alpha: 0.10,
    ink: 0xeceff4,
    ink_soft: 0xd8dee9,
    muted: 0xb6bfca,
    accent: 0x88c0d0,
    accent_hi: 0x9fd3d3,
    on_accent: 0x2e3440,
    success: 0xafc99a,
    warn: 0xebcb8b,
    danger: 0xeeb3b8,
    dark: true,
    art: SkyArt {
        sun: [0x8fbcbb, 0x88c0d0, 0x5e81ac],
        far: 0x3b4252,
        near: 0x272c36,
        cloud: [0x3b4252, 0x434c5e, 0x4c566a],
        lit: [0x4c566a, 0x5e6a7d, 0x81a1c1],
        shade: 0x272c36,
        star: 0xeceff4,
    },
};

pub const NORD_LIGHT: Night = Night {
    sky: [0xeceff4, 0xeceff4, 0xe5e9f0, 0xe5e9f0, 0xd8dee9, 0xc8d2e0],
    page: 0xeceff4,
    side: 0xe5e9f0,
    card: 0xf8f9fb,
    card_alpha: 0.90,
    card_hi: 0xffffff,
    card_hi_alpha: 0.95,
    stroke: 0x4c566a,
    stroke_alpha: 0.14,
    rule_alpha: 0.10,
    ink: 0x2e3440,
    ink_soft: 0x434c5e,
    muted: 0x536070,
    accent: 0x4c6a92,
    accent_hi: 0x4c6a92,
    on_accent: 0xffffff,
    success: 0x3f6b2e,
    warn: 0x7a5d00,
    danger: 0xa8323f,
    dark: false,
    art: SkyArt {
        sun: [0xffffff, 0xeceff4, 0xd8dee9],
        far: 0xd8dee9,
        near: 0xc8d2e0,
        cloud: [0xf8f9fb, 0xeceff4, 0xe5e9f0],
        lit: [0xffffff; 3],
        shade: 0xd8dee9,
        star: 0xffffff,
    },
};

/// First Light: the sky just before sunrise, with a harbour skyline.
pub const FIRST_LIGHT: Theme = Theme {
    id: "first_light",
    name: "First Light",
    sky: [0x0b1030, 0x1d2a63, 0x3d4c94, 0x7f78c0, 0xe2a0b0, 0xffd6a0],
    accent: ThemeAccent::Hue(246.),
    palette: None,
};

/// Nord: the Nord palette's polar night, snow storm and frost.
pub const NORD: Theme = Theme {
    id: "nord",
    name: "Nord",
    sky: NORD_DARK.sky,
    accent: ThemeAccent::Colour(0x88c0d0),
    palette: Some((&NORD_DARK, &NORD_LIGHT)),
};

/// The launcher themes past Horizon's own sky.
pub const THEMES: [Theme; 2] = [FIRST_LIGHT, NORD];

/// The theme named `id` (`Settings::theme`); None for Horizon's own sky or a name this launcher
/// doesn't know.
pub fn theme(id: Option<&str>) -> Option<&'static Theme> {
    THEMES.iter().find(|t| Some(t.id) == id)
}

/// The glass look's colour for each of [`Accent::ALL`]: the mockup's swatches.
pub const SWATCHES: [u32; 7] = [
    0x6b4bff, 0x2563eb, 0x0f766e, 0x15803d, 0xc2410c, 0xdc2626, 0xbe185d,
];

/// [`Accent::ALL`]'s names, in its order.
const NAMES: [&str; 7] = ["violet", "blue", "teal", "green", "amber", "red", "pink"];

impl Accent {
    pub const VIOLET: Accent = Accent {
        hue: None,
        rgb: None,
    };
    pub const BLUE: Accent = Accent {
        hue: Some(212.),
        rgb: None,
    };
    pub const TEAL: Accent = Accent {
        hue: Some(176.),
        rgb: None,
    };
    pub const GREEN: Accent = Accent {
        hue: Some(140.),
        rgb: None,
    };
    pub const AMBER: Accent = Accent {
        hue: Some(36.),
        rgb: None,
    };
    pub const RED: Accent = Accent {
        hue: Some(356.),
        rgb: None,
    };
    pub const PINK: Accent = Accent {
        hue: Some(322.),
        rgb: None,
    };

    pub const ALL: [Accent; 7] = [
        Self::VIOLET,
        Self::BLUE,
        Self::TEAL,
        Self::GREEN,
        Self::AMBER,
        Self::RED,
        Self::PINK,
    ];

    /// Any hue the player picks.
    pub fn custom(hue: f32) -> Self {
        Self {
            hue: Some(hue.rem_euclid(360.)),
            rgb: None,
        }
    }

    /// A colour (`0xrrggbb`): one of the swatches is its named accent.
    pub fn from_rgb(rgb: u32) -> Self {
        let rgb = rgb & 0xff_ffff;
        match SWATCHES.iter().position(|&c| c == rgb) {
            Some(i) => Self::ALL[i],
            None => Self {
                hue: Some(hsl(rgb).0),
                rgb: Some(rgb),
            },
        }
    }

    /// The colour the glass look wears: the one typed, a named accent's swatch, else the hue at
    /// the violet's saturation and lightness.
    pub fn rgb(self) -> u32 {
        if let Some(rgb) = self.rgb {
            return rgb;
        }
        match (Self::ALL.iter().position(|a| *a == self), self.hue) {
            (Some(i), _) => SWATCHES[i],
            (None, Some(hue)) => {
                let (_, s, l) = hsl(SWATCHES[0]);
                from_hsl(hue, s, l)
            }
            (None, None) => SWATCHES[0],
        }
    }

    /// [`rgb`](Self::rgb) as `#rrggbb`.
    pub fn hex(self) -> String {
        format!("#{:06x}", self.rgb())
    }

    /// Its name in `settings.json` and on the command line: a named hue's, a typed colour's
    /// `#rrggbb`, else its degrees.
    pub fn name(self) -> String {
        if self.rgb.is_some() {
            return self.hex();
        }
        let named = Self::ALL
            .iter()
            .position(|a| *a == self)
            .map(|i| NAMES[i].to_owned());
        named.unwrap_or_else(|| match self.hue {
            None => "violet".to_owned(),
            Some(hue) => format!("{hue}"),
        })
    }

    /// A name, `#rrggbb`, or a hue in degrees.
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        if let Some(hex) = name.strip_prefix('#') {
            return (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| u32::from_str_radix(hex, 16).ok())
                .flatten()
                .map(Self::from_rgb);
        }
        let name = if name == "purple" || name == "default" {
            "violet"
        } else {
            &name
        };
        if let Some(i) = NAMES.iter().position(|n| *n == name) {
            return Some(Self::ALL[i]);
        }
        name.parse::<f32>()
            .ok()
            .filter(|hue| hue.is_finite())
            .map(Self::custom)
    }

    /// Its hue in degrees; None for the violet, whose tokens stay as they are.
    pub fn hue(self) -> Option<f32> {
        self.hue
    }

    /// `palette` with this accent.
    pub fn apply(self, mut p: Blocks) -> Blocks {
        let Some(hue) = self.hue else {
            return p;
        };
        let turn = |c: u32| {
            let (_, s, l) = hsl(c);
            (hue, s, l)
        };
        // The face holds its label: white on a dark face, Nord's ink on a light one. Move the
        // face away from the label until the two read.
        let step = if contrast(p.on_face, 0x000000) < contrast(p.on_face, 0xffffff) {
            1.
        } else {
            -1.
        };
        let face = fit(turn(p.face), |c| contrast(p.on_face, c) >= 4.5, step);
        let (_, _, face_l) = hsl(face);
        let (h, s, l) = turn(p.face_bevel);
        p.face_bevel = from_hsl(h, s, l.max(face_l + 0.08).min(0.92));
        let (h, s, l) = turn(p.face_lip);
        p.face_lip = from_hsl(h, s, l.min(face_l - 0.12).max(0.05));
        p.face = face;
        // The accent marks things on every background: lighter on dark, darker on light.
        let grounds = [p.page, p.raised, p.button, p.panel, p.well];
        let reads = |c: u32| grounds.iter().all(|&g| contrast(c, g) >= 3.2);
        p.accent = fit(turn(p.accent), reads, if p.dark { 1. } else { -1. });
        p
    }
}

impl TryFrom<String> for Accent {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::parse(&name).ok_or_else(|| format!("{name}: a colour name or a hue in degrees"))
    }
}

impl From<Accent> for String {
    fn from(accent: Accent) -> Self {
        accent.name()
    }
}

/// The colour the hue picker shows for `hue`: a strip of them, at one saturation and lightness.
pub fn hue_colour(hue: f32) -> u32 {
    from_hsl(hue.rem_euclid(360.), 0.62, 0.55)
}

/// The colour's hue (degrees), saturation and lightness (0 to 1).
fn hsl(c: u32) -> (f32, f32, f32) {
    let [r, g, b] = [16, 8, 0].map(|s| ((c >> s) & 255) as f32 / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.;
    if max == min {
        return (0., 0., l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2. - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6. } else { 0. }
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    (h * 60., s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> u32 {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - ((h / 60.) % 2. - 1.).abs());
    let m = l - c / 2.;
    let (r, g, b) = match (h.rem_euclid(360.) / 60.) as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let byte = |v: f32| ((v + m).clamp(0., 1.) * 255.).round() as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// The colour, made lighter (`step` 1) or darker (-1) a little at a time until `good` holds.
fn fit((h, s, l): (f32, f32, f32), good: impl Fn(u32) -> bool, step: f32) -> u32 {
    let mut l = l;
    for _ in 0..100 {
        let c = from_hsl(h, s, l);
        if good(c) || !(0.0..=1.0).contains(&l) {
            return c;
        }
        l += step * 0.01;
    }
    from_hsl(h, s, l.clamp(0., 1.))
}

/// The pictures Home can show: what the decoders read.
const PICTURES: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
/// A picture above this is refused: a screen's worth is far less.
const MAX_BYTES: u64 = 40 << 20;

/// Keeps `file` as Home's background in `dir` (`background-<hash>.<ext>`: a new picture gets a
/// new name, so the window doesn't show the one it cached), after checking it is a picture, and
/// returns the kept file's name. An older background goes.
pub fn keep_background(dir: &Path, file: &Path) -> Result<String, String> {
    let shown = file.display();
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| PICTURES.contains(&e.as_str()))
        .ok_or_else(|| format!("{shown}: a PNG, JPEG or WebP picture, please."))?;
    let size = std::fs::metadata(file)
        .map_err(|e| format!("{shown}: {e}"))?
        .len();
    if size > MAX_BYTES {
        return Err(format!("{shown} is over 40 MB."));
    }
    let bytes = std::fs::read(file).map_err(|e| format!("{shown}: {e}"))?;
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .ok_or_else(|| format!("{shown} isn't a picture the launcher can read."))?;
    if w < 64 || h < 64 {
        return Err(format!("{shown} is {w}×{h}: too small to fill Home."));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let hash = crate::sources::sha1_hex(&bytes);
    let name = format!("background-{}.{ext}", &hash[..12]);
    std::fs::write(dir.join(&name), &bytes).map_err(|e| format!("{name}: {e}"))?;
    for old in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let other = old.file_name().to_string_lossy().into_owned();
        if other.starts_with("background-") && other != name {
            let _ = std::fs::remove_file(old.path());
        }
    }
    Ok(name)
}

/// Home's background, if the player keeps one and it is still there.
pub fn background(dir: &Path, name: Option<&str>) -> Option<PathBuf> {
    let name = name.filter(|n| n.starts_with("background-") && !n.contains(['/', '\\']))?;
    Some(dir.join(name)).filter(|p| p.is_file())
}

#[cfg(test)]
#[path = "themes/tests.rs"]
mod tests;
