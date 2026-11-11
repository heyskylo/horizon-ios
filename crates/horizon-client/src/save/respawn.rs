//! Personal respawn record leaves from Level, using its existing typed NBT rules.
use super::{Tag, number, string, text};

pub(super) fn read(p: &Tag) -> Option<(String, [i32; 3], f32, f32, bool)> {
    let mut value = None;
    if let Some(respawn) = p.get("respawn")
        && let Some(Tag::IntArray(pos)) = respawn.get("pos")
        && pos.len() == 3
    {
        let dimension =
            text(respawn.get("dimension")).unwrap_or_else(|| "minecraft:overworld".into());
        let forced = number(respawn.get("forced")).is_some_and(|v| v != 0);
        // Forced points, including Overworld ones, are validated through
        // their blocks on death, just like beds and respawn anchors.
        let yaw = match respawn.get("yaw") {
            Some(Tag::Float(b)) => f32::from_bits(*b),
            _ => 0.0,
        };
        let pitch = match respawn.get("pitch") {
            Some(Tag::Float(b)) => f32::from_bits(*b),
            _ => 0.0,
        };
        value = Some((dimension, [pos[0], pos[1], pos[2]], yaw, pitch, forced));
    }
    value
}

pub(super) fn write(
    (dimension, block, yaw, pitch, forced): &(String, [i32; 3], f32, f32, bool),
) -> Tag {
    let mut respawn = vec![
        ("dimension", string(dimension)),
        ("pos", Tag::IntArray(block.to_vec())),
        ("yaw", Tag::Float(yaw.to_bits())),
        ("pitch", Tag::Float(pitch.to_bits())),
    ];
    if *forced {
        respawn.push(("forced", Tag::Byte(1)));
    }
    Tag::compound(respawn)
}

#[cfg(test)]
mod tests {
    use super::super::{Level, read_dat, write_dat};
    use super::*;

    /// Added model control: old records without pitch retain the existing positive-zero default.
    #[test]
    fn personal_respawn_missing_pitch_loads_positive_zero() {
        let tag = Tag::compound(vec![(
            "respawn",
            Tag::compound(vec![
                ("dimension", string("minecraft:overworld")),
                ("pos", Tag::IntArray(vec![12, 100, 12])),
                ("yaw", Tag::Float(45f32.to_bits())),
                ("forced", Tag::Byte(1)),
            ]),
        )]);
        let mut level = Level::default();
        level.read_player(&tag);
        let (_, pos, yaw, pitch, forced) = level.respawn_elsewhere.unwrap();
        assert_eq!(pos, [12, 100, 12]);
        assert_eq!(yaw.to_bits(), 45f32.to_bits());
        assert_eq!(pitch.to_bits(), 0.0f32.to_bits());
        assert!(forced);
    }

    /// Added model/I/O control, separate from the connected runtime proof.
    #[test]
    fn personal_respawn_nonzero_pitch_survives_actual_player_file_write_and_read() {
        let root = std::env::temp_dir().join(format!(
            "horizon-personal-pitch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("player.dat");
        let original = Level {
            respawn_elsewhere: Some((
                "minecraft:overworld".into(),
                [12, 100, 12],
                45.0,
                10.0,
                true,
            )),
            ..Level::default()
        };
        write_dat(&file, original.player_tag()).unwrap();
        let tag = read_dat(&file).expect("actual written NBT player file");
        assert_eq!(
            tag.get("respawn").unwrap().get("pitch"),
            Some(&Tag::Float(10f32.to_bits()))
        );
        let mut loaded = Level::default();
        loaded.read_player(&tag);
        assert_eq!(loaded.respawn_elsewhere, original.respawn_elsewhere);
        println!("actual saved personal respawn {:?}", tag.get("respawn"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
