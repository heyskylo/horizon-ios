//! The official Recipes viewer's artwork from Just Enough Items, under MIT (`jei_recipes/LICENSE.txt`,
//! THIRD_PARTY_NOTICES.md): `HudAtlas` loads each as the sprite `horizon/jei/<name>`.

/// Each sprite's name and its PNG.
pub(crate) const SPRITES: &[(&str, &[u8])] = &[
    (
        "button_enabled_v2",
        include_bytes!("jei_recipes/button_enabled_v2.png"),
    ),
    (
        "button_disabled_v2",
        include_bytes!("jei_recipes/button_disabled_v2.png"),
    ),
    (
        "button_highlight_v2",
        include_bytes!("jei_recipes/button_highlight_v2.png"),
    ),
    (
        "icons/arrow_previous_v2",
        include_bytes!("jei_recipes/icons/arrow_previous_v2.png"),
    ),
    (
        "icons/arrow_next_v2",
        include_bytes!("jei_recipes/icons/arrow_next_v2.png"),
    ),
    (
        "search_background_v2",
        include_bytes!("jei_recipes/search_background_v2.png"),
    ),
];
