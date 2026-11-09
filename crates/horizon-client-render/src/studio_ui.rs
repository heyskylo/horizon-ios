//! Replay Studio Browser & Saved Recordings UI
//!
//! Provides an in-game graphical browser to inspect saved `.mcpr` recordings,
//! view recording metadata (duration, packet count, timestamp, server),
//! and open the recordings directory.

use crate::{
    graphics_style as style,
    hud::{HudAtlas, HudBatch},
    lang,
    text::Text,
    ui,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

/// High-level session metadata read from `metaData.json` inside `.mcpr`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingMetadata {
    #[serde(default)]
    pub server_name: String,
    #[serde(default)]
    pub custom_server_name: Option<String>,
    #[serde(default)]
    pub singleplayer: bool,
    #[serde(default)]
    pub duration: u32,
    #[serde(default)]
    pub date: u64,
    #[serde(default)]
    pub mcversion: String,
    #[serde(default)]
    pub file_format: String,
    #[serde(default)]
    pub file_format_version: u32,
    #[serde(default)]
    pub generator: String,
    #[serde(default)]
    pub players: Vec<String>,
}

/// A saved replay item displayed in the Studio Browser list.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedRecordingItem {
    pub path: PathBuf,
    pub filename: String,
    pub title: String,
    pub duration_ms: u32,
    pub date: u64,
    pub file_size: u64,
    pub server_name: String,
    pub singleplayer: bool,
}

impl SavedRecordingItem {
    /// Formatted duration string `MM:SS` or `HH:MM:SS`.
    pub fn formatted_duration(&self) -> String {
        let total_secs = self.duration_ms / 1000;
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;
        if hours > 0 {
            format!("{hours:02}:{mins:02}:{secs:02}")
        } else {
            format!("{mins:02}:{secs:02}")
        }
    }

    /// Formatted file size string (KB or MB).
    pub fn formatted_size(&self) -> String {
        let bytes = self.file_size;
        if bytes >= 1024 * 1024 {
            let mb = bytes as f64 / (1024.0 * 1024.0);
            format!("{mb:.1} MB")
        } else {
            let kb = bytes.div_ceil(1024);
            format!("{kb} KB")
        }
    }
}

/// User actions triggered by the Studio Browser UI.
#[derive(Clone, Debug, PartialEq)]
pub enum StudioAction {
    None,
    OpenReplay(PathBuf),
    OpenFolder,
    Refresh,
    Back,
}

/// The Studio Replay Browser screen state and renderer.
#[derive(Clone, Debug)]
pub struct StudioUi {
    pub recordings: Vec<SavedRecordingItem>,
    pub selected: Option<usize>,
    pub scroll: f32,
    pub search: String,
    pub record_dir: PathBuf,
}

/// A recording's row and the gap under it, in GUI pixels.
const ROW_STEP: f32 = 40.0;
const ROW_GAP: f32 = 4.0;
/// The panel's margins above and below its rows.
const LIST_MARGINS: f32 = 10.0;

impl Default for StudioUi {
    fn default() -> Self {
        let dir = PathBuf::from("recordings");
        let mut ui = Self {
            recordings: Vec::new(),
            selected: None,
            scroll: 0.0,
            search: String::new(),
            record_dir: dir.clone(),
        };
        ui.scan(&dir);
        ui
    }
}

impl StudioUi {
    /// Create a new StudioUi for the given recordings directory.
    pub fn new(record_dir: impl Into<PathBuf>) -> Self {
        let dir = record_dir.into();
        let mut ui = Self {
            recordings: Vec::new(),
            selected: None,
            scroll: 0.0,
            search: String::new(),
            record_dir: dir.clone(),
        };
        ui.scan(&dir);
        ui
    }

    /// Re-scan the recordings folder.
    pub fn refresh(&mut self) {
        self.scan(&self.record_dir.clone());
    }

