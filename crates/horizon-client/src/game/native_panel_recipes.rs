//! A mod's recipes shown in its item panel's place (`native_panel_recipes`,
//! or `browser.set-recipes` of a WebAssembly mod, docs/mods.md), laid out by the game with its own slots, arrows and
//! buttons, paged to the panel's height, under a title and a back button.
//! The mod hears clicks on their slots, their action buttons, the back
//! button and page turns.
use super::{
    Game,
    native_hooks::{
        NativePanelEvent as BrowserEvent, NativeRecipeLayout as BrowserLayout,
        NativeRecipeView as BrowserRecipe, NativeStack as BrowserStack,
    },
    native_panel::{Layout, Owner, browser_stack, inside},
};
use horizon_client::{
    hud::{HudBatch, rgba},
    items::ItemStack,
    text::Text,
};
use std::time::Instant;

const SLOT: f32 = 18.0;
/// The title row's height, and the gap under each recipe.
const TITLE: f32 = 18.0;
/// A tab's step: an 18 px slot and a gap.
const TAB: f32 = 20.0;
const GAP: f32 = 6.0;
const FUEL_COLUMNS: usize = 10;
const FUEL_ROWS: usize = 4;
const FUEL_POPUP_SIZE: [f32; 2] = [216.0, 124.0];

#[derive(Clone, Copy)]
pub(super) struct FuelPopup {
    origin: (f32, f32),
    slot: [f32; 4],
    recipe: usize,
    row: usize,
    preview: usize,
}

impl FuelPopup {
    fn bounds(self) -> [f32; 4] {
        [
            self.origin.0,
            self.origin.1,
            FUEL_POPUP_SIZE[0],
            FUEL_POPUP_SIZE[1],
        ]
    }

    fn grid(self) -> (f32, f32) {
        (self.origin.0 + 10.0, self.origin.1 + 32.0)
    }

    fn hit(self, cursor: (f32, f32)) -> Option<usize> {
        let (x, y) = self.grid();
        if !inside(
            cursor,
            [x, y, FUEL_COLUMNS as f32 * SLOT, FUEL_ROWS as f32 * SLOT],
        ) {
            return None;
        }
        let column = ((cursor.0 - x) / SLOT) as usize;
        let row = ((cursor.1 - y) / SLOT) as usize;
        Some((self.row + row) * FUEL_COLUMNS + column)
    }

    fn marker_height(last_row: usize) -> f32 {
        (70.0 * FUEL_ROWS as f32 / (last_row + FUEL_ROWS) as f32)
            .round()
            .clamp(14.0, 70.0)
    }

    fn last_row(count: usize) -> usize {
        count.div_ceil(FUEL_COLUMNS).saturating_sub(FUEL_ROWS)
    }
}

/// Recipes a page may show, slots in one, stacks a slot cycles, and tabs.
const RECIPES: usize = 256;
const RECIPE_SLOTS: usize = 9;
const SLOT_STACKS: usize = 64;
const TABS: usize = 16;

/// What the panel shows instead of its items: a title, tabs (`tab` the
/// selected one) and the recipes.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RecipePage {
    pub(super) title: String,
    pub(super) tabs: Vec<BrowserStack>,
    pub(super) tab: u32,
    pub(super) recipes: Vec<BrowserRecipe>,
}
impl RecipePage {
    /// `native_panel_recipes`' page, cut as `set-recipes` cuts one; none
    /// without recipes (the items again).
    pub(super) fn new(
        title: &str,
        tabs: Vec<BrowserStack>,
        tab: u32,
        recipes: Vec<BrowserRecipe>,
    ) -> Option<Self> {
        let cycle = |mut list: Vec<BrowserStack>| {
            list.truncate(SLOT_STACKS);
            list
        };
        let recipes: Vec<BrowserRecipe> = recipes
            .into_iter()
            .take(RECIPES)
            .map(|r| BrowserRecipe {
                width: r.width.clamp(1, 3),
                height: r.height.clamp(1, 3),
                slots: r.slots.into_iter().take(RECIPE_SLOTS).map(cycle).collect(),
                results: cycle(r.results),
                ..r
            })
            .collect();
        (!recipes.is_empty()).then(|| Self {
            title: title.to_owned(),
            tabs: tabs.into_iter().take(TABS).collect(),
            tab,
            recipes,
        })
    }
}

