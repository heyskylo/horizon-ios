use super::{MapBanner, MapData, MapFrame};
use horizon_world::nbt::Tag;

const OPTIONAL: [&str; 6] = [
    "scale",
    "locked",
    "trackingPosition",
    "unlimitedTracking",
    "banners",
    "frames",
];

#[test]
fn default_map_fields_are_omitted_without_losing_nondefaults() {
    let mut map = MapData::fresh(0.0, 0.0, 0, "minecraft:overworld");
    map.colors[0] = 5;
    map.colors[16383] = 255;
    let initial = map.to_nbt();
    let data = initial.get("data").unwrap();
    for key in OPTIONAL {
        assert!(data.get(key).is_none(), "default {key}");
    }
    assert_eq!(data.get("xCenter"), Some(&Tag::Int(0)));
    assert_eq!(data.get("zCenter"), Some(&Tag::Int(0)));
    assert_eq!(
        data.get("dimension"),
        Some(&Tag::String("minecraft:overworld".into()))
    );
    assert_eq!(
        data.get("colors"),
        Some(&Tag::ByteArray(map.colors.clone()))
    );

    map.scale = 4;
    map.locked = true;
    map.tracking = false;
    map.unlimited = true;
    map.banners.push(MapBanner {
        pos: [7, 70, -11],
        color: "red".into(),
        name: None,
    });
    map.frames.push(MapFrame {
        pos: [-3, 80, 9],
        rotation: 270,
        entity_id: 42,
    });
    let saved = map.to_nbt();
    let data = saved.get("data").unwrap();
    for (key, expected) in [
        ("scale", 4),
        ("locked", 1),
        ("trackingPosition", 0),
        ("unlimitedTracking", 1),
    ] {
        assert_eq!(data.get(key), Some(&Tag::Byte(expected)), "{key}");
    }
    for key in ["banners", "frames"] {
        assert!(matches!(data.get(key), Some(Tag::List(list)) if list.len() == 1));
    }
    for key in ["dimension", "xCenter", "zCenter", "colors"] {
        assert_eq!(
            data.get(key),
            initial.get("data").unwrap().get(key),
            "{key}"
        );
    }

    map.scale = 0;
    map.locked = false;
    map.tracking = true;
    map.unlimited = false;
    map.banners.clear();
    map.frames.clear();
    let saved = map.to_nbt();
    for key in OPTIONAL {
        assert!(saved.get("data").unwrap().get(key).is_none(), "{key}");
    }
}

/// Sorted, typed NBT transport shared with the Java oracle for these cases.
fn canonical(tag: &Tag) -> String {
    match tag {
        Tag::Compound(c) => format!(
            "{{{}}}",
            c.iter()
                .map(|(key, value)| format!("{}:{}", key.to_string_lossy(), canonical(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Tag::List(list) => format!(
            "[{}]",
            list.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Tag::Byte(value) => format!("{value}b"),
        Tag::Int(value) => value.to_string(),
        Tag::String(value) => serde_json::to_string(&value.to_string_lossy()).unwrap(),
        Tag::ByteArray(values) => format!(
            "[B;{}]",
            values
                .iter()
                .map(|v| (*v as i8).to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        Tag::IntArray(values) => format!(
            "[I;{}]",
            values
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => panic!("unexpected tag in saved-map fixture: {tag:?}"),
    }
}

#[test]
#[ignore = "run through tools/test_map_saved_data_parity.py"]
fn saved_map_codec_fixture() {
    let cases = std::fs::read_to_string(std::env::var("HORIZON_MAP_SAVE_CASES").unwrap()).unwrap();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&cases).unwrap();
    let mut lines = Vec::new();
    for case in cases {
        let mut map = MapData::fresh(0.0, 0.0, 0, case["dimension"].as_str().unwrap());
        map.center = [
            case["x"].as_i64().unwrap() as i32,
            case["z"].as_i64().unwrap() as i32,
        ];
        map.scale = case["scale"].as_u64().unwrap() as u8;
        map.locked = case["locked"].as_bool().unwrap();
        map.tracking = case["tracking"].as_bool().unwrap();
        map.unlimited = case["unlimited"].as_bool().unwrap();
        map.colors = (0..128 * 128).map(|i| (i * 17) as u8).collect();
        if case["banner"].as_bool().unwrap() {
            map.banners.push(MapBanner {
                pos: [7, 70, -11],
                color: "red".into(),
                name: None,
            });
        }
        if case["frame"].as_bool().unwrap() {
            map.frames.push(MapFrame {
                pos: [-3, 80, 9],
                rotation: 270,
                entity_id: 42,
            });
        }
        let saved = map.to_nbt();
        lines.push(format!(
            "{} {}",
            case["name"].as_str().unwrap(),
            canonical(saved.get("data").unwrap())
        ));
    }
    let output = std::env::var("HORIZON_MAP_SAVE_OUTPUT").unwrap();
    std::fs::write(output, lines.join("\n") + "\n").unwrap();
}
