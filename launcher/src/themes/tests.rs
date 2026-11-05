use super::*;
use horizon_theme::night::{DAWN, NIGHT};

fn blend(top: u32, under: u32, alpha: f32) -> u32 {
    let channel = |shift: u32| {
        let (a, b) = ((top >> shift & 0xff) as f32, (under >> shift & 0xff) as f32);
        ((a * alpha + b * (1. - alpha)).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

#[test]
fn every_accent_reads_as_well_as_the_violet() {
    for base in [NIGHT.blocks(), DAWN.blocks()] {
        for accent in Accent::ALL
            .into_iter()
            .chain((0..720).map(|i| Accent::custom(i as f32 * 0.5)))
        {
            let p = accent.apply(base);
            let what = format!(
                "{} {}",
                accent.name(),
                if p.dark { "dark" } else { "light" }
            );
            let on_face = contrast(p.on_face, p.face);
            assert!(on_face >= 4.5, "{what}: text on the face {on_face:.2}");
            for g in [p.page, p.raised, p.button, p.panel, p.well] {
                let ring = contrast(p.accent, g);
                assert!(ring >= 3.0, "{what}: accent on {g:06x} {ring:.2}");
            }
            // Text over a selected suggestion (the launcher's theme::SELECTION).
            let selected = contrast(p.ink, blend(p.accent, p.panel, 0.22));
            assert!(selected >= 4.5, "{what}: text on a selection {selected:.2}");
            let (_, _, face) = hsl(p.face);
            assert!(
                hsl(p.face_bevel).2 > face && hsl(p.face_lip).2 < face,
                "{what}"
            );
            if let Some(want) = accent.hue() {
                let (h, _, _) = hsl(p.face);
                let off = ((h - want).rem_euclid(360.)).min((want - h).rem_euclid(360.));
                assert!(off < 3., "{what}: face hue {h:.0}, not {want}");
            }
        }
        let violet = Accent::VIOLET.apply(base);
        assert_eq!((violet.accent, violet.face), (base.accent, base.face));
    }
}

#[test]
fn colours_go_to_hsl_and_back() {
    for c in [
        0x5b3fd6, 0x9a86ff, 0x000000, 0xffffff, 0x45c27a, 0xf0b84f, 0x123456,
    ] {
        let (h, s, l) = hsl(c);
        assert_eq!(from_hsl(h, s, l), c, "{c:06x}");
    }
    assert_eq!(Accent::parse("Teal"), Some(Accent::TEAL));
    assert_eq!(Accent::parse("purple"), Some(Accent::VIOLET));
    assert_eq!(Accent::parse("chartreuse"), None);
    assert_eq!(serde_json::to_string(&Accent::AMBER).unwrap(), "\"amber\"");
}

#[test]
fn a_background_is_a_picture_kept_once() {
    let root = std::env::temp_dir().join(format!("horizon-background-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let picture = |name: &str, w: u32, h: u32| {
        let path = root.join(name);
        image::RgbImage::new(w, h).save(&path).unwrap();
        path
    };
    let dir = root.join("data");
    let png = picture("dusk.png", 160, 90);
    let first = keep_background(&dir, &png).unwrap();
    assert!(
        first.starts_with("background-") && first.ends_with(".png"),
        "{first}"
    );
    let jpg = picture("noon.jpg", 160, 90);
    let second = keep_background(&dir, &jpg).unwrap();
    assert!(second.ends_with(".jpg") && second != first, "{second}");
    assert!(!dir.join(&first).exists(), "the older one goes");
    assert_eq!(background(&dir, Some(&second)), Some(dir.join(&second)));
    assert_eq!(background(&dir, Some(&first)), None, "gone");
    assert_eq!(background(&dir, Some("../noon.jpg")), None);
    assert_eq!(background(&dir, None), None);

    let tiny = picture("tiny.png", 16, 16);
    assert!(keep_background(&dir, &tiny)
        .unwrap_err()
        .contains("too small"));
    let text = root.join("notes.txt");
    std::fs::write(&text, "hi").unwrap();
    assert!(keep_background(&dir, &text)
        .unwrap_err()
        .contains("PNG, JPEG or WebP"));
    let fake = root.join("fake.png");
    std::fs::write(&fake, "not a picture").unwrap();
    assert!(keep_background(&dir, &fake)
        .unwrap_err()
        .contains("can read"));
    assert!(
        dir.join(&second).exists(),
        "a refused file keeps the old one"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_picked_hue_is_any_degree_and_survives_settings_json() {
    assert_eq!(Accent::parse("blue"), Some(Accent::BLUE));
    assert_eq!(Accent::parse("217"), Some(Accent::custom(217.)));
    assert_eq!(Accent::custom(370.).name(), "10");
    assert_eq!(Accent::custom(212.).name(), "blue");
    assert_eq!(Accent::VIOLET.name(), "violet");
    let saved = serde_json::to_string(&Accent::custom(217.5)).unwrap();
    assert_eq!(saved, "\"217.5\"");
    assert_eq!(
        serde_json::from_str::<Accent>(&saved).unwrap(),
        Accent::custom(217.5)
    );
}

#[test]
fn a_typed_colour_is_worn_as_is_and_survives_settings_json() {
    // The mockup's swatches are the named accents.
    assert_eq!(Accent::parse("#2563EB"), Some(Accent::BLUE));
    assert_eq!(Accent::BLUE.hex(), "#2563eb");
    assert_eq!(Accent::VIOLET.hex(), "#6b4bff");
    // Any other colour keeps its exact value for the glass look, and its hue for the light one.
    let typed = Accent::parse("#ff8800").unwrap();
    assert_eq!(typed.rgb(), 0xff8800);
    assert!((typed.hue().unwrap() - 32.).abs() < 0.5);
    let saved = serde_json::to_string(&typed).unwrap();
    assert_eq!(saved, "\"#ff8800\"");
    assert_eq!(serde_json::from_str::<Accent>(&saved).unwrap(), typed);
    // Half-typed or not hex: no accent.
    for wrong in ["#ff88", "#ff88001", "#gg8800", "#"] {
        assert_eq!(Accent::parse(wrong), None, "{wrong}");
    }
    // A picked hue wears the violet's saturation and lightness.
    let (h, s, l) = hsl(Accent::custom(120.).rgb());
    let (_, vs, vl) = hsl(0x6b4bff);
    assert!((h - 120.).abs() < 1. && (s - vs).abs() < 0.02 && (l - vl).abs() < 0.02);
}
