//! The screen a phone shows when it has no game files, and the upload page that carries them in.
//!
//! Before the pack pipeline has run there is nothing to draw menus with: the loading window's atlas
//! is a 1x1 empty texture, and every menu's text is a Mojang bitmap font that lives inside it. The
//! theme's own font is the exception -- `ui_theme` rasterizes the Geist bytes shipped in
//! `horizon-theme` into the `horizon/ui-text` slot at run time -- so a screen written here can say
//! "I have no files, and here is how to give me some" on a device that has never had a file.
//!
//! It is in the library rather than in `game/` because `ui_theme`'s widgets are crate-private, the
//! same reason `level_loading` draws its own progress bar here.

use std::path::Path;

use crate::{
    hud::{HudAtlas, HudBatch},
    ui_theme::{self, Face, color},
};

/// The plate's left edge and the width of a line of text, in dp.
const MARGIN: f32 = 56.0;
/// Space under a heading, in dp.
const GAP: f32 = 8.0;

/// What the screen says, as of this frame.
pub struct Status<'a> {
    /// The game version whose files are wanted, so a 26.3 archive is not sent for a 26.4 app.
    pub version: &'a str,
    /// Why the game is here: the loader's own words, or nothing on a first run with no archive yet.
    pub reason: Option<&'a str>,
    /// The folder the Files app shows, where an archive dropped by cable installs itself.
    pub folder: &'a Path,
    /// Where to post an archive over Wi-Fi, once the listener is up: `None` keeps the page out.
    pub address: Option<(&'a str, u16)>,
    /// The six digits that page asks for, shown on the phone and typed on the computer.
    pub pin: Option<&'a str>,
    /// The upload in flight, for the bar and the count.
    pub received: u64,
    pub total: u64,
    /// The last thing that went wrong with an archive, if one tried and failed.
    pub error: Option<&'a str>,
}

impl Status<'_> {
    /// `received / total`, or nothing until the size is known.
    fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.received as f64 / self.total as f64) as f32)
    }
}

/// The height a line of `size` dp takes, leading and gap included: every advance on the screen is
/// this, so a paragraph and a heading cannot drift apart.
fn line_h(size: f32) -> f32 {
    (size * 1.45).round() + 2.0
}

/// Body text in `text`, wrapped to `width`, advanced past its last line.
fn paragraph(ui: &mut ui_theme::Ui, y: &mut f32, size: f32, ink: u32, width: f32, text: &str) {
    for line in wrap(ui, Face::Regular, size, width, text) {
        ui.text(Face::Regular, size, MARGIN, *y, ink, &line);
        *y += line_h(size);
    }
}

/// A heading, in the launcher's own voice: medium weight, the accent colour it uses for a section.
fn heading(ui: &mut ui_theme::Ui, y: &mut f32, text: &str) {
    *y += GAP;
    ui.text(Face::Medium, 15.0, MARGIN, *y, color::accent(), text);
    *y += line_h(15.0);
}

