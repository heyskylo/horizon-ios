//! Top menu bar for replay Studio.
//!
//! The bar exposes replay, camera-path, marker, and timeline actions that are
//! already implemented by the playback and timeline controllers.

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
    SplitClip,
}

#[derive(Clone, Copy, Debug)]
struct TopBarLayout {
    compact: bool,
    menu_x: [f32; 3],
    menu_width: [f32; 3],
    pause_x: f32,
    pause_width: f32,
    exit_x: f32,
    exit_width: f32,
}

impl TopBarLayout {
    fn new(width: f32) -> Self {
        let compact = width < 360.0;
        let menu_width = if width < 220.0 {
            [32.0, 30.0, 28.0]
        } else if width < 240.0 {
            [32.0, 48.0, 32.0]
        } else if compact {
            [34.0, 50.0, 36.0]
        } else {
            [36.0, 52.0, 40.0]
        };
        let first_x = if width < 240.0 {
            4.0
        } else if compact {
            8.0
        } else {
            76.0
        };
        let menu_x = [
            first_x,
            first_x + menu_width[0] + 4.0,
            first_x + menu_width[0] + menu_width[1] + 8.0,
        ];
        let exit_width = 46.0;
        let exit_x = (width - exit_width - 4.0).max(0.0);
        let pause_width = if width < 240.0 {
            40.0
        } else if compact {
            48.0
        } else {
            86.0
        };
        let pause_x = (exit_x - pause_width - 4.0).max(0.0);
        Self {
            compact,
            menu_x,
            menu_width,
            pause_x,
            pause_width,
            exit_x,
            exit_width,
        }
    }

