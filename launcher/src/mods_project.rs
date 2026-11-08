//! The Mods page's project page, search filters and project icons (`horizon_launcher::browser`:
//! `details` for the data, `images` for the icons and gallery). Drawn only with the launcher's
//! shared components, so a restyle of those restyles this.
//!
//! - A result's title opens its page inside the launcher: icon, author, downloads, followers,
//!   licence, how well it runs and Install, then Description (Markdown), Gallery, Versions and
//!   the project's links.
//! - Filters: sort, loader, game version, environment and categories, with the values the
//!   source lists for the kind.
//! - Icons and gallery images come from the source's image servers at run time, cached on disk;
//!   a placeholder stands in until one arrives.
use super::mods_page::{Checking, Event};
use super::*;
use gpui::img;
use horizon_launcher::browser::{
    self,
    details::{self, Details, Environment, Filters, Image, Sort, Tags},
    images, java, Kind as PackKind, Project, SourceId, Support, Version,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Category and tag names from Modrinth / CurseForge, present in code so the i18n
/// tables can provide translations for them without failing the unused strings test.
const CATEGORY_NAMES: &[&str] = &[
    "adventure",
    "audio",
    "blocks",
    "combat",
    "core shaders",
    "cursed",
    "decoration",
    "economy",
    "entities",
    "environment",
    "equipment",
    "food",
    "fonts",
    "game mechanics",
    "gui",
    "items",
    "locale",
    "magic",
    "management",
    "minigame",
    "mobs",
    "modded",
    "models",
    "optimization",
    "realistic",
    "simplistic",
    "social",
    "storage",
    "technology",
    "themed",
    "transportation",
    "tweaks",
    "utility",
    "vanilla like",
    "worldgen",
];

/// A category's name in the launcher's language: the tables' translation of its words where they
/// have one (Modrinth's names), else the name playhorizon.dev gives it.
fn category_name(slug: &str) -> String {
    let words = slug.replace('-', " ");
    let said = tr(&words);
    if said == words {
        return details::category_label(slug);
    }
    let mut chars = said.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// One of the registry's tags as a chip shows it.
fn tag_name(tag: &str) -> String {
    format!("#{tag}")
}

/// A project page's tabs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    Description,
    Gallery,
    Versions,
}

const TABS: [(Tab, &str); 3] = [
    (Tab::Description, "Description"),
    (Tab::Gallery, "Gallery"),
    (Tab::Versions, "Versions"),
];

/// The open project page.
pub(super) struct Opened {
    pub(super) project: Project,
    tab: Tab,
    details: Option<Result<Arc<Details>, String>>,
    versions: Option<Result<Arc<[ProjectVersion]>, String>>,
    version_list: cull::Cull,
    checked_versions: HashMap<String, horizon_port_pack::check::Verdict>,
    /// The gallery image shown large.
    shot: usize,
}

/// Cached verdicts are read with the version request, never while drawing a frame.
#[derive(Clone)]
pub(super) struct ProjectVersion {
    version: Version,
    verdict: Option<horizon_port_pack::check::Verdict>,
    /// A registry native mod's fit with the Horizon played, and whether it is the one to pick.
    fitted: Option<browser::horizon::Fitted>,
}

/// The project page, filters and images' share of the Mods page's state.
#[derive(Default)]
pub(super) struct Browse {
    pub(super) open: Option<Opened>,
    pub(super) filters: Filters,
    pub(super) show_filters: bool,
    /// The Sort by dropdown's list is open.
    pub(super) sort_menu: bool,
    tags: HashMap<(SourceId, PackKind), Tags>,
    tags_asked: HashSet<(SourceId, PackKind)>,
    /// Images on disk, by URL and size; `None` while they load or when they failed.
    images: HashMap<(String, u32), Option<PathBuf>>,
    image_queue: Option<std::sync::mpsc::Sender<(String, u32)>>,
}

/// What the background sends this module.
pub(super) enum Loaded {
    Details((SourceId, String), Result<Arc<Details>, String>),
    Versions((SourceId, String), Result<Vec<ProjectVersion>, String>),
    Checked(
        (SourceId, String),
        String,
        horizon_port_pack::check::Verdict,
    ),
    Image((String, u32), Option<PathBuf>),
    Tags((SourceId, PackKind), Result<Tags, String>),
}

/// Images fetched at once.
const IMAGE_WORKERS: usize = 4;

impl Launcher {
    pub(super) fn mods_loaded(&mut self, loaded: Loaded) {
        let b = &mut self.mods.browse;
        fn open<'a>(b: &'a mut Browse, key: &(SourceId, String)) -> Option<&'a mut Opened> {
            b.open
                .as_mut()
                .filter(|o| (o.project.source, &o.project.id) == (key.0, &key.1))
        }
        match loaded {
            Loaded::Details(key, d) => {
                if let Some(o) = open(b, &key) {
                    o.details = Some(d);
                }
            }
            Loaded::Versions(key, v) => {
                if let Some(o) = open(b, &key) {
                    o.versions = Some(v.map(Arc::from));
                    o.apply_checks();
                    o.version_list.reset();
                }
            }
            Loaded::Checked(key, version, verdict) => {
                if let Some(o) = open(b, &key) {
                    o.checked_versions.insert(version, verdict);
                    o.apply_checks();
                }
            }
            Loaded::Image(key, path) => {
                // A refresh that failed (offline) keeps the cached image.
                if path.is_some() || !matches!(b.images.get(&key), Some(Some(_))) {
                    b.images.insert(key, path);
                }
            }
            Loaded::Tags(key, Ok(tags)) => {
                b.tags.insert(key, tags);
            }
            Loaded::Tags(_, Err(_)) => {}
        }
    }

    /// Opens a project's page and asks its source for the rest of it.
    pub(super) fn mods_open(&mut self, project: Project) {
        let key = (project.source, project.id.clone());
        let tx = self.mods.tx.clone();
        let p = project.clone();
        std::thread::spawn(move || {
            let Some(source) = browser::source(p.source) else {
                return;
            };
            let d = source.details(&p).map(Arc::new);
            let _ = tx.send(Event::Loaded(Loaded::Details(key.clone(), d)));
            let v = source.versions(&p).map(|versions| {
                let home = backend::game_home();
                let mut fitted = match p.source {
                    SourceId::Horizon => {
                        browser::horizon::fitted(&p.id, &horizon_launcher::instances::selected())
                    }
                    _ => HashMap::new(),
                };
                versions
                    .into_iter()
                    .map(|version| {
                        let verdict = version.file().and_then(|file| {
                            (p.kind == PackKind::Mod)
                                .then(|| java::cached(&home, file))
                                .flatten()
                                .map(|check| check.verdict)
                        });
                        let fitted = fitted.remove(&version.id);
                        ProjectVersion {
                            version,
                            verdict,
                            fitted,
                        }
                    })
                    .collect()
            });
            let _ = tx.send(Event::Loaded(Loaded::Versions(key, v)));
        });
        self.mods.browse.open = Some(Opened {
            project,
            tab: Tab::Description,
            details: None,
            versions: None,
            version_list: cull::Cull::default(),
            checked_versions: HashMap::new(),
            shot: 0,
        });
    }

    /// The image of `url` at `size` when it is on disk; else asks for it once. One cached longer
    /// than `images::FRESH` is shown and asked for again, with its validators.
    pub(super) fn mods_image(&mut self, url: &str, size: u32) -> Option<PathBuf> {
        let key = (url.to_owned(), size);
        if let Some(state) = self.mods.browse.images.get(&key) {
            return state.clone();
        }
        // The launcher's data folder, shared by every instance.
        let home = backend::data_dir();
        if let Some(p) = images::cached(&home, url, size) {
            self.mods.browse.images.insert(key.clone(), Some(p.clone()));
            if images::stale(&home, url, size) {
                let _ = self.mods_image_queue().send(key);
            }
            return Some(p);
        }
        self.mods.browse.images.insert(key.clone(), None);
        if images::refuse(url).is_some() {
            return None;
        }
        let _ = self.mods_image_queue().send(key);
        None
    }

    /// The image workers' queue, started the first time an image is asked for.
    fn mods_image_queue(&mut self) -> std::sync::mpsc::Sender<(String, u32)> {
        let tx = self.mods.tx.clone();
        self.mods
            .browse
            .image_queue
            .get_or_insert_with(|| {
                let (send, receive) = std::sync::mpsc::channel::<(String, u32)>();
                let receive = std::sync::Arc::new(std::sync::Mutex::new(receive));
                let home = backend::data_dir();
                for _ in 0..IMAGE_WORKERS {
                    let (receive, tx, home) = (receive.clone(), tx.clone(), home.clone());
                    std::thread::spawn(move || loop {
                        let Ok(next) = receive.lock().map(|r| r.recv()) else {
                            break;
                        };
                        let Ok((url, size)) = next else {
                            break;
                        };
                        let path = images::fetch(&home, &url, size).ok();
                        if tx
                            .send(Event::Loaded(Loaded::Image((url, size), path)))
                            .is_err()
                        {
                            break;
                        }
                    });
                }
                send
            })
            .clone()
    }

    /// A project's icon, `side` pixels: its image, or its first letter on a plate until then.
    pub(super) fn mods_icon(&mut self, p: &Project, side: f32) -> AnyElement {
        let t = self.theme;
        let path = p
            .icon
            .clone()
            .and_then(|url| self.mods_image(&url, images::ICON));
        blocks::slot(path.map(gpui::ImageSource::from), &p.title, side, &t).into_any_element()
    }

    /// The values the filters can take for the current kind and source, asked for once.
    fn mods_tags(&mut self) -> Option<&Tags> {
        let key = (self.mods.source, self.mods.kind);
        if self.mods.browse.tags_asked.insert(key) {
            let tx = self.mods.tx.clone();
            std::thread::spawn(move || {
                let tags = browser::source(key.0)
                    .ok_or_else(|| "not set up".to_string())
                    .and_then(|s| s.tags(key.1));
                let _ = tx.send(Event::Loaded(Loaded::Tags(key, tags)));
            });
        }
        self.mods.browse.tags.get(&key)
    }

    /// Sets the filters and searches again.
    pub(super) fn mods_filter(&mut self, change: impl FnOnce(&mut Filters)) {
        change(&mut self.mods.browse.filters);
        self.mods_search_keeping(0, true);
    }

    /// The orders the current source can sort by, "Best match" first.
    pub(super) fn mods_sorts(&self) -> Vec<(Option<Sort>, &'static str)> {
        let own =
            self.mods.source == SourceId::Horizon && self.mods.java_for(self.mods.kind).is_none();
        let all: &[Sort] = if own {
            &browser::horizon::SORTS
        } else {
            &Sort::ALL
        };
        std::iter::once((None, "Best match"))
            .chain(all.iter().map(|s| (Some(*s), s.label())))
            .collect()
    }

    /// The registry's bar: only the sort, since it has no categories, loaders or versions to filter.
    pub(super) fn mods_sort_bar(&mut self, cx: &mut Context<Self>) -> Div {
        let sorts = self.mods_sorts();
        let current = self.mods.browse.filters.sort;
        div()
            .flex()
            .child(self.mods_sort_dropdown(&sorts, current, cx))
    }

    /// The "Sort by" dropdown: the chosen order on a button, the choices in a list below it.
    fn mods_sort_dropdown(
        &self,
        sorts: &[(Option<Sort>, &'static str)],
        current: Option<Sort>,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        // A sort the source can't do (Follows, picked on Modrinth) reads as the default.
        let name = sorts
            .iter()
            .find(|(value, _)| *value == current)
            .or(sorts.first())
            .map_or("", |(_, label)| *label);
        let mut wrap = div().relative().flex_none().child(
            keyboard(div().id("mods-sort"), &t)
                .relative()
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(theme::CONTROL_LARGE))
                .px(px(14.))
                // The button's bottom lip isn't part of its face: centre the text on the face.
                .pb(px(theme::LIP))
                .border_2()
                .border_color(t.outline)
                .bg(t.button)
                .children(blocks::edges(t.button_bevel, t.lip, theme::LIP))
                .cursor_pointer()
                .hover(move |s| s.bg(t.raised))
                .child(
                    div()
                        .font_family(theme::PIXEL)
                        .text_size(px(15.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink)
                        .child(trf("Sort by: {order}", &[("order", tr(name))])),
                )
                .child(blocks::icon("chevron-down", 16., t.muted))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.mods.browse.sort_menu = !this.mods.browse.sort_menu;
                    cx.notify();
                })),
        );
        if self.mods.browse.sort_menu {
            let mut menu = div()
                .id("mods-sort-menu")
                .absolute()
                .top(px(theme::CONTROL_LARGE + 4.))
                .left_0()
                .w(px(220.))
                .p(px(6.))
                .border_2()
                .border_color(t.outline)
                .bg(t.surface)
                .flex()
                .flex_col()
                .occlude();
            for &(value, label) in sorts {
                menu = menu.child(
                    div()
                        .id(SharedString::from(format!("mods-sort-pick-{label}")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .h(px(30.))
                        .px(px(10.))
                        .rounded(px(theme::RADIUS))
                        .cursor_pointer()
                        .hover(move |s| s.bg(t.plate_soft))
                        .child(text(theme::LABEL).text_color(t.ink).child(tr(label)))
                        .when(
                            value == current
                                || (current.is_some()
                                    && value.is_none()
                                    && !sorts.iter().any(|(v, _)| *v == current)),
                            |d| d.child(div().size(px(6.)).bg(t.accent)),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.mods.browse.sort_menu = false;
                            this.mods_filter(|f| f.sort = value);
                            cx.notify();
                        })),
                );
            }
            wrap = wrap.child(gpui::deferred(menu).with_priority(1));
        }
        wrap
    }

    /// The filter bar: sort always (Horizon's registry has its own bar); loader, version,
    /// environment and categories when shown, only categories and tags for the registry. While
    /// the panel is closed, what is picked stays in view as chips that each remove theirs.
    pub(super) fn mods_filters(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let f = self.mods.browse.filters.clone();
        let sorts = self.mods_sorts();
        let registry =
            self.mods.source == SourceId::Horizon && self.mods.java_for(self.mods.kind).is_none();
        let sort = (!t.glass && !registry).then(|| self.mods_sort_dropdown(&sorts, f.sort, cx));
        let shown = self.mods.browse.show_filters;
        let active = f.categories.len()
            + f.tags.len()
            + f.loader.is_some() as usize
            + f.game_version.is_some() as usize
            + f.environment.is_some() as usize;
        let toggle = button(
            "mods-filters",
            if active > 0 {
                trf("Filters ({count})", &[("count", &active.to_string())])
            } else {
                tr("Filters").to_string()
            },
            if shown { Kind::Secondary } else { Kind::Ghost },
            &t,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.mods.browse.show_filters = !this.mods.browse.show_filters;
            cx.notify();
        }));
        let mut bar = div().flex().flex_col().gap(px(12.)).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .children(sort)
                .child(toggle),
        );
        if !shown {
            if active > 0 {
                bar = bar.child(self.mods_picked_filters(&f, cx));
            }
            return bar;
        }
        let Some(tags) = self.mods_tags().cloned() else {
            return bar.child(caps(tr("Loading filters")).text_color(t.muted));
        };
        // A chip: on or off, one value of a filter.
        let chip = |id: String, label: String, on: bool| {
            button(
                id,
                label,
                if on { Kind::Secondary } else { Kind::Ghost },
                &t,
            )
        };
        let row = |title: &'static str| {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(4.))
                .child(caps(tr(title)).text_color(t.muted).w(px(120.)))
        };
        let mut loaders = row("Loader");
        for l in &tags.loaders {
            let on = f.loader.as_deref() == Some(l.as_str());
            let value = l.clone();
            loaders = loaders.child(chip(format!("mods-loader-{l}"), l.clone(), on).on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let v = value.clone();
                    this.mods_filter(|f| f.loader = if on { None } else { Some(v) });
                    cx.notify();
                }),
            ));
        }
        let mut versions = row("Game version");
        if let Some(grass) = self.grass_mark(20.) {
            versions = versions.child(img(grass).size(px(20.)).flex_none());
        }
        for v in tags.game_versions.iter().take(12) {
            let on = f.game_version.as_deref() == Some(v.as_str());
            let value = v.clone();
            versions = versions.child(chip(format!("mods-version-{v}"), v.clone(), on).on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let v = value.clone();
                    this.mods_filter(|f| f.game_version = if on { None } else { Some(v) });
                    cx.notify();
                }),
            ));
        }
        let mut environment = row("Environment");
        for e in Environment::ALL {
            let on = f.environment == Some(e);
            environment = environment.child(
                chip(format!("mods-env-{}", e.label()), e.label().into(), on).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.mods_filter(|f| f.environment = if on { None } else { Some(e) });
                        cx.notify();
                    }),
                ),
            );
        }
        let mut categories = row("Categories");
        for c in &tags.categories {
            let on = f.categories.contains(&c.name);
            let value = c.name.clone();
            categories = categories.child(
                chip(format!("mods-cat-{}", c.name), category_name(&c.name), on).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let v = value.clone();
                        this.mods_filter(|f| {
                            if !f.categories.contains(&v) {
                                f.categories.push(v);
                            } else {
                                f.categories.retain(|x| *x != v);
                            }
                        });
                        cx.notify();
                    }),
                ),
            );
        }
        let _ = CATEGORY_NAMES;
        let mut tag_row = row("Tags");
        for tag in &tags.tags {
            let on = f.tags.contains(tag);
            let value = tag.clone();
            tag_row = tag_row.child(chip(format!("mods-tag-{tag}"), tag_name(tag), on).on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let v = value.clone();
                    this.mods_filter(|f| {
                        if !f.tags.contains(&v) {
                            f.tags.push(v);
                        } else {
                            f.tags.retain(|x| *x != v);
                        }
                    });
                    cx.notify();
                }),
            ));
        }
        let clear = button("mods-clear", "Clear filters", Kind::Ghost, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.mods_filter(|f| *f = Filters::default());
                cx.notify();
            },
        ));
        if !registry {
            bar = bar.child(loaders).child(versions).child(environment);
        }
        bar = bar.child(categories);
        if registry && !tags.tags.is_empty() {
            bar = bar.child(tag_row);
        }
        if active > 0 {
            bar = bar.child(div().flex().child(clear));
        }
        bar
    }

    /// What the filters pick, each a chip with a cross that removes it, then Clear filters.
    fn mods_picked_filters(&self, f: &Filters, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        type Remove = Rc<dyn Fn(&mut Filters)>;
        let mut picked: Vec<(String, String, Remove)> = Vec::new();
        for c in &f.categories {
            let v = c.clone();
            picked.push((
                format!("cat-{c}"),
                category_name(c),
                Rc::new(move |f| f.categories.retain(|x| *x != v)),
            ));
        }
        for tag in &f.tags {
            let v = tag.clone();
            picked.push((
                format!("tag-{tag}"),
                tag_name(tag),
                Rc::new(move |f| f.tags.retain(|x| *x != v)),
            ));
        }
        if let Some(l) = &f.loader {
            picked.push(("loader".into(), l.clone(), Rc::new(|f| f.loader = None)));
        }
        if let Some(v) = &f.game_version {
            picked.push((
                "version".into(),
                v.clone(),
                Rc::new(|f| f.game_version = None),
            ));
        }
        if let Some(e) = f.environment {
            picked.push((
                "env".into(),
                e.label().into(),
                Rc::new(|f| f.environment = None),
            ));
        }
        let mut row = div().flex().flex_wrap().items_center().gap(px(6.));
        for (id, label, remove) in picked {
            let cross = if t.glass {
                crate::glass_ui::icon("cancel-01", 14., t.ink_soft)
            } else {
                blocks::icon("close", 14., t.ink_soft)
            };
            row = row.child(
                button(format!("mods-picked-{id}"), label, Kind::Secondary, &t)
                    .gap(px(6.))
                    .child(cross)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let remove = remove.clone();
                        this.mods_filter(move |f| remove(f));
                        cx.notify();
                    })),
            );
        }
        row.child(
            button("mods-picked-clear", "Clear filters", Kind::Ghost, &t).on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    // The sort has its own control: only what filters goes.
                    this.mods_filter(|f| {
                        *f = Filters {
                            sort: f.sort,
                            ..Filters::default()
                        }
                    });
                    cx.notify();
                },
            )),
        )
    }

    /// The open project's page.
    pub(super) fn mods_project_page(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.theme;
        let o = self.mods.browse.open.as_ref()?;
        let p = o.project.clone();
        let tab = o.tab;
        let shot = o.shot;
        let support = self.mods.support(&p);
        let color = match support {
            Support::Works => t.success,
            Support::Partial(_) => t.warn,
            Support::No(_) => t.muted,
        };
        let back = button("mods-back", "Back to results", Kind::Ghost, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.mods.browse.open = None;
                cx.notify();
            },
        ));
        let details = o.details.clone();
        let versions = o.versions.clone();
        let version_list = o.version_list.clone();
        let installed_version = self.mods.entry(&p).map(|e| e.version.clone());
        let mut meta = vec![trf(
            "{count} downloads",
            &[("count", &thousands(p.downloads))],
        )];
        if let Some(Ok(d)) = &details {
            // A registry project has no followers: its source repository's stars stand there.
            match &d.github {
                Some(g) => meta.push(trf(
                    "{count} stars on GitHub",
                    &[("count", &thousands(g.stars))],
                )),
                None if p.source != SourceId::Horizon => meta.push(trf(
                    "{count} followers",
                    &[("count", &thousands(d.followers))],
                )),
                None => {}
            }
            if let Some(day) = d.updated.get(..10) {
                meta.push(trf("updated {date}", &[("date", day)]));
            }
        }
        if !p.license.is_empty() {
            meta.push(p.license.clone());
        }
        if let Some(source) = p.source.shown_for(&p.id) {
            meta.push(trf("from {source}", &[("source", source)]));
        }
        let authors = match &details {
            Some(Ok(d)) if !d.authors.is_empty() => d.authors.join(", "),
            _ => p.author.clone(),
        };
        // The project's categories, as its source lists them.
        let categories = match &details {
            Some(Ok(d)) if !d.categories.is_empty() => Some(
                div().flex().flex_wrap().gap(px(6.)).children(
                    d.categories
                        .iter()
                        .map(|c| blocks::badge(category_name(c), t.muted, &t)),
                ),
            ),
            _ => None,
        };
        let icon = self.mods_icon(&p, 72.);
        let busy = self.mods.busy.as_ref().map(|b| b.0.clone());
        let installed = self.mods.entry(&p).is_some();
        let checking = self.mods.checking(&p);
        let mut action = div().flex().items_center().gap(px(8.)).child(
            caps(if checking {
                tr("Checking")
            } else {
                tr(support.label())
            })
            .flex_none()
            .text_color(color),
        );
        if busy.as_deref() == Some(p.id.as_str()) {
            action = action.child(caps(tr("Installing")).text_color(t.accent));
        } else if installed {
            action = action.child(caps(tr("Installed")).text_color(t.muted));
        } else if support.installable() && !checking {
            let label = if p.kind == PackKind::Modpack {
                "Import"
            } else {
                "Install"
            };
            let mut install = button("mods-page-install", label, Kind::Primary, &t);
            if busy.is_some() {
                install = disabled(install);
            } else {
                let p = p.clone();
                install = install.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.mods_install(p.source, p.id.clone(), Some(p.clone()));
                    cx.notify();
                }));
            }
            action = action.child(install);
        }
        let head = div()
            .flex()
            .items_center()
            .gap(px(20.))
            .child(icon)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .child(text(theme::TITLE).text_color(t.ink).child(p.title.clone()))
                    .child(
                        text(theme::UI)
                            .text_color(t.muted)
                            .child(if authors.is_empty() {
                                p.summary.clone()
                            } else {
                                format!(
                                    "{} · {}",
                                    trf("by {author}", &[("author", &authors)]),
                                    p.summary
                                )
                            }),
                    )
                    .child(
                        text(theme::CAPTION)
                            .text_color(t.muted)
                            .child(meta.join(" · ")),
                    )
                    .children(categories),
            )
            .child(action);
        let mut page = div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(div().flex().child(back))
            .child(head);
        // Registry projects are rated on the site, signed in with GitHub (one rating per account),
        // and starred on their source repository, which the player does there themselves.
        if p.source == SourceId::Horizon {
            let url = browser::horizon::rate_url(&p.id);
            let repo = match &details {
                Some(Ok(d)) => d.github.as_ref().map(|g| g.url.clone()),
                _ => None,
            };
            page = page.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .children(super::mods_page::stars(&p.id, &t).map(|s| text(theme::UI).child(s)))
                    .child(
                        button("mods-rate", "Sign in with GitHub to rate", Kind::Ghost, &t)
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                    .children(repo.map(|repo| {
                        button("mods-star", "Star on GitHub", Kind::Ghost, &t)
                            .on_click(move |_, _, cx| cx.open_url(&repo))
                    })),
            );
        }
        if let Some(why) = support.reason() {
            page = page.child(
                text(theme::UI)
                    .text_color(t.ink_soft)
                    .child(why.to_string()),
            );
        }
        if let Some(Checking::Done(c)) = self.mods.checks.get(&(p.source, p.id.clone())) {
            let lines = |title: &str, list: &[String]| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(caps(tr(title)).text_color(t.muted))
                    .children(
                        list.iter()
                            .map(|l| text(theme::CAPTION).text_color(t.ink_soft).child(l.clone())),
                    )
            };
            let mut what = div().flex().gap(px(32.));
            if !c.ports.is_empty() {
                what = what.child(lines("Ports", &c.ports));
            }
            if !c.skipped.is_empty() {
                what = what.child(lines("Doesn't port", &c.skipped));
            }
            page = page.child(what);
        }
        if let Some(Ok(d)) = &details {
            let mut links = div().flex().flex_wrap().gap(px(8.));
            let page_url = p.page.clone();
            if !page_url.is_empty() {
                links = links.child(
                    button(
                        "mods-link-page",
                        trf("{source} page", &[("source", p.source.label())]),
                        Kind::Ghost,
                        &t,
                    )
                    .on_click(move |_, _, cx| cx.open_url(&page_url)),
                );
            }
            for (i, l) in d.links.iter().enumerate() {
                let url = l.url.clone();
                links = links.child(
                    button(format!("mods-link-{i}"), l.label.clone(), Kind::Ghost, &t)
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                );
            }
            page = page.child(links);
        }
        let this = cx.entity().downgrade();
        // No Gallery tab for a project without images.
        let no_gallery = matches!(&details, Some(Ok(d)) if d.gallery.is_empty());
        let tabs: Vec<(Tab, &str)> = TABS
            .into_iter()
            .filter(|(tab, _)| !(no_gallery && *tab == Tab::Gallery))
            .collect();
        page = page.child(div().flex().child(segments("mods-tab", &tabs, tab, &t, {
            move |value, _, cx| {
                let _ = this.update(cx, |this, cx| {
                    if let Some(o) = &mut this.mods.browse.open {
                        o.tab = value;
                    }
                    cx.notify();
                });
            }
        })));
        let body = match tab {
            Tab::Description => match &details {
                None => centered(tr("Loading…"), &t),
                Some(Err(e)) => notice(e.clone(), &t),
                Some(Ok(d)) => div().max_w(px(760.)).child(markdown::render(&d.body, &t)),
            },
            Tab::Gallery => match &details {
                None => centered(tr("Loading…"), &t),
                Some(Err(e)) => notice(e.clone(), &t),
                Some(Ok(d)) if d.gallery.is_empty() => centered(tr("No gallery images."), &t),
                Some(Ok(d)) => self.mods_gallery(&d.gallery, shot, cx),
            },
            Tab::Versions => match &versions {
                None => centered(tr("Loading…"), &t),
                Some(Err(e)) => notice(e.clone(), &t),
                Some(Ok(list)) => version_list.column(
                    &self.content_scroll.handle,
                    list.len().min(50),
                    8.,
                    |_| false,
                    |i| {
                        let v = &list[i].version;
                        if let Some(fitted) = &list[i].fitted {
                            return self.mods_native_version_row(
                                &p,
                                v,
                                fitted,
                                installed_version.as_deref() == Some(v.id.as_str()),
                                cx,
                            );
                        }
                        let file = v.file();
                        let mut detail = vec![
                            v.published.get(..10).unwrap_or_default().to_string(),
                            v.loaders.join(", "),
                        ];
                        let games: Vec<&str> = v
                            .game_versions
                            .iter()
                            .rev()
                            .take(4)
                            .map(String::as_str)
                            .collect();
                        detail.push(games.join(", "));
                        if let Some(f) = file {
                            detail.push(format!("{} · {:.1} MB", f.name, f.size as f64 / 1e6));
                        }
                        index_row(
                            i + 1,
                            v.number.clone(),
                            Some(detail.join(" · ").into_any_element()),
                            list[i].verdict.as_ref().map(|verdict| {
                                caps(tr(verdict.label()))
                                    .text_color(t.muted)
                                    .into_any_element()
                            }),
                            &t,
                        )
                    },
                ),
            },
        };
        Some(page.child(body))
    }
}

