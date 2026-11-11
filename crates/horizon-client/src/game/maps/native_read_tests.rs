use super::{MapData, map_read};

#[test]
fn a_painted_picture_reads_back_as_painted() {
    let mut map = MapData::fresh(0.0, 0.0, 0, "minecraft:overworld");
    let colors: Vec<u8> = (0..16384).map(|i| (i * 7 % 256) as u8).collect();
    map.paint(&colors);
    assert_eq!(
        map_read(Some(&map), || None, |m| m.colors.clone()),
        Some(colors)
    );
}

#[test]
fn a_loaded_map_wins_over_its_saved_file() {
    let loaded = MapData::fresh(0.0, 0.0, 0, "minecraft:overworld");
    let saved = MapData::fresh(0.0, 0.0, 2, "minecraft:the_end");
    let scale = |cached: Option<&MapData>| {
        map_read(
            cached,
            || Some(MapData::fresh(0.0, 0.0, 2, "minecraft:the_end")),
            |m| m.scale,
        )
    };
    assert_eq!(scale(Some(&loaded)), Some(0));
    assert_eq!(scale(None), Some(2));
    assert_eq!(saved.dimension, "minecraft:the_end");
}

#[test]
fn a_map_neither_loaded_nor_saved_reads_as_none() {
    assert_eq!(map_read(None, || None, |m| m.locked), None);
}
