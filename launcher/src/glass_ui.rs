//! The glass look's controls ("Glass" in `docs/design-system.md`): the parts of the mockup's
//! `components/launcher/launcher-ui.tsx` and its shell (`launcher-app.tsx`), at its sizes. White
//! text and fills at a few opacities over [`glass`](crate::glass) plates; Geist throughout.
use crate::{hover_fade, hover_is, keyboard, note_hover, theme::Theme, track_hover, Kind};
use gpui::{
    div, prelude::*, px, rgb, rgba, svg, App, ClickEvent, Div, FontWeight, Hsla, SharedString,
    Stateful, Svg, Window,
};
use horizon_theme::glass::{fill, ink, radius, Glass};
use std::cell::Cell;

thread_local! {
    /// Whether this frame's pages draw the glass look (the dark appearance).
    static ON: Cell<bool> = const { Cell::new(false) };
    /// The look's colours for the player's accent.
    static LOOK: Cell<Glass> = Cell::new(Glass::default_look());
    /// Whether the rows being built go in a Settings card ([`flat_rows`]).
    static FLAT: Cell<bool> = const { Cell::new(false) };
    /// The light appearance: the glass look's white ink and fills turn to [`INK`].
    static LIGHT: Cell<bool> = const { Cell::new(false) };
}

/// The light appearance's ink: the mockup's text on white (`#100a1f`), at the opacities the dark
/// look gives white.
const INK: u32 = horizon_theme::glass::ON_LIGHT;

/// Set as each frame's pages start being built: whether the appearance is light.
pub fn set_light(light: bool) {
    LIGHT.set(light);
}

/// Whether the light appearance is drawn.
pub fn light() -> bool {
    LIGHT.get()
}

/// A solid control's fill (`SOLID`'s white, the sliders' knob): white, or the ink in light.
pub fn paper() -> Hsla {
    if light() {
        color(INK)
    } else {
        gpui::white()
    }
}

/// Text on [`paper`].
pub fn on_paper() -> Hsla {
    if light() {
        gpui::white()
    } else {
        color(INK)
    }
}

/// The look's green (`--live`): darker in light, to read on white plates.
pub fn live() -> Hsla {
    color(if light() {
        0x15803d
    } else {
        horizon_theme::glass::LIVE
    })
}

/// The look's amber (`--warn`).
pub fn warn() -> Hsla {
    color(if light() {
        0xb45309
    } else {
        horizon_theme::glass::WARN
    })
}

/// The look's red (`--danger`).
pub fn danger() -> Hsla {
    color(if light() {
        0xdc2626
    } else {
        horizon_theme::glass::DANGER
    })
}

/// Builds `f`'s rows as the mockup's `SettingRow`s, for a Settings card: `index_row` and
/// `section_title` ask [`flat`].
pub fn flat_rows<R>(f: impl FnOnce() -> R) -> R {
    let was = FLAT.replace(true);
    let built = f();
    FLAT.set(was);
    built
}

/// Whether rows are being built for a Settings card.
pub fn flat() -> bool {
    FLAT.get()
}

/// A Settings row (`SettingRow`): the name in 15 px semibold, its note in 13 px on 1.5 at 60 %,
/// the control on the right, 40 px away; 16 px around, no fill.
pub fn setting_row(
    title: impl IntoElement,
    note: Option<gpui::AnyElement>,
    control: Option<gpui::AnyElement>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(40.))
        .px(px(16.))
        .py(px(16.))
        .rounded(px(radius::LARGE))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(4.))
                .child(
                    text(15., FontWeight::SEMIBOLD, ink::FULL)
                        .line_height(px(21.))
                        .child(title),
                )
                .children(note.map(|n| {
                    text(13., FontWeight::NORMAL, 0.6)
                        .line_height(px(19.5))
                        .max_w(px(560.))
                        .child(n)
                })),
        )
        .children(control.map(|c| div().flex().flex_none().items_center().gap(px(8.)).child(c)))
}

