//! The HUD editor (`docs/pvp.md`): the PvP & HUD widgets on the game, each in a frame the
//! player drags anywhere (snapping to the screen's edges and middle and to the other
//! widgets), resizes with the wheel and sends back to its corner with a right click; buttons
//! for the text colour, shadow, boxes and their opacity, and Reset. Opened from the PvP & HUD
//! page or its key. Nothing here reaches the server.
use super::{Game, Screen};
use horizon_client::{
    hud::{HudBatch, rgba},
    pvp::Opt,
    pvp_hud::{self, Layout, Place, WidgetId},
    text::Text,
    ui,
};

#[derive(Default)]
pub(super) struct HudEditor {
    /// The widget held, and where the cursor took it from its top left corner.
    drag: Option<(WidgetId, f32, f32)>,
    /// The lines the dragged box snapped to: a vertical one at x, a horizontal one at y.
    guides: (Option<f32>, Option<f32>),
    /// Opened from the PvP & HUD page: Done goes back there.
    from_options: bool,
}

/// The style buttons, two to a row, then Reset and Done.
const STYLE: [Opt; 4] = [
    Opt::HudTextColour,
    Opt::HudShadow,
    Opt::HudBackground,
    Opt::HudOpacity,
];
const BUTTON_W: f32 = 150.0;
const BUTTON_H: f32 = 20.0;
const GAP: f32 = 4.0;
/// A frame's colour, and the held or hovered one's.
const FRAME: u32 = 0xB0FF_FFFF;
const FRAME_HOT: u32 = 0xFFFF_FF55;
const GUIDE: u32 = 0xFF55_FFFF;

/// The buttons' places, in the middle of the screen: style buttons, then Reset and Done.
fn buttons((w, h): (f32, f32)) -> [(f32, f32); 6] {
    let left = (w / 2.0 - BUTTON_W - GAP / 2.0).floor();
    let right = left + BUTTON_W + GAP;
    let top = (h / 2.0 - (3.0 * BUTTON_H + 2.0 * GAP) / 2.0).floor();
    let row = |i: f32| top + i * (BUTTON_H + GAP);
    [
        (left, row(0.0)),
        (right, row(0.0)),
        (left, row(1.0)),
        (right, row(1.0)),
        (left, row(2.0)),
        (right, row(2.0)),
    ]
}

