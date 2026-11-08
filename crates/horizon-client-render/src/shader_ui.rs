//! Horizon's shader settings screen, on the settings shell (`settings_shell`): the shader packs in
//! `shaderpacks/` and the options of the picked one. Its pages are the pack list, then the pack's
//! own screens (a `shaders/` pack's `screen.NAME`s, a Horizon pack's `[[pages]]`), read from the
//! pack each time the rows are built; the shell draws and drives them, searches every screen,
//! collapses the screens into one row when the page list has no room for them, and has Undo. A
//! `shaders/` pack's options apply when Apply is pressed or the screen closes, as the pack loads
//! again with them; a Horizon pack's apply at once.
use crate::shader_pack_list::Kind;
use crate::{
    lang,
    options_ui::Event,
    settings::Settings,
    settings_shell::{self as shell, Group, Output, Row, Sheet, Value},
    text::Text,
};
use horizon_shader_api::{OptionKind, PageItem};
use std::collections::BTreeMap;

/// A page of the screen: the pack list, or screen `.0` of the picked pack (`screens`, 0 its
/// main one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Packs,
    Screen(u16),
}

/// A row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    /// `s.shader_packs[.0]`: picks it, or turns it off when it is the pick.
    Pack(u16),
    /// Opens `shaderpacks/`.
    Folder,
    /// Option `.0` of the picked pack: of a `shaders/` pack, the `.0`th of its options by name;
    /// of a Horizon pack, its `[[options]]` entry.
    Option(u16),
    /// Opens screen `.0`.
    Link(u16),
    /// The next of a `shaders/` pack's profiles.
    Profile,
    /// Every option of the pack back to its default.
    Reset,
    /// Horizon's own Shadow Cache (`Graphics::shadow_cache`), for every pack: not the pack's.
    ShadowCache,
}

const TITLE: &str = "Shaders";
const PACKS: &str = "Shader packs";
const GENERAL: &str = "General";
/// The groups: the packs, Horizon's own setting beside them, a pack's options.
const INSTALLED: &str = "Installed";
const BUILT_IN: &str = "Built-in settings";
const PACK_OPTIONS: &str = "Pack settings";
/// The line over the picked pack's screens in the page list.
const PACK_HEADING: &str = "%s settings";
/// What the strip says of a pack's own screens: they are the pack's, not Horizon's.
const PACK_SETTINGS: &str = "Settings made by the pack's author: they change only this pack.";
const NO_PACKS: &str = "No shader packs yet: open the pack folder and put some in it.";
const PICK_A_PACK: &str = "Pick a shader pack. Picking the one in use turns shaders off.";
const PENDING: &str = "Changed options apply with Apply, or when you leave.";
/// Pack descriptions run longer than Horizon's own: the strip has room for a line more.
const NOTE_LINES: usize = 3;
/// Screens this many links from the main one, or fewer, are in the page list.
const LISTED_DEPTH: u8 = 1;
/// Short of a float slider's steps when the range isn't a whole number of them (`slider_value`).
const STEP_SLACK: f32 = 1e-6;

/// One of the picked pack's screens.
struct PackScreen {
    title: String,
    /// What the strip says while nothing on it is pointed at.
    about: String,
    items: Vec<Item>,
    /// How many links away from the main screen: the page list holds the first two levels.
    depth: u8,
}

/// The picked pack: its index in `s.shader_packs` and kind, and for a `shaders/` pack whether its
/// options (`s.shader_options`) are read.
fn picked(s: &Settings) -> Option<(usize, Kind)> {
    let i = s
        .shader_packs
        .iter()
        .position(|p| !s.horizon.shader_pack.is_empty() && p.key == s.horizon.shader_pack)?;
    Some((i, s.shader_packs[i].kind))
}

/// The picked `shaders/` pack's options, when they are the picked pack's.
fn classic(s: &Settings) -> Option<&crate::shader_options::ShaderOptions> {
    s.shader_options
        .as_ref()
        .filter(|o| o.pack == s.horizon.shader_pack)
}

/// The picked Horizon pack, and its index in `s.shader_packs`.
fn native(s: &Settings) -> Option<(usize, &crate::shader_pack_list::Entry)> {
    picked(s)
        .filter(|p| p.1 == Kind::Horizon)
        .map(|(i, _)| (i, &s.shader_packs[i]))
}

/// The `.0`th option name of a `shaders/` pack.
fn option_name(o: &crate::shader_options::ShaderOptions, i: u16) -> Option<&str> {
    o.options.keys().nth(usize::from(i)).map(String::as_str)
}

fn plain(t: Text) -> String {
    t.strip_legacy().plain()
}

/// The picked pack's screens, its main one first, then in the order their links are met.
fn screens(s: &Settings) -> Vec<PackScreen> {
    match picked(s) {
        Some((_, Kind::Classic)) => classic(s).map_or_else(Vec::new, classic_screens),
        Some((i, Kind::Horizon)) => native_screens(&s.shader_packs[i]),
        None => Vec::new(),
    }
}

