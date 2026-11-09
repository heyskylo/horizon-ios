//! The pages crates add to the options screens: a native mod's settings page (its keys and
//! buttons), and the pages crates above the engine register (`horizon_hooks::pages`, docs/layers.md
//! "Platform registries") with their entries on Horizon Settings. Kept out of options_ui.rs, which
//! may not grow (tools/check_file_size.py).
use super::*;
use horizon_hooks::pages as added;

/// `selected_key` while a registered page's key waits (`selected_native_key`: page, row).
pub(super) const PAGE_KEY: &str = "horizon.pageKey";

/// A registered page's title (a translation key).
pub(super) fn title(page: usize) -> &'static str {
    added::engine::all().get(page).map_or("", |p| p.title())
}

fn label_text(label: added::Label) -> Text {
    match label {
        added::Label::Key(key) => tr(key),
        added::Label::Value(value) => Text::literal(value),
    }
}

impl OptionsUi {
    /// Horizon Settings: its pages, then the pages crates registered for it.
    pub(super) fn horizon_rows(&self, s: &Settings) -> Vec<Row> {
        // Two to a row, as vanilla's OptionsList.addSmall lays out
        // buttons; each opens one of Horizon's settings pages. Each page has
        // one way in: graphics and shader packs are Video Settings' (the graphics
        // screen), the controller is Controls', mods are the Options screen's.
        let open = |key: &str, page: Page| button(0.0, 0.0, 150.0, tr(key), Act::Push(page));
        let mut rows: Vec<Row> = vec![
            Row::Buttons(
                open("horizon.pages.open.extras", Page::Extras),
                Some(open("horizon.pages.open.pvp", Page::Pvp)),
            ),
            Row::Buttons(
                open(
                    "horizon.pages.open.split_setup",
                    if self.split_guest {
                        Page::SplitProfiles
                    } else {
                        Page::SplitSetup
                    },
                ),
                Some(open("horizon.pages.open.studio", Page::Studio)),
            ),
            // No AI hides the AI Agent page and everything AI.
            Row::Buttons(
                self.extra_button(s, crate::extras::Item::DiscordPresence),
                Some(if s.extras.no_ai {
                    self.extra_button(s, crate::extras::Item::NoAi)
                } else {
                    open("horizon.pages.open.agent", Page::Agent)
                }),
            ),
        ]
        .into_iter()
        .chain(
            (!s.extras.no_ai)
                .then(|| Row::Buttons(self.extra_button(s, crate::extras::Item::NoAi), None)),
        )
        .collect();
        self.added_entries(&mut rows);
        rows
    }

