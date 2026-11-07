//! Microsoft's "Sign in with Microsoft" button, as Microsoft publishes it for apps that sign in
//! with Microsoft accounts (learn.microsoft.com/en-us/entra/identity-platform/howto-add-branding-in-apps):
//! 41 pixels tall, the four-square symbol (21 pixels) 12 pixels from the edge and from the text,
//! the text semibold at 15 pixels; dark: `#2F2F2F` with white text, light: white with `#5E5E5E`
//! text and a `#8C8C8C` border. The symbol is drawn from the official `ms-symbollockup_mssymbol_19.svg`
//! (its squares, places and colours as they are there), never recoloured or reshaped; see
//! THIRD_PARTY_NOTICES.md. Square corners, as Microsoft's own: not the launcher's cut plates.
use super::*;

/// The symbol's squares in its 21-pixel box: left, top, colour.
const SQUARES: [(f32, f32, u32); 4] = [
    (1., 1., 0xf25022),
    (11., 1., 0x7fba00),
    (1., 11., 0x00a4ef),
    (11., 11., 0xffb900),
];

/// Microsoft's four-square symbol, 21 pixels square.
pub(super) fn symbol() -> Div {
    div()
        .relative()
        .flex_none()
        .size(px(21.))
        .children(SQUARES.map(|(x, y, colour)| {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .size(px(9.))
                .bg(gpui::rgb(colour))
        }))
}

/// The symbol at `size` pixels square (the glass top bar's button draws it at 16).
pub(super) fn logo(size: f32) -> Div {
    let u = size / 21.;
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .children(SQUARES.map(|(x, y, colour)| {
            div()
                .absolute()
                .left(px(x * u))
                .top(px(y * u))
                .size(px(9. * u))
                .bg(gpui::rgb(colour))
        }))
}

/// The button, in the dark or light form the launcher's look calls for.
pub(super) fn sign_in_button(id: &'static str, t: &Theme) -> Stateful<Div> {
    let (fill, ink, border) = if t.dark {
        (0x2f2f2f, 0xffffff, None)
    } else {
        (0xffffff, 0x5e5e5e, Some(0x8c8c8c))
    };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .h(px(41.))
        .px(px(12.))
        .bg(gpui::rgb(fill))
        .when_some(border, |d, b| d.border_1().border_color(gpui::rgb(b)))
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .child(symbol())
        .child(
            div()
                // Microsoft's face where the system has it; the launcher's elsewhere.
                .when(cfg!(windows), |d| d.font_family("Segoe UI"))
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(ink))
                .whitespace_nowrap()
                .child(tr("Sign in with Microsoft").to_string()),
        )
}

/// How long the logout page gets before the code page opens after it.
const LOGOUT_WAIT: Duration = Duration::from_millis(2500);

/// Whether the player's default browser has a private window to open the code page in. The
/// first call asks the system (`begin_sign_in` warms it).
pub(super) fn has_private_window() -> bool {
    horizon_launcher::private_window::detected().is_some()
}

/// Opens the code page in the default browser's private window, where no Microsoft account is
/// signed in; in the normal window if the browser can't be started.
pub(super) fn open_private(uri: &str, cx: &mut App) {
    if horizon_launcher::private_window::open(uri).is_err() {
        cx.open_url(uri);
    }
}

/// "Use a different Microsoft account": Microsoft's logout page first, then the code page, in
/// the player's normal browser.
pub(super) fn use_different_account(uri: &str, cx: &mut App) {
    cx.open_url(horizon_launcher::private_window::LOGOUT_URL);
    let uri = uri.to_owned();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(LOGOUT_WAIT).await;
        cx.update(|cx| cx.open_url(&uri));
    })
    .detach();
}
