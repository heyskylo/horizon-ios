//! A project page's rules (docs/design/registry-v2.md): its categories, tags,
//! description and images. Shared by the registry's CI ([`super::registry`]),
//! the submission service and the index's readers.
use super::Kind;

/// At most this many categories, at least one ([`check_required`]).
pub const MAX_CATEGORIES: usize = 3;
pub const MAX_TAGS: usize = 10;
pub const MAX_TAG: usize = 24;
pub const MAX_SCREENSHOTS: usize = 8;
pub const MIN_DESCRIPTION: usize = 50;
pub const MAX_DESCRIPTION: usize = 10_000;
/// The biggest screenshot file.
pub const MAX_SCREENSHOT: u64 = 1 << 20;
/// A screenshot's smallest and biggest size, in pixels.
pub const SCREENSHOT_MIN: (u32, u32) = (320, 180);
pub const SCREENSHOT_MAX: (u32, u32) = (3840, 2160);
/// An icon is square, this many pixels a side.
pub const ICON_SIDE: std::ops::RangeInclusive<u32> = 64..=1024;

const MODS: &[&str] = &[
    "adventure",
    "decoration",
    "economy",
    "equipment",
    "food",
    "game-mechanics",
    "library",
    "magic",
    "management",
    "minigame",
    "mobs",
    "optimization",
    "social",
    "storage",
    "technology",
    "transportation",
    "utility",
    "worldgen",
];
const MODPACKS: &[&str] = &[
    "adventure",
    "challenging",
    "combat",
    "kitchen-sink",
    "lightweight",
    "magic",
    "multiplayer",
    "optimization",
    "quests",
    "technology",
];
const RESOURCE_PACKS: &[&str] = &[
    "8x",
    "16x",
    "32x",
    "48x",
    "64x",
    "128x",
    "256x",
    "512x",
    "audio",
    "blocks",
    "combat",
    "core-shaders",
    "decoration",
    "entities",
    "environment",
    "equipment",
    "fonts",
    "gui",
    "items",
    "locale",
    "models",
    "realistic",
    "simplistic",
    "themed",
    "tweaks",
    "utility",
    "vanilla-like",
];
const SHADERS: &[&str] = &[
    "cartoon",
    "fantasy",
    "realistic",
    "semi-realistic",
    "vanilla-like",
    "atmosphere",
    "bloom",
    "colored-lighting",
    "foliage",
    "path-tracing",
    "pbr",
    "reflections",
    "shadows",
    "potato",
    "low",
    "medium",
    "high",
    "screenshot",
];

/// Words a tag may not hold: no listing is about gambling (Mojang's Usage
/// Guidelines name it for every use).
const REFUSED_TAG_WORDS: &[&str] = &[
    "casino",
    "gambl",
    "slot-machine",
    "lottery",
    "betting",
    "jackpot",
    "roulette",
    "poker",
];

/// The categories a project of `kind` picks from: Modrinth's lists, without
/// `cursed`. Empty for a kind this build doesn't know.
pub fn categories(kind: &Kind) -> &'static [&'static str] {
    match kind {
        Kind::Mod | Kind::NativeMod | Kind::DataPack => MODS,
        Kind::Modpack => MODPACKS,
        Kind::ResourcePack => RESOURCE_PACKS,
        Kind::Shader | Kind::ShaderPack => SHADERS,
        Kind::Other(_) => &[],
    }
}