/// One recipe as the game draws it.
pub(super) struct Shown {
    layout: BrowserLayout,
    width: usize,
    height: usize,
    /// Each slot's stacks (the result last), as the mod sent them and as
    /// stacks; a stack the game doesn't have is left out of the cycle.
    slots: Vec<Vec<(BrowserStack, ItemStack)>>,
    note: String,
    action: Option<bool>,
    has_fuel: bool,
}

/// The recipes a panel shows, and its page.
pub(super) struct Pages {
    title: String,
    /// The tabs' icons and the selected one.
    tabs: Vec<(BrowserStack, Option<ItemStack>)>,
    tab: usize,
    recipes: Vec<Shown>,
    page: usize,
    since: Instant,
    fuels: Vec<ItemStack>,
}

impl Pages {
    fn fuel_preview(&self) -> usize {
        self.since.elapsed().as_secs() as usize % self.fuels.len()
    }

    pub(super) fn new(
        items: &horizon_client::items::Items,
        providers: &horizon_client::providers::Providers,
        shown: &RecipePage,
        old: Option<Pages>,
        official: bool,
    ) -> Self {
        let (title, list, tab) = (&shown.title, &shown.recipes, shown.tab as usize);
        let station = shown
            .tabs
            .get(shown.tab as usize)
            .map(|stack| stack.item.as_str());
        let fuels = match station {
            Some("minecraft:furnace" | "minecraft:blast_furnace" | "minecraft:smoker")
                if official =>
            {
                let fast = station != Some("minecraft:furnace");
                // AnyFuel uses DataComponentLookup: first occurrence of each
                // component value sets the group order, then registry order within it.
                let mut groups: Vec<(_, Vec<_>)> = Vec::new();
                for id in &items.registry_order {
                    let Some(fuel) = items.get(id).and_then(|item| item.fuel.as_ref()) else {
                        continue;
                    };
                    let stack = ItemStack::new(id.as_ref(), 1);
                    if providers.burn_time(items, &stack, fast) <= 0 {
                        continue;
                    }
                    if let Some((_, group)) = groups.iter_mut().find(|(value, _)| *value == fuel) {
                        group.push(stack);
                    } else {
                        groups.push((fuel, vec![stack]));
                    }
                }
                groups.into_iter().flat_map(|(_, group)| group).collect()
            }
            _ => Vec::new(),
        };
        let kept = old.filter(|o| o.title == *title && o.tab == tab);
        let cycle = |stacks: &[BrowserStack]| -> Vec<(BrowserStack, ItemStack)> {
            stacks
                .iter()
                .filter_map(|s| Some((s.clone(), browser_stack(items, s)?)))
                .collect()
        };
        let recipes = list
            .iter()
            .map(|r| {
                let mut slots: Vec<_> = r.slots.iter().map(|s| cycle(s)).collect();
                slots.push(cycle(&r.results));
                Shown {
                    layout: r.layout,
                    width: r.width as usize,
                    height: r.height as usize,
                    slots,
                    note: r.note.clone(),
                    action: r.action,
                    has_fuel: !fuels.is_empty(),
                }
            })
            .collect();
        Self {
            title: title.clone(),
            tabs: shown
                .tabs
                .iter()
                .map(|t| (t.clone(), browser_stack(items, t)))
                .collect(),
            tab,
            recipes,
            page: kept.as_ref().map_or(0, |o| o.page),
            since: kept.map_or_else(Instant::now, |o| o.since),
            fuels,
        }
    }
}