/// A stacked Settings row (`SettingRow stacked`): the name and note, the control under them at
/// full width.
pub fn setting_row_stacked(
    title: impl IntoElement,
    note: Option<gpui::AnyElement>,
    control: gpui::AnyElement,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .px(px(16.))
        .py(px(16.))
        .rounded(px(radius::LARGE))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    text(15., FontWeight::SEMIBOLD, ink::FULL)
                        .line_height(px(21.))
                        .child(title),
                )
                .children(note.map(|n| {
                    text(13., FontWeight::NORMAL, 0.6)
                        .line_height(px(19.5))
                        .max_w(px(580.))
                        .child(n)
                })),
        )
        .child(div().w_full().child(control))
}

/// The mockup's `WIDE_FIELD` look on a field built for the old one: 44 px, up to 560 px wide,
/// 12 px corners, white at 7 % (11 % while typed in), 14 px Geist.
pub fn wide_field(field: Stateful<Div>, focused: bool) -> Stateful<Div> {
    field
        .w_full()
        .max_w(px(560.))
        .h(px(44.))
        .px(px(16.))
        .border_0()
        .rounded(px(radius::ROW))
        .bg(white(if focused { 0.11 } else { 0.07 }))
        .font_family(horizon_theme::glass::FONT)
        .text_size(px(14.))
}

/// Whether the pages being built draw the glass look: helpers with no theme at hand ask.
pub fn on() -> bool {
    ON.get()
}

/// Set as each frame's pages start being built.
pub fn set_on(on: bool) {
    ON.set(on);
}

/// Set as each frame's pages start being built: the colours for the player's accent.
pub fn set_look(look: Glass) {
    LOOK.set(look);
}

/// The look's colours.
pub fn look() -> Glass {
    LOOK.get()
}

/// White at `a`: the dark look's ink and fills. In light, the ink at `a`, so every part keeps
/// its weight on the light plates.
pub fn white(a: f32) -> Hsla {
    if light() {
        color(INK).opacity(a)
    } else {
        gpui::white().opacity(a)
    }
}

/// A colour of the look (`0xrrggbb`).
pub fn color(c: u32) -> Hsla {
    rgb(c).into()
}

/// Text at `size` px in Geist, white at `a`, at `weight`.
pub fn text(size: f32, weight: FontWeight, a: f32) -> Div {
    div()
        .font_family(horizon_theme::glass::FONT)
        .text_size(px(size))
        .line_height(px((size * 1.4).round()))
        .font_weight(weight)
        .text_color(white(a))
}

/// One of the mockup's Hugeicons, stroked in `color`.
pub fn icon(name: &'static str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/glass/{name}.svg"))
        .flex_none()
        .size(px(size))
        .text_color(color)
}

/// A layer filling its parent, round at `r`, for hover fills.
fn layer(r: f32) -> impl Fn() -> Div {
    move || div().absolute().top_0().left_0().size_full().rounded(px(r))
}

/// `id`'s fill eased between `from` (white at that opacity) and `to` while hovered, as the
/// mockup's `transition-colors` does.
fn hover_fill(id: &SharedString, r: f32, from: f32, to: f32) -> gpui::AnyElement {
    hover_fade(id, layer(r), move |d, t| d.bg(white((to - from) * t)))
}

/// The state dot before a row's second line (`Dot`): live green, amber for busy (the mockup
/// pulses it) and for a warning, the accent's soft shade (given) for an update, white at 35 % for
/// nothing yet.
#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Live,
    Busy,
    Warn,
    Accent(u32),
    Off,
}

pub fn dot(tone: Tone) -> Div {
    let c = match tone {
        Tone::Live => live(),
        Tone::Busy | Tone::Warn => warn(),
        Tone::Accent(soft) => color(soft),
        Tone::Off => white(0.35),
    };
    div().flex_none().size(px(7.)).rounded_full().bg(c)
}