impl Game<'_> {
    /// Opens the editor over the game, from the page (`from_options`) or the key.
    pub(super) fn open_hud_editor(&mut self, from_options: bool) {
        if self.player.is_none() || self.title.is_some() {
            return;
        }
        self.keys.clear();
        self.pvp.editor = Some(HudEditor {
            from_options,
            ..HudEditor::default()
        });
        self.screen = Screen::HudEditor;
        self.capture(false);
    }
    pub(super) fn close_hud_editor(&mut self) {
        let from_options = self.pvp.editor.take().is_some_and(|e| e.from_options);
        self.settings.save(&Self::options_path());
        if from_options {
            self.open_options_at(false, horizon_client::options_ui::Page::Pvp);
        } else {
            self.screen = Screen::Playing;
            self.capture(true);
        }
    }
    /// The editor key, from the game.
    pub(super) fn hud_editor_key_press(&mut self, name: &str) -> bool {
        if self.screen != Screen::Playing
            || self.settings.extras.pvp.editor_key != name
            || name == "key.keyboard.unknown"
        {
            return false;
        }
        self.open_hud_editor(false);
        self.screen == Screen::HudEditor
    }
    /// Whether the HUD draws the editor's stand-ins for widgets that have nothing to show.
    pub(super) fn hud_editing(&self) -> bool {
        self.screen == Screen::HudEditor
    }
    fn style_label(&self, opt: Opt) -> Text {
        let p = &self.settings.extras.pvp;
        let value = match opt.switch(p) {
            Some(true) => Text::translate("options.on", &[]),
            Some(false) => Text::translate("options.off", &[]),
            None => Text::literal(opt.value(p).unwrap_or_default()),
        };
        Text::option_name_value(Text::translate(opt.caption_key(), &[]), value)
    }
    /// The topmost widget under the cursor.
    fn widget_under_cursor(&self) -> Option<(WidgetId, [f32; 4])> {
        let (cx, cy) = self.cursor;
        self.pvp
            .boxes
            .iter()
            .rev()
            .copied()
            .find(|(_, [x, y, w, h])| cx >= *x && cx < x + w && cy >= *y && cy < y + h)
    }
    pub(super) fn draw_hud_editor(&mut self, batch: &mut HudBatch) {
        let atlas = &self.gui.atlas;
        let (w, h) = (batch.width, batch.height);
        let colour = |c: u32| {
            let [a, r, g, b] = c.to_be_bytes();
            rgba(r, g, b, a)
        };
        let held = self.pvp.editor.as_ref().and_then(|e| e.drag).map(|d| d.0);
        let hot = held.or(self.widget_under_cursor().map(|(id, _)| id));
        for &(id, [x, y, bw, bh]) in &self.pvp.boxes {
            let c = colour(if Some(id) == hot { FRAME_HOT } else { FRAME });
            batch.fill(atlas, x - 1.0, y - 1.0, bw + 2.0, 1.0, c);
            batch.fill(atlas, x - 1.0, y + bh, bw + 2.0, 1.0, c);
            batch.fill(atlas, x - 1.0, y, 1.0, bh, c);
            batch.fill(atlas, x + bw, y, 1.0, bh, c);
        }
        if let Some(e) = &self.pvp.editor {
            if let Some(x) = e.guides.0 {
                batch.fill(atlas, x, 0.0, 1.0, h, colour(GUIDE));
            }
            if let Some(y) = e.guides.1 {
                batch.fill(atlas, 0.0, y, w, 1.0, colour(GUIDE));
            }
        }
        let places = buttons((w, h));
        let hint = Text::translate("horizon.pvp.editor.hint", &[]);
        let hw = atlas.font.width_text(&hint);
        let top = places[0].1;
        let hx = ((w - hw) / 2.0).floor();
        batch.text_styled(atlas, &hint, hx, top - 14.0, u32::MAX, true);
        let labels = STYLE.map(|o| self.style_label(o));
        let rest = [
            Text::translate("controls.reset", &[]),
            Text::translate("gui.done", &[]),
        ];
        // Opacity does nothing without the boxes.
        let boxes = self.settings.extras.pvp.hud.background;
        for (i, (label, &(x, y))) in labels.iter().chain(&rest).zip(&places).enumerate() {
            let active = i != 3 || boxes;
            let hover = active && ui::hovered(self.cursor, x, y, BUTTON_W, BUTTON_H);
            ui::button(batch, atlas, x, y, BUTTON_W, BUTTON_H, label, active, hover);
        }
    }
    /// A press: a button, else the widget under the cursor (right button: back to its corner).
    pub(super) fn hud_editor_press(&mut self, right: bool) {
        let size = self.gui_size();
        for (i, &(x, y)) in buttons(size).iter().enumerate() {
            if right || !ui::hovered(self.cursor, x, y, BUTTON_W, BUTTON_H) {
                continue;
            }
            let back = self.has_shift_down();
            let pvp = &mut self.settings.extras.pvp;
            match i {
                3 if !pvp.hud.background => return,
                0..4 => STYLE[i].cycle(pvp, back),
                4 => pvp.layout = Layout::default(),
                _ => {
                    self.audio.play_button_click();
                    self.close_hud_editor();
                    return;
                }
            }
            self.audio.play_button_click();
            self.settings.save(&Self::options_path());
            return;
        }
        let Some((id, [x, y, _, _])) = self.widget_under_cursor() else {
            return;
        };
        if right {
            *self.settings.extras.pvp.layout.get_mut(id) = Place::default();
            self.settings.save(&Self::options_path());
        } else if let Some(e) = &mut self.pvp.editor {
            e.drag = Some((id, self.cursor.0 - x, self.cursor.1 - y));
        }
    }
    /// The cursor moved: the held widget follows, snapping to edges near it.
    pub(super) fn hud_editor_drag(&mut self) {
        let Some((id, dx, dy)) = self.pvp.editor.as_ref().and_then(|e| e.drag) else {
            return;
        };
        let Some(&(_, [_, _, w, h])) = self.pvp.boxes.iter().find(|(b, _)| *b == id) else {
            return;
        };
        let screen = self.gui_size();
        let others = || self.pvp.boxes.iter().filter(|(b, _)| *b != id);
        let xs: Vec<(f32, f32)> = others().map(|(_, r)| (r[0], r[2])).collect();
        let ys: Vec<(f32, f32)> = others().map(|(_, r)| (r[1], r[3])).collect();
        let (x, gx) = pvp_hud::snap(self.cursor.0 - dx, w, screen.0, &xs);
        let (y, gy) = pvp_hud::snap(self.cursor.1 - dy, h, screen.1, &ys);
        let at = pvp_hud::to_fractions((x, y), (w, h), screen);
        self.settings.extras.pvp.layout.get_mut(id).at = Some(at);
        if let Some(e) = &mut self.pvp.editor {
            e.guides = (gx, gy);
        }
    }
    pub(super) fn hud_editor_release(&mut self) {
        if let Some(e) = &mut self.pvp.editor
            && e.drag.take().is_some()
        {
            e.guides = (None, None);
            self.settings.save(&Self::options_path());
        }
    }
    /// The wheel over a widget resizes it by 10%.
    pub(super) fn hud_editor_scroll(&mut self, amount: f64) {
        let Some((id, _)) = self.widget_under_cursor() else {
            return;
        };
        let place = self.settings.extras.pvp.layout.get_mut(id);
        let (lo, hi) = pvp_hud::SCALES;
        place.scale = if amount > 0.0 {
            place.scale.saturating_add(10).min(hi)
        } else {
            place.scale.saturating_sub(10).max(lo)
        };
        self.settings.save(&Self::options_path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buttons_sit_in_the_middle_without_overlapping() {
        let screen = (480.0, 270.0);
        let places = buttons(screen);
        for (i, &(x, y)) in places.iter().enumerate() {
            assert!(x >= 0.0 && x + BUTTON_W <= screen.0 && y >= 0.0 && y + BUTTON_H <= screen.1);
            for &(ox, oy) in &places[i + 1..] {
                let apart = x + BUTTON_W <= ox
                    || ox + BUTTON_W <= x
                    || y + BUTTON_H <= oy
                    || oy + BUTTON_H <= y;
                assert!(apart);
            }
        }
        let (left, right) = (places[0].0, places[1].0 + BUTTON_W);
        assert!((screen.0 - right - left).abs() <= 1.0, "centred");
    }
}
