//! The pages crates add to the options screens: a native mod's settings page (its keys and
//! buttons). Kept out of options_ui.rs, which may not grow (tools/check_file_size.py).
use super::*;

impl OptionsUi {
    /// A native mod's page: its name, its keys as Key Binds rows (the
    /// player's choice, else the default), then its buttons.
    pub(super) fn native_rows(&self, s: &Settings, i: usize) -> Vec<Row> {
        let Some(m) = s.mods.installed.get(i) else {
            return Vec::new();
        };
        let mut rows = vec![Row::Note(m.title.clone())];
        let Some(page) = &m.native else {
            return rows;
        };
        let chosen = s.mods.keys.get(&m.id);
        for (k, [name, label, default]) in page.keys.iter().enumerate() {
            let key = chosen.and_then(|c| c.get(name)).unwrap_or(default);
            let mut message = keybinds::display_name(key);
            if self.selected_native_key == Some((i, k)) {
                message = Text::literal("> ")
                    .append(message.with_color(text::WHITE))
                    .append(" <")
                    .with_color(text::YELLOW);
            }
            let caption = button(0.0, 0.0, 150.0, Text::literal(label.clone()), Act::None);
            let change = button(0.0, 0.0, 150.0, message, Act::NativeKey(i, k));
            rows.push(Row::Buttons(inactive(caption), Some(change)));
        }
        for (b, label) in page.buttons.iter().enumerate() {
            let widget = button(
                0.0,
                0.0,
                310.0,
                Text::literal(label.clone()),
                Act::NativeButton(i, b),
            );
            rows.push(Row::Buttons(widget, None));
        }
        rows
    }

    /// A native mod's key waits for the next key or button pressed.
    pub(super) fn wait_native_key(&mut self, i: usize, k: usize) {
        self.selected_key = Some(NATIVE_KEY);
        self.selected_native_key = Some((i, k));
    }

    /// The key pressed for the native mod's key that waited for one.
    pub(super) fn bind_native_key(&mut self, s: &mut Settings, value: String) -> Vec<Event> {
        let (i, k) = self.selected_native_key.take().unwrap_or_default();
        let Some(m) = s.mods.installed.get(i) else {
            return Vec::new();
        };
        let Some([key, ..]) = m.native.as_ref().and_then(|p| p.keys.get(k)) else {
            return Vec::new();
        };
        let keys = s.mods.keys.entry(m.id.clone()).or_default();
        keys.insert(key.clone(), value);
        vec![Event::Changed]
    }
}