    /// Read metadata from a `.mcpr` archive without extracting the full packet stream.
    pub fn read_metadata(path: &Path) -> Option<RecordingMetadata> {
        let file = File::open(path).ok()?;
        let mut archive = zip::ZipArchive::new(file).ok()?;
        // Match the replay reader's metadata ceiling. ZIP compressed sizes are
        // not a memory bound: a small imported recording can expand to gigabytes.
        const MAX_METADATA_BYTES: u64 = 1 << 20;
        let meta_file = archive.by_name("metaData.json").ok()?;
        if meta_file.size() > MAX_METADATA_BYTES {
            return None;
        }
        let mut meta_bytes = Vec::new();
        meta_file
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut meta_bytes)
            .ok()?;
        if meta_bytes.len() as u64 > MAX_METADATA_BYTES {
            return None;
        }
        serde_json::from_slice(&meta_bytes).ok()
    }

    /// Scan `dir` for `.mcpr` files and parse their metadata.
    pub fn scan(&mut self, dir: &Path) {
        self.recordings.clear();
        self.selected = None;
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("mcpr") {
                continue;
            }

            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let file_size = meta.len();
            let filename = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            let title = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| filename.clone());

            let (duration_ms, date, server_name, singleplayer) =
                if let Some(parsed) = Self::read_metadata(&path) {
                    let srv = if parsed.singleplayer {
                        lang::get("menu.singleplayer")
                    } else if !parsed.server_name.is_empty() {
                        parsed.server_name
                    } else {
                        lang::get("menu.multiplayer")
                    };
                    (parsed.duration, parsed.date, srv, parsed.singleplayer)
                } else {
                    let modified = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    (
                        0,
                        modified,
                        lang::get("horizon.studio.unknown_server"),
                        true,
                    )
                };

            self.recordings.push(SavedRecordingItem {
                path,
                filename,
                title,
                duration_ms,
                date,
                file_size,
                server_name,
                singleplayer,
            });
        }

        // Sort descending by date (newest recordings first)
        self.recordings.sort_by_key(|a| std::cmp::Reverse(a.date));
        if !self.recordings.is_empty() {
            self.selected = Some(0);
        }
    }

    /// Draw the Studio Browser screen.
    pub fn draw(
        &mut self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        in_world: bool,
    ) {
        let (w, h) = (batch.width, batch.height);
        ui::menu_background(batch, atlas, 0.0, 0.0, w, h, in_world);

        // Header Title: centered at top
        style::title(batch, atlas, &Text::tr("horizon.pages.studio", &[]));

        // Subtitle strip: centered info text
        let subtitle = Text::tr("horizon.studio.saved_recordings", &[]);
        ui::centered_text(batch, atlas, &subtitle, 0.0, 26.0, w, style::GREY);

        // Main content area layout
        let panel_w = (w - 40.0).clamp(280.0, 440.0);
        let panel_x = ((w - panel_w) / 2.0).floor();
        let panel_y = 42.0;
        let panel_h = (h - 76.0).max(100.0);

        // Glass panel backdrop for recordings list
        style::panel(batch, atlas, [panel_x, panel_y, panel_w, panel_h]);

        if self.recordings.is_empty() {
            // Empty state
            let empty_text = Text::tr("horizon.studio.empty", &[]);
            let hint_text = Text::tr("horizon.studio.empty_hint", &[]);
            ui::centered_text(
                batch,
                atlas,
                &empty_text,
                panel_x,
                panel_y + panel_h / 2.0 - 16.0,
                panel_w,
                style::WHITE,
            );
            ui::centered_text(
                batch,
                atlas,
                &hint_text,
                panel_x,
                panel_y + panel_h / 2.0,
                panel_w,
                style::GREY,
            );
        } else {
            // Render scrollable cards
            let card_h = ROW_STEP - ROW_GAP;
            let row_step = ROW_STEP;
            // Rows that fit whole between the panel's margins, as the loop below draws them.
            let visible_cards = ((panel_h - LIST_MARGINS + ROW_GAP) / row_step).floor() as usize;
            let max_scroll =
                (self.recordings.len().saturating_sub(visible_cards) as f32 * row_step).max(0.0);
            self.scroll = self.scroll.clamp(0.0, max_scroll);

            let card_w = panel_w - 12.0;
            let card_x = panel_x + 6.0;
            // Whole rows: nothing here clips one that's partly scrolled off.
            let mut y = panel_y + 6.0;
            let start_idx = (self.scroll / row_step).round() as usize;

            for (idx, item) in self.recordings.iter().enumerate().skip(start_idx) {
                if y + card_h > panel_y + panel_h - 4.0 {
                    break;
                }

                let rect = [card_x, y, card_w, card_h];
                crate::widgets::record_as(&item.title, rect, true, crate::widgets::Kind::Row);
                crate::widgets::target_as(rect, crate::widgets::Kind::Row);
                let is_hovered = ui::hovered(cursor, card_x, y, card_w, card_h);
                let is_selected = self.selected == Some(idx);

                // Card background & highlight
                if is_selected {
                    style::fill(batch, atlas, rect, style::OPEN);
                } else if is_hovered {
                    style::fill(batch, atlas, rect, style::HOVER);
                } else {
                    style::well(batch, atlas, rect);
                }

                let border_color = if is_selected {
                    style::ACCENT
                } else if is_hovered {
                    style::WHITE
                } else {
                    style::ROW_RULE
                };
                style::ring(batch, atlas, rect, border_color);

                // Card Line 1: Recording Title (bold white)
                let title_text = Text::literal(&item.title);
                batch.text_styled(
                    atlas,
                    &title_text,
                    card_x + 8.0,
                    y + 6.0,
                    style::WHITE,
                    true,
                );

                // Card Line 2: Duration, Size, Server
                let meta_str = format!(
                    "{} • {} • {}",
                    item.formatted_duration(),
                    item.formatted_size(),
                    item.server_name
                );
                let meta_text = Text::literal(&meta_str);
                batch.text_styled(atlas, &meta_text, card_x + 8.0, y + 20.0, style::GREY, true);

                y += row_step;
            }

            crate::widgets::scroll_area(
                [panel_x, panel_y, panel_w, panel_h],
                self.scroll,
                max_scroll,
            );

            // Scrollbar track & thumb if scrollable
            if max_scroll > 0.0 {
                let track_x = panel_x + panel_w - 4.0;
                let track_y = panel_y + 2.0;
                let track_h = panel_h - 4.0;
                let thumb_h = ((panel_h / (panel_h + max_scroll)) * track_h).max(12.0);
                let thumb_y = track_y + (self.scroll / max_scroll) * (track_h - thumb_h);
                style::scroller(
                    batch,
                    atlas,
                    [track_x, track_y, 2.0, track_h],
                    [track_x, thumb_y, 2.0, thumb_h],
                );
            }
        }

        // Bottom Button Bar
        let btn_y = h - 28.0;
        let btn_h = 20.0;

        // Button 1: Open Folder (Left)
        let f_btn = [panel_x, btn_y, 130.0, btn_h];
        let f_hover = ui::hovered(cursor, f_btn[0], f_btn[1], f_btn[2], f_btn[3]);
        let label = Text::tr("horizon.studio.open_folder", &[]);
        crate::widgets::record(&label.plain(), f_btn, true);
        style::button(batch, atlas, f_btn, &label, (f_hover, true, false));

        // Button 2: Refresh (Center-Left)
        let r_btn = [panel_x + 136.0, btn_y, 65.0, btn_h];
        let r_hover = ui::hovered(cursor, r_btn[0], r_btn[1], r_btn[2], r_btn[3]);
        let label = Text::tr("horizon.studio.refresh", &[]);
        crate::widgets::record(&label.plain(), r_btn, true);
        style::button(batch, atlas, r_btn, &label, (r_hover, true, false));

        // Button 3: Open the selected recording for playback.
        let e_btn = [panel_x + panel_w - 180.0, btn_y, 110.0, btn_h];
        let e_hover = ui::hovered(cursor, e_btn[0], e_btn[1], e_btn[2], e_btn[3]);
        style::button(
            batch,
            atlas,
            e_btn,
            &Text::tr("horizon.studio.open_replay", &[]),
            (e_hover, true, false),
        );

        // Button 3: Done / Back (Right)
        let d_btn = [panel_x + panel_w - 64.0, btn_y, 64.0, btn_h];
        let d_hover = ui::hovered(cursor, d_btn[0], d_btn[1], d_btn[2], d_btn[3]);
        let label = Text::tr("gui.done", &[]);
        crate::widgets::record(&label.plain(), d_btn, true);
        style::button(batch, atlas, d_btn, &label, (d_hover, true, false));
        crate::widgets::declare_action(crate::widgets::Action::Done, d_btn, true);
    }

    /// Process mouse click on the Studio Browser screen.
    pub fn click(&mut self, cursor: (f32, f32), size: (f32, f32)) -> StudioAction {
        let (w, h) = size;
        let panel_w = (w - 40.0).clamp(280.0, 440.0);
        let panel_x = ((w - panel_w) / 2.0).floor();
        let panel_y = 42.0;
        let panel_h = (h - 76.0).max(100.0);
        let btn_y = h - 28.0;
        let btn_h = 20.0;

        // Button 1: Open Folder
        let f_btn = [panel_x, btn_y, 130.0, btn_h];
        if ui::hovered(cursor, f_btn[0], f_btn[1], f_btn[2], f_btn[3]) {
            return StudioAction::OpenFolder;
        }

        // Button 2: Refresh
        let r_btn = [panel_x + 136.0, btn_y, 65.0, btn_h];
        if ui::hovered(cursor, r_btn[0], r_btn[1], r_btn[2], r_btn[3]) {
            self.refresh();
            return StudioAction::Refresh;
        }

        // Button 3: Open Replay
        let e_btn = [panel_x + panel_w - 180.0, btn_y, 110.0, btn_h];
        if ui::hovered(cursor, e_btn[0], e_btn[1], e_btn[2], e_btn[3]) {
            if let Some(idx) = self.selected
                && let Some(item) = self.recordings.get(idx)
            {
                return StudioAction::OpenReplay(item.path.clone());
            } else if let Some(item) = self.recordings.first() {
                self.selected = Some(0);
                return StudioAction::OpenReplay(item.path.clone());
            }
        }

        // Button 4: Done
        let d_btn = [panel_x + panel_w - 64.0, btn_y, 64.0, btn_h];
        if ui::hovered(cursor, d_btn[0], d_btn[1], d_btn[2], d_btn[3]) {
            return StudioAction::Back;
        }

        // Card clicks
        let card_h = ROW_STEP - ROW_GAP;
        let row_step = ROW_STEP;
        let card_w = panel_w - 12.0;
        let card_x = panel_x + 6.0;
        let start_idx = (self.scroll / row_step).round() as usize;
        let mut y = panel_y + 6.0;

        for (idx, _) in self.recordings.iter().enumerate().skip(start_idx) {
            if y + card_h > panel_y + panel_h - 4.0 {
                break;
            }

            if ui::hovered(cursor, card_x, y, card_w, card_h) {
                self.selected = Some(idx);
                return StudioAction::None;
            }

            y += row_step;
        }

        StudioAction::None
    }

    /// Scroll list by mouse wheel delta.
    pub fn wheel(&mut self, amount: f32) {
        // A notch is a row, as a focus step needs (`gamepad::list_step`).
        self.scroll = (self.scroll - amount * ROW_STEP).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn formatted_duration_and_size() {
        let item = SavedRecordingItem {
            path: PathBuf::from("recordings/test.mcpr"),
            filename: "test.mcpr".to_string(),
            title: "test".to_string(),
            duration_ms: 125_000,
            date: 1_700_000_000,
            file_size: 4_500_000,
            server_name: "Local World".to_string(),
            singleplayer: true,
        };
        assert_eq!(item.formatted_duration(), "02:05");
        assert_eq!(item.formatted_size(), "4.3 MB");

        let short_item = SavedRecordingItem {
            path: PathBuf::from("recordings/short.mcpr"),
            filename: "short.mcpr".to_string(),
            title: "short".to_string(),
            duration_ms: 45_000,
            date: 1_700_000_000,
            file_size: 512 * 1024,
            server_name: "Hypixel".to_string(),
            singleplayer: false,
        };
        assert_eq!(short_item.formatted_duration(), "00:45");
        assert_eq!(short_item.formatted_size(), "512 KB");
    }

    #[test]
    fn scan_and_select_recordings() {
        let temp_dir = std::env::temp_dir().join(format!("studio_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let rec_path = temp_dir.join("sample.mcpr");
        {
            let file = File::create(&rec_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("metaData.json", options).unwrap();
            let meta = RecordingMetadata {
                server_name: "Test Server".to_string(),
                duration: 65_000,
                date: 1_700_000_500,
                ..Default::default()
            };
            let json_bytes = serde_json::to_vec(&meta).unwrap();
            zip.write_all(&json_bytes).unwrap();
            zip.finish().unwrap();
        }

        let mut ui = StudioUi::new(&temp_dir);
        assert_eq!(ui.recordings.len(), 1);
        assert_eq!(ui.recordings[0].title, "sample");
        assert_eq!(ui.recordings[0].duration_ms, 65_000);
        assert_eq!(ui.recordings[0].server_name, "Test Server");
        assert_eq!(ui.selected, Some(0));

        // Clicking a card selects it for inspection without opening the editor.
        let action = ui.click((200.0, 60.0), (427.0, 720.0));
        assert_eq!(action, StudioAction::None);
        assert_eq!(ui.selected, Some(0));

        // Click the bottom-bar Open Replay button.
        // Panel width = 387, panel_x = 20, button x = 227..337, y = 692..712.
        let open_replay_action = ui.click((280.0, 700.0), (427.0, 720.0));
        assert!(matches!(open_replay_action, StudioAction::OpenReplay(_)));

        // Clean up
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn oversized_compressed_metadata_does_not_prevent_browsing_other_recordings() {
        let temp_dir = std::env::temp_dir().join(format!(
            "studio_metadata_budget_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        for (name, padding) in [("small", 0), ("oversized", 2 << 20)] {
            let file = File::create(temp_dir.join(format!("{name}.mcpr"))).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file(
                "metaData.json",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            let metadata = serde_json::json!({
                "duration": 42_000,
                "serverName": "Recorded server",
                "padding": "x".repeat(padding),
            });
            zip.write_all(&serde_json::to_vec(&metadata).unwrap())
                .unwrap();
            zip.finish().unwrap();
        }
        // The malicious entry is only a few KiB on disk, so an archive-size
        // check alone would not protect the browser.
        assert!(fs::metadata(temp_dir.join("oversized.mcpr")).unwrap().len() < 16 << 10);
        assert!(StudioUi::read_metadata(&temp_dir.join("oversized.mcpr")).is_none());
        let ui = StudioUi::new(&temp_dir);
        assert_eq!(ui.recordings.len(), 2);
        let small = ui
            .recordings
            .iter()
            .find(|entry| entry.title == "small")
            .unwrap();
        assert_eq!(small.duration_ms, 42_000);
        assert_eq!(small.server_name, "Recorded server");
        let bad = ui
            .recordings
            .iter()
            .find(|entry| entry.title == "oversized")
            .unwrap();
        assert_eq!(bad.duration_ms, 0);
        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn studio_wheel_scrolling() {
        let mut ui = StudioUi::default();
        ui.wheel(2.0);
        assert_eq!(ui.scroll, 0.0); // clamped at 0
        ui.scroll = 100.0;
        ui.wheel(1.0);
        assert_eq!(ui.scroll, 100.0 - ROW_STEP);
    }
}
