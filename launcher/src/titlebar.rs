//! Window chrome: Windows and Linux client-side decorations are custom drawn; macOS and Linux
//! server-side decorations use the platform's native chrome.
use crate::theme::Theme;
use gpui::{Div, TitlebarOptions, Window};
use horizon_launcher::backend::Appearance;

/// Whether the launcher draws its own title bar and needs the native chrome to be transparent.
#[cfg(not(target_os = "linux"))]
pub const DRAWN: bool = cfg!(target_os = "windows");

pub fn draws_brand(window: &Window) -> bool {
    #[cfg(target_os = "linux")]
    return matches!(
        window.window_decorations(),
        gpui::Decorations::Client { .. }
    );
    #[cfg(not(target_os = "linux"))]
    {
        let _ = window;
        DRAWN
    }
}

pub fn options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("Horizon".into()),
        // macOS and Windows place the GPUI content under the native title bar. Linux uses
        // client-side decorations below, so its title bar is transparent by construction.
        appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
        ..Default::default()
    }
}

/// Draw the macOS title bar under the native traffic lights: glass, or the side colour without
/// it. The native title bar is transparent either way, so the bar is always drawn: without it the
/// page would start under the traffic lights. AppKit keeps its controls and window interactions.
#[cfg(target_os = "macos")]
pub fn render(theme: &Theme, _window: &Window) -> Option<Div> {
    use crate::glass::{self, Kind};
    use gpui::{div, prelude::*, px};

    Some(
        div()
            .relative()
            .flex_none()
            .h(px(32.))
            .when(theme.glass, |bar| {
                bar.child(glass::backdrop(Kind::Glass, 0.))
            })
            .when(!theme.glass, |bar| bar.bg(theme.side))
            .text_color(theme.muted),
    )
}

#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "linux"),
    not(target_os = "macos")
))]
pub fn render(_theme: &Theme, _window: &Window) -> Option<Div> {
    None
}

/// Draw the launcher's Linux title bar when the platform uses client-side decorations.
#[cfg(target_os = "linux")]
pub fn render(theme: &Theme, window: &Window) -> Option<Div> {
    use crate::glass::{self, Kind};
    use crate::{blocks, mark};
    use gpui::{div, prelude::*, px, Decorations, FontWeight, MouseButton};

    if matches!(window.window_decorations(), Decorations::Server) {
        return None;
    }

    let controls = window.window_controls();
    let drag = div()
        .id("window-drag")
        .flex()
        .flex_1()
        .h_full()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
        .child(mark(16., theme))
        .child(blocks::pixel(13., FontWeight::MEDIUM).child("Horizon"));

    let mut bar = div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .h(px(32.))
        .when(theme.glass, |bar| {
            bar.child(glass::backdrop(Kind::Glass, 0.))
        })
        .when(!theme.glass, |bar| bar.bg(theme.side))
        .text_color(theme.muted)
        .border_b_1()
        .border_color(theme.outline)
        .child(drag);

    if controls.minimize {
        let color = theme.ink_soft;
        let hover = theme.raised;
        bar = bar.child(
            div()
                .id("window-minimize")
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(46.))
                .h_full()
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .child(div().w(px(10.)).h(px(1.)).bg(color))
                .on_click(|_, window, _| window.minimize_window()),
        );
    }

    if controls.maximize {
        let color = theme.ink_soft;
        let hover = theme.raised;
        let maximized = window.is_maximized();
        let square = || {
            div()
                .absolute()
                .size(px(10.))
                .border_1()
                .border_color(color)
        };
        let icon = div().relative().size(px(14.));
        let icon = if maximized {
            icon.child(
                div()
                    .absolute()
                    .left(px(4.))
                    .top_0()
                    .size(px(10.))
                    .border_t_1()
                    .border_r_1()
                    .border_color(color),
            )
            .child(square().left(px(1.)).top(px(3.)))
        } else {
            icon.child(square().left(px(2.)).top(px(2.)))
        };
        bar = bar.child(
            div()
                .id("window-maximize")
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(46.))
                .h_full()
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .child(icon)
                .on_click(|_, window, _| window.zoom_window()),
        );
    }

    let close_hover = theme.page;
    let close_bg = theme.danger;
    Some(
        bar.child(
            div()
                .id("window-close")
                .group("linux-window-close")
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(46.))
                .h_full()
                .cursor_pointer()
                .hover(move |style| style.bg(close_bg).text_color(close_hover))
                .child(
                    blocks::icon("close", 14., theme.ink_soft)
                        .group_hover("linux-window-close", move |style| {
                            style.text_color(close_hover)
                        }),
                )
                .on_click(|_, window, _| window.remove_window()),
        ),
    )
}