/// The progress bar (`Bar`): 4 px, white at 12 %, filled in the accent's soft shade.
pub fn bar(value: f32, accent_soft: u32) -> Div {
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .bg(white(fill::ACTIVE))
        .child(
            div()
                .h_full()
                .rounded_full()
                .w(gpui::relative(value.clamp(0., 1.)))
                .bg(color(accent_soft)),
        )
}

/// A sidebar row (an instance, a server): `rounded-[12px] p-2`, white at 10 % when it is the one
/// shown, 5 % under the pointer. Its icon and lines go in as children; `pr` leaves room for a
/// button laid over its right end.
pub fn row(id: impl Into<SharedString>, current: bool, pr: f32, t: &Theme) -> Stateful<Div> {
    let id = id.into();
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .items_center()
        .gap(px(12.))
        .w_full()
        .p(px(8.))
        .pr(px(pr))
        .rounded(px(radius::ROW))
        .cursor_pointer();
    let d = if current {
        d.bg(white(fill::SELECTED))
    } else {
        d.child(hover_fill(&id, radius::ROW, 0., fill::HOVER))
    };
    keyboard(track_hover(d, id), t)
}

/// A row's two lines: the name (14 px semibold) and its state after a dot (12.5 px, white at
/// 65 %), both at the page's 1.5 line height.
pub fn row_text(name: impl Into<SharedString>, state: (Tone, String)) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(
            text(14., FontWeight::SEMIBOLD, ink::FULL)
                .line_height(px(21.))
                .truncate()
                .child(name.into()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w_0()
                .child(dot(state.0))
                .child(
                    text(12.5, FontWeight::NORMAL, ink::MUTED)
                        .line_height(px(18.75))
                        .truncate()
                        .child(state.1),
                ),
        )
}

/// A square button with one icon (`ICON_BUTTON`): white at 65 %, white with a 8 % fill under
/// the pointer, white with 12 % while `active`.
pub fn icon_button(
    id: impl Into<SharedString>,
    name: &'static str,
    size: f32,
    icon_size: f32,
    r: f32,
    active: bool,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let group = SharedString::from(format!("{id}-group"));
    let d = div()
        .id(id.clone())
        .group(group.clone())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(r))
        .cursor_pointer();
    let d = if active {
        d.bg(white(fill::ACTIVE))
    } else {
        d.child(hover_fill(&id, r, 0., fill::BUTTON))
    };
    let glyph = icon(
        name,
        icon_size,
        white(if active { ink::FULL } else { ink::MUTED }),
    )
    .relative()
    .group_hover(group, |s| s.text_color(white(ink::FULL)));
    keyboard(track_hover(d.child(glyph), id), t)
}

/// One tab of a tab strip (`Tabs`, size `md`): 36 px tall, 15 px; the open one white on 12 %,
/// medium; the others white at 70 %, white under the pointer.
pub fn tab(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    open: bool,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let d = div()
        .id(id.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(36.))
        .px(px(14.))
        .rounded(px(radius::CONTROL))
        .cursor_pointer()
        .font_family(horizon_theme::glass::FONT)
        .text_size(px(15.))
        .line_height(px(20.));
    let d = if open {
        d.bg(white(fill::ACTIVE))
            .font_weight(FontWeight::MEDIUM)
            .text_color(white(ink::FULL))
    } else {
        d.text_color(white(ink::BODY))
            .hover(|s| s.text_color(white(ink::FULL)))
    };
    let label: SharedString = label.into();
    let d = d.when(!label.is_empty(), |d| d.child(label));
    keyboard(track_hover(d, id), t)
}

/// A tab with an icon and a count (`Tabs`, size `md`): as [`tab`], the icon 16 px before the
/// label, the count after it in white at 45 %.
pub fn count_tab(
    id: impl Into<SharedString>,
    icon_name: Option<&'static str>,
    label: impl Into<SharedString>,
    count: Option<usize>,
    open: bool,
    t: &Theme,
) -> Stateful<Div> {
    let colour = white(if open { ink::FULL } else { ink::BODY });
    tab(id, "", open, t)
        .children(icon_name.map(|n| icon(n, 16., colour)))
        .child(label.into())
        .children(count.map(|n| div().text_color(white(ink::DIM)).child(n.to_string())))
}