/// Break `text` into lines no wider than `width` dp, on spaces, in the theme's own font.
fn wrap(ui: &ui_theme::Ui, face: Face, size: f32, width: f32, text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let trial = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if ui.measure(face, size, &trial) > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = trial;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// One frame of the screen: the plate, what the game wants, and both doors it can come through.
pub fn draw(batch: &mut HudBatch, atlas: &HudAtlas, status: &Status) {
    let mut ui = ui_theme::Ui::new(atlas, batch, (0.0, 0.0));
    let (w, h) = (ui.w, ui.h);
    let folder = status.folder;
    let text = (w - MARGIN * 2.0).max(120.0);
    // The page itself: the panorama behind is black, and an opaque plate keeps the text off it.
    ui.rounded(
        MARGIN / 2.0,
        16.0,
        w - MARGIN,
        h - 32.0,
        ui_theme::radius(),
        color::bg(),
    );
    let mut y = 34.0;
    ui.text(
        Face::Medium,
        24.0,
        MARGIN,
        y,
        color::text(),
        &format!("Horizon {}", status.version),
    );
    y += line_h(24.0) + GAP - 2.0;
    paragraph(
        &mut ui,
        &mut y,
        15.0,
        color::text(),
        text,
        "This app has no game files to run. They are yours already, on the computer that prepared them.",
    );
    if let Some(reason) = status.reason {
        y += GAP;
        paragraph(&mut ui, &mut y, 13.0, color::muted(), text, reason);
    }
    if let Some((host, port)) = status.address {
        heading(&mut ui, &mut y, "OVER WI-FI");
        paragraph(
            &mut ui,
            &mut y,
            13.0,
            color::text(),
            text,
            "Open this address on any device on the same network and choose the archive:",
        );
        ui.text(
            Face::Medium,
            20.0,
            MARGIN,
            y,
            color::accent(),
            &format!("http://{host}:{port}/"),
        );
        y += line_h(20.0);
        if let Some(pin) = status.pin {
            paragraph(
                &mut ui,
                &mut y,
                13.0,
                color::muted(),
                text,
                &format!("The page will ask for this code: {pin}"),
            );
        }
        if let Some(fraction) = status.fraction() {
            let top = y + 4.0;
            ui.rounded(MARGIN, top, text, 6.0, 3.0, color::hover());
            ui.rounded(
                MARGIN,
                top,
                (text * fraction).max(6.0),
                6.0,
                3.0,
                color::accent(),
            );
            y = top + 6.0 + GAP;
            paragraph(
                &mut ui,
                &mut y,
                13.0,
                color::muted(),
                text,
                &format!(
                    "{} of {} received",
                    megabytes(status.received),
                    megabytes(status.total)
                ),
            );
        }
    }
    heading(&mut ui, &mut y, "OR BY CABLE");
    paragraph(
        &mut ui,
        &mut y,
        13.0,
        color::text(),
        text,
        "Copy one .zip archive into this app's folder in the Files app. It installs itself as soon as it arrives:",
    );
    paragraph(
        &mut ui,
        &mut y,
        13.0,
        color::muted(),
        text,
        &folder.display().to_string(),
    );
    heading(&mut ui, &mut y, "ON THE COMPUTER");
    paragraph(
        &mut ui,
        &mut y,
        13.0,
        color::text(),
        text,
        &format!(
            "horizon-client --pack-resources .local/horizon-client/{} --out horizon-{}.zip",
            status.version, status.version
        ),
    );
    if let Some(error) = status.error {
        y += GAP;
        paragraph(
            &mut ui,
            &mut y,
            13.0,
            color::hex(0xff_5a5a, 255),
            text,
            error,
        );
    }
    ui.finish();
}

/// The download screen: the version, one line saying what is happening, and one bar. `fraction` is
/// None while the size is unknown, which draws the bar full and lets the line carry the count.
pub fn draw_download(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    version: &str,
    line: &str,
    fraction: Option<f32>,
) {
    let mut ui = ui_theme::Ui::new(atlas, batch, (0.0, 0.0));
    let (w, h) = (ui.w, ui.h);
    let width = (w - MARGIN * 2.0).clamp(120.0, 520.0);
    let left = ((w - width) / 2.0).floor();
    let top = (h / 2.0 - 30.0).floor();
    ui.text(
        Face::Medium,
        20.0,
        left,
        top,
        color::text(),
        &format!("Horizon {version}"),
    );
    let bar = top + line_h(20.0) + GAP;
    ui.rounded(left, bar, width, 6.0, 3.0, color::hover());
    let filled = fraction.unwrap_or(1.0).clamp(0.0, 1.0);
    ui.rounded(
        left,
        bar,
        (width * filled).max(6.0),
        6.0,
        3.0,
        color::accent(),
    );
    ui.text(
        Face::Regular,
        13.0,
        left,
        bar + 6.0 + GAP,
        color::muted(),
        line,
    );
    ui.finish();
}

/// What the download screen's line says.
pub fn download_line(stage: &crate::import::download::Stage) -> (String, Option<f32>) {
    match *stage {
        crate::import::download::Stage::Downloading { received, total } if total > 0 => (
            format!(
                "Downloading game files: {} of {}",
                megabytes(received),
                megabytes(total)
            ),
            Some(received as f32 / total as f32),
        ),
        crate::import::download::Stage::Downloading { received, .. } => (
            format!("Downloading game files: {}", megabytes(received)),
            None,
        ),
        crate::import::download::Stage::Installing => ("Installing game files...".into(), None),
    }
}

/// Bytes as the screen says them: whole megabytes, because a phone is reading this at arm's length.
fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes / 1_048_576)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status<'a>(folder: &'a Path, host: &'a str) -> Status<'a> {
        Status {
            version: "26.3",
            reason: Some("no manifest at the root it looked in, and no archive to read"),
            folder,
            address: Some((host, 41234)),
            pin: Some("481930"),
            received: 0,
            total: 0,
            error: None,
        }
    }

    #[test]
    fn the_screen_draws_with_the_atlas_of_a_game_that_has_no_files() {
        let atlas = HudAtlas::without_pack(Path::new("/horizon-test-no-assets")).unwrap();
        ui_theme::reset_atlas();
        let mut batch = HudBatch::new([1792, 828]);
        let folder = Path::new("/private/var/Containers/0/Documents");
        draw(&mut batch, &atlas, &status(folder, "192.168.1.23"));
        assert!(!batch.vertices.is_empty(), "the screen drew nothing");
        // Text is rasterized into the atlas on the way out, and that is the only font there is.
        assert!(
            !batch.patches.is_empty(),
            "no glyph was uploaded, so nothing would be legible"
        );
    }

    #[test]
    fn the_download_screen_draws_its_bar_and_says_how_far() {
        use crate::import::download::Stage;
        let atlas = HudAtlas::without_pack(Path::new("/horizon-test-no-assets")).unwrap();
        ui_theme::reset_atlas();
        let mut batch = HudBatch::new([1792, 828]);
        let stage = Stage::Downloading {
            received: 300 << 20,
            total: 600 << 20,
        };
        let (line, fraction) = download_line(&stage);
        assert_eq!(line, "Downloading game files: 300 MB of 600 MB");
        assert_eq!(fraction, Some(0.5));
        draw_download(&mut batch, &atlas, "26.3", &line, fraction);
        assert!(!batch.vertices.is_empty(), "the screen drew nothing");
        assert_eq!(download_line(&Stage::Installing).1, None);
    }

    #[test]
    fn the_page_is_left_out_until_the_listener_is_up() {
        let atlas = HudAtlas::without_pack(Path::new("/horizon-test-no-assets")).unwrap();
        ui_theme::reset_atlas();
        let folder = Path::new("/Documents");
        let mut quiet = HudBatch::new([1792, 828]);
        let mut listening = HudBatch::new([1792, 828]);
        let host = "10.0.0.5";
        draw(
            &mut quiet,
            &atlas,
            &Status {
                address: None,
                ..status(folder, host)
            },
        );
        draw(&mut listening, &atlas, &status(folder, host));
        assert!(
            listening.vertices.len() > quiet.vertices.len(),
            "an address costs quads; without one the page is not drawn"
        );
    }

    #[test]
    fn long_words_break_where_they_are_reachable() {
        let atlas = HudAtlas::without_pack(Path::new("/horizon-test-no-assets")).unwrap();
        let mut batch = HudBatch::new([1792, 828]);
        let ui = ui_theme::Ui::new(&atlas, &mut batch, (0.0, 0.0));
        let one = wrap(&ui, Face::Regular, 13.0, 40.0, "alpha beta gamma delta");
        assert!(one.len() > 1, "{one:?}");
        assert_eq!(
            wrap(&ui, Face::Regular, 13.0, 900.0, "a b c").join(" "),
            "a b c"
        );
        // A word longer than the line is not lost; it gets its own line and is what it is.
        assert_eq!(
            wrap(&ui, Face::Regular, 13.0, 10.0, "supercalifragilistic").len(),
            1
        );
    }

    #[test]
    fn sizes_are_said_in_megabytes() {
        assert_eq!(megabytes(0), "0 MB");
        assert_eq!(megabytes(589 * 1_048_576), "589 MB");
        assert_eq!(megabytes(1_048_575), "0 MB");
    }
}