#[cfg(target_os = "linux")]
pub fn resize_handles(window: &Window) -> Option<Div> {
    use gpui::{div, prelude::*, px, CursorStyle, Decorations, MouseButton, ResizeEdge};

    if matches!(window.window_decorations(), Decorations::Server) || window.is_maximized() {
        return None;
    }

    let handle = |id, edge, cursor| {
        div()
            .id(id)
            .absolute()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                window.start_window_resize(edge)
            })
    };

    Some(
        div()
            .absolute()
            .size_full()
            .child(
                handle("resize-left", ResizeEdge::Left, CursorStyle::ResizeLeft)
                    .left_0()
                    .top(px(8.))
                    .bottom(px(8.))
                    .w(px(5.)),
            )
            .child(
                handle("resize-right", ResizeEdge::Right, CursorStyle::ResizeRight)
                    .right_0()
                    .top(px(8.))
                    .bottom(px(8.))
                    .w(px(5.)),
            )
            .child(
                handle("resize-bottom", ResizeEdge::Bottom, CursorStyle::ResizeDown)
                    .bottom_0()
                    .left(px(8.))
                    .right(px(8.))
                    .h(px(5.)),
            )
            .child(
                handle(
                    "resize-top-left",
                    ResizeEdge::TopLeft,
                    CursorStyle::ResizeUpLeftDownRight,
                )
                .left_0()
                .top_0()
                .size(px(8.)),
            )
            .child(
                handle(
                    "resize-top-right",
                    ResizeEdge::TopRight,
                    CursorStyle::ResizeUpRightDownLeft,
                )
                .right_0()
                .top_0()
                .size(px(8.)),
            )
            .child(
                handle(
                    "resize-bottom-left",
                    ResizeEdge::BottomLeft,
                    CursorStyle::ResizeUpRightDownLeft,
                )
                .left_0()
                .bottom_0()
                .size(px(8.)),
            )
            .child(
                handle(
                    "resize-bottom-right",
                    ResizeEdge::BottomRight,
                    CursorStyle::ResizeUpLeftDownRight,
                )
                .right_0()
                .bottom_0()
                .size(px(8.)),
            ),
    )
}

#[cfg(not(target_os = "linux"))]
pub fn resize_handles(_: &Window) -> Option<Div> {
    None
}

/// macOS keeps the window's appearance on the system setting, so a launcher set to Dark still
/// draws a light title bar on a light desktop. Pin the NSWindow's appearance to the choice
/// instead (`theme::pinned_dark`: `System` clears the pin, handing the chrome back to the
/// system). Windows uses custom chrome; Linux draws it only when the compositor requests it.
#[cfg(target_os = "macos")]
pub fn apply_appearance(window: &Window, choice: Appearance) {
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceName, NSAppearanceNameAqua,
        NSAppearanceNameDarkAqua, NSView,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    // AppKit's name constants are extern statics: reading them is `unsafe` to rustc only.
    let name: Option<&'static NSAppearanceName> = match crate::theme::pinned_dark(choice) {
        Some(true) => Some(unsafe { NSAppearanceNameDarkAqua }),
        Some(false) => Some(unsafe { NSAppearanceNameAqua }),
        None => None,
    };
    // The trait, not `Window::window_handle` (which is gpui's own `AnyWindowHandle`).
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    // The handle is the window's content view; its `window` is the NSWindow itself. Both outlive
    // this call.
    let view = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    let Some(native) = view.window() else {
        return;
    };
    let appearance = name.and_then(NSAppearance::appearanceNamed);
    native.setAppearance(appearance.as_deref());
}