/// A popover's row (`MENU_ROW`): 40 px, 14 px text after a 17 px icon at 60 %, white at 7 %
/// under the pointer. `danger` colours it as the mockup's Delete.
pub fn menu_row(
    id: impl Into<SharedString>,
    name: &'static str,
    label: impl Into<SharedString>,
    danger: bool,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let (fg, icon_color) = if danger {
        let c = self::danger();
        (c, c)
    } else {
        (white(ink::FULL), white(ink::NOTE))
    };
    let hover_color = if danger {
        rgba(horizon_theme::glass::DANGER_HOVER << 8 | 0x1f).into()
    } else {
        white(0.07)
    };
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .items_center()
        .gap(px(12.))
        .h(px(40.))
        .w_full()
        .px(px(12.))
        .rounded(px(radius::CONTROL))
        .cursor_pointer()
        .child(hover_fade(&id, layer(radius::CONTROL), move |d, a| {
            d.bg(hover_color.opacity(hover_color.a * a))
        }))
        .child(icon(name, 17., icon_color).relative())
        .child(
            text(14., FontWeight::NORMAL, 1.)
                .relative()
                .text_color(fg)
                .child(label.into()),
        );
    keyboard(track_hover(d, id), t)
}

/// A popover (`POPOVER`): a float plate round at 16 with 6 px around its rows.
pub fn popover(width: f32) -> Div {
    crate::glass::float(radius::POPOVER)
        .w(px(width))
        .p(px(6.))
        .flex()
        .flex_col()
}

/// What a dialog or the palette lays over the launcher (`bg-[#07050f]/50 backdrop-blur-xl`):
/// GPUI can't blur what it drew, so the scene's blurred copy (a menu's, `hzl-float`) covers the
/// page. The scrim goes over it.
pub fn scrim_backdrop() -> impl IntoElement {
    crate::glass::backdrop(crate::glass::Kind::Float, 0.)
}

/// The scrim over [`scrim_backdrop`]: 50 % in the dark look, 20 % in light, so the dimmed
/// launcher stays day-lit.
pub fn scrim() -> Hsla {
    color(horizon_theme::glass::SCRIM).opacity(if light() { 0.2 } else { 0.5 })
}

/// A dialog's plate, round at `radius` (`hzl-glass` over the scrim): its piece of the scene
/// under the scrim's dimming, then the plates' white. The content goes after it.
pub fn dialog_plate(radius: f32) -> Div {
    let layer = |c: Hsla| {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded(px(radius))
            .bg(c)
    };
    crate::glass::plate(radius)
        // Light plates are already day-lit: no dimming under them.
        .child(layer(
            color(horizon_theme::glass::SCRIM).opacity(if light() { 0. } else { 0.5 }),
        ))
        .child(layer(white(horizon_theme::glass::TINT)))
}

/// Text on a white button (`SOLID`): `#100a1f`, medium.
pub fn solid(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    h: f32,
    size: f32,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    keyboard(
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .h(px(h))
            .px(px(16.))
            .rounded(px(radius::CONTROL))
            .bg(paper())
            .cursor_pointer()
            .hover(|s| s.opacity(0.9))
            .font_family(horizon_theme::glass::FONT)
            .text_size(px(size))
            .font_weight(FontWeight::MEDIUM)
            .text_color(on_paper())
            .child(label.into()),
        t,
    )
}