fn classic_screens(o: &crate::shader_options::ShaderOptions) -> Vec<PackScreen> {
    use horizon_shaders::ScreenItem;
    let keys: Vec<&String> = o.options.keys().collect();
    let mut names = vec![(String::new(), 0u8)];
    let mut out = Vec::new();
    let mut at = 0;
    while let Some((name, depth)) = names.get(at).cloned() {
        at += 1;
        let mut items = Vec::new();
        for item in o.screens.screen(&name).map_or(&[][..], |(items, _)| items) {
            match item {
                ScreenItem::Option(n) => {
                    if let Some(k) = keys.iter().position(|k| *k == n) {
                        items.push(Item::Option(k as u16));
                    }
                }
                ScreenItem::Screen(n) if o.screens.screen(n).is_some() => {
                    let k = match names.iter().position(|(m, _)| m == n) {
                        Some(k) => k,
                        None => {
                            names.push((n.clone(), depth.saturating_add(1)));
                            names.len() - 1
                        }
                    };
                    items.push(Item::Link(k as u16));
                }
                ScreenItem::Profile if !o.screens.profiles.is_empty() => items.push(Item::Profile),
                _ => {}
            }
        }
        let comment = |key: &str| o.comment(key).map(plain);
        let (title, about) = if name.is_empty() {
            items.push(Item::Reset);
            (lang::get(GENERAL), lang::get(PACK_SETTINGS))
        } else {
            let title = plain(o.screen_label(&name));
            let title = title.strip_suffix("...").unwrap_or(&title).to_owned();
            let about = comment(&format!("screen.{name}")).unwrap_or_default();
            (title, about)
        };
        out.push(PackScreen {
            title,
            about,
            items,
            depth,
        });
    }
    out
}

fn native_screens(pack: &crate::shader_pack_list::Entry) -> Vec<PackScreen> {
    let count = pack.options.len().min(pack.values.len());
    let reset = |mut items: Vec<Item>, first: bool| {
        if first {
            items.push(Item::Reset);
        }
        items
    };
    if pack.pages.is_empty() {
        if count == 0 {
            return Vec::new();
        }
        let items = (0..count).map(|o| Item::Option(o as u16)).collect();
        return vec![PackScreen {
            title: lang::get(GENERAL),
            about: lang::get(PACK_SETTINGS),
            items: reset(items, true),
            depth: 0,
        }];
    }
    // A page's depth: links from the first page are one away, the rest two.
    let linked: Vec<usize> = pack.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            PageItem::Page(p) => Some(*p),
            _ => None,
        })
        .collect();
    pack.pages
        .iter()
        .enumerate()
        .map(|(i, page)| {
            let items = page
                .items
                .iter()
                .filter_map(|item| match *item {
                    PageItem::Option(o) if o < count => Some(Item::Option(o as u16)),
                    PageItem::Page(p) if p < pack.pages.len() && p != i => {
                        Some(Item::Link(p as u16))
                    }
                    _ => None,
                })
                .collect();
            PackScreen {
                title: if i == 0 {
                    lang::get(GENERAL)
                } else {
                    page.name.clone()
                },
                about: page.description.clone(),
                items: reset(items, i == 0),
                depth: match i {
                    0 => 0,
                    _ if linked.contains(&i) => 1,
                    _ => 2,
                },
            }
        })
        .collect()
}

/// A screen's name in the page list and over its rows.
fn screen_name(i: u16, screens: &[PackScreen]) -> String {
    let screen = screens.get(usize::from(i));
    screen.map_or_else(String::new, |s| s.title.clone())
}

/// A row as its name and its value; `None` makes it a button across the row.
fn parts(item: Item, s: &Settings, screens: &[PackScreen]) -> (String, Option<String>) {
    match item {
        Item::Pack(i) => {
            let pack = s.shader_packs.get(usize::from(i));
            let name = pack.map_or_else(String::new, |p| p.title.clone());
            (name, Some(String::new()))
        }
        Item::Folder => (lang::get("pack.openFolder"), None),
        Item::Link(i) => (format!("{}...", screen_name(i, screens)), None),
        Item::Reset => (lang::get("Reset all options"), None),
        Item::ShadowCache => (lang::get("Shadow Cache"), Some(String::new())),
        Item::Profile => match classic(s) {
            Some(o) => shell::name_and_value(&plain(o.profile_label())),
            None => (String::new(), None),
        },
        Item::Option(i) => {
            if let Some((_, pack)) = native(s) {
                let i = usize::from(i);
                let (Some(o), Some(v)) = (pack.options.get(i), pack.values.get(i)) else {
                    return (String::new(), None);
                };
                let value = crate::shader_pack_list::show_value(&o.kind, *v);
                return (o.name.clone(), Some(value));
            }
            match classic(s).and_then(|o| Some((o, option_name(o, i)?))) {
                Some((o, name)) => (plain(o.option_name(name)), Some(plain(o.value_text(name)))),
                None => (String::new(), None),
            }
        }
    }
}

/// A tick box's state, for a pack (picked) or an on/off option.
fn switch(item: Item, s: &Settings) -> Option<bool> {
    match item {
        Item::ShadowCache => Some(s.horizon.shadow_cache),
        Item::Pack(i) => Some(
            s.shader_packs
                .get(usize::from(i))
                .is_some_and(|p| p.key == s.horizon.shader_pack),
        ),
        Item::Option(i) => match picked(s)? {
            (p, Kind::Horizon) => {
                let pack = &s.shader_packs[p];
                let i = usize::from(i);
                match pack.options.get(i)?.kind {
                    OptionKind::Bool { .. } => Some(*pack.values.get(i)? != 0.0),
                    _ => None,
                }
            }
            (_, Kind::Classic) => {
                let o = classic(s)?;
                let name = option_name(o, i)?;
                o.options[name]
                    .switch
                    .then(|| o.value(name) == Some("true"))
            }
        },
        _ => None,
    }
}

