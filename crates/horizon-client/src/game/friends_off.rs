//! Play with Friends left out of the build (the `friends` feature off, as in the server build):
//! no world is shared, the pause menu has no Friends button (`friends_shown` is false, as on
//! another server) and `horizon-ctl friends` says it isn't there. game/friends.rs is the real one.
use super::{Game, Screen};
use horizon_client::{Result, hud::HudBatch};

pub const HELP: &[&str] = &[];

/// The pause menu's Friends icon is vanilla's, drawn inactive: its name for the widget tree.
pub const TITLE: &str = "Play with Friends";
pub fn title() -> String {
    horizon_client::lang::get_or("horizon.friends.title", TITLE)
}

impl Game<'_> {
    pub(super) fn friends_lan_closed(&mut self) {}

    pub(super) fn tick_friends(&mut self) {}

    /// Only reached by naming the screen (`horizon-ctl`): back to the pause menu.
    pub(super) fn draw_friends_screen(&mut self, _batch: &mut HudBatch) {
        self.screen = Screen::Paused;
    }

    pub(super) fn friends_screen_click(&mut self) {}

    pub(super) fn friends_shown(&self) -> bool {
        false
    }

    /// `horizon-ctl friends ...`.
    pub(super) fn friends_control(&mut self, words: &[&str]) -> Result<Option<serde_json::Value>> {
        if words[0] != "friends" {
            return Ok(None);
        }
        Err("this build has no Play with Friends (the `friends` feature)".into())
    }
}