impl Launcher {
    /// A registry native mod's version on its Versions tab: its number (and Recommended on the one
    /// the launcher would pick), its date and how it fits the Horizon played, and Install, or
    /// Installed on the one in the instance.
    fn mods_native_version_row(
        &self,
        p: &Project,
        v: &Version,
        fitted: &browser::horizon::Fitted,
        installed: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let name = match fitted.recommended {
            true => format!("{} · {}", v.number, tr("Recommended")),
            false => v.number.clone(),
        };
        let mut detail = vec![v.published.get(..10).unwrap_or_default().to_string()];
        detail.extend(fitted.fit.clone());
        let right = match installed {
            true => caps(tr("Installed")).text_color(t.muted).into_any_element(),
            false => {
                let (id, version) = (p.id.clone(), v.id.clone());
                button(
                    format!("mods-version-install-{}", v.id),
                    "Install",
                    if fitted.recommended {
                        Kind::Primary
                    } else {
                        Kind::Ghost
                    },
                    &t,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.mods_install_version(id.clone(), version.clone());
                    cx.notify();
                }))
                .into_any_element()
            }
        };
        index_row(
            0,
            name,
            Some(detail.join(" · ").into_any_element()),
            Some(right),
            &t,
        )
    }

    /// A gallery: image `shot` large, the previous and next ones a click away, and every image
    /// as a strip of thumbnails that picks the large one.
    fn mods_gallery(&mut self, gallery: &[Image], shot: usize, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let count = gallery.len();
        let shot = shot.min(count.saturating_sub(1));
        let pick = |to: usize| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                if let Some(o) = &mut this.mods.browse.open {
                    o.shot = to;
                }
                cx.notify();
            })
        };
        let image = |this: &mut Self, url: &str, w: f32, h: f32| match this
            .mods_image(url, images::GALLERY)
        {
            Some(path) => img(path)
                .w(px(w))
                .h(px(h))
                .object_fit(gpui::ObjectFit::Contain)
                .into_any_element(),
            None => plate(&t).w(px(w)).h(px(h)).into_any_element(),
        };
        let current = &gallery[shot];
        let large = image(self, &current.url, 760., 428.);
        let step = |id: &'static str, back: bool, to: usize| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(32.))
                .cursor_pointer()
                .child(
                    blocks::icon("chevron-right", 16., t.ink_soft).with_transformation(
                        gpui::Transformation::rotate(gpui::radians(if back {
                            std::f32::consts::PI
                        } else {
                            0.
                        })),
                    ),
                )
                .on_click(pick(to))
        };
        let mut out = div().flex().flex_col().gap(px(10.)).max_w(px(760.)).child(
            plate(&t)
                .id("mods-shot")
                .overflow_hidden()
                .cursor_pointer()
                .child(large)
                .on_click(pick((shot + 1) % count)),
        );
        if !current.title.is_empty() {
            out = out.child(
                text(theme::CAPTION)
                    .text_color(t.ink_soft)
                    .child(current.title.clone()),
            );
        }
        if count < 2 {
            return out;
        }
        let mut strip = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(step("mods-shot-back", true, (shot + count - 1) % count));
        for (i, g) in gallery.iter().enumerate() {
            let thumb = image(self, &g.url, 120., 68.);
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("mods-shot-{i}")))
                    .flex_none()
                    .overflow_hidden()
                    .border_2()
                    .border_color(if i == shot { t.accent } else { t.outline })
                    .cursor_pointer()
                    .child(thumb)
                    .on_click(pick(i)),
            );
        }
        strip = strip
            .child(step("mods-shot-next", false, (shot + 1) % count))
            .child(
                text(theme::CAPTION)
                    .text_color(t.muted)
                    .child(format!("{} / {count}", shot + 1)),
            );
        out.child(strip)
    }
}

impl Opened {
    /// A check can finish before or after the version request; retain either event order.
    fn apply_checks(&mut self) {
        let Some(Ok(versions)) = &mut self.versions else {
            return;
        };
        for row in Arc::make_mut(versions) {
            if let Some(verdict) = self.checked_versions.get(&row.version.id) {
                row.verdict = Some(*verdict);
            }
        }
    }
}