/// `[a-z0-9][a-z0-9-]*`, at most [`MAX_TAG`] long.
fn plain_tag(tag: &str) -> bool {
    tag.len() <= MAX_TAG
        && tag
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && tag
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Every problem with a page's description, categories and tags, each a
/// sentence a submitter can act on.
pub fn check(
    kind: &Kind,
    description: &str,
    categories: &[String],
    tags: &[String],
) -> Vec<String> {
    let mut errors = Vec::new();
    let length = description.trim().chars().count();
    if length > MAX_DESCRIPTION {
        errors.push(format!(
            "the description is {length} characters: at most {MAX_DESCRIPTION}"
        ));
    }
    let known = self::categories(kind);
    if categories.len() > MAX_CATEGORIES {
        errors.push(format!(
            "{} categories: pick at most {MAX_CATEGORIES}",
            categories.len()
        ));
    }
    for (i, c) in categories.iter().enumerate() {
        if !known.contains(&c.as_str()) {
            errors.push(format!(
                "{c:?} is not a category of a {}: {}",
                kind.name(),
                known.join(", ")
            ));
        } else if categories[..i].contains(c) {
            errors.push(format!("category {c:?} is listed twice"));
        }
    }
    if tags.len() > MAX_TAGS {
        errors.push(format!("{} tags: at most {MAX_TAGS}", tags.len()));
    }
    for (i, t) in tags.iter().enumerate() {
        if !plain_tag(t) {
            errors.push(format!(
                "tag {t:?}: lowercase letters, digits and '-', at most {MAX_TAG} characters"
            ));
        } else if REFUSED_TAG_WORDS.iter().any(|w| t.contains(w)) {
            errors.push(format!(
                "tag {t:?}: the registry lists nothing about gambling"
            ));
        } else if tags[..i].contains(t) {
            errors.push(format!("tag {t:?} is listed twice"));
        }
    }
    errors
}

/// What every listed page must have: a description of [`MIN_DESCRIPTION`] characters or more, a
/// category, and an icon kept in the registry (`icons/`), screenshots too: the site and the launcher
/// load images only from the registry's own host.
pub fn check_required(
    id: &str,
    description: &str,
    categories: &[String],
    icon: Option<&str>,
    screenshots: &[String],
) -> Vec<String> {
    let mut errors = Vec::new();
    let length = description.trim().chars().count();
    if length < MIN_DESCRIPTION {
        errors.push(format!(
            "the description is {length} characters: write at least {MIN_DESCRIPTION} (what it does, how to use it)"
        ));
    }
    if categories.is_empty() {
        errors.push(format!("pick 1 to {MAX_CATEGORIES} categories"));
    }
    match icon {
        None => errors.push("has no icon: icons/<id>.png, a square PNG".into()),
        Some(icon) if icon != format!("icons/{id}.png") => errors.push(format!(
            "its icon {icon:?} must be kept in the registry as icons/{id}.png"
        )),
        Some(_) => {}
    }
    for s in screenshots
        .iter()
        .filter(|s| !s.starts_with("screenshots/"))
    {
        errors.push(format!(
            "screenshot {s:?} must be kept in the registry, screenshots/<id>/"
        ));
    }
    errors
}

/// An image's format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Image {
    Png,
    Jpeg,
}

impl Image {
    /// The extensions its files carry.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Image::Png => &["png"],
            Image::Jpeg => &["jpg", "jpeg"],
        }
    }
}

/// The format and size in pixels of a PNG or a JPEG, read from its header;
/// None for anything else.
pub fn image_size(bytes: &[u8]) -> Option<(Image, u32, u32)> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG) && bytes.get(12..16) == Some(b"IHDR") {
        let w = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
        return Some((Image::Png, w, h));
    }
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    // JPEG: walk the segments to the first start-of-frame.
    let mut at = 2;
    loop {
        while *bytes.get(at)? == 0xff && *bytes.get(at + 1)? == 0xff {
            at += 1;
        }
        if *bytes.get(at)? != 0xff {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        let length = u16::from_be_bytes([*bytes.get(at + 2)?, *bytes.get(at + 3)?]) as usize;
        let frame = matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
        if frame {
            let h = u16::from_be_bytes([*bytes.get(at + 5)?, *bytes.get(at + 6)?]);
            let w = u16::from_be_bytes([*bytes.get(at + 7)?, *bytes.get(at + 8)?]);
            return Some((Image::Jpeg, u32::from(w), u32::from(h)));
        }
        if length < 2 {
            return None;
        }
        at += 2 + length;
    }
}

/// Checks an icon's bytes: a square PNG of [`ICON_SIDE`] pixels.
pub fn check_icon(bytes: &[u8]) -> Result<(), String> {
    match image_size(bytes) {
        Some((Image::Png, w, h)) if w == h && ICON_SIDE.contains(&w) => Ok(()),
        Some((Image::Png, w, h)) => Err(format!(
            "the icon is {w}×{h}: make it square, {} to {} pixels a side",
            ICON_SIDE.start(),
            ICON_SIDE.end()
        )),
        _ => Err("the icon must be a PNG".into()),
    }
}