/// A slider's position, for an option the pack shows as one.
fn slider(item: Item, s: &Settings) -> Option<f32> {
    let Item::Option(i) = item else { return None };
    match picked(s)? {
        (p, Kind::Horizon) => s.shader_packs[p].slider_position(usize::from(i)),
        (_, Kind::Classic) => {
            let o = classic(s)?;
            o.slider_position(option_name(o, i)?)
        }
    }
}

/// A slider's stops, which Left and Right step between: a `shaders/` option's values, a Horizon
/// float's steps from its minimum (the last one its maximum) or an int's whole numbers.
fn slider_steps(item: Item, s: &Settings) -> Option<u32> {
    let Item::Option(i) = item else { return None };
    let stops = match picked(s)? {
        (p, Kind::Horizon) => match s.shader_packs[p].options.get(usize::from(i))?.kind {
            OptionKind::Float { min, max, step, .. } => ((max - min) / step - STEP_SLACK).ceil(),
            OptionKind::Int { min, max, .. } => (max - min) as f32,
            _ => return None,
        },
        (_, Kind::Classic) => {
            let o = classic(s)?;
            o.options[option_name(o, i)?].values.len().saturating_sub(1) as f32
        }
    };
    (stops >= 1.0).then_some(stops as u32)
}

/// What a row does, or says of itself, for the strip and the search.
fn description(item: Item, s: &Settings, screens: &[PackScreen]) -> String {
    match item {
        Item::Pack(i) => {
            let Some(pack) = s.shader_packs.get(usize::from(i)) else {
                return String::new();
            };
            let kind = lang::get(match pack.kind {
                Kind::Horizon => "A Horizon shader pack.",
                Kind::Classic => "An OptiFine or Iris shader pack.",
            });
            let what = lang::get(if pack.key == s.horizon.shader_pack {
                "In use: press to turn shaders off."
            } else {
                "Press to use it."
            });
            format!("{kind} {what} ({})", pack.file)
        }
        Item::Folder => lang::get("Put shader pack .zip files or folders in it."),
        Item::Reset => lang::get("Puts every option of this pack back to its default."),
        Item::ShadowCache => {
            lang::get("Reuses the pack shadow map when sun, view and world match. Off by default.")
        }
        Item::Profile => classic(s)
            .and_then(|o| o.comment("profile"))
            .map(plain)
            .unwrap_or_else(|| lang::get("Sets many options at once, as the pack groups them.")),
        Item::Link(i) => screens
            .get(usize::from(i))
            .map(|s| s.about.clone())
            .unwrap_or_default(),
        Item::Option(i) => match native(s) {
            Some((_, pack)) => pack
                .options
                .get(usize::from(i))
                .map(|o| o.description.clone())
                .unwrap_or_default(),
            None => classic(s)
                .and_then(|o| o.comment(&format!("option.{}", option_name(o, i)?)))
                .map(plain)
                .unwrap_or_default(),
        },
    }
}

/// A row as the shell draws it: a tick box for a pack and an on/off option, a slider for an
/// option the pack shows as one, a value between `<` `>` for the other options and the profile,
/// a button across the row for the folder, a link and Reset.
fn row(item: Item, s: &Settings, screens: &[PackScreen]) -> Row<Item> {
    let (name, text) = parts(item, s, screens);
    let value = match (item, switch(item, s), slider(item, s), text) {
        (_, Some(on), ..) => Value::Switch(on),
        (_, None, Some(position), Some(text)) => Value::Slider {
            position,
            steps: slider_steps(item, s),
            text,
        },
        (Item::Option(_) | Item::Profile, None, None, Some(text)) => Value::Choice(text),
        _ => Value::Press,
    };
    Row::new(item, name, value).about(description(item, s, screens))
}

/// The pack list, and Horizon's own setting beside it.
fn pack_sheet(s: &Settings) -> Sheet<Tab, Item> {
    let packs = (0..s.shader_packs.len()).map(|i| Item::Pack(i as u16));
    let installed = packs.chain([Item::Folder]);
    let about = lang::get(if s.shader_packs.is_empty() {
        NO_PACKS
    } else {
        PICK_A_PACK
    });
    let groups = vec![
        Group {
            name: INSTALLED,
            rows: installed.map(|item| row(item, s, &[])).collect(),
        },
        Group {
            name: BUILT_IN,
            rows: vec![row(Item::ShadowCache, s, &[])],
        },
    ];
    Sheet::new(Tab::Packs, lang::get(PACKS), about, groups)
}

/// The picked pack's name, over its screens in the page list.
fn pack_name(s: &Settings) -> &str {
    match native(s) {
        Some((_, pack)) => pack.title.as_str(),
        None => (s.horizon.shader_pack)
            .strip_suffix(".zip")
            .unwrap_or(&s.horizon.shader_pack),
    }
}

/// The shader screen. Its rows read the settings; what it keeps is for Undo and a drag.
#[derive(Default)]
pub(crate) struct ShaderScreen {
    /// The Horizon pack whose slider a drag moved: saved once, when it lets go.
    drag_changed: Option<usize>,
    /// The values this screen saved to a `shaders/` pack's file (picking another pack, Apply),
    /// by pack: Undo puts back the ones the screen opened with over them.
    written: BTreeMap<String, BTreeMap<String, String>>,
}