#[cfg(not(target_os = "macos"))]
pub fn apply_appearance(_: &Window, _: Appearance) {}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use crate::glass::{self, Kind};
    use crate::{blocks, mark};
    use gpui::{div, prelude::*, px, FontWeight, Stateful, WindowControlArea};

    const HEIGHT: f32 = 32.;
    const BUTTON_WIDTH: f32 = 46.;
    const RESIZE_INSET: f32 = 5.;

    #[derive(Clone, Copy)]
    enum Control {
        Minimize,
        Maximize,
        Close,
    }

    impl Control {
        fn area(self) -> WindowControlArea {
            match self {
                Self::Minimize => WindowControlArea::Min,
                Self::Maximize => WindowControlArea::Max,
                Self::Close => WindowControlArea::Close,
            }
        }

        fn name(self) -> &'static str {
            match self {
                Self::Minimize => "window-minimize",
                Self::Maximize => "window-maximize",
                Self::Close => "window-close",
            }
        }
    }

    fn control_icon(control: Control, maximized: bool, theme: &Theme) -> Div {
        let square = || {
            div()
                .absolute()
                .size(px(10.))
                .border_1()
                .border_color(theme.ink_soft)
        };
        let icon = div().relative().size(px(14.));
        match control {
            Control::Minimize => icon.child(
                div()
                    .absolute()
                    .left(px(2.))
                    .top(px(7.))
                    .w(px(10.))
                    .h(px(1.))
                    .bg(theme.ink_soft),
            ),
            Control::Maximize if maximized => icon
                .child(
                    div()
                        .absolute()
                        .left(px(4.))
                        .top_0()
                        .size(px(10.))
                        .border_t_1()
                        .border_r_1()
                        .border_color(theme.ink_soft),
                )
                .child(square().left(px(1.)).top(px(3.))),
            Control::Maximize => icon.child(square().left(px(2.)).top(px(2.))),
            Control::Close => {
                let hover_color = theme.page;
                icon.child(
                    blocks::icon("close", 14., theme.ink_soft)
                        .group_hover("window-close", move |style| style.text_color(hover_color)),
                )
            }
        }
    }

    fn control_button(control: Control, theme: &Theme, maximized: bool) -> Stateful<Div> {
        let theme = *theme;
        div()
            .id(control.name())
            .group(control.name())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(BUTTON_WIDTH))
            .h_full()
            .text_color(theme.ink_soft)
            // GPUI maps these to native non-client hit tests, retaining Snap Layouts,
            // Alt+Space and close handling. Click callbacks would run the action twice.
            .window_control_area(control.area())
            .hover(move |style| match control {
                Control::Close => style.bg(theme.danger).text_color(theme.page),
                _ => style.bg(theme.raised),
            })
            .child(control_icon(control, maximized, &theme))
    }

    pub fn render(theme: &Theme, window: &Window) -> Option<Div> {
        let drag = div()
            .id("window-drag")
            .flex()
            .flex_1()
            .h_full()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .window_control_area(WindowControlArea::Drag)
            .child(mark(16., theme))
            .child(blocks::pixel(13., FontWeight::MEDIUM).child("Horizon"));
        Some(
            div()
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .h(px(HEIGHT))
                // Control hit tests run before native edge resizing in GPUI 0.2.2.
                .when(!window.is_maximized(), |bar| {
                    bar.pt(px(RESIZE_INSET)).px(px(RESIZE_INSET))
                })
                .when(theme.glass, |bar| {
                    bar.child(glass::backdrop(Kind::Glass, 0.))
                })
                .when(!theme.glass, |bar| bar.bg(theme.side))
                .text_color(theme.muted)
                .border_b_1()
                .border_color(theme.outline)
                .child(drag)
                .children(
                    [Control::Minimize, Control::Maximize, Control::Close]
                        .map(|control| control_button(control, theme, window.is_maximized())),
                ),
        )
    }
}

#[cfg(target_os = "windows")]
pub use windows::render;