/// Checks a screenshot's bytes and its file name's extension: a PNG or a
/// JPEG within [`SCREENSHOT_MIN`] and [`SCREENSHOT_MAX`], at most
/// [`MAX_SCREENSHOT`] bytes.
pub fn check_screenshot(name: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_SCREENSHOT {
        return Err(format!(
            "{name}: {} KiB, at most {} KiB",
            bytes.len() >> 10,
            MAX_SCREENSHOT >> 10
        ));
    }
    let Some((format, w, h)) = image_size(bytes) else {
        return Err(format!("{name}: not a PNG or a JPEG"));
    };
    let extension = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    if !extension.is_some_and(|e| format.extensions().contains(&e.as_str())) {
        return Err(format!(
            "{name}: its extension isn't .{}",
            format.extensions()[0]
        ));
    }
    let ((min_w, min_h), (max_w, max_h)) = (SCREENSHOT_MIN, SCREENSHOT_MAX);
    if w < min_w || h < min_h || w > max_w || h > max_h {
        return Err(format!(
            "{name}: {w}×{h} pixels, from {min_w}×{min_h} to {max_w}×{max_h}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn categories_are_the_kinds_own() {
        let ok = check(&Kind::Mod, "", &strings(&["magic", "mobs"]), &[]);
        assert!(ok.is_empty(), "{ok:?}");
        // A shader category on a mod, a repeat, and one too many.
        let wrong = check(&Kind::Mod, "", &strings(&["bloom"]), &[]);
        assert!(wrong[0].contains("not a category of a mod"), "{wrong:?}");
        let twice = check(&Kind::Shader, "", &strings(&["pbr", "pbr"]), &[]);
        assert_eq!(twice, ["category \"pbr\" is listed twice"]);
        let many = check(
            &Kind::Mod,
            "",
            &strings(&["magic", "mobs", "food", "storage"]),
            &[],
        );
        assert_eq!(many, ["4 categories: pick at most 3"]);
        assert!(categories(&Kind::Other("world".into())).is_empty());
    }

    #[test]
    fn tags_are_plain_and_never_about_gambling() {
        let ok = check(&Kind::Mod, "", &[], &strings(&["bosses", "1-21"]));
        assert!(ok.is_empty(), "{ok:?}");
        for bad in ["Bosses", "two words", "-x", "", &"a".repeat(25)] {
            assert_eq!(
                check(&Kind::Mod, "", &[], &[bad.to_owned()]).len(),
                1,
                "{bad:?}"
            );
        }
        let casino = check(&Kind::Mod, "", &[], &strings(&["casino-games"]));
        assert!(casino[0].contains("gambling"), "{casino:?}");
        let eleven: Vec<String> = (0..11).map(|i| format!("t{i}")).collect();
        assert_eq!(check(&Kind::Mod, "", &[], &eleven), ["11 tags: at most 10"]);
    }

    #[test]
    fn a_listed_page_is_complete_and_its_images_are_ours() {
        let text = "x".repeat(MIN_DESCRIPTION);
        let ok = check_required(
            "a",
            &text,
            &strings(&["mobs"]),
            Some("icons/a.png"),
            &strings(&["screenshots/a/1.png"]),
        );
        assert!(ok.is_empty(), "{ok:?}");
        let all = check_required("a", "short", &[], None, &[]);
        assert_eq!(all.len(), 3, "{all:?}");
        let far = check_required(
            "a",
            &text,
            &strings(&["mobs"]),
            Some("https://x.test/i.png"),
            &strings(&["https://x.test/1.png"]),
        );
        assert_eq!(far.len(), 2, "{far:?}");
        let other_icon = check_required("a", &text, &strings(&["mobs"]), Some("icons/b.png"), &[]);
        assert_eq!(other_icon.len(), 1, "{other_icon:?}");
    }

    #[test]
    fn a_description_has_a_ceiling() {
        let long = "x".repeat(MAX_DESCRIPTION + 1);
        assert_eq!(check(&Kind::Mod, &long, &[], &[]).len(), 1);
        assert!(check(&Kind::Mod, &"x".repeat(MAX_DESCRIPTION), &[], &[]).is_empty());
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        // SOI, an APP0 segment of 16 bytes, then a baseline SOF0.
        let mut b = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16];
        b.extend([0; 14]);
        b.extend([0xff, 0xc0, 0, 17, 8]);
        b.extend(h.to_be_bytes());
        b.extend(w.to_be_bytes());
        b.extend([3; 12]);
        b
    }

    #[test]
    fn image_headers_give_their_size() {
        assert_eq!(image_size(&png(256, 128)), Some((Image::Png, 256, 128)));
        assert_eq!(
            image_size(&jpeg(1920, 1080)),
            Some((Image::Jpeg, 1920, 1080))
        );
        assert_eq!(image_size(b"GIF89a"), None);
        assert_eq!(image_size(&[0xff, 0xd8, 0xff]), None);
    }

    #[test]
    fn icons_are_square_pngs() {
        assert!(check_icon(&png(256, 256)).is_ok());
        assert!(check_icon(&png(256, 128)).unwrap_err().contains("square"));
        assert!(check_icon(&png(32, 32)).is_err());
        assert!(check_icon(&jpeg(256, 256)).unwrap_err().contains("PNG"));
    }

    #[test]
    fn screenshots_are_bounded() {
        assert!(check_screenshot("1.jpg", &jpeg(1920, 1080)).is_ok());
        assert!(check_screenshot("1.png", &png(1280, 720)).is_ok());
        assert!(
            check_screenshot("1.png", &jpeg(1920, 1080))
                .unwrap_err()
                .contains(".jpg")
        );
        assert!(
            check_screenshot("1.png", &png(100, 100))
                .unwrap_err()
                .contains("pixels")
        );
        assert!(check_screenshot("1.png", &png(7680, 4320)).is_err());
        let mut big = png(1920, 1080);
        big.resize(MAX_SCREENSHOT as usize + 1, 0);
        assert!(check_screenshot("1.png", &big).unwrap_err().contains("KiB"));
    }
}
