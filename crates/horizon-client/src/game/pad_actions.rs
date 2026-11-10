//! The controller's side of the buttons screens declare (`widgets::Action`): which pad button does
//! which job, and the glyph on the button while the pad drives. A screen only declares; nothing
//! here names one (docs/controllers.md, "Adding a screen").
use super::{Game, Screen};
use horizon_client::{
    gamepad::{Button, MenuJob},
    hud::HudBatch,
    lang,
    pad_glyph::{self, Glyph},
    widgets::{self, Action, DeclaredAction, Kind, Target},
};

/// The menu jobs that can do a declared job.
const ACTION_JOBS: [MenuJob; 3] = [MenuJob::Alternate, MenuJob::Back, MenuJob::Extra];

/// The declared button `job` presses on this screen, if any.
fn action_for_job(job: MenuJob, declared: &[DeclaredAction]) -> Option<DeclaredAction> {
    let find = |action| {
        declared
            .iter()
            .find(|d| d.action == action && d.enabled)
            .copied()
    };
    let leave = || {
        find(Action::Back)
            .or_else(|| find(Action::Cancel))
            .or_else(|| find(Action::Done))
    };
    match job {
        MenuJob::Alternate => find(Action::Apply).or_else(|| find(Action::Options)),
        MenuJob::Back => leave().or_else(|| find(Action::Exit)),
        // B is taken by leaving the screen: Y goes to the exit.
        MenuJob::Extra => leave().and_then(|_| find(Action::Exit)),
        _ => None,
    }
}

/// Where a glyph `wide` goes on a button: inside its left end (or right), centred on its height.
/// `None` when the button is too small for it or isn't all in the view.
fn glyph_at(
    [x, y, w, h]: [f32; 4],
    wide: f32,
    (view_w, view_h): (f32, f32),
    right: bool,
) -> Option<(f32, f32)> {
    const INSET: f32 = 4.0;
    let fits = w >= wide + 2.0 * INSET && h >= pad_glyph::HEIGHT;
    let shown = x >= 0.0 && y >= 0.0 && x + w <= view_w && y + h <= view_h;
    (fits && shown).then(|| {
        let at = if right {
            x + w - INSET - wide + 2.0
        } else {
            x + INSET
        };
        (at, y + ((h - 8.0) / 2.0).floor())
    })
}