impl Shown {
    /// The inputs' block: its size and each input slot's place in it.
    fn inputs(&self) -> ((f32, f32), Vec<(f32, f32)>) {
        let row = |n: usize| (0..n).map(|i| (i as f32 * SLOT, 0.0)).collect();
        match self.layout {
            BrowserLayout::Grid => {
                let (w, h) = (self.width.max(1), self.height.max(1));
                let at = (0..w * h)
                    .map(|i| ((i % w) as f32 * SLOT, (i / w) as f32 * SLOT))
                    .collect();
                ((w as f32 * SLOT, h as f32 * SLOT), at)
            }
            // The input over the flame.
            BrowserLayout::Cooking => (
                (
                    SLOT,
                    if self.has_fuel {
                        3.0 * SLOT
                    } else {
                        2.0 * SLOT
                    },
                ),
                vec![(0.0, 0.0)],
            ),
            BrowserLayout::Single => ((SLOT, SLOT), row(1)),
            BrowserLayout::Smithing => ((3.0 * SLOT, SLOT), row(3)),
            BrowserLayout::Brewing => ((2.0 * SLOT, SLOT), row(2)),
        }
    }
    /// The height of its slots, arrow and button, `room` wide.
    fn block(&self, room: f32) -> f32 {
        let h = self.inputs().0.1;
        let (_, _, action) = self.places(room);
        match self.action {
            Some(_) => h.max(action.1 + 12.0),
            None => h,
        }
    }
    fn height(&self, room: f32) -> f32 {
        let note = if self.note.is_empty() { 0.0 } else { 11.0 };
        self.block(room) + note + GAP
    }
    /// Every slot's place from the recipe's corner (the result last), the
    /// arrow's and the action button's: beside the result, or under it
    /// when the panel is `room` wide and that doesn't fit.
    fn places(&self, room: f32) -> (Vec<(f32, f32)>, (f32, f32), (f32, f32)) {
        let ((w, h), mut at) = self.inputs();
        // The mod's slots beyond the layout's are not shown.
        at.truncate(self.slots.len() - 1);
        let arrow = (w + 4.0, ((h - 16.0) / 2.0).floor());
        let result = (w + 32.0, ((h - SLOT) / 2.0).floor());
        let action = if result.0 + SLOT + 16.0 <= room {
            (result.0 + SLOT + 4.0, ((h - 12.0) / 2.0).floor())
        } else {
            (result.0 + 3.0, result.1 + SLOT + 2.0)
        };
        while at.len() < self.slots.len() - 1 {
            at.push((f32::NAN, f32::NAN));
        }
        at.push(result);
        (at, arrow, action)
    }
}

/// The tabs' places, in rows under the title row.
fn tab_places(pages: &Pages, layout: &Layout) -> Vec<(f32, f32)> {
    let per_row = ((layout.width() / TAB).floor() as usize).max(1);
    (0..pages.tabs.len())
        .map(|i| {
            (
                layout.grid.0 + (i % per_row) as f32 * TAB,
                layout.grid.1 + TITLE + 3.0 + (i / per_row) as f32 * TAB,
            )
        })
        .collect()
}
/// Where the recipes start: under the title row and the tabs.
fn recipes_top(pages: &Pages, layout: &Layout) -> f32 {
    let tabs = tab_places(pages, layout);
    tabs.last()
        .map_or(layout.grid.1 + TITLE, |&(_, y)| y + TAB + 3.0)
}

fn recipe_skin(
    batch: &mut HudBatch,
    atlas: &horizon_client::hud::HudAtlas,
    sprite: &str,
    rect: [f32; 4],
) {
    let [x, y, w, h] = rect;
    horizon_client::ui::nine_slice(batch, atlas, sprite, x, y, w, h, u32::MAX);
}