/// Horizon's mark on its dark tile (`MarkTile`): the mark 64 % of the tile, round at 26 %; the
/// stems white, their reflection white at 40 %, the horizon in the accent's soft shade.
pub fn mark_tile(size: f32, accent_soft: u32) -> Div {
    let inner = size * 0.64;
    let u = inner / 16.;
    let bar = |x: f32, y: f32, w: f32, h: f32, c: Hsla| {
        div()
            .absolute()
            .left(px(x * u))
            .top(px(y * u))
            .w(px(w * u))
            .h(px(h * u))
            .bg(c)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px((size * 0.26).round()))
        .bg(color(0x0c0818))
        .child(
            div()
                .relative()
                .size(px(inner))
                .child(bar(4., 3., 2., 4., gpui::white()))
                .child(bar(10., 3., 2., 4., gpui::white()))
                .child(bar(4., 8., 2., 4., gpui::white().opacity(0.4)))
                .child(bar(10., 8., 2., 4., gpui::white().opacity(0.4)))
                .child(bar(1., 7., 14., 1., color(accent_soft))),
        )
}

/// A big square button with one icon (`BIG_ICON`): 56 px, white at 8 %, 13 % under the pointer;
/// the glyph white at 80 %, white under the pointer.
pub fn big_icon(id: impl Into<SharedString>, name: &'static str, t: &Theme) -> Stateful<Div> {
    let id = id.into();
    let group = SharedString::from(format!("{id}-group"));
    let glyph = icon(name, 21., white(ink::STRONG))
        .group_hover(group.clone(), |s| s.text_color(white(ink::FULL)));
    keyboard(
        div()
            .id(id)
            .group(group)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(56.))
            .rounded(px(radius::LARGE))
            .bg(white(fill::BUTTON))
            .hover(|s| s.bg(white(0.13)))
            .cursor_pointer()
            .child(glyph),
        t,
    )
}

/// A menu row with a note under its label (`MENU_ITEM`): 14 px medium over 12 px at 55 %, white
/// at 7 % under the pointer; at 40 % when `locked`. A mark (a tick) can follow on the right.
pub fn menu_item(
    id: impl Into<SharedString>,
    label: SharedString,
    note: String,
    locked: bool,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .w_full()
        .px(px(12.))
        .py(px(8.))
        .rounded(px(radius::CONTROL))
        .when(!locked, |d| {
            d.cursor_pointer()
                .child(hover_fade(&id, layer(radius::CONTROL), |d, a| {
                    d.bg(white(0.07 * a))
                }))
        })
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    text(14., FontWeight::MEDIUM, ink::FULL)
                        .line_height(px(21.))
                        .child(label),
                )
                .child(
                    text(12., FontWeight::NORMAL, ink::QUIET)
                        .line_height(px(18.))
                        .truncate()
                        .child(note),
                ),
        );
    if locked {
        return d.opacity(0.4);
    }
    keyboard(track_hover(d, id), t)
}

/// A quiet button (`QUIET`): white at 8 %, 14 % under the pointer, 14 px medium, `h` tall.
pub fn quiet(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    h: f32,
    t: &Theme,
) -> Stateful<Div> {
    quiet_icon(id, None, label, h, t)
}

/// [`quiet`] with a 16 px icon before its label.
pub fn quiet_icon(
    id: impl Into<SharedString>,
    name: Option<&'static str>,
    label: impl Into<SharedString>,
    h: f32,
    t: &Theme,
) -> Stateful<Div> {
    keyboard(
        div()
            .id(id.into())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .h(px(h))
            .px(px(14.))
            .rounded(px(radius::CONTROL))
            .bg(white(fill::BUTTON))
            .hover(|s| s.bg(white(0.14)))
            .cursor_pointer()
            .font_family(horizon_theme::glass::FONT)
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(white(ink::FULL))
            .children(name.map(|n| icon(n, 16., white(ink::FULL))))
            .child(label.into()),
        t,
    )
}

