//! The pages crates add to the options screens: a native mod's settings page (its keys and
//! buttons), and the pages crates above the engine register (`horizon_hooks::pages`, docs/layers.md
//! "Platform registries") with their entries on Horizon Settings. Kept out of options_ui.rs, which
//! may not grow (tools/check_file_size.py).
use super::*;
use horizon_hooks::pages as added;

/// `selected_key` while a registered page's key waits (`selected_native_key`: page, row).
pub(super) const PAGE_KEY: &str = "horizon.pageKey";

#[cfg(test)]
thread_local! {
    /// A test's own pages, instead of the game's registered ones: each test runs on its thread.
    static TEST_PAGES: std::cell::Cell<Option<&'static [&'static dyn added::Page]>> =
        const { std::cell::Cell::new(None) };
}

/// The pages crates registered.
fn registered() -> &'static [&'static dyn added::Page] {
    #[cfg(test)]
    if let Some(pages) = TEST_PAGES.get() {
        return pages;
    }
    added::engine::all()
}

/// A registered page's title (a translation key).
pub(super) fn title(page: usize) -> &'static str {
    registered().get(page).map_or("", |p| p.title())
}

/// Row `r` of a registered page, as it is now.
fn row(page: usize, r: usize) -> Option<added::Row> {
    let mut rows = Vec::new();
    registered().get(page)?.rows(&mut rows);
    rows.into_iter().nth(r)
}

/// A registered page's slider ([`Opt::Page`]): its caption (a translation key).
pub(super) fn slider_caption(page: usize, r: usize) -> &'static str {
    match row(page, r) {
        Some(added::Row::Slider { label, .. }) => label,
        _ => "",
    }
}

/// A registered page's slider: its position.
pub(super) fn slider_kind(page: usize, r: usize) -> Kind {
    match row(page, r) {
        Some(added::Row::Slider { value, .. }) => Kind::Slider(value.clamp(0.0, 1.0)),
        _ => Kind::Cycle,
    }
}

/// A registered page's slider: the value its message shows after the caption.
pub(super) fn slider_text(page: usize, r: usize) -> Text {
    match row(page, r) {
        Some(added::Row::Slider { text, .. }) => label_text(text),
        _ => Text::empty(),
    }
}

/// A registered page's slider moved.
pub(super) fn slide(page: usize, r: usize, value: f32) {
    if let Some(registered) = registered().get(page) {
        registered.act(r, added::Act::Slide(value));
    }
}

/// What a registered page's row says on hover.
pub(super) fn tooltip(page: usize, r: usize) -> Option<Text> {
    registered().get(page)?.tooltip(r).map(label_text)
}

/// What a button opening one of Horizon's pages says on hover.
pub(super) fn horizon_page_tooltip(page: Page) -> Option<&'static str> {
    Some(match page {
        Page::Horizon => "horizon.pages.settings.tooltip",
        Page::Studio => "horizon.pages.studio.tooltip",
        Page::Graphics => "horizon.pages.graphics.tooltip",
        Page::GraphicsAdvanced => "horizon.pages.graphics_advanced.tooltip",
        Page::Controller => "horizon.pages.controller.tooltip",
        Page::Extras => "horizon.pages.extras.tooltip",
        Page::Pvp => "horizon.pages.pvp.tooltip",
        Page::Agent => "horizon.pages.agent.tooltip",
        Page::SplitProfiles => "horizon.pages.split_profiles.tooltip",
        Page::ShaderPacks => "horizon.pages.shader_packs.tooltip",
        Page::Mods => "horizon.pages.mods.tooltip",
        Page::SplitSetup => "horizon.pages.split_setup.tooltip",
        Page::Music => "horizon.pages.music.tooltip",
        _ => return None,
    })
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
        let mut entries = registered()
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
        let Some(registered) = registered().get(page) else {
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
            added::Row::Slider { .. } => Row::Big(Opt::Page(page, r)),
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
        let Some(registered) = registered().get(page) else {
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
            if let Some(page) = registered().get(i) {
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

    /// A page with a heading, a switch, a number that steps either way, a key and a volume.
    struct Sample {
        on: Mutex<bool>,
        step: Mutex<i32>,
        key: Mutex<String>,
        volume: Mutex<f32>,
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
            let volume = *self.volume.lock().unwrap();
            rows.push(added::Row::Slider {
                label: "test.sample.volume",
                value: volume,
                text: added::Label::Value(format!("{}%", (volume * 100.0).round())),
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
                (4, added::Act::Slide(v)) => *self.volume.lock().unwrap() = v,
                other => panic!("row and act the page doesn't expect: {other:?}"),
            }
        }
        fn tooltip(&self, row: usize) -> Option<added::Label> {
            (row == 1).then_some(added::Label::Key("test.sample.switch.tooltip"))
        }
    }

    static SAMPLE: Sample = Sample {
        on: Mutex::new(false),
        step: Mutex::new(0),
        key: Mutex::new(String::new()),
        volume: Mutex::new(0.0),
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
        TEST_PAGES.set(Some(&PAGES));
        let mut ui = OptionsUi::default();
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
        // The volume is one of the screen's own sliders: dragged to its middle, then let go.
        let size = (480.0, 240.0);
        let (widgets, ..) = ui.layout(&s, size.0, size.1);
        let slider = widgets
            .iter()
            .find(|w| {
                matches!(
                    w.kind,
                    WKind::Slider {
                        opt: Opt::Page(0, 4),
                        ..
                    }
                )
            })
            .expect("the volume's slider");
        let (x, y) = (slider.x + slider.w / 2.0, slider.y + 1.0);
        ui.click(&mut s, &atlas, x, y, size, [480, 240], false, false);
        let volume = *SAMPLE.volume.lock().unwrap();
        assert!((0.45..0.55).contains(&volume), "{volume}");
        assert!(ui.release().contains(&Event::Changed));
        // The slider shows the page's value once let go.
        let (widgets, ..) = ui.layout(&s, size.0, size.1);
        let shown = widgets.iter().find_map(|w| match w.kind {
            WKind::Slider {
                opt: Opt::Page(0, 4),
                value,
                ..
            } => Some(value),
            _ => None,
        });
        assert_eq!(shown, Some(volume));
        // The page's tooltip for its switch.
        let switch = widgets
            .iter()
            .find(|w| {
                matches!(
                    &w.kind,
                    WKind::Button {
                        act: Act::PageRow(0, 1),
                        ..
                    }
                )
            })
            .expect("the switch");
        assert_eq!(
            tooltip_text(&s, switch).map(|t| t.plain()),
            Some(tr("test.sample.switch.tooltip").plain())
        );
    }
}
