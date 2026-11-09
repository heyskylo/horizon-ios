//! Horizon's look, given to the engine's extension points at startup (docs/layers.md): the
//! windows' icon. Its own module so it can leave the engine crate as a pure file move, for the
//! official executable to call.

/// Give the engine Horizon's look; call it once, before the first window opens.
pub(crate) fn give() {
    horizon_client::window::set_icon_png(horizon_theme::ICON_PNG);
}
