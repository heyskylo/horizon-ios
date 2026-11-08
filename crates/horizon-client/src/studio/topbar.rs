//! Top menu bar for replay Studio.
//!
//! The bar exposes replay, camera-path, marker, and timeline actions that are
//! already implemented by the playback and timeline controllers.

use super::replay::ReplayDriver;
use horizon_client_render::{
    graphics_style as style,
    hud::{HudAtlas, HudBatch},
    text::Text,
    ui,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopBarMenu {
    File,
    Camera,
    View,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TopBarAction {
    None,
    OpenPauseMenu,
    ExitStudio,
    OpenRecordingsFolder,
    ResetCameraToPlayer,
    ToggleLockToPath,
    ToggleFreecam,
    AddKeyframe,
    AddMarker,
    ClearKeyframes,
    ToggleTimeline,
}

#[derive(Clone, Debug)]
pub struct TopBarUi {
    pub open_menu: Option<TopBarMenu>,
    pub height: f32,
}

impl Default for TopBarUi {
    fn default() -> Self {
        Self {
            open_menu: None,
            height: 22.0,
        }
    }
}

impl TopBarUi {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn close_dropdown(&mut self) {
        self.open_menu = None;
    }

    pub fn render(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        replay: Option<&ReplayDriver>,
        recording_name: Option<&str>,
        path_locked: bool,
    ) {
        let width = batch.width;
        style::fill(batch, atlas, [0.0, 0.0, width, self.height], 0xEE111218);
        style::fill(
            batch,
            atlas,
            [0.0, self.height - 1.0, width, 1.0],
            0x442C2E40,
        );
        batch.text_styled(
            atlas,
            &Text::translate("horizon.studio.topbar.title", &[]),
            8.0,
            6.0,
            style::WHITE,
            true,
        );

        for (menu, key, x, button_width) in [
            (TopBarMenu::File, "horizon.studio.topbar.file", 80.0, 36.0),
            (
                TopBarMenu::Camera,
                "horizon.studio.topbar.camera",
                120.0,
                52.0,
            ),
            (TopBarMenu::View, "horizon.studio.topbar.view", 176.0, 40.0),
        ] {
            let open = self.open_menu == Some(menu);
            let hovered = ui::hovered(cursor, x, 2.0, button_width, self.height - 4.0);
            if open || hovered {
                style::fill(
                    batch,
                    atlas,
                    [x, 2.0, button_width, self.height - 4.0],
                    if open { 0x446B5BFF } else { 0x22FFFFFF },
                );
            }
            let color = if open {
                style::ACCENT
            } else if hovered {
                style::WHITE
            } else {
                style::GREY
            };
            batch.text_styled(atlas, &Text::translate(key, &[]), x + 6.0, 6.0, color, true);
        }

        let pause_x = 224.0;
        let pause_width = 84.0;
        let pause_hovered = ui::hovered(cursor, pause_x, 2.0, pause_width, self.height - 4.0);
        if pause_hovered {
            style::fill(
                batch,
                atlas,
                [pause_x, 2.0, pause_width, self.height - 4.0],
                0x28FFFFFF,
            );
        }
        batch.text_styled(
            atlas,
            &Text::translate("horizon.studio.topbar.game_pause_menu", &[]),
            pause_x + 6.0,
            6.0,
            if pause_hovered {
                style::WHITE
            } else {
                style::MUTED
            },
            true,
        );

        let exit_x = width - 74.0;
        let exit_width = 68.0;
        let exit_hovered = ui::hovered(cursor, exit_x, 2.0, exit_width, self.height - 4.0);
        style::fill(
            batch,
            atlas,
            [exit_x, 2.0, exit_width, self.height - 4.0],
            if exit_hovered { 0xCC7A2424 } else { 0x66441818 },
        );
        style::ring(
            batch,
            atlas,
            [exit_x, 2.0, exit_width, self.height - 4.0],
            0x889A3838,
        );
        batch.text_styled(
            atlas,
            &Text::translate("horizon.studio.topbar.close_replay", &[]),
            exit_x + 8.0,
            6.0,
            style::WHITE,
            true,
        );

        let mut right_x = exit_x - 8.0;
        if let Some(replay) = replay {
            let pos = replay.position_ms() / 1000;
            let duration = replay.duration_ms() / 1000;
            let label = Text::literal(format!(
                "{:02}:{:02} / {:02}:{:02}",
                pos / 60,
                pos % 60,
                duration / 60,
                duration % 60
            ));
            let badge_width = atlas.font.width_text(&label) + 12.0;
            right_x -= badge_width + 8.0;
            style::fill(
                batch,
                atlas,
                [right_x, 2.0, badge_width, self.height - 4.0],
                0x22FFFFFF,
            );
            batch.text_styled(atlas, &label, right_x + 6.0, 6.0, style::MUTED, true);
        }
        if let Some(name) = recording_name {
            let clean_name = name.strip_suffix(".mcpr").unwrap_or(name);
            let label = Text::literal(clean_name);
            let badge_width = atlas.font.width_text(&label) + 12.0;
            right_x -= badge_width + 6.0;
            style::fill(
                batch,
                atlas,
                [right_x, 2.0, badge_width, self.height - 4.0],
                0x1AFFFFFF,
            );
            batch.text_styled(atlas, &label, right_x + 6.0, 6.0, style::GREY, true);
        }

        if let Some(menu) = self.open_menu {
            self.render_dropdown(batch, atlas, cursor, menu, path_locked);
        }
    }

    fn render_dropdown(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        menu: TopBarMenu,
        path_locked: bool,
    ) {
        let (x, items): (f32, Vec<(&str, &str)>) = match menu {
            TopBarMenu::File => (
                80.0,
                vec![
                    ("horizon.studio.topbar.open_recordings", ""),
                    ("horizon.studio.topbar.game_pause_menu", ""),
                    ("horizon.studio.topbar.close_replay", ""),
                ],
            ),
            TopBarMenu::Camera => (
                120.0,
                vec![
                    ("horizon.studio.topbar.toggle_freecam", ""),
                    ("horizon.studio.topbar.reset_to_player", ""),
                    (
                        if path_locked {
                            "horizon.studio.topbar.unlock_freecam"
                        } else {
                            "horizon.studio.topbar.lock_to_spline"
                        },
                        "",
                    ),
                    ("horizon.studio.topbar.add_keyframe", ""),
                    ("horizon.studio.topbar.add_marker", "M"),
                    ("horizon.studio.topbar.clear_keyframes", ""),
                ],
            ),
            TopBarMenu::View => (176.0, vec![("horizon.studio.topbar.timeline_hud", "")]),
        };

        let menu_width = 175.0;
        let item_height = 22.0;
        let menu_height = items.len() as f32 * item_height + 6.0;
        let menu_y = self.height;
        style::fill(
            batch,
            atlas,
            [x + 2.0, menu_y + 2.0, menu_width, menu_height],
            0x88000000,
        );
        style::fill(
            batch,
            atlas,
            [x, menu_y, menu_width, menu_height],
            0xF814151E,
        );
        style::ring(
            batch,
            atlas,
            [x, menu_y, menu_width, menu_height],
            0x553C3E54,
        );

        for (index, (key, shortcut)) in items.iter().enumerate() {
            let y = menu_y + 3.0 + index as f32 * item_height;
            let hovered = ui::hovered(cursor, x + 3.0, y, menu_width - 6.0, item_height);
            if hovered {
                style::fill(
                    batch,
                    atlas,
                    [x + 3.0, y, menu_width - 6.0, item_height],
                    0x336B5BFF,
                );
            }
            batch.text_styled(
                atlas,
                &Text::translate(key, &[]),
                x + 8.0,
                y + 6.0,
                if hovered { style::WHITE } else { style::MUTED },
                true,
            );
            if !shortcut.is_empty() {
                let shortcut = Text::literal(*shortcut);
                let shortcut_width = atlas.font.width_text(&shortcut);
                batch.text_styled(
                    atlas,
                    &shortcut,
                    x + menu_width - shortcut_width - 8.0,
                    y + 6.0,
                    style::SOFT,
                    true,
                );
            }
        }
    }

    pub fn handle_mouse_click(
        &mut self,
        cursor: (f32, f32),
        width: f32,
        _height: f32,
    ) -> TopBarAction {
        let (cx, cy) = cursor;
        if let Some(menu) = self.open_menu {
            let (menu_x, count) = match menu {
                TopBarMenu::File => (80.0, 3),
                TopBarMenu::Camera => (120.0, 6),
                TopBarMenu::View => (176.0, 1),
            };
            let menu_width = 175.0;
            let item_height = 22.0;
            let menu_height = count as f32 * item_height + 6.0;
            if ui::hovered(cursor, menu_x, self.height, menu_width, menu_height) {
                let index = ((cy - self.height - 3.0) / item_height).floor() as usize;
                self.open_menu = None;
                return match (menu, index) {
                    (TopBarMenu::File, 0) => TopBarAction::OpenRecordingsFolder,
                    (TopBarMenu::File, 1) => TopBarAction::OpenPauseMenu,
                    (TopBarMenu::File, 2) => TopBarAction::ExitStudio,
                    (TopBarMenu::Camera, 0) => TopBarAction::ToggleFreecam,
                    (TopBarMenu::Camera, 1) => TopBarAction::ResetCameraToPlayer,
                    (TopBarMenu::Camera, 2) => TopBarAction::ToggleLockToPath,
                    (TopBarMenu::Camera, 3) => TopBarAction::AddKeyframe,
                    (TopBarMenu::Camera, 4) => TopBarAction::AddMarker,
                    (TopBarMenu::Camera, 5) => TopBarAction::ClearKeyframes,
                    (TopBarMenu::View, 0) => TopBarAction::ToggleTimeline,
                    _ => TopBarAction::None,
                };
            }
        }

        if cy <= self.height {
            if cx >= width - 74.0 && cx <= width - 6.0 {
                self.open_menu = None;
                return TopBarAction::ExitStudio;
            }
            if (224.0..=308.0).contains(&cx) {
                self.open_menu = None;
                return TopBarAction::OpenPauseMenu;
            }
            for (menu, x, menu_width) in [
                (TopBarMenu::File, 80.0, 36.0),
                (TopBarMenu::Camera, 120.0, 52.0),
                (TopBarMenu::View, 176.0, 40.0),
            ] {
                if cx >= x && cx <= x + menu_width {
                    self.open_menu = (self.open_menu != Some(menu)).then_some(menu);
                    return TopBarAction::None;
                }
            }
            self.open_menu = None;
            return TopBarAction::None;
        }

        if self.open_menu.is_some() {
            self.open_menu = None;
        }
        TopBarAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topbar_menu_toggle_and_file_actions_work() {
        let mut topbar = TopBarUi::new();
        topbar.handle_mouse_click((90.0, 10.0), 1280.0, 720.0);
        assert_eq!(topbar.open_menu, Some(TopBarMenu::File));
        let action = topbar.handle_mouse_click((90.0, 47.0), 1280.0, 720.0);
        assert_eq!(action, TopBarAction::OpenPauseMenu);
        let action = topbar.handle_mouse_click((1280.0 - 40.0, 10.0), 1280.0, 720.0);
        assert_eq!(action, TopBarAction::ExitStudio);
    }

    #[test]
    fn camera_and_view_menus_offer_marker_and_timeline_actions() {
        let mut topbar = TopBarUi::new();
        topbar.handle_mouse_click((130.0, 10.0), 1280.0, 720.0);
        assert_eq!(
            topbar.handle_mouse_click((130.0, 124.0), 1280.0, 720.0),
            TopBarAction::AddMarker
        );
        topbar.handle_mouse_click((190.0, 10.0), 1280.0, 720.0);
        assert_eq!(
            topbar.handle_mouse_click((190.0, 35.0), 1280.0, 720.0),
            TopBarAction::ToggleTimeline
        );
    }
}
