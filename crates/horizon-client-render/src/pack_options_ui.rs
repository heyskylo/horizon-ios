//! The Pack Options page (#11486): the config entries of the selected resource packs' Polytone
//! `config_entries` (wiki "Polytone Configs"), in the frame of Horizon's settings screens
//! (`settings_shell`). Numbers are sliders, texts cycle, booleans switch; the entries sit in
//! their `section`s, named by the packs' own translations. Leaving with Done keeps what changed,
//! and the packs load again with it.
use crate::widgets::Action as Declared;
use crate::{
    graphics_style as style,
    hud::{HudAtlas, HudBatch},
    lang,
    settings_shell::{self as shell, Columns, Line, Lines},
    text::Text,
    ui,
};
use horizon_polytone::pack::{ConfigKind, ConfigValue, Configs};

/// What a click asks of the options screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ButtonClick,
    SliderDrag,
    /// Done: leave, keeping what changed.
    Done,
}

/// The page's state: the entries being edited, how they are listed, and the pointer's hold.
#[derive(Default)]
pub struct PackOptionsUi {
    /// The selected packs' entries and the values being picked.
    pub configs: Configs,
    /// Entry ids, by the row index the list uses.
    ids: Vec<String>,
    /// The sections in order: the `section` id (none for entries without one) and its rows.
    sections: Vec<(Option<String>, Vec<u16>)>,
    scroll: ui::Scroll,
    dragging: Option<u16>,
    /// A value changed since the page opened.
    pub changed: bool,
}

/// The presets' row (and its section): past every entry's index.
const PRESETS: u16 = u16::MAX;

/// A translation, or `fallback` where no language has the key.
fn translated(key: &str, fallback: impl FnOnce() -> String) -> String {
    if lang::has(key) {
        lang::get(key)
    } else {
        fallback()
    }
}

/// `namespace:path` split, `minecraft` when there is no namespace.
fn split(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("minecraft", id))
}