/// A list row (`ROW`): `rounded-[12px] px-3 py-2.5`, white at 6 % under the pointer. `on_hover`
/// hears the pointer come and go too, for rows that show their actions then. `held`: the row
/// keeps its fill while the pointer is on one of its buttons or its menu is open, as CSS's
/// `:hover` stays on a parent.
pub fn list_row(
    id: impl Into<SharedString>,
    held: bool,
    on_hover: impl Fn(&bool, &mut Window, &mut App) + 'static,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let key = id.clone();
    let fill = if held && !hover_is(&id) {
        layer(radius::ROW)()
            .bg(white(fill::ROW_HOVER))
            .into_any_element()
    } else {
        hover_fill(&id, radius::ROW, 0., fill::ROW_HOVER)
    };
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .items_center()
        .gap(px(12.))
        .w_full()
        .px(px(12.))
        .py(px(10.))
        .rounded(px(radius::ROW))
        .child(fill)
        .on_hover(move |hovered, window, cx| {
            note_hover(&key, *hovered, window);
            on_hover(hovered, window, cx);
        });
    keyboard(d, t)
}

/// A list row's two lines: the name (14 px medium) and a note (13 px, white at 55 %).
pub fn list_text(name: impl Into<SharedString>, note: impl Into<SharedString>) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(
            text(14., FontWeight::MEDIUM, ink::FULL)
                .line_height(px(21.))
                .truncate()
                .child(name.into()),
        )
        .child(
            text(13., FontWeight::NORMAL, ink::QUIET)
                .line_height(px(19.5))
                .truncate()
                .child(note.into()),
        )
}

/// A tab's line while it has nothing to list (`Empty`): 14 px, white at 60 %, `px-3 py-10`.
pub fn empty() -> Div {
    text(14., FontWeight::NORMAL, ink::NOTE)
        .line_height(px(21.))
        .px(px(12.))
        .py(px(40.))
}

/// The line over a tab's list (`px-3 pb-2`): its note at 13 px, white at 60 %, then buttons.
pub fn list_head(note: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .px(px(12.))
        .pb(px(8.))
        .child(
            text(13., FontWeight::NORMAL, ink::NOTE)
                .line_height(px(19.5))
                .min_w_0()
                .truncate()
                .child(note.into()),
        )
}

/// A link in a line of text (`font-medium text-white underline-offset-4 hover:underline`).
pub fn link(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id.into())
        .font_weight(FontWeight::MEDIUM)
        .text_color(white(ink::FULL))
        .cursor_pointer()
        .hover(|s| s.underline())
        .child(label.into())
}

/// A field (`WIDE_FIELD`): 44 px, round at 12, white at 7 %, 11 % with the focus, 14 px. Red at
/// 12 % while what it holds is refused.
pub fn field(focused: bool, invalid: bool) -> Div {
    let bg: Hsla = if invalid {
        rgba(horizon_theme::glass::DANGER_HOVER << 8 | 0x1f).into()
    } else {
        white(if focused {
            fill::FIELD_FOCUS
        } else {
            fill::FIELD
        })
    };
    div()
        .flex()
        .items_center()
        .h(px(44.))
        .px(px(16.))
        .overflow_hidden()
        .rounded(px(radius::ROW))
        .bg(bg)
        .font_family(horizon_theme::glass::FONT)
        .text_size(px(14.))
}

/// A switch (`Toggle`): 40×24, round; the accent with an `on_accent` knob on the right while on,
/// white at 14 % with a white knob at 70 % on the left while off.
pub fn toggle(
    id: impl Into<SharedString>,
    on: bool,
    accent: u32,
    on_accent: u32,
    t: &Theme,
) -> Stateful<Div> {
    let knob = div()
        .absolute()
        .top(px(4.))
        .left(px(if on { 20. } else { 4. }))
        .size(px(16.))
        .rounded_full()
        .bg(if on { color(on_accent) } else { white(0.7) });
    keyboard(
        div()
            .id(id.into())
            .relative()
            .flex_none()
            .w(px(40.))
            .h(px(24.))
            .rounded_full()
            .bg(if on {
                color(accent)
            } else {
                white(fill::BUTTON_HOVER)
            })
            .cursor_pointer()
            .child(knob),
        t,
    )
}