    fn menu_width_for_dropdown(self, width: f32, x: f32) -> f32 {
        175.0_f32.min((width - x - 2.0).max(1.0))
    }
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
        path_locked: bool,
    ) {
        let width = batch.width;
        let layout = TopBarLayout::new(width);
        style::fill(batch, atlas, [0.0, 0.0, width, self.height], 0xEE111218);
        style::fill(
            batch,
            atlas,
            [0.0, self.height - 1.0, width, 1.0],
            0x442C2E40,
        );
        if !layout.compact {
            batch.text_styled(
                atlas,
                &Text::translate("horizon.studio.topbar.title", &[]),
                8.0,
                6.0,
                style::WHITE,
                true,
            );
        }

        for (index, (menu, full_key, short_key)) in [
            (
                TopBarMenu::File,
                "horizon.studio.topbar.file",
                "horizon.studio.topbar.file_short",
            ),
            (
                TopBarMenu::Camera,
                "horizon.studio.topbar.camera",
                "horizon.studio.topbar.camera_short",
            ),
            (
                TopBarMenu::View,
                "horizon.studio.topbar.view",
                "horizon.studio.topbar.view_short",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let x = layout.menu_x[index];
            let button_width = layout.menu_width[index];
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
                style::accent()
            } else if hovered {
                style::WHITE
            } else {
                style::GREY
            };
            batch.text_styled(
                atlas,
                &Text::translate(if width < 220.0 { short_key } else { full_key }, &[]),
                x + 6.0,
                6.0,
                color,
                true,
            );
        }

        let pause_x = layout.pause_x;
        let pause_width = layout.pause_width;
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
            &Text::translate(
                if layout.compact {
                    "horizon.studio.topbar.pause_short"
                } else {
                    "horizon.studio.topbar.game_pause_menu"
                },
                &[],
            ),
            pause_x + 6.0,
            6.0,
            if pause_hovered {
                style::WHITE
            } else {
                style::muted()
            },
            true,
        );

        let exit_x = layout.exit_x;
        let exit_width = layout.exit_width;
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
            &Text::translate("horizon.studio.topbar.exit", &[]),
            exit_x + 4.0,
            6.0,
            style::WHITE,
            true,
        );

        if let Some(menu) = self.open_menu {
            self.render_dropdown(batch, atlas, cursor, menu, path_locked, layout);
        }
    }

    fn render_dropdown(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        menu: TopBarMenu,
        path_locked: bool,
        layout: TopBarLayout,
    ) {
        let (index, items): (usize, Vec<(&str, &str)>) = match menu {
            TopBarMenu::File => (
                0,
                vec![
                    ("horizon.studio.topbar.open_recordings", ""),
                    ("horizon.studio.topbar.game_pause_menu", "P"),
                    ("horizon.studio.topbar.exit", "Esc"),
                ],
            ),
            TopBarMenu::Camera => (
                1,
                vec![
                    ("horizon.studio.topbar.toggle_freecam", "F"),
                    ("horizon.studio.topbar.reset_to_player", "Home"),
                    (
                        if path_locked {
                            "horizon.studio.topbar.unlock_freecam"
                        } else {
                            "horizon.studio.topbar.lock_to_spline"
                        },
                        "L",
                    ),
                    ("horizon.studio.topbar.add_keyframe", "K"),
                    ("horizon.studio.topbar.add_marker", "M"),
                    ("horizon.studio.topbar.clear_keyframes", ""),
                ],
            ),
            TopBarMenu::View => (
                2,
                vec![
                    ("horizon.studio.topbar.timeline_hud", "T"),
                    ("horizon.studio.topbar.split_scene", ""),
                ],
            ),
        };

        let x = layout.menu_x[index];
        let menu_width = layout.menu_width_for_dropdown(batch.width, x);
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
                if hovered {
                    style::WHITE
                } else {
                    style::muted()
                },
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
                    style::soft(),
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
        let layout = TopBarLayout::new(width);
        let (cx, cy) = cursor;
        if let Some(menu) = self.open_menu {
            let (index, count) = match menu {
                TopBarMenu::File => (0, 3),
                TopBarMenu::Camera => (1, 6),
                TopBarMenu::View => (2, 2),
            };
            let menu_x = layout.menu_x[index];
            let menu_width = layout.menu_width_for_dropdown(width, menu_x);
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
                    (TopBarMenu::View, 1) => TopBarAction::SplitClip,
                    _ => TopBarAction::None,
                };
            }
        }

        if cy <= self.height {
            if cx >= layout.exit_x && cx <= layout.exit_x + layout.exit_width {
                self.open_menu = None;
                return TopBarAction::ExitStudio;
            }
            if (layout.pause_x..=layout.pause_x + layout.pause_width).contains(&cx) {
                self.open_menu = None;
                return TopBarAction::OpenPauseMenu;
            }
            for (index, menu) in [TopBarMenu::File, TopBarMenu::Camera, TopBarMenu::View]
                .into_iter()
                .enumerate()
            {
                let x = layout.menu_x[index];
                let menu_width = layout.menu_width[index];
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
        topbar.handle_mouse_click((190.0, 10.0), 1280.0, 720.0);
        assert_eq!(
            topbar.handle_mouse_click((190.0, 47.0), 1280.0, 720.0),
            TopBarAction::SplitClip
        );
    }

    #[test]
    fn compact_topbar_keeps_menu_pause_and_exit_targets_separate() {
        for width in [220.0, 240.0, 320.0] {
            let layout = TopBarLayout::new(width);
            let menu_end = layout.menu_x[2] + layout.menu_width[2];
            assert!(menu_end <= layout.pause_x, "menus overlap at width {width}");
            assert!(layout.pause_x + layout.pause_width <= layout.exit_x);
            assert!(layout.exit_x + layout.exit_width <= width);
        }

        let width = 320.0;
        let layout = TopBarLayout::new(width);
        let mut topbar = TopBarUi::new();
        assert_eq!(
            topbar.handle_mouse_click((layout.pause_x + 2.0, 10.0), width, 720.0),
            TopBarAction::OpenPauseMenu
        );
        assert_eq!(
            topbar.handle_mouse_click((layout.exit_x + 2.0, 10.0), width, 720.0),
            TopBarAction::ExitStudio
        );
    }
}