/// A name for an id without a translation: `fog_density` is "Fog density".
fn readable(path: &str) -> String {
    let words = path.rsplit('/').next().unwrap_or(path).replace('_', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

impl PackOptionsUi {
    /// The page opens on `configs`, the selected packs' entries with the player's values.
    pub fn open(configs: Configs) -> Self {
        let mut ids: Vec<(String, Option<String>, i64)> = configs
            .entries()
            .iter()
            .map(|(id, e)| (id.clone(), e.section.clone(), e.display_order))
            .collect();
        ids.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
        // A section's place: the smallest `section_order` among its entries, else its name.
        let mut sections: Vec<(Option<String>, Option<i64>, Vec<u16>)> = Vec::new();
        for (row, (id, section, _)) in ids.iter().enumerate() {
            let order = configs.entries()[id].section_order;
            match sections.iter_mut().find(|s| &s.0 == section) {
                Some(s) => {
                    s.1 = match (s.1, order) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                    s.2.push(row as u16);
                }
                None => sections.push((section.clone(), order, vec![row as u16])),
            }
        }
        sections.sort_by(|a, b| match (a.1, b.1) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.cmp(&b.0),
        });
        Self {
            configs,
            ids: ids.into_iter().map(|(id, ..)| id).collect(),
            sections: sections.into_iter().map(|(s, _, rows)| (s, rows)).collect(),
            ..Self::default()
        }
    }

    fn layout(&self, w: f32, h: f32) -> (Columns, Vec<Line<u16, u16>>, f32) {
        let cols = Columns::of(w, h);
        let mut lines = Lines::new();
        // The pack-wide presets first, when the entries have some.
        if !self.configs.presets().is_empty() {
            lines.titled(PRESETS, vec![PRESETS]);
        }
        for (i, (_, rows)) in self.sections.iter().enumerate() {
            lines.titled(i as u16, rows.clone());
        }
        let (lines, content) = lines.finish();
        (cols, lines, content)
    }

    /// The namespace the presets' translations are under: of the first entry with presets.
    fn presets_namespace(&self) -> &str {
        self.configs
            .entries()
            .iter()
            .find(|(_, e)| !e.presets.is_empty())
            .map_or("minecraft", |(id, _)| split(id).0)
    }

    /// The pack-wide preset every entry's value matches, if one does.
    fn current_preset(&self) -> Option<String> {
        self.configs.presets().into_iter().find(|preset| {
            self.configs
                .entries()
                .iter()
                .all(|(id, e)| match e.presets.get(preset) {
                    Some(value) => e.accept(value).as_ref() == self.configs.get(id),
                    None => true,
                })
        })
    }

    /// A preset's label (`<namespace>.presets.<name>`), or the label of none (`.none`).
    fn preset_label(&self, preset: Option<&str>) -> String {
        let namespace = self.presets_namespace();
        match preset {
            Some(name) => translated(&format!("{namespace}.presets.{name}"), || readable(name)),
            None => translated(&format!("{namespace}.presets.none"), || {
                lang::get("horizon.pack.options.presets.custom")
            }),
        }
    }

    /// The next pack-wide preset after the current one (`back`: before it), applied.
    fn cycle_preset(&mut self, back: bool) {
        let presets = self.configs.presets();
        if presets.is_empty() {
            return;
        }
        let n = presets.len();
        let next = match self
            .current_preset()
            .and_then(|c| presets.iter().position(|p| *p == c))
        {
            Some(i) if back => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
            None if back => n - 1,
            None => 0,
        };
        let before = self.configs.clone();
        self.configs.apply_preset(&presets[next]);
        if self.configs != before {
            self.changed = true;
        }
    }

    /// A section's heading: `config.<namespace>.section.<id>`, by its first entry's namespace.
    fn section_name(&self, section: u16) -> String {
        if section == PRESETS {
            let key = format!("{}.presets", self.presets_namespace());
            return translated(&key, || lang::get("horizon.pack.options.presets"));
        }
        let Some((id, rows)) = self.sections.get(usize::from(section)) else {
            return String::new();
        };
        let Some(id) = id else {
            return lang::get("horizon.pack.options.general");
        };
        let namespace = rows
            .first()
            .and_then(|&r| self.ids.get(usize::from(r)))
            .map_or("minecraft", |entry| split(entry).0);
        translated(&format!("config.{namespace}.section.{id}"), || readable(id))
    }

    /// An entry's name (`config.<namespace>.<name>`) and its value's label.
    fn parts(&self, row: u16) -> (String, Option<String>) {
        if row == PRESETS {
            let label = self.preset_label(self.current_preset().as_deref());
            return (self.section_name(PRESETS), Some(label));
        }
        let Some(id) = self.ids.get(usize::from(row)) else {
            return (String::new(), None);
        };
        let (namespace, path) = split(id);
        let name = translated(&format!("config.{namespace}.{path}"), || readable(path));
        let entry = &self.configs.entries()[id];
        let value = match self.configs.get(id) {
            Some(ConfigValue::Number(n)) => {
                let step = match entry.kind {
                    ConfigKind::Number { step, .. } => step,
                    _ => 1.0,
                };
                let text = if step >= 1.0 && step.fract() == 0.0 {
                    format!("{n:.0}")
                } else {
                    format!("{n:.2}")
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_owned()
                };
                Some(text)
            }
            Some(ConfigValue::Text(t)) => Some(t.clone()),
            Some(ConfigValue::Bool(_)) | None => None,
        };
        let value = value.map(|v| match &entry.value_translation {
            Some(key) if lang::has(key) => lang::tr(key, &[&v]),
            _ => v,
        });
        (name, value)
    }

    /// The tooltip (`config.<namespace>.<name>.tooltip`) and the performance hint.
    fn description(&self, row: u16) -> String {
        let Some(id) = self.ids.get(usize::from(row)) else {
            return String::new();
        };
        let (namespace, path) = split(id);
        let key = format!("config.{namespace}.{path}.tooltip");
        let mut text = if lang::has(&key) {
            lang::get(&key)
        } else {
            String::new()
        };
        if let Some(impact) = &self.configs.entries()[id].performance_impact {
            let level = lang::get(&format!("horizon.pack.options.impact.{impact}"));
            let line = lang::tr("horizon.pack.options.impact", &[&level]);
            text = if text.is_empty() {
                line
            } else {
                format!("{text} {line}")
            };
        }
        text
    }

    /// A slider's place (0 to 1) for a number entry.
    fn slider(&self, row: u16) -> Option<f32> {
        let id = self.ids.get(usize::from(row))?;
        let ConfigKind::Number { min, max, .. } = self.configs.entries()[id].kind else {
            return None;
        };
        let Some(ConfigValue::Number(n)) = self.configs.get(id) else {
            return None;
        };
        Some(if max > min {
            ((n - min) / (max - min)) as f32
        } else {
            0.0
        })
    }

    fn switch(&self, row: u16) -> Option<bool> {
        match self.configs.get(self.ids.get(usize::from(row))?)? {
            ConfigValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn draw(
        &mut self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        in_world: bool,
    ) {
        let (w, h) = (batch.width, batch.height);
        let (cols, lines, content) = self.layout(w, h);
        self.scroll.clamp(cols.top, cols.bottom, content);
        shell::backdrop(batch, atlas, in_world);
        shell::mark(
            batch,
            atlas,
            &cols,
            &lang::get("horizon.pack.options.title"),
        );
        shell::plate(
            batch,
            atlas,
            &cols,
            &lang::get("horizon.pack.options.title"),
        );
        let over_list = cursor.1 >= cols.top && cursor.1 < cols.bottom;
        let mut described = None;
        shell::list(
            batch,
            atlas,
            &cols,
            &lines,
            self.scroll.amount,
            |section| self.section_name(section),
            |batch, row: u16, y| {
                let [rx, ry, rw, rh] = cols.row(y);
                let hover = over_list && ui::hovered(cursor, rx, ry, rw, rh);
                if hover || self.dragging == Some(row) {
                    described = Some(row);
                }
                let (name, value) = self.parts(row);
                let label = match &value {
                    Some(v) => format!("{name}: {v}"),
                    None => name.clone(),
                };
                let view = shell::RowView {
                    label: &label,
                    name: &name,
                    value: value.as_deref(),
                    switch: self.switch(row),
                    slider: self.slider(row),
                    live: true,
                    hover,
                    focused: false,
                    held: self.dragging == Some(row),
                    key: None,
                };
                shell::row(batch, atlas, &cols, y, &view);
            },
        );
        shell::scroller(batch, atlas, &cols, content, self.scroll.amount);
        // The strip: the pointed-at entry's tooltip.
        let [x, y, ..] = cols.dock;
        style::panel(batch, atlas, cols.dock);
        if let Some(row) = described {
            let name = Text::literal(self.parts(row).0);
            batch.text_styled(atlas, &name, x + 12.0, y + 8.0, style::WHITE, true);
            shell::dock_note(batch, atlas, &cols, self.description(row), style::GREY);
        }
        let mut foot = vec![(cols.button(0), "gui.done", Some(Declared::Done))];
        if self
            .configs
            .chosen()
            .iter()
            .any(|(id, _)| self.ids.contains(id))
        {
            foot.push((cols.button(1), "controls.reset", None));
        }
        for (rect, key, declared) in foot {
            if let Some(declared) = declared {
                crate::widgets::declare_action(declared, rect, true);
            }
            let label = Text::literal(lang::get(key));
            shell::foot_button(batch, atlas, rect, &label, cursor, (false, true));
        }
    }

    /// A press (or a drag with `drag`) at `(x, y)`; `back` turns a cycle backwards.
    pub fn click(
        &mut self,
        (x, y): (f32, f32),
        (w, h): (f32, f32),
        drag: bool,
        back: bool,
    ) -> Vec<Action> {
        let (cols, lines, content) = self.layout(w, h);
        let at = |[rx, ry, rw, rh]: [f32; 4]| ui::hovered((x, y), rx, ry, rw, rh);
        if drag {
            if self.scroll.held() {
                self.scroll.drag(y, cols.top, cols.bottom, content);
            } else if let Some(row) = self.dragging {
                self.set_slider(row, cols.slider_at(x));
                return vec![Action::SliderDrag];
            }
            return Vec::new();
        }
        if cols.scrolls()
            && self
                .scroll
                .press((x, y), cols.bar_x(), cols.top, cols.bottom, content)
        {
            return Vec::new();
        }
        if at(cols.button(0)) {
            return vec![Action::ButtonClick, Action::Done];
        }
        if at(cols.button(1)) {
            self.reset();
            return vec![Action::ButtonClick];
        }
        if y < cols.top || y >= cols.bottom {
            return Vec::new();
        }
        let Some(row) = lines.iter().find_map(|line| {
            let row = line.setting?;
            at(cols.row(cols.top + line.y - self.scroll.amount)).then_some(row)
        }) else {
            return Vec::new();
        };
        if row == PRESETS {
            self.cycle_preset(back);
            return vec![Action::ButtonClick];
        }
        let Some(id) = self.ids.get(usize::from(row)).cloned() else {
            return Vec::new();
        };
        let entry = self.configs.entries()[&id].clone();
        match (&entry.kind, self.configs.get(&id).cloned()) {
            (ConfigKind::Number { .. }, _) => {
                self.dragging = Some(row);
                self.set_slider(row, cols.slider_at(x));
                return vec![Action::SliderDrag];
            }
            (ConfigKind::Bool, Some(ConfigValue::Bool(b))) => {
                self.set(&id, ConfigValue::Bool(!b));
            }
            (ConfigKind::Text { allowed }, Some(ConfigValue::Text(t))) if !allowed.is_empty() => {
                let i = allowed.iter().position(|a| *a == t).unwrap_or(0);
                let n = allowed.len();
                let next = if back { (i + n - 1) % n } else { (i + 1) % n };
                self.set(&id, ConfigValue::Text(allowed[next].clone()));
            }
            _ => return Vec::new(),
        }
        vec![Action::ButtonClick]
    }

    fn set(&mut self, id: &str, value: ConfigValue) {
        let before = self.configs.get(id).cloned();
        self.configs.set(id, value);
        if self.configs.get(id).cloned() != before {
            self.changed = true;
        }
    }

    /// Number entry `row` at slider place `at` (0 to 1).
    fn set_slider(&mut self, row: u16, at: f32) {
        let Some(id) = self.ids.get(usize::from(row)).cloned() else {
            return;
        };
        if let ConfigKind::Number { min, max, .. } = self.configs.entries()[&id].kind {
            self.set(&id, ConfigValue::Number(min + f64::from(at) * (max - min)));
        }
    }

    /// Every entry back to its default.
    fn reset(&mut self) {
        let defaults: Vec<_> = self
            .configs
            .entries()
            .iter()
            .map(|(id, e)| (id.clone(), e.default.clone()))
            .collect();
        for (id, value) in defaults {
            self.set(&id, value);
        }
    }

    /// The pointer lets go.
    pub fn release(&mut self) {
        self.scroll.release();
        self.dragging = None;
    }

    /// The mouse wheel scrolls the list.
    pub fn scroll(&mut self, (w, h): (f32, f32), amount: f32) {
        let (cols, _, content) = self.layout(w, h);
        self.scroll
            .wheel(amount, 18.0, cols.top, cols.bottom, content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configs() -> Configs {
        let mut c = Configs::default();
        c.add_entry(
            "p:fog".into(),
            &serde_json::json!({"default_value": 0.5, "section": "sky", "section_order": 2}),
        );
        c.add_entry("p:rain".into(), &serde_json::json!({"default_value": "drops", "allowed_values": ["drops", "streaks"], "section": "weather", "section_order": 1}));
        c.add_entry(
            "p:birds".into(),
            &serde_json::json!({"default_value": true}),
        );
        c
    }

    #[test]
    fn sections_sort_by_their_order_and_the_unsectioned_last() {
        let ui = PackOptionsUi::open(configs());
        let order: Vec<_> = ui.sections.iter().map(|(s, _)| s.as_deref()).collect();
        assert_eq!(order, [Some("weather"), Some("sky"), None]);
    }

    #[test]
    fn rows_switch_cycle_and_slide() {
        let mut ui = PackOptionsUi::open(configs());
        let size = (480.0, 270.0);
        let (cols, lines, _) = ui.layout(size.0, size.1);
        let ids = ui.ids.clone();
        let point = |id: &str| {
            let row = ids.iter().position(|i| i == id).unwrap() as u16;
            let line = lines.iter().find(|l| l.setting == Some(row)).unwrap();
            let [x, y, w, h] = cols.row(cols.top + line.y);
            (x + w - 4.0, y + h / 2.0)
        };
        let (birds, rain) = (point("p:birds"), point("p:rain"));
        ui.click(birds, size, false, false);
        assert_eq!(ui.configs.get("p:birds"), Some(&ConfigValue::Bool(false)));
        ui.click(rain, size, false, false);
        assert_eq!(
            ui.configs.get("p:rain"),
            Some(&ConfigValue::Text("streaks".into()))
        );
        // The slider's far right end is its maximum.
        let fog = point("p:fog");
        let (tx, tw) = cols.track();
        ui.click((tx + tw, fog.1), size, false, false);
        ui.release();
        assert_eq!(ui.configs.get("p:fog"), Some(&ConfigValue::Number(1.0)));
        assert!(ui.changed);
        assert_eq!(ui.configs.chosen().len(), 3);
        ui.reset();
        assert!(ui.configs.chosen().is_empty());
    }

    #[test]
    fn the_presets_row_cycles_the_pack_wide_presets_and_shows_custom_otherwise() {
        let mut c = Configs::default();
        let entry = |default: f64, fast: f64, pretty: f64| serde_json::json!({"default_value": default, "presets": {"fast": fast, "pretty": pretty}});
        c.add_entry("p:fog".into(), &entry(0.5, 0.0, 1.0));
        c.add_entry("p:dust".into(), &entry(0.5, 0.0, 0.8));
        let mut ui = PackOptionsUi::open(c);
        // The defaults are no preset.
        assert_eq!(ui.current_preset(), None);
        ui.cycle_preset(false);
        assert_eq!(ui.current_preset().as_deref(), Some("fast"));
        assert_eq!(ui.configs.get("p:fog"), Some(&ConfigValue::Number(0.0)));
        ui.cycle_preset(false);
        assert_eq!(ui.current_preset().as_deref(), Some("pretty"));
        ui.cycle_preset(true);
        assert_eq!(ui.current_preset().as_deref(), Some("fast"));
        // One value off the preset: custom again.
        ui.set("p:dust", ConfigValue::Number(0.3));
        assert_eq!(ui.current_preset(), None);
        assert!(ui.changed);
    }
}