impl ShaderScreen {
    /// The picked `shaders/` pack's options are saved: what Undo has to put back over.
    fn saving(&mut self, s: &Settings) {
        if let Some(o) = s.shader_options.as_ref() {
            self.written.insert(o.pack.clone(), o.chosen.clone());
        }
    }
}

impl shell::Screen for ShaderScreen {
    type Page = Tab;
    type Row = Item;
    fn title(&self) -> &'static str {
        TITLE
    }
    fn note_lines(&self) -> usize {
        NOTE_LINES
    }
    /// The rows read the packs, the pick, its options and the Shadow Cache.
    fn stamp(&self, s: &Settings) -> u64 {
        let packs: Vec<_> = (s.shader_packs.iter())
            .map(|p| (&p.key, &p.title, &p.values, p.options.len(), p.pages.len()))
            .collect();
        let options = (s.shader_options.as_ref()).map(|o| (&o.pack, &o.chosen, o.changed));
        let h = &s.horizon;
        shell::stamp_of(&(&h.shader_pack, h.shadow_cache, packs, options))
    }
    /// The pack list, then the picked pack's screens under its name: those the main screen links
    /// to in the page list, the ones farther in opened by their links.
    fn sheets(&self, s: &Settings) -> Vec<Sheet<Tab, Item>> {
        let screens = screens(s);
        let pending = self.pending(s);
        let mut sheets = vec![pack_sheet(s)];
        for (i, screen) in screens.iter().enumerate() {
            let about = if pending {
                lang::get(PENDING)
            } else {
                screen.about.clone()
            };
            let rows = (screen.items.iter())
                .map(|&item| row(item, s, &screens))
                .collect();
            let groups = vec![Group {
                name: PACK_OPTIONS,
                rows,
            }];
            let mut sheet = Sheet::new(Tab::Screen(i as u16), screen.title.clone(), about, groups);
            sheet.listed = screen.depth <= LISTED_DEPTH;
            sheet.heading = (i == 0).then(|| lang::tr(PACK_HEADING, &[&pack_name(s)]));
            sheets.push(sheet);
        }
        sheets
    }
    fn link(&self, item: Item) -> Option<Tab> {
        match item {
            Item::Link(i) => Some(Tab::Screen(i)),
            _ => None,
        }
    }
    /// A pack is picked (or turned off) and saved, the last pack's changed options first; an
    /// option steps (back with Shift); the folder opens.
    fn press(&mut self, s: &mut Settings, item: Item, back: bool) -> Vec<Output> {
        let changed = vec![Output::Event(Event::Changed)];
        let pack = s.horizon.shader_pack.clone();
        let native = native(s).map(|p| p.0);
        match item {
            Item::Pack(i) => {
                let Some(p) = s.shader_packs.get(usize::from(i)) else {
                    return Vec::new();
                };
                s.horizon.shader_pack = if s.horizon.shader_pack == p.key {
                    String::new()
                } else {
                    p.key.clone()
                };
                let mut outputs = Vec::new();
                // The last pack's changed options are kept before the next one's are read.
                if s.shader_options.as_ref().is_some_and(|o| o.changed) {
                    self.saving(s);
                    outputs.push(Output::Event(Event::ShaderOptions));
                }
                outputs.push(Output::Event(Event::ShaderPack));
                outputs.push(Output::Event(Event::OpenShaderOptions));
                outputs
            }
            Item::Folder => vec![Output::Event(Event::OpenShaderPackFolder)],
            Item::ShadowCache => {
                s.horizon.shadow_cache = !s.horizon.shadow_cache;
                changed
            }
            // The shell follows a link (`link`).
            Item::Link(_) => Vec::new(),
            Item::Profile => match s.shader_options.as_mut().filter(|o| o.pack == pack) {
                Some(o) => {
                    o.next_profile();
                    changed
                }
                None => Vec::new(),
            },
            Item::Reset => match native {
                Some(p) => {
                    s.shader_packs[p].reset_options();
                    vec![Output::Event(Event::NativePackOptions(p))]
                }
                None => match s.shader_options.as_mut().filter(|o| o.pack == pack) {
                    Some(o) => {
                        o.reset();
                        changed
                    }
                    None => Vec::new(),
                },
            },
            Item::Option(i) => match native {
                Some(p) if s.shader_packs[p].step_option(usize::from(i), back, false) => {
                    vec![Output::Event(Event::NativePackOptions(p))]
                }
                Some(_) => Vec::new(),
                None => {
                    let options = s.shader_options.as_mut().filter(|o| o.pack == pack);
                    match options.and_then(|o| Some((option_name(o, i)?.to_owned(), o))) {
                        Some((name, o)) => {
                            o.cycle(&name, back);
                            changed
                        }
                        None => Vec::new(),
                    }
                }
            },
        }
    }
    /// A slider moves an option to the value nearest it.
    fn slide(&mut self, s: &mut Settings, item: Item, position: f32) {
        let Item::Option(i) = item else {
            return;
        };
        if let Some((p, _)) = native(s) {
            if s.shader_packs[p].set_slider_position(usize::from(i), position) {
                self.drag_changed = Some(p);
            }
            return;
        }
        let pack = s.horizon.shader_pack.clone();
        if let Some(o) = s.shader_options.as_mut().filter(|o| o.pack == pack)
            && let Some(name) = option_name(o, i).map(str::to_owned)
        {
            o.set_position(&name, position);
        }
    }
    /// A Horizon pack's options are saved, and the pack rebuilt, once a drag.
    fn let_go(&mut self) -> Vec<Output> {
        let changed = self.drag_changed.take();
        changed.map_or_else(Vec::new, |p| {
            vec![Output::Event(Event::NativePackOptions(p))]
        })
    }
    /// With the "Scroll wheel changes settings" extra, the wheel steps an option between `<` `>`
    /// and the profile (up goes back); the other rows leave it to scroll the list.
    fn wheel_step(&mut self, s: &mut Settings, item: Item, back: bool) -> Option<Vec<Output>> {
        let steps = match item {
            Item::Profile => true,
            Item::Option(_) => switch(item, s).is_none() && slider(item, s).is_none(),
            _ => false,
        };
        if !steps {
            return None;
        }
        let mut outputs = vec![Output::Event(Event::ButtonClick)];
        outputs.extend(self.press(s, item, back));
        Some(outputs)
    }
    fn undoes(&self) -> bool {
        true
    }
    /// The pick, the Shadow Cache, and every pack's options as the screen opened with them: a
    /// Horizon pack's saved again, a `shaders/` pack's saved when the screen closes (or Apply).
    fn undo(&mut self, s: &mut Settings, before: &Settings) -> Vec<Output> {
        let mut outputs = Vec::new();
        s.horizon.shadow_cache = before.horizon.shadow_cache;
        for (i, pack) in s.shader_packs.iter_mut().enumerate() {
            let was = before.shader_packs.iter().find(|p| p.key == pack.key);
            if let Some(was) = was.filter(|was| was.values != pack.values) {
                pack.values = was.values.clone();
                outputs.push(Output::Event(Event::NativePackOptions(i)));
            }
        }
        if s.horizon.shader_pack != before.horizon.shader_pack {
            // The pick goes back with its options as they were; what this screen saved over
            // them is saved again when it closes. The other pack's changes are dropped.
            s.horizon.shader_pack = before.horizon.shader_pack.clone();
            s.shader_options = before.shader_options.clone();
            if let Some(o) = s.shader_options.as_mut()
                && let Some(written) = self.written.get(&o.pack)
            {
                o.saved_as(written.clone());
            }
            outputs.push(Output::Event(Event::ShaderPack));
            if s.shader_options.is_none() {
                outputs.push(Output::Event(Event::OpenShaderOptions));
            }
        } else if let Some(o) = s.shader_options.as_mut()
            && let Some(was) = before.shader_options.as_ref().filter(|w| w.pack == o.pack)
        {
            o.restore(was.chosen.clone());
        }
        outputs
    }
    /// Undo shows exactly while the pick, the Shadow Cache or a pack's options differ from what
    /// the screen opened with: an option stepped back to its value isn't a change.
    fn differs(&self, s: &Settings, before: &Settings) -> Option<bool> {
        let native = s.shader_packs.iter().any(|pack| {
            let was = before.shader_packs.iter().find(|p| p.key == pack.key);
            was.is_some_and(|was| was.values != pack.values)
        });
        let options = match (classic(s), before.shader_options.as_ref()) {
            (Some(o), Some(was)) => o.pack == was.pack && o.chosen != was.chosen,
            _ => false,
        };
        let h = (&s.horizon, &before.horizon);
        let pick = h.0.shader_pack != h.1.shader_pack;
        Some(pick || h.0.shadow_cache != h.1.shadow_cache || native || options)
    }
    /// A `shaders/` pack's options wait for Apply until the pack loads again with them.
    fn pending(&self, s: &Settings) -> bool {
        classic(s).is_some_and(|o| o.changed)
    }
    /// The options are saved, and the pack loads again with them.
    fn apply(&mut self, s: &mut Settings) -> Vec<Output> {
        self.saving(s);
        vec![
            Output::Event(Event::ShaderOptions),
            Output::Event(Event::OpenShaderOptions),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::{HudAtlas, HudBatch};
    use crate::settings_shell::{Hosted, Part, Screen as _, Shell, Target};
    use crate::shader_options::ShaderOptions;
    use crate::shader_pack_list::Entry;
    use horizon_shaders::{Lang, Macros, PackOption, Properties, Screens};
    use winit::keyboard::KeyCode as K;

    const SIZE: (f32, f32) = (854.0, 480.0);
    const CLICK: Output = Output::Event(Event::ButtonClick);
    const CHANGED: Output = Output::Event(Event::Changed);

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    fn entry(file: &str, kind: Kind) -> Entry {
        Entry {
            file: file.into(),
            key: file.into(),
            title: file.into(),
            kind,
            options: Vec::new(),
            values: Vec::new(),
            pages: Vec::new(),
        }
    }

    fn option(name: &str, values: &[&str], switch: bool) -> (String, PackOption) {
        let o = PackOption {
            name: name.into(),
            default: values[0].into(),
            values: values.iter().map(|v| (*v).into()).collect(),
            switch,
            tested: true,
        };
        (name.to_owned(), o)
    }

    /// `A.zip` picked, with `options` laid out by `properties`.
    fn classic_pack(options: BTreeMap<String, PackOption>, properties: &str) -> Settings {
        let mut s = Settings {
            shader_packs: vec![entry("A.zip", Kind::Classic), entry("B.zip", Kind::Classic)],
            ..Settings::default()
        };
        s.horizon.shader_pack = "A.zip".into();
        let screens = Screens::parse(
            &Properties::parse(properties, &Macros::standard()),
            &options,
        );
        s.shader_options = Some(ShaderOptions::new(
            "A.zip".into(),
            options,
            screens,
            Lang::default(),
            BTreeMap::new(),
        ));
        s
    }

    /// Two packs, the first picked: a `shaders/` pack with a main screen, a Lighting screen and
    /// a Shadows screen inside it. Options by name: BLOOM 0, QUALITY 1, SHADOW_RES 2, SUN 3.
    fn settings() -> Settings {
        let options = BTreeMap::from([
            option("BLOOM", &["false", "true"], true),
            option("QUALITY", &["1", "2", "3"], false),
            option("SHADOW_RES", &["1024", "2048", "4096"], false),
            option("SUN", &["1", "2"], false),
        ]);
        classic_pack(
            options,
            "screen = <profile> [LIGHTING] BLOOM QUALITY\nscreen.LIGHTING = SUN [SHADOWS]\n\
             screen.SHADOWS = SHADOW_RES\nsliders = SHADOW_RES\n\
             profile.LOW = SHADOW_RES=1024\nprofile.HIGH = SHADOW_RES=4096\n",
        )
    }

    /// The picked pack with `n` screens, each linked from its main one: more than the page list
    /// of a short window holds.
    fn many_screens(n: usize) -> Settings {
        let names: Vec<String> = (0..n).map(|i| format!("S{i}")).collect();
        let options = names
            .iter()
            .map(|name| option(&format!("O_{name}"), &["false", "true"], true))
            .collect();
        let links: Vec<String> = names.iter().map(|n| format!("[{n}]")).collect();
        let mut text = format!("screen = {}\n", links.join(" "));
        for name in &names {
            text += &format!("screen.{name} = O_{name}\n");
        }
        classic_pack(options, &text)
    }

    fn shell_at(tab: Tab) -> Shell<ShaderScreen> {
        let mut shell = Shell::<ShaderScreen>::default();
        shell.show(tab);
        shell
    }

    fn press(shell: &mut Shell<ShaderScreen>, s: &mut Settings, item: Item) -> Vec<Output> {
        let at = shell.point_at(s, SIZE, item);
        shell.click(s, at, SIZE, false, false, false)
    }

    #[test]
    fn a_packs_screens_are_pages_its_main_one_first_under_the_packs_name() {
        let s = settings();
        let sheets = ShaderScreen::default().sheets(&s);
        let pages: Vec<(Tab, &str, bool)> = (sheets.iter())
            .map(|p| (p.id, p.name.as_str(), p.listed))
            .collect();
        // The page list holds the main screen and the ones it links to; Shadows opens from
        // Lighting.
        assert_eq!(
            pages,
            [
                (Tab::Packs, "Shader packs", true),
                (Tab::Screen(0), "General", true),
                (Tab::Screen(1), "LIGHTING", true),
                (Tab::Screen(2), "SHADOWS", false),
            ]
        );
        assert_eq!(sheets[1].heading.as_deref(), Some("A settings"));
        assert!(
            sheets
                .iter()
                .filter(|p| p.id != Tab::Screen(0))
                .all(|p| p.heading.is_none())
        );
        let rows: Vec<Item> = sheets[1].groups[0].rows.iter().map(|r| r.id).collect();
        assert_eq!(
            rows,
            [
                Item::Profile,
                Item::Link(1),
                Item::Option(0),
                Item::Option(1),
                Item::Reset
            ]
        );
        // A tick box, a value between arrows, a slider in its pack's steps, and links as buttons.
        let value = |sheet: usize, item| {
            let rows = &sheets[sheet].groups[0].rows;
            rows.iter().find(|r| r.id == item).unwrap().value.clone()
        };
        assert_eq!(value(1, Item::Option(0)), Value::Switch(false));
        assert_eq!(value(1, Item::Option(1)), Value::Choice("1".into()));
        assert_eq!(value(1, Item::Link(1)), Value::Press);
        assert!(matches!(
            value(3, Item::Option(2)),
            Value::Slider { steps: Some(2), .. }
        ));
        // Another pack's options are not this one's: only the pack list.
        let mut other = s.clone();
        other.horizon.shader_pack = "B.zip".into();
        assert_eq!(ShaderScreen::default().sheets(&other).len(), 1);
    }

    #[test]
    fn a_short_page_list_collapses_a_packs_screens_and_leaves_none_out() {
        let s = many_screens(12);
        let mut shell = shell_at(Tab::Screen(4));
        // 427x480 GUI px: tall enough for the pack list, the pack's line and its 13 screens.
        let mut batch = HudBatch::new([854, 960]);
        shell.draw(&s, &mut batch, &atlas(), (0.0, 0.0), false, None);
        assert_eq!(shell.page_places().len(), 1 + 13);
        let mut batch = HudBatch::new([854, 480]);
        shell.draw(&s, &mut batch, &atlas(), (0.0, 0.0), false, None);
        let parts: Vec<(Tab, Part)> = shell.page_places().iter().map(|p| (p.0, p.2)).collect();
        assert_eq!(
            parts,
            [
                (Tab::Packs, Part::Tab),
                (Tab::Screen(3), Part::Prev),
                (Tab::Screen(4), Part::Shown),
                (Tab::Screen(5), Part::Next)
            ]
        );
    }

    #[test]
    fn picking_a_pack_and_picking_it_again_turns_shaders_off() {
        for (kind, key) in [(Kind::Classic, "B.zip"), (Kind::Horizon, "horizon:warm")] {
            let mut s = settings();
            s.shader_packs[1].kind = kind;
            s.shader_packs[1].key = key.into();
            let mut shell = shell_at(Tab::Packs);
            let picks = vec![
                CLICK,
                Output::Event(Event::ShaderPack),
                Output::Event(Event::OpenShaderOptions),
            ];
            assert_eq!(press(&mut shell, &mut s, Item::Pack(1)), picks);
            assert_eq!(s.horizon.shader_pack, key);
            assert_eq!(picked(&s), Some((1, kind)));
            assert_eq!(switch(Item::Pack(1), &s), Some(true));
            press(&mut shell, &mut s, Item::Pack(1));
            assert_eq!(s.horizon.shader_pack, "");
            assert_eq!(switch(Item::Pack(0), &s), Some(false));
        }
    }

    #[test]
    fn options_wait_for_apply_and_links_go_in_and_escape_back_out() {
        let mut s = settings();
        let mut shell = shell_at(Tab::Screen(0));
        let is_pending = |shell: &mut Shell<ShaderScreen>, s: &Settings| {
            shell.foot_at(s, SIZE).iter().any(|f| f.0 == Target::Apply)
        };
        assert!(!is_pending(&mut shell, &s));
        assert_eq!(press(&mut shell, &mut s, Item::Option(0)), [CLICK, CHANGED]);
        assert_eq!(switch(Item::Option(0), &s), Some(true));
        assert!(is_pending(&mut shell, &s));
        // Stepped back, nothing waits.
        press(&mut shell, &mut s, Item::Option(0));
        assert!(!is_pending(&mut shell, &s));
        press(&mut shell, &mut s, Item::Option(0));
        // Apply saves them and reads the pack again.
        let apply = shell
            .foot_at(&s, SIZE)
            .into_iter()
            .find(|f| f.0 == Target::Apply);
        let [x, y, w, h] = apply.unwrap().1;
        let applied = shell.click(
            &mut s,
            (x + w / 2.0, y + h / 2.0),
            SIZE,
            false,
            false,
            false,
        );
        let reload = [Event::ShaderOptions, Event::OpenShaderOptions].map(Output::Event);
        assert_eq!(applied, [&[CLICK][..], &reload].concat());
        // Into Lighting, then Shadows, which isn't in the page list: Escape backs out of each.
        press(&mut shell, &mut s, Item::Link(1));
        press(&mut shell, &mut s, Item::Link(2));
        assert_eq!(shell.open_page(), Some(Tab::Screen(2)));
        let mut batch = HudBatch::new([1708, 960]);
        shell.draw(&s, &mut batch, &atlas(), (0.0, 0.0), false, None);
        assert_eq!(shell.lit_page(), Some(Tab::Screen(1)));
        assert!(shell.key(&mut s, K::Escape, None, false, SIZE).is_empty());
        assert_eq!(shell.open_page(), Some(Tab::Screen(1)));
        shell.key(&mut s, K::Escape, None, false, SIZE);
        let done = shell.key(&mut s, K::Escape, None, false, SIZE);
        assert_eq!(
            done,
            [Output::Done],
            "the main screen's Escape closes the screen"
        );
        // Reset puts the option back.
        press(&mut shell, &mut s, Item::Reset);
        assert_eq!(switch(Item::Option(0), &s), Some(false));
    }

    #[test]
    fn left_and_right_step_an_option_and_the_right_button_steps_it_back() {
        let mut s = settings();
        let mut shell = shell_at(Tab::Screen(0));
        let value = |s: &Settings| classic(s).unwrap().value("QUALITY").map(str::to_owned);
        shell.set_focus(Some(Target::Row(Item::Option(1))));
        shell.key(&mut s, K::ArrowRight, None, false, SIZE);
        assert_eq!(value(&s).as_deref(), Some("2"));
        shell.key(&mut s, K::ArrowLeft, None, false, SIZE);
        shell.key(&mut s, K::ArrowLeft, None, false, SIZE);
        assert_eq!(value(&s).as_deref(), Some("3"));
        let at = shell.point_at(&s, SIZE, Item::Option(1));
        shell.click(&mut s, at, SIZE, false, true, false);
        assert_eq!(value(&s).as_deref(), Some("2"));
        assert_eq!(shell.focused_row(), Some(Item::Option(1)));
    }

    #[test]
    fn the_search_finds_an_option_on_a_screen_inside_another() {
        let s = settings();
        let mut shell = shell_at(Tab::Packs);
        shell.set_search("shadow_res");
        assert_eq!(shell.listed(&s, SIZE), [(Tab::Screen(2), Item::Option(2))]);
        assert_eq!(slider(Item::Option(2), &s), Some(0.0));
    }

    #[test]
    fn tab_reaches_every_pack_the_folder_and_the_built_in_setting() {
        let mut s = settings();
        let mut shell = shell_at(Tab::Packs);
        let mut seen = Vec::new();
        for _ in 0..12 {
            shell.key(&mut s, K::Tab, None, false, SIZE);
            if let Some(item) = shell.focused_row()
                && !seen.contains(&item)
            {
                seen.push(item);
            }
        }
        let all = [
            Item::Pack(0),
            Item::Pack(1),
            Item::Folder,
            Item::ShadowCache,
        ];
        assert_eq!(seen, all);
    }

    /// A Horizon pack picked, with a float option from 0 to 1 in steps of 0.1 at 0.5.
    fn horizon_pack() -> Settings {
        let mut pack = entry("H", Kind::Horizon);
        pack.key = "horizon:slider_test".into();
        pack.options = vec![horizon_shader_api::PackOption {
            id: "strength".into(),
            name: "Strength".into(),
            description: String::new(),
            kind: OptionKind::Float {
                default: 0.5,
                min: 0.0,
                max: 1.0,
                step: 0.1,
            },
        }];
        pack.values = vec![0.5];
        let mut s = Settings {
            shader_packs: vec![pack],
            ..Settings::default()
        };
        s.horizon.shader_pack = "horizon:slider_test".into();
        s
    }

    #[test]
    fn a_horizon_packs_slider_drag_is_saved_once_when_let_go_and_a_key_step_at_once() {
        let mut s = horizon_pack();
        let mut shell = shell_at(Tab::Screen(0));
        let (_, y) = shell.point_at(&s, SIZE, Item::Option(0));
        let (track, width) = crate::settings_shell::Columns::of(SIZE.0, SIZE.1).track();
        let start = shell.click(&mut s, (track + 2.0, y), SIZE, false, false, false);
        assert_eq!(start, [Output::Event(Event::SliderDrag)]);
        shell.click(&mut s, (track + width, y), SIZE, true, false, false);
        assert_eq!(s.shader_packs[0].values[0], 1.0);
        let saved = vec![Output::Event(Event::NativePackOptions(0))];
        assert_eq!(shell.release(), Some(saved.clone()));
        assert_eq!(shell.release(), None);
        // Left steps it one of its steps, saved as it goes.
        let stepped = shell.key(&mut s, K::ArrowLeft, None, false, SIZE);
        assert_eq!(
            stepped,
            [&[Output::Event(Event::SliderDrag)][..], &saved].concat()
        );
        assert!((s.shader_packs[0].values[0] - 0.9).abs() < 1e-6);
    }

    #[test]
    fn undo_puts_back_the_pick_the_options_and_a_horizon_packs_values() {
        let mut s = settings();
        s.shader_packs.push(horizon_pack().shader_packs.remove(0));
        let mut shell = shell_at(Tab::Screen(0));
        let undo_shows = |shell: &mut Shell<ShaderScreen>, s: &Settings| {
            shell.foot_at(s, SIZE).iter().any(|f| f.0 == Target::Undo)
        };
        press(&mut shell, &mut s, Item::Option(0));
        assert!(undo_shows(&mut shell, &s));
        // Stepped back to how it opened, there is nothing to undo.
        press(&mut shell, &mut s, Item::Option(0));
        assert!(!undo_shows(&mut shell, &s));
        // A changed option, then the Horizon pack picked and its option moved.
        press(&mut shell, &mut s, Item::Option(0));
        shell.show(Tab::Packs);
        let picked = press(&mut shell, &mut s, Item::Pack(2));
        assert!(
            picked.contains(&Output::Event(Event::ShaderOptions)),
            "{picked:?}"
        );
        // The game reads the next pack's options: a Horizon pack has none of these.
        s.shader_options = None;
        // Its slider, a step on from the keyboard.
        shell.show(Tab::Screen(0));
        shell.set_focus(Some(Target::Row(Item::Option(0))));
        shell.key(&mut s, K::Enter, None, false, SIZE);
        shell.key(&mut s, K::ArrowRight, None, false, SIZE);
        assert_ne!(s.shader_packs[2].values[0], 0.5);
        let undo = shell
            .foot_at(&s, SIZE)
            .into_iter()
            .find(|f| f.0 == Target::Undo);
        let [x, y, w, h] = undo.unwrap().1;
        let outputs = shell.click(
            &mut s,
            (x + w / 2.0, y + h / 2.0),
            SIZE,
            false,
            false,
            false,
        );
        // The pick and the Horizon pack's value are back and saved; the `shaders/` pack's options
        // are back as it opened, to be saved over what picking away saved.
        assert_eq!(s.horizon.shader_pack, "A.zip");
        assert_eq!(s.shader_packs[2].values[0], 0.5);
        for event in [Event::NativePackOptions(2), Event::ShaderPack] {
            assert!(
                outputs.contains(&Output::Event(event.clone())),
                "{outputs:?}"
            );
        }
        assert_eq!(switch(Item::Option(0), &s), Some(false));
        assert!(classic(&s).unwrap().changed);
        assert!(!undo_shows(&mut shell, &s));
    }

    #[test]
    fn every_text_is_in_the_language_table() {
        let mut s = settings();
        let (_, mut missed) = lang::scoped(lang::Language::load(&[], "de_de"), || {
            let screen = ShaderScreen::default();
            screen.sheets(&s);
            s.shader_packs[0].kind = Kind::Horizon;
            screen.sheets(&s);
            s.shader_packs.clear();
            screen.sheets(&s);
            for text in [TITLE, PENDING, "Apply", "Undo", "Search options"] {
                lang::get(text);
            }
        });
        // The folder's name and how a value is joined to its name are vanilla's.
        missed.retain(|key| !VANILLA_KEYS.contains(&key.as_str()));
        assert_eq!(missed, Vec::<String>::new());
    }
    const VANILLA_KEYS: [&str; 3] = ["pack.openFolder", "options.generic_value", "options.off"];
}
