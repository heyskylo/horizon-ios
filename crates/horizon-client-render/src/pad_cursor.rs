//! The controller's menu cursor in inventories: a crosshair whose centre is
//! the point it clicks. Horizon's own art (pad_cursor.png).

/// The atlas sprite.
pub const SPRITE: &str = "horizon/pad_cursor";
/// The texture's size in texels, and the size it's drawn at in GUI pixels
/// (an item's).
pub const SIZE: u32 = 32;
pub const DRAWN: f32 = 16.0;

const PNG: &[u8] = include_bytes!("pad_cursor.png");

/// The texture as `SIZE` square RGBA texels.
pub fn pixels() -> Vec<u8> {
    decode(PNG).expect("pad_cursor.png is a 32x32 PNG")
}

fn decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    if (info.width, info.height) != (SIZE, SIZE) {
        return None;
    }
    let mut buf = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let channels = frame.color_type.samples();
    Some(
        buf.chunks(channels)
            .take((SIZE * SIZE) as usize)
            .flat_map(|p| match channels {
                4 => [p[0], p[1], p[2], p[3]],
                3 => [p[0], p[1], p[2], 255],
                2 => [p[0], p[0], p[0], p[1]],
                _ => [p[0], p[0], p[0], 255],
            })
            .collect(),
    )
}

/// Where to draw the cursor (top-left, GUI pixels) so its centre lands on
/// the GUI point `at`.
pub fn origin(at: (f32, f32)) -> (f32, f32) {
    (at.0 - DRAWN / 2.0, at.1 - DRAWN / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_is_a_crosshair_centred_on_its_point() {
        let px = pixels();
        assert_eq!(px.len(), (SIZE * SIZE * 4) as usize);
        let alpha = |x: u32, y: u32| px[((y * SIZE + x) * 4 + 3) as usize];
        let middle = (SIZE / 2 - 1)..=(SIZE / 2);
        assert!(
            middle
                .clone()
                .any(|x| middle.clone().any(|y| alpha(x, y) > 0)),
            "the clicked point sits on the crosshair"
        );
        assert_eq!(alpha(0, 0), 0, "corners are clear: a plus, not a square");
        assert_eq!(origin((100.0, 50.0)), (92.0, 42.0));
    }
}