impl Game<'_> {
    /// The first recipe of each page, filling the panel from the tabs to
    /// the search box.
    fn recipe_pages(pages: &Pages, layout: &Layout) -> Vec<usize> {
        let room = layout.search[1] - 4.0 - recipes_top(pages, layout);
        let mut starts = vec![0];
        let mut used = 0.0;
        for (i, r) in pages.recipes.iter().enumerate() {
            let h = r.height(layout.width());
            if used > 0.0 && used + h > room {
                starts.push(i);
                used = 0.0;
            }
            used += h;
        }
        starts
    }
    /// The recipes on the open page, each with its corner.
    fn recipes_shown(&self, layout: &Layout) -> Vec<(usize, (f32, f32))> {
        let Some(pages) = &self.panel.recipes else {
            return Vec::new();
        };
        let starts = Self::recipe_pages(pages, layout);
        let page = pages.page.min(starts.len() - 1);
        let end = starts.get(page + 1).copied().unwrap_or(pages.recipes.len());
        let mut y = recipes_top(pages, layout);
        (starts[page]..end)
            .map(|i| {
                let at = (layout.grid.0, y);
                y += pages.recipes[i].height(layout.width());
                (i, at)
            })
            .collect()
    }
    fn back_button(&self, layout: &Layout) -> ([f32; 4], String) {
        let label = self.translate("gui.back", "Back");
        let w = HudBatch::text_width(&self.gui.atlas, &label) + 8.0;
        ([layout.grid.0, layout.grid.1, w, 14.0], label)
    }
    /// What a slot of a shown recipe holds now: its stacks cycle once a second.
    fn recipe_slot_now(&self, recipe: usize, slot: usize) -> Option<&(BrowserStack, ItemStack)> {
        let pages = self.panel.recipes.as_ref()?;
        let stacks = pages.recipes.get(recipe)?.slots.get(slot)?;
        let n = pages.since.elapsed().as_secs() as usize;
        stacks.get(n % stacks.len().max(1))
    }
    /// The tab under the pointer: its index and icon.
    pub(super) fn recipe_tab_hovered(
        &self,
        layout: &Layout,
    ) -> Option<(usize, BrowserStack, ItemStack)> {
        let pages = self.panel.recipes.as_ref()?;
        let places = tab_places(pages, layout);
        places.iter().enumerate().find_map(|(i, &(x, y))| {
            if !inside(self.cursor, [x, y, SLOT, SLOT]) {
                return None;
            }
            let (sent, stack) = pages.tabs.get(i)?;
            Some((i, sent.clone(), stack.clone()?))
        })
    }
    /// The recipe slot under the pointer: recipe, slot, and its stack now.
    pub(super) fn recipe_slot_hovered(
        &self,
        layout: &Layout,
    ) -> Option<(usize, usize, BrowserStack, ItemStack)> {
        let pages = self.panel.recipes.as_ref()?;
        self.recipes_shown(layout)
            .into_iter()
            .find_map(|(i, (x, y))| {
                let (at, _, _) = pages.recipes[i].places(layout.width());
                at.iter().enumerate().find_map(|(k, &(sx, sy))| {
                    if !inside(self.cursor, [x + sx, y + sy, SLOT, SLOT]) {
                        return None;
                    }
                    let (mod_stack, stack) = self.recipe_slot_now(i, k)?.clone();
                    Some((i, k, mod_stack, stack))
                })
            })
    }

    /// Draws the title row, the page's recipes and the page arrows.
    pub(super) fn draw_browser_recipes(&mut self, batch: &mut HudBatch, layout: &Layout) {
        let Some(pages) = &self.panel.recipes else {
            return;
        };
        let cursor = self.cursor;
        let starts = Self::recipe_pages(pages, layout);
        let page = pages.page.min(starts.len() - 1);
        let (title, pages_n) = (pages.title.clone(), starts.len());
        let (back, label) = self.back_button(layout);
        let atlas = &self.gui.atlas;
        let hover = inside(cursor, back);
        horizon_client::widgets::declare_action(horizon_client::widgets::Action::Back, back, true);
        let sprite = if hover {
            "gui/sprites/widget/button_highlighted"
        } else {
            "gui/sprites/widget/button"
        };
        horizon_client::ui::nine_slice(
            batch,
            atlas,
            sprite,
            back[0],
            back[1],
            back[2],
            back[3],
            u32::MAX,
        );
        batch.text(atlas, &label, back[0] + 4.0, back[1] + 3.0, u32::MAX, true);
        horizon_client::widgets::record(&label, back, true);
        let room = layout.width() - back[2] - 4.0;
        // Cut to the panel with vanilla's ellipsis (CommonComponents.ELLIPSIS).
        let shown = if atlas.font.fit(&title, room) >= title.chars().count() {
            title
        } else {
            let fit = atlas.font.fit(&title, room - atlas.font.width("..."));
            let kept: String = title.chars().take(fit).collect();
            format!("{}...", kept.trim_end())
        };
        batch.text(
            atlas,
            &shown,
            back[0] + back[2] + 4.0,
            back[1] + 3.0,
            u32::MAX,
            true,
        );
        self.draw_recipe_tabs(batch, layout);
        if pages_n > 1 {
            let labels = [
                self.translate("gui.recipebook.previous_page", "Previous Page"),
                self.translate("gui.recipebook.next_page", "Next Page"),
            ];
            let visible = [page > 0, page + 1 < pages_n];
            let atlas = &self.gui.atlas;
            for (((x, y), name), (label, visible)) in layout
                .arrows()
                .into_iter()
                .zip(["page_backward", "page_forward"])
                .zip(labels.iter().zip(visible))
            {
                if !visible {
                    continue;
                }
                let suffix = if inside(cursor, [x, y, 12.0, 17.0]) {
                    "_highlighted"
                } else {
                    ""
                };
                batch.sprite(
                    atlas,
                    &format!("gui/sprites/recipe_book/{name}{suffix}"),
                    x,
                    y,
                );
                horizon_client::widgets::record(label, [x, y, 12.0, 17.0], true);
            }
            let text = format!("{}/{pages_n}", page + 1);
            let w = HudBatch::text_width(atlas, &text);
            let x = (layout.grid.0 + (layout.width() - w) / 2.0).floor();
            batch.text(atlas, &text, x, layout.arrows_y + 5.0, u32::MAX, true);
        }
        for (i, (x, y)) in self.recipes_shown(layout) {
            let Some(r) = self.panel.recipes.as_ref().map(|p| &p.recipes[i]) else {
                break;
            };
            let (at, arrow, action) = r.places(layout.width());
            let h = r.block(layout.width());
            let (cooking, note, button) =
                (r.layout == BrowserLayout::Cooking, r.note.clone(), r.action);
            let atlas = &self.gui.atlas;
            for &(sx, sy) in at.iter().filter(|p| p.0.is_finite()) {
                batch.sprite(atlas, "gui/sprites/container/slot", x + sx, y + sy);
            }
            if cooking {
                self.draw_cooking_fuel(batch, x, y);
                batch.sprite(
                    atlas,
                    "gui/sprites/container/furnace/lit_progress",
                    x + 2.0,
                    y + SLOT + 2.0,
                );
            }
            batch.sprite(
                atlas,
                "gui/sprites/container/furnace/burn_progress",
                x + arrow.0,
                y + arrow.1,
            );
            if self.screen != super::Screen::Creative
                && let Some(enabled) = button
            {
                let rect = [x + action.0, y + action.1, 12.0, 12.0];
                let sprite = match (enabled, inside(cursor, rect)) {
                    (false, _) => "gui/sprites/widget/button_disabled",
                    (true, true) => "gui/sprites/widget/button_highlighted",
                    (true, false) => "gui/sprites/widget/button",
                };
                horizon_client::ui::nine_slice(
                    batch,
                    atlas,
                    sprite,
                    rect[0],
                    rect[1],
                    12.0,
                    12.0,
                    u32::MAX,
                );
                let color = if enabled {
                    u32::MAX
                } else {
                    rgba(160, 160, 160, 255)
                };
                batch.text(atlas, "+", rect[0] + 3.0, rect[1] + 2.0, color, true);
                horizon_client::widgets::record(&format!("+ {i}"), rect, enabled);
            }
            if !note.is_empty() {
                batch.text(atlas, &note, x, y + h + 2.0, rgba(224, 224, 224, 255), true);
            }
            for (k, &(sx, sy)) in at.iter().enumerate().filter(|(_, p)| p.0.is_finite()) {
                let Some((_, stack)) = self.recipe_slot_now(i, k).cloned() else {
                    continue;
                };
                self.draw_item(batch, &stack, x + sx + 1.0, y + sy + 1.0);
                if inside(cursor, [x + sx, y + sy, SLOT, SLOT]) {
                    batch.fill(
                        &self.gui.atlas,
                        x + sx + 1.0,
                        y + sy + 1.0,
                        16.0,
                        16.0,
                        rgba(255, 255, 255, 128),
                    );
                }
                let name = self.pack.names.display_item(&stack.item);
                horizon_client::widgets::record_as(
                    &format!("{name} {i}:{k}"),
                    [x + sx + 1.0, y + sy + 1.0, 16.0, 16.0],
                    true,
                    horizon_client::widgets::Kind::Button,
                );
            }
        }
        if let Some((key, fallback)) = self.panel.transfer_warning {
            let warning = self.translate(key, fallback);
            let lines = [(
                horizon_client::text::Text::literal(warning.clone()).resolve_legacy(),
                rgba(255, 100, 100, 255),
            )];
            let (x, y, w, h) = Self::tooltip_geometry(
                &self.gui.atlas,
                &lines,
                cursor,
                (batch.width, batch.height),
                [0.0; 2],
            );
            horizon_client::widgets::record(&warning, [x, y, w, h], false);
            self.draw_tooltip_lines(batch, &lines, cursor.0, cursor.1);
        }
    }

    fn draw_cooking_fuel(&self, batch: &mut HudBatch, x: f32, y: f32) {
        let Some(pages) = &self.panel.recipes else {
            return;
        };
        let atlas = &self.gui.atlas;
        if !pages.fuels.is_empty() {
            let fuel_y = y + 36.0;
            let index = self
                .panel
                .fuel_popup
                .filter(|popup| popup.slot[..2] == [x, fuel_y])
                .map_or_else(|| pages.fuel_preview(), |popup| popup.preview);
            batch.sprite(atlas, "horizon/jei/slot", x, fuel_y);
            self.draw_item(batch, &pages.fuels[index], x + 1.0, fuel_y + 1.0);
            if pages.fuels.len() > 1 {
                batch.sprite(atlas, "horizon/jei/icons/list_badge", x + 9.0, fuel_y);
            }
        }
    }

    pub(super) fn draw_recipe_fuels(&mut self, batch: &mut HudBatch, layout: &Layout) -> bool {
        let held = self.has_shift_down();
        let cursor = self.cursor;
        if self.panel.fuel_popup.is_some_and(|popup| {
            !inside(cursor, popup.slot) && !(held && inside(cursor, popup.bounds()))
        }) {
            self.panel.fuel_popup = None;
        }
        let Some(pages) = self
            .panel
            .recipes
            .as_ref()
            .filter(|pages| pages.fuels.len() > 1)
        else {
            return false;
        };
        if self.panel.fuel_popup.is_none() {
            let Some((recipe, slot)) =
                self.recipes_shown(layout)
                    .into_iter()
                    .find_map(|(recipe, (x, y))| {
                        let slot = [x, y + 36.0, SLOT, SLOT];
                        (pages.recipes[recipe].layout == BrowserLayout::Cooking
                            && inside(cursor, slot))
                        .then_some((recipe, slot))
                    })
            else {
                return false;
            };
            let preview = pages.fuel_preview();
            let origin = (
                (slot[0] + 14.0)
                    .min(batch.width - FUEL_POPUP_SIZE[0] - 2.0)
                    .max(2.0),
                (slot[1] - 10.0)
                    .min(batch.height - FUEL_POPUP_SIZE[1] - 2.0)
                    .max(2.0),
            );
            self.panel.fuel_popup = Some(FuelPopup {
                origin,
                slot,
                recipe,
                row: 0,
                preview,
            });
        }
        let popup = self.panel.fuel_popup.unwrap();
        let [x, y, w, h] = popup.bounds();
        let atlas = &self.gui.atlas;
        let hint = if held {
            Vec::new()
        } else {
            let label = self.translate(
                "horizon.recipes.fuel.options",
                "Hold SHIFT to pin this tooltip, then click an ingredient to view recipes.",
            );
            atlas
                .font
                .wrap_components(&Text::literal(label).italic(), w as i32 - 8)
        };
        let preview_shift = if held { 0.0 } else { -2.0 };
        if held {
            batch.fill(
                atlas,
                0.0,
                0.0,
                batch.width,
                batch.height,
                rgba(0, 0, 0, 64),
            );
            recipe_skin(
                batch,
                atlas,
                "horizon/jei/interactive_ingredient_tooltip_background",
                [x, y, w, h],
            );
        }
        let text_x = x + 6.0;
        let frame = [
            x - 6.0,
            y - 6.0,
            w + 12.0,
            h + 12.0 + preview_shift + hint.len() as f32 * 10.0,
        ];
        for skin in [
            "gui/sprites/tooltip/background",
            "gui/sprites/tooltip/frame",
        ] {
            recipe_skin(batch, atlas, skin, frame);
        }
        let stack = &pages.fuels[popup.preview];
        batch.text_styled(
            atlas,
            &Text::literal(self.translate("horizon.recipes.fuel.any", "Any Fuel")).italic(),
            text_x,
            y + 6.0,
            rgba(255, 170, 0, 255),
            true,
        );
        batch.text(
            atlas,
            &self.pack.names.display_item(&stack.item),
            text_x,
            y + 18.0 + preview_shift,
            u32::MAX,
            true,
        );
        let (gx, gy) = popup.grid();
        let gy = gy + preview_shift;
        recipe_skin(
            batch,
            atlas,
            "horizon/jei/recipe_preview_background_v2",
            [gx - 4.0, gy - 4.0, 204.0, 82.0],
        );
        let first = popup.row * FUEL_COLUMNS;
        for (i, fuel) in pages
            .fuels
            .iter()
            .skip(first)
            .take(FUEL_COLUMNS * FUEL_ROWS)
            .enumerate()
        {
            let sx = gx + (i % FUEL_COLUMNS) as f32 * SLOT;
            let sy = gy + (i / FUEL_COLUMNS) as f32 * SLOT;
            batch.sprite(
                atlas,
                "horizon/jei/ingredient_list_slot_background_v2",
                sx,
                sy,
            );
            self.draw_item(batch, fuel, sx + 1.0, sy + 1.0);
            if held && inside(cursor, [sx, sy, SLOT, SLOT]) {
                batch.fill(
                    atlas,
                    sx + 1.0,
                    sy + 1.0,
                    16.0,
                    16.0,
                    rgba(255, 255, 255, 96),
                );
            }
            horizon_client::widgets::record(
                &self.pack.names.display_item(&fuel.item),
                [sx, sy, SLOT, SLOT],
                held,
            );
        }
        let last = FuelPopup::last_row(pages.fuels.len());
        if last > 0 {
            recipe_skin(
                batch,
                atlas,
                "horizon/jei/scrollbar_background_v2",
                [gx + 182.0, gy, 14.0, 72.0],
            );
            let marker = FuelPopup::marker_height(last);
            let offset = (popup.row as f32 * (70.0 - marker) / last as f32).round();
            recipe_skin(
                batch,
                atlas,
                "horizon/jei/scrollbar_marker_v2",
                [gx + 183.0, gy + 1.0 + offset, 12.0, marker],
            );
        }
        let mut footer = y + 110.0 + preview_shift;
        for line in &hint {
            batch.text_styled(atlas, line, text_x, footer, rgba(170, 170, 170, 255), true);
            footer += 10.0;
        }
        if let Some((namespace, _)) = stack.item.split_once(':') {
            let name = if namespace == "minecraft" {
                self.translate("horizon.recipes.fuel.minecraft", "Minecraft")
            } else {
                namespace.to_owned()
            };
            batch.text_styled(
                atlas,
                &Text::literal(name).italic(),
                text_x,
                footer,
                rgba(85, 85, 255, 255),
                true,
            );
        }
        true
    }

    pub(super) fn recipe_fuel_scroll(&mut self, amount: f32) -> bool {
        if self.has_shift_down()
            && let Some(popup) = self.panel.fuel_popup.as_mut()
            && inside(self.cursor, popup.bounds())
            && let Some(pages) = &self.panel.recipes
        {
            let step = amount.abs().ceil() as usize;
            popup.row = if amount > 0.0 {
                popup.row.saturating_sub(step)
            } else {
                popup
                    .row
                    .saturating_add(step)
                    .min(FuelPopup::last_row(pages.fuels.len()))
            };
            return true;
        }
        false
    }

    pub(super) fn recipe_fuel_press(&mut self, owner: &Owner, right: bool) -> bool {
        if !self.has_shift_down()
            || !matches!(
                self.screen,
                super::Screen::Inventory | super::Screen::Creative
            )
        {
            return false;
        }
        let Some(popup) = self
            .panel
            .fuel_popup
            .filter(|popup| inside(self.cursor, popup.bounds()))
        else {
            return false;
        };
        let Some(pages) = &self.panel.recipes else {
            return false;
        };
        let (gx, gy) = popup.grid();
        let last = FuelPopup::last_row(pages.fuels.len());
        if !right && last > 0 && inside(self.cursor, [gx + 182.0, gy, 14.0, 72.0]) {
            let marker = FuelPopup::marker_height(last);
            let fraction =
                ((self.cursor.1 - gy - 1.0 - marker / 2.0) / (70.0 - marker)).clamp(0.0, 1.0);
            let row = (fraction * last as f32).round() as usize;
            self.panel.fuel_popup = Some(FuelPopup { row, ..popup });
        } else if let Some(fuel) = popup.hit(self.cursor).and_then(|i| pages.fuels.get(i)) {
            let event = BrowserEvent::SlotClicked {
                recipe: popup.recipe as u32,
                slot: pages.recipes[popup.recipe].slots.len() as u32,
                stack: BrowserStack {
                    item: fuel.item.to_string(),
                    count: 1,
                    components: String::new(),
                },
                right,
            };
            self.panel.fuel_popup = None;
            self.panel_event(owner, event);
        }
        true
    }

    /// The tabs, on slots; the selected one framed.
    fn draw_recipe_tabs(&mut self, batch: &mut HudBatch, layout: &Layout) {
        let Some(pages) = &self.panel.recipes else {
            return;
        };
        let places = tab_places(pages, layout);
        let tabs: Vec<Option<ItemStack>> = pages.tabs.iter().map(|t| t.1.clone()).collect();
        let selected = pages.tab;
        for (i, (&(x, y), stack)) in places.iter().zip(tabs).enumerate() {
            let atlas = &self.gui.atlas;
            batch.sprite(atlas, "gui/sprites/container/slot", x, y);
            let Some(stack) = stack else {
                continue;
            };
            self.draw_item(batch, &stack, x + 1.0, y + 1.0);
            // The selected tab: a white frame around its slot.
            if i == selected {
                let white = rgba(255, 255, 255, 255);
                let atlas = &self.gui.atlas;
                batch.fill(atlas, x - 1.0, y - 1.0, SLOT + 2.0, 1.0, white);
                batch.fill(atlas, x - 1.0, y + SLOT, SLOT + 2.0, 1.0, white);
                batch.fill(atlas, x - 1.0, y, 1.0, SLOT, white);
                batch.fill(atlas, x + SLOT, y, 1.0, SLOT, white);
            }
            if inside(self.cursor, [x, y, SLOT, SLOT]) {
                batch.fill(
                    &self.gui.atlas,
                    x + 1.0,
                    y + 1.0,
                    16.0,
                    16.0,
                    rgba(255, 255, 255, 128),
                );
            }
            let name = self.pack.names.display_item(&stack.item);
            let rect = [x + 1.0, y + 1.0, 16.0, 16.0];
            let kind = horizon_client::widgets::Kind::Tab(i == selected);
            horizon_client::widgets::record_as(&format!("{name} tab"), rect, true, kind);
            // The kind reaches the bumpers only as a target (`key_focus::drawn_targets`).
            horizon_client::widgets::target_as(rect, kind);
        }
    }

    /// A press on the recipes: true when it hit one of their parts.
    pub(super) fn browser_recipes_press(
        &mut self,
        layout: &Layout,
        owner: &Owner,
        right: bool,
    ) -> bool {
        let Some(pages) = &self.panel.recipes else {
            return false;
        };
        let cursor = self.cursor;
        let starts = Self::recipe_pages(pages, layout);
        let page = pages.page.min(starts.len() - 1);
        let [back_arrow, forward] = layout.arrows();
        let turn = if page > 0 && inside(cursor, [back_arrow.0, back_arrow.1, 12.0, 17.0]) {
            Some(page - 1)
        } else if page + 1 < starts.len() && inside(cursor, [forward.0, forward.1, 12.0, 17.0]) {
            Some(page + 1)
        } else {
            None
        };
        if let Some(to) = turn {
            if let Some(p) = self.panel.recipes.as_mut() {
                p.page = to;
            }
            self.audio
                .play("minecraft:ui.button.click", None, 0.25, 1.0);
            self.panel_event(owner, BrowserEvent::Page(starts[to] as u32));
            return true;
        }
        if let Some((tab, _, _)) = self.recipe_tab_hovered(layout) {
            self.audio
                .play("minecraft:ui.button.click", None, 0.25, 1.0);
            self.panel_event(owner, BrowserEvent::Tab(tab as u32));
            return true;
        }
        if inside(cursor, self.back_button(layout).0) {
            self.audio
                .play("minecraft:ui.button.click", None, 0.25, 1.0);
            self.panel_event(owner, BrowserEvent::Back);
            return true;
        }
        if let Some((recipe, slot, stack, _)) = self.recipe_slot_hovered(layout) {
            let event = BrowserEvent::SlotClicked {
                recipe: recipe as u32,
                slot: slot as u32,
                stack,
                right,
            };
            self.panel_event(owner, event);
            return true;
        }
        let action = self.recipes_shown(layout).into_iter().find(|&(i, (x, y))| {
            let r = &pages.recipes[i];
            let (_, _, (ax, ay)) = r.places(layout.width());
            self.screen != super::Screen::Creative
                && r.action.is_some()
                && inside(cursor, [x + ax, y + ay, 12.0, 12.0])
        });
        if let Some((i, _)) = action {
            if pages.recipes[i].action == Some(true) {
                self.audio
                    .play("minecraft:ui.button.click", None, 0.25, 1.0);
                self.panel_event(owner, BrowserEvent::Action(i as u32));
            }
            return true;
        }
        false
    }
}