impl Game<'_> {
    /// A key row waits for its key: B is Escape, which unbinds; its buttons aren't pressed. A
    /// prompt over the row has the buttons first.
    pub(super) fn pad_binding(&self) -> bool {
        !self.prompt_open()
            && self.screen == Screen::Options
            && (self.options_waiting_for_key() || self.ui.options.selected_pad_action.is_some())
    }

    /// The declared buttons the pad can do now.
    fn pad_actions(&self) -> Vec<DeclaredAction> {
        if self.pad_binding() {
            return Vec::new();
        }
        let mut declared = widgets::last_actions(self.key_screen());
        // A split guest's Quit Game does nothing.
        if self.options.split_guest && self.title.is_some() {
            declared.retain(|d| d.action != Action::Exit);
        }
        declared
    }

    /// The job `button` does here, on the target that does it. `None` while it isn't on screen,
    /// so B still falls back to Escape.
    pub(super) fn pad_action_target(&self, button: Button) -> Option<(Action, Target)> {
        let declared = action_for_job(MenuJob::of(button)?, &self.pad_actions())?;
        let [x, y, w, h] = declared.rect;
        let at = (x + w / 2.0, y + h / 2.0);
        // A button that registers no focus target (a sign's Done) is still pressed.
        let target = self
            .pad_targets()
            .into_iter()
            .find(|t| {
                let [x, y, w, h] = t.rect;
                (x..x + w).contains(&at.0) && (y..y + h).contains(&at.1)
            })
            .unwrap_or(Target {
                rect: declared.rect,
                kind: Kind::Button,
            });
        Some((declared.action, target))
    }

    /// Declared buttons show their glyph: the pad drives, and a menu is open. Focus or crosshair.
    pub(super) fn pad_action_glyphs_on(&self) -> bool {
        self.pad.in_use
            && !self.pad.menu.keyboard
            && !self.pad_keyboard_captures()
            && self.pad.menu.pos.is_some()
            && self.pad_in_menu()
    }

    /// Each pad button's glyph on the button it does, A on an exit the focus is already on: this
    /// player's glyphs in this player's view. One that doesn't fit is left out.
    fn pad_glyphs(&self, view: (f32, f32)) -> Vec<(Glyph, f32, f32)> {
        if !self.pad_action_glyphs_on() {
            return Vec::new();
        }
        let focus = self.pad.menu.focus.filter(|_| self.pad_focus_mode());
        let atlas = &self.gui.atlas;
        let glyph = |job| self.menu_glyph(job);
        let width = |glyph: Glyph| pad_glyph::width(atlas, glyph);
        let mut glyphs = Vec::new();
        // A on a text field opens the keyboard, and on a slider takes hold of it: at the right end,
        // the text stays in view.
        let confirm = glyph(MenuJob::Confirm);
        if let Some(focus) = focus.filter(|f| {
            f.kind == Kind::Field
                || matches!(f.kind, Kind::Slider { .. }) && self.pad.menu.slider_hold.is_none()
        }) && let Some((x, y)) = glyph_at(focus.rect, width(confirm), view, true)
        {
            glyphs.push((confirm, x, y));
        }
        let declared = self.pad_actions();
        for job in ACTION_JOBS {
            let Some(d) = action_for_job(job, &declared) else {
                continue;
            };
            let shown = if d.action == Action::Exit && focus.is_some_and(|f| f.rect == d.rect) {
                confirm
            } else {
                glyph(job)
            };
            if let Some((x, y)) = glyph_at(d.rect, width(shown), view, false) {
                glyphs.push((shown, x, y));
            }
        }
        glyphs
    }

    pub(super) fn draw_pad_action_glyphs(&self, batch: &mut HudBatch) {
        let atlas = &self.gui.atlas;
        for (glyph, x, y) in self.pad_glyphs((batch.width, batch.height)) {
            pad_glyph::draw(batch, atlas, glyph, x, y);
        }
    }

    /// `pad status`: the glyphs drawn now and where: the part each is for, its label, its text's
    /// place, what it covers, and its sprite (none: the label on a key).
    pub(super) fn pad_glyphs_status(&self) -> Vec<serde_json::Value> {
        let view = self.gui_scale().map_or((0.0, 0.0), |b| (b.width, b.height));
        let atlas = &self.gui.atlas;
        self.pad_glyphs(view)
            .iter()
            .map(|&(glyph, x, y)| {
                serde_json::json!({
                    "button": format!("{:?}", glyph.part()),
                    "label": glyph.label(),
                    "at": [x, y],
                    "rect": pad_glyph::rect(atlas, glyph, x, y),
                    "sprite": pad_glyph::art(atlas, glyph),
                })
            })
            .collect()
    }

    /// `pad status`: what this screen declared.
    pub(super) fn pad_actions_status(&self) -> Vec<serde_json::Value> {
        self.pad_actions()
            .iter()
            .map(|d| {
                serde_json::json!({
                    "action": format!("{:?}", d.action),
                    "rect": d.rect,
                    "enabled": d.enabled,
                })
            })
            .collect()
    }

    /// `pad status`: buttons labelled Done, Back or Cancel that didn't declare their job.
    pub(super) fn pad_undeclared_buttons(&self) -> Vec<String> {
        let names = [
            ("gui.done", "Done"),
            ("gui.back", "Back"),
            ("gui.cancel", "Cancel"),
        ]
        .map(|(key, fallback)| lang::get_or(key, fallback));
        let declared = widgets::last_actions(self.key_screen());
        widgets::last_labelled()
            .into_iter()
            .filter(|w| names.contains(&w.label))
            .filter(|w| !declared.iter().any(|d| d.rect == [w.x, w.y, w.w, w.h]))
            .map(|w| w.label)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(action: Action, enabled: bool) -> DeclaredAction {
        DeclaredAction {
            action,
            rect: [0.0, 0.0, 10.0, 10.0],
            enabled,
        }
    }

    fn does(button: Button, on: &[DeclaredAction]) -> Option<Action> {
        action_for_job(MenuJob::of(button)?, on).map(|d| d.action)
    }

    #[test]
    fn b_leaves_the_screen_back_first() {
        let on = [
            declared(Action::Done, true),
            declared(Action::Cancel, true),
            declared(Action::Back, true),
        ];
        assert_eq!(does(Button::East, &on), Some(Action::Back));
        assert_eq!(does(Button::East, &on[..2]), Some(Action::Cancel));
        assert_eq!(does(Button::East, &on[..1]), Some(Action::Done));
    }

    #[test]
    fn a_disabled_button_does_nothing() {
        let on = [
            declared(Action::Apply, false),
            declared(Action::Done, false),
        ];
        assert_eq!(does(Button::West, &on), None);
        assert_eq!(does(Button::East, &on), None);
    }

    #[test]
    fn x_applies_or_opens_the_options() {
        let on = [
            declared(Action::Options, true),
            declared(Action::Apply, true),
        ];
        assert_eq!(does(Button::West, &on), Some(Action::Apply));
        assert_eq!(does(Button::West, &on[..1]), Some(Action::Options));
    }

    #[test]
    fn the_exit_is_b_with_nothing_to_leave_by_else_y() {
        let title = [declared(Action::Exit, true)];
        assert_eq!(does(Button::East, &title), Some(Action::Exit));
        assert_eq!(does(Button::North, &title), None);
        let pause = [declared(Action::Back, true), declared(Action::Exit, true)];
        assert_eq!(does(Button::East, &pause), Some(Action::Back));
        assert_eq!(does(Button::North, &pause), Some(Action::Exit));
    }

    #[test]
    fn a_face_glyph_on_a_button_stays_clear_of_its_label() {
        let (x, y) = glyph_at(
            [10.0, 20.0, 150.0, 20.0],
            pad_glyph::FACE_WIDTH,
            (640.0, 480.0),
            false,
        )
        .unwrap();
        // Vertically inside the button, ending before the label's left (its x + 2 + room).
        assert_eq!((y - 2.0, y - 2.0 + pad_glyph::HEIGHT), (24.0, 36.0));
        widgets::set_action_glyphs(true);
        let label_left = 10.0 + 2.0 + widgets::action_glyph_room();
        widgets::set_action_glyphs(false);
        assert!(x - 1.0 + pad_glyph::FACE_WIDTH < label_left);
    }

    #[test]
    fn a_glyph_goes_on_a_button_it_fits_in_the_view() {
        let view = (320.0, 240.0);
        let button = [10.0, 20.0, 150.0, 20.0];
        assert_eq!(glyph_at(button, 10.0, view, false), Some((14.0, 26.0)));
        // A text field's is at its right end.
        assert_eq!(glyph_at(button, 10.0, view, true), Some((148.0, 26.0)));
        // Too narrow, too short, running off a small split view.
        assert_eq!(glyph_at([10.0, 20.0, 15.0, 20.0], 10.0, view, false), None);
        assert_eq!(glyph_at([10.0, 20.0, 150.0, 8.0], 10.0, view, false), None);
        // Art as tall as the row fits; a row any shorter would clip it.
        assert_eq!(
            glyph_at([10.0, 20.0, 150.0, 12.0], 12.0, view, false),
            Some((14.0, 22.0))
        );
        assert_eq!(glyph_at([10.0, 20.0, 150.0, 11.0], 12.0, view, false), None);
        assert_eq!(glyph_at(button, 10.0, (120.0, 240.0), false), None);
    }

    #[test]
    fn a_screen_that_declares_nothing_keeps_its_buttons() {
        for job in ACTION_JOBS {
            assert_eq!(does(job.button(), &[]), None);
        }
    }
}
