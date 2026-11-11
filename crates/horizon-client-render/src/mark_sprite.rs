//! Horizon's mark (`docs/media/brand/mark-dark.svg`) on the text grid, for an inline text sprite.

/// The mark's name in the HUD atlas.
pub const SPRITE: &str = "horizon:gui/mark";
const PNG: &[u8] = include_bytes!("../../horizon-client/assets/horizon/textures/gui/mark.png");

/// The mark as the HUD atlas takes an image.
pub fn image() -> std::iter::Once<(String, u32, u32, Vec<u8>)> {
    let (width, height, rgba) = crate::hud::decode_bytes(PNG).expect("Horizon's mark is PNG");
    std::iter::once((SPRITE.to_owned(), width, height, rgba))
}