/// The generic [`button`](crate::button) in the glass look, 36 px: `Primary` is the mockup's
/// `PLAY` (the accent, semibold), `Secondary` its `QUIET` (white at 8 %, 14 % under the pointer),
/// `Ghost` its text button (white at 70 %, white on 6 % under the pointer).
pub fn button(id: SharedString, label: SharedString, kind: Kind, t: &Theme) -> Stateful<Div> {
    let look = look();
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .h(px(36.))
        .px(px(14.))
        .rounded(px(radius::CONTROL))
        .cursor_pointer()
        .font_family(horizon_theme::glass::FONT)
        .text_size(px(14.))
        .line_height(px(20.))
        .font_weight(FontWeight::MEDIUM);
    let d = match kind {
        Kind::Primary => d
            .bg(color(look.accent))
            .text_color(color(look.on_accent))
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|s| s.opacity(0.92)),
        Kind::Secondary => d
            .bg(white(fill::BUTTON))
            .text_color(white(ink::FULL))
            .hover(|s| s.bg(white(fill::BUTTON_HOVER))),
        Kind::Ghost => d
            .text_color(white(ink::BODY))
            .hover(|s| s.bg(white(0.06)).text_color(white(ink::FULL))),
    };
    keyboard(track_hover(d.child(label), id), t)
}

/// A segmented control in the glass look (the server dialog's difficulty): white at 4 % round
/// at 14, 4 px around 32 px options round at 10; the chosen one white at 14 %, medium, the others
/// white at 65 %, white under the pointer.
pub fn segments<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(T, &'static str)],
    current: T,
    t: &Theme,
    on_pick: impl Fn(T, &mut Window, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .gap(px(4.))
        .p(px(4.))
        .rounded(px(radius::LARGE))
        .bg(white(0.04))
        .children(options.iter().map(|&(value, label)| {
            let active = current == value;
            let pick = on_pick.clone();
            let d = div()
                .id(SharedString::from(format!("{id}-{label}")))
                .flex()
                .items_center()
                .h(px(32.))
                .px(px(14.))
                .rounded(px(radius::CONTROL))
                .cursor_pointer()
                .font_family(horizon_theme::glass::FONT)
                .text_size(px(14.))
                .line_height(px(20.))
                .when(active, |d| {
                    d.bg(white(fill::BUTTON_HOVER))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(white(ink::FULL))
                })
                .when(!active, |d| {
                    d.text_color(white(ink::MUTED))
                        .hover(|s| s.text_color(white(ink::FULL)))
                })
                .child(crate::tr(label))
                .on_click(move |_: &ClickEvent, window, cx| pick(value, window, cx));
            keyboard(d, t)
        }))
}

/// A page's head (`PageHead`): its title in Bricolage, 46 px at this width
/// (`clamp(2.25rem, 3.6vw, 3rem)`), its note under it at 15 px on 1.55, white at 70 %, at most
/// 62 characters wide; 40 px above, 28 below.
pub fn page_head(title: impl Into<SharedString>, note: impl Into<SharedString>) -> Div {
    let note: SharedString = note.into();
    div()
        .flex()
        .flex_col()
        .pt(px(40.))
        .mb(px(28.))
        .child(
            div()
                .font_family(horizon_theme::glass::DISPLAY_FONT)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(46.))
                .line_height(px(46.))
                .text_color(white(ink::FULL))
                .child(title.into()),
        )
        .when(!note.is_empty(), |d| {
            d.child(
                text(15., FontWeight::NORMAL, ink::BODY)
                    .mt(px(12.))
                    .max_w(px(615.))
                    .line_height(px(23.25))
                    .child(note),
            )
        })
}

/// A small heading in a card (`h3`): 13 px, medium, white at 70 %.
pub fn card_heading(label: impl Into<SharedString>) -> Div {
    text(13., FontWeight::MEDIUM, ink::BODY)
        .line_height(px(19.5))
        .child(label.into())
}

/// A card's note: 13 px on 1.5, white at 55 %.
pub fn card_note(label: impl Into<SharedString>) -> Div {
    text(13., FontWeight::NORMAL, ink::QUIET)
        .line_height(px(19.5))
        .child(label.into())
}