    /// The entries of the pages registered for Horizon Settings, two to a row like the engine's.
    fn added_entries(&self, rows: &mut Vec<Row>) {
        let mut entries = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| page.place() == added::Place::HorizonSettings)
            .map(|(i, page)| button(0.0, 0.0, 150.0, tr(page.title()), Act::Push(Page::Added(i))));
        while let Some(first) = entries.next() {
            rows.push(Row::Buttons(first, entries.next()));
        }
    }

    /// A registered page's rows, asked of it each time the page is drawn.
    pub(super) fn added_rows(&self, page: usize) -> Vec<Row> {
        let Some(registered) = self.pages.get(page) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        registered.rows(&mut rows);
        rows.into_iter()
            .enumerate()
            .map(|(r, row)| self.added_row(page, r, row))
            .collect()
    }

    fn added_row(&self, page: usize, r: usize, row: added::Row) -> Row {
        let act = Act::PageRow(page, r);
        let wide = |label: Text| Row::Buttons(button(0.0, 0.0, 310.0, label, act.clone()), None);
        match row {
            added::Row::Heading(key) => Row::Heading(key.to_owned()),
            added::Row::Note(text) => Row::Note(text),
            added::Row::Toggle { label, on } => wide(generic(label, on_off(on))),
            added::Row::Cycle { label, value } => wide(generic(label, label_text(value))),
            added::Row::Button { label } => wide(tr(label)),
            added::Row::Key { label, key } => {
                let mut message = keybinds::display_name(&key);
                if self.selected_key == Some(PAGE_KEY)
                    && self.selected_native_key == Some((page, r))
                {
                    message = Text::literal("> ")
                        .append(message.with_color(text::WHITE))
                        .append(" <")
                        .with_color(text::YELLOW);
                }
                let caption = button(0.0, 0.0, 150.0, tr(label), Act::None);
                let change = button(0.0, 0.0, 150.0, message, act);
                Row::Buttons(inactive(caption), Some(change))
            }
            // A row kind newer than this engine: shown empty.
            _ => Row::Note(String::new()),
        }
    }

    /// A registered page's row was pressed: a key row waits for a key, any other row hears it.
    pub(super) fn page_act(&mut self, page: usize, r: usize, shift: bool) -> Vec<Event> {
        let Some(registered) = self.pages.get(page) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        registered.rows(&mut rows);
        match rows.get(r) {
            Some(added::Row::Key { .. }) => {
                self.selected_key = Some(PAGE_KEY);
                self.selected_native_key = Some((page, r));
                Vec::new()
            }
            Some(_) => {
                registered.act(
                    r,
                    if shift {
                        added::Act::PressBack
                    } else {
                        added::Act::Press
                    },
                );
                vec![Event::Changed]
            }
            None => Vec::new(),
        }
    }

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

    /// The key pressed for the native mod's key, or the registered page's ([`PAGE_KEY`]), that
    /// waited for one.
    pub(super) fn bind_native_key(
        &mut self,
        s: &mut Settings,
        name: &str,
        value: String,
    ) -> Vec<Event> {
        let (i, k) = self.selected_native_key.take().unwrap_or_default();
        if name == PAGE_KEY {
            if let Some(page) = self.pages.get(i) {
                page.act(k, added::Act::Bind(value));
            }
            return vec![Event::Changed];
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A page with a heading, a switch, a number that steps either way and a key.
    struct Sample {
        on: Mutex<bool>,
        step: Mutex<i32>,
        key: Mutex<String>,
    }

    impl added::Page for Sample {
        fn id(&self) -> &'static str {
            "test.sample"
        }
        fn title(&self) -> &'static str {
            "test.sample.title"
        }
        fn place(&self) -> added::Place {
            added::Place::HorizonSettings
        }
        fn rows(&self, rows: &mut Vec<added::Row>) {
            rows.push(added::Row::Heading("test.sample.heading"));
            rows.push(added::Row::Toggle {
                label: "test.sample.switch",
                on: *self.on.lock().unwrap(),
            });
            rows.push(added::Row::Cycle {
                label: "test.sample.step",
                value: added::Label::Value(self.step.lock().unwrap().to_string()),
            });
            rows.push(added::Row::Key {
                label: "test.sample.key",
                key: self.key.lock().unwrap().clone(),
            });
        }
        fn act(&self, row: usize, act: added::Act) {
            match (row, act) {
                (1, added::Act::Press) => {
                    let mut on = self.on.lock().unwrap();
                    *on = !*on;
                }
                (2, added::Act::Press) => *self.step.lock().unwrap() += 1,
                (2, added::Act::PressBack) => *self.step.lock().unwrap() -= 1,
                (3, added::Act::Bind(key)) => *self.key.lock().unwrap() = key,
                other => panic!("row and act the page doesn't expect: {other:?}"),
            }
        }
    }

    static SAMPLE: Sample = Sample {
        on: Mutex::new(false),
        step: Mutex::new(0),
        key: Mutex::new(String::new()),
    };
    static PAGES: [&dyn added::Page; 1] = [&SAMPLE];

    /// Clicks the active button that does `act`, with Shift held or not.
    fn press(ui: &mut OptionsUi, s: &mut Settings, act: Act, shift: bool) -> Vec<Event> {
        let size = (480.0, 240.0);
        let (widgets, ..) = ui.layout(s, size.0, size.1);
        let (x, y) = widgets
            .iter()
            .find_map(|w| match &w.kind {
                WKind::Button {
                    act: a,
                    active: true,
                    ..
                } if *a == act => Some((w.x + 1.0, w.y + 1.0)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no button does {act:?}"));
        ui.shift = shift;
        let atlas = super::super::tests::atlas();
        ui.click(s, &atlas, x, y, size, [480, 240], false, false)
    }

    #[test]
    fn a_registered_page_opens_from_horizon_settings_and_hears_its_rows() {
        let mut s = Settings::default();
        let mut ui = OptionsUi {
            pages: &PAGES,
            ..OptionsUi::default()
        };
        ui.show(Page::Horizon);
        press(&mut ui, &mut s, Act::Push(Page::Added(0)), false);
        assert_eq!(ui.page(), Page::Added(0));
        // Rows: the heading (0), the switch (1), the step (2), the key (3).
        let events = press(&mut ui, &mut s, Act::PageRow(0, 1), false);
        assert_eq!(
            events.last(),
            Some(&Event::Changed),
            "the settings get saved"
        );
        assert!(*SAMPLE.on.lock().unwrap());
        press(&mut ui, &mut s, Act::PageRow(0, 2), false);
        press(&mut ui, &mut s, Act::PageRow(0, 2), true);
        press(&mut ui, &mut s, Act::PageRow(0, 2), true);
        assert_eq!(*SAMPLE.step.lock().unwrap(), -1, "Shift steps back");
        // A key row waits for the next key, as on Key Binds, and the page gets its name.
        press(&mut ui, &mut s, Act::PageRow(0, 3), false);
        let atlas = super::super::tests::atlas();
        let events = ui.key(&mut s, &atlas, winit::keyboard::KeyCode::KeyZ, None);
        assert_eq!(events, [Event::Changed]);
        assert_eq!(*SAMPLE.key.lock().unwrap(), "key.keyboard.z");
        assert_eq!(ui.selected_key, None);
    }
}
