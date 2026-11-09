//! `Hud.extractRenderState`'s subtitle rule and `Screen.isInGameUi`.
use super::Screen;

/// `Screen.isInGameUi`: the screens that keep the game behind them — the
/// container screens (`AbstractContainerScreen`) and the block editors
/// (`AbstractCommandBlockEditScreen`, structure block, jigsaw, test block) —
/// plus the two Horizon draws over `Screen::Playing`: a book (`BookViewScreen`)
/// and a sign editor (`AbstractSignEditScreen`).
pub(super) fn in_game_ui(screen: Screen, book_or_sign: bool) -> bool {
    book_or_sign
        || matches!(
            screen,
            Screen::Inventory | Screen::Creative | Screen::CommandBlock | Screen::BlockEditor
        )
}

/// Where `Hud.extractRenderState` draws the subtitles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Subtitles {
    /// Hidden with the rest of the HUD: F1 with no screen over the game.
    Hidden,
    /// In the HUD's own pass, under any screen that draws later.
    Now,
    /// Deferred (`Hud.extractDeferredSubtitles`): an in-game screen draws
    /// them from `Screen.extractBackground`, under its slots and tooltips,
    /// whether or not the HUD is hidden. Horizon draws them just before the
    /// screen, which keeps that order (only the screen's dimming goes over them).
    Deferred,
}

/// `Hud.extractRenderState`'s subtitle rule: with no in-game screen the
/// subtitles follow the HUD (`!isHidden`); with one they are deferred and
/// draw over it, which is the one HUD element F1 does not take away.
pub(super) fn subtitles(show_hud: bool, in_game_ui: bool) -> Subtitles {
    if in_game_ui {
        Subtitles::Deferred
    } else if show_hud {
        Subtitles::Now
    } else {
        Subtitles::Hidden
    }
}
