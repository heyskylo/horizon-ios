//! Timeline HUD Overlay
//!
//! Unified sequence bar and replay transport docked to the bottom of the screen.
//! Supports scrubbing, keyframe insertion, selection, deletion, and scene markers.

use super::camera::StudioFreecam;
use super::mcpr::Marker;
use horizon_client_render::{
    graphics_style as style,
    hud::{HudAtlas, HudBatch},
    text::Text,
    ui,
};

/// Action emitted when a user interacts with the timeline UI.
#[derive(Clone, Debug, PartialEq)]
pub enum TimelineAction {
    PauseForSeek,
    Seek(u64),
    TogglePlay,
    StepBack,
    StepForward,
    SeekStart,
    SeekEnd,
    SetSpeed(f64),
    ToggleFreecam,
    AddCameraKeyframe,
    RemoveCameraKeyframe(usize),
    SelectKeyframe(usize),
    KeyframeMoved,
    SplitClip(u64),
    None,
}

#[derive(Clone, Copy, Debug)]
struct TimelineHeaderLayout {
    transport_x: [f32; 5],
    time_x: f32,
    compact_time: bool,
    speed_x: Option<f32>,
    freecam_x: Option<f32>,
    freecam_width: f32,
    keyframe_x: Option<f32>,
}

impl TimelineHeaderLayout {
    fn new(screen_width: f32) -> Self {
        let transport_x = [6.0, 26.0, 46.0, 66.0, 86.0];
        let compact_time = screen_width < 460.0;
        let time_x = 110.0;
        let mut next = time_x + (if compact_time { 68.0 } else { 135.0 }) + 6.0;
        let speed_x = (screen_width >= 230.0).then_some(next);
        if speed_x.is_some() {
            next += 38.0;
        }
        let freecam_width = if screen_width < 360.0 {
            42.0
        } else if screen_width < 520.0 {
            78.0
        } else {
            100.0
        };
        let freecam_x =
            (screen_width >= 280.0 && next + freecam_width <= screen_width).then_some(next);
        if freecam_x.is_some() {
            next += freecam_width + 6.0;
        }
        let keyframe_x = (screen_width >= 460.0 && next + 122.0 <= screen_width).then_some(next);
        Self {
            transport_x,
            time_x,
            compact_time,
            speed_x,
            freecam_x,
            freecam_width,
            keyframe_x,
        }
    }
}

fn timeline_track_bounds(screen_width: f32) -> (f32, f32) {
    let start = (screen_width * 0.28).clamp(0.0, 90.0);
    let right_margin = (screen_width * 0.04).min(10.0);
    (start, (screen_width - start - right_margin).max(1.0))
}

/// In-game timeline UI state and renderer.
#[derive(Clone, Debug)]
pub struct TimelineUi {
    pub visible: bool,
    pub current_time_ms: u64,
    pub duration_ms: u64,
    pub playing: bool,
    pub speed: f64,
    pub selected_keyframe: Option<usize>,
    pub dragging_playhead: bool,
    pending_seek_ms: Option<u64>,
    pub dragging_keyframe: Option<usize>,
    pub split_points: Vec<u64>,
    pub selected_scene: usize,
    pub markers: Vec<Marker>,
}

impl Default for TimelineUi {
    fn default() -> Self {
        Self {
            visible: true,
            current_time_ms: 0,
            duration_ms: 30_000, // 30 seconds default
            playing: false,
            speed: 1.0,
            selected_keyframe: None,
            dragging_playhead: false,
            pending_seek_ms: None,
            dragging_keyframe: None,
            split_points: Vec::new(),
            selected_scene: 0,
            markers: Vec::new(),
        }
    }
}

impl TimelineUi {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total timeline height in GUI pixels: Header (24px) + Ruler (18px) + Master Sequence Bar (26px) + Spacing (8px).
    pub fn height(&self) -> f32 {
        76.0
    }

    /// Returns the scene index (0-based) for a given time in milliseconds.
    pub fn scene_at_time(&self, time_ms: u64) -> usize {
        let mut idx = 0;
        for &split_t in &self.split_points {
            if time_ms >= split_t {
                idx += 1;
            } else {
                break;
            }
        }
        idx
    }

    /// Returns the start time in milliseconds for the given scene index.
    pub fn scene_start_time(&self, scene_idx: usize) -> u64 {
        if scene_idx == 0 {
            0
        } else if scene_idx <= self.split_points.len() {
            self.split_points[scene_idx - 1]
        } else {
            self.duration_ms
        }
    }

    /// Split or cut the clip/sequence at `time_ms`.
    /// If within 250ms of an existing cut, toggles/removes that cut.
    pub fn split_at(&mut self, time_ms: u64) {
        if time_ms == 0 || time_ms >= self.duration_ms {
            return;
        }
        if let Some(pos) = self
            .split_points
            .iter()
            .position(|&t| (t as i64 - time_ms as i64).abs() < 250)
        {
            self.split_points.remove(pos);
        } else {
            self.split_points.push(time_ms);
            self.split_points.sort_unstable();
        }
        self.selected_scene = self.scene_at_time(self.current_time_ms);
    }

    /// Returns the index of a keyframe under `cursor`, if any.
    pub fn keyframe_at_cursor(
        &self,
        cursor: (f32, f32),
        screen_w: f32,
        screen_h: f32,
        freecam: &StudioFreecam,
    ) -> Option<usize> {
        let panel_h = self.height();
        let panel_y = screen_h - panel_h;
        let ruler_y = panel_y + 24.0;
        let bar_y = ruler_y + 18.0;
        let bar_h = 24.0;
        let (track_start_x, track_w) = timeline_track_bounds(screen_w);
        let total_dur = self.duration_ms.max(1) as f32;

        for (idx, kf) in freecam.track.keyframes.iter().enumerate() {
            let norm = (kf.time_ms as f32 / total_dur).clamp(0.0, 1.0);
            let kx = track_start_x + norm * track_w;
            let ky = bar_y + (bar_h * 0.5);
            if (cursor.0 - kx).abs() <= 8.0 && (cursor.1 - ky).abs() <= 10.0 {
                return Some(idx);
            }
        }
        None
    }

    /// Render the timeline overlay onto `batch`.
    pub fn render(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        freecam: &StudioFreecam,
    ) {
        if !self.visible {
            return;
        }

        let screen_w = batch.width;
        let screen_h = batch.height;
        let panel_h = self.height();
        let panel_y = screen_h - panel_h;
        let panel_rect = [0.0, panel_y, screen_w, panel_h];

        // 1. Draw glass background panel
        style::panel(batch, atlas, panel_rect);

        // 2. Transport & Action Header Bar
        let header_y = panel_y + 3.0;
        self.render_header(batch, atlas, cursor, freecam, header_y);

        // 3. Time Ruler Bar
        let ruler_y = panel_y + 24.0;
        let (track_start_x, track_w) = timeline_track_bounds(screen_w);
        self.render_ruler(batch, atlas, cursor, ruler_y, track_start_x, track_w);

        // 4. Unified Big Master Sequence Bar with Scenes and Keyframes
        let bar_y = ruler_y + 18.0;
        self.render_master_sequence_bar(
            batch,
            atlas,
            cursor,
            freecam,
            bar_y,
            track_start_x,
            track_w,
        );

        // 5. Draw Playhead Vertical Scrubber
        self.render_playhead(
            batch,
            atlas,
            panel_y + 24.0,
            panel_h - 24.0,
            track_start_x,
            track_w,
        );
    }

    fn render_header(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        freecam: &StudioFreecam,
        y: f32,
    ) {
        let layout = TimelineHeaderLayout::new(batch.width);

        // Transport buttons: |<<  <<  > / ||  >>  >>|
        let btn_w = 18.0;
        let btn_h = 16.0;
        let buttons = [
            "|<<",
            "<<",
            if self.playing { "||" } else { ">" },
            ">>",
            ">>|",
        ];

        for (index, label) in buttons.into_iter().enumerate() {
            let x = layout.transport_x[index];
            let rect = [x, y, btn_w, btn_h];
            let hover = ui::hovered(cursor, x, y, btn_w, btn_h);
            style::button(
                batch,
                atlas,
                rect,
                &Text::literal(label),
                (hover, true, false),
            );
        }

        // Time Readout: 00:00.00 / 00:30.00
        let time_str = if layout.compact_time {
            format!(
                "{}:{:02}/{}:{:02}",
                self.current_time_ms / 60_000,
                (self.current_time_ms % 60_000) / 1000,
                self.duration_ms / 60_000,
                (self.duration_ms % 60_000) / 1000
            )
        } else {
            format!(
                "{:02}:{:02}.{:02} / {:02}:{:02}.{:02}",
                self.current_time_ms / 60_000,
                (self.current_time_ms % 60_000) / 1000,
                (self.current_time_ms % 1000) / 10,
                self.duration_ms / 60_000,
                (self.duration_ms % 60_000) / 1000,
                (self.duration_ms % 1000) / 10
            )
        };
        batch.text_styled(
            atlas,
            &Text::literal(&time_str),
            layout.time_x,
            y + 4.0,
            style::WHITE,
            true,
        );
        if let Some(x) = layout.speed_x {
            let speed_str = format!("{:.1}x", self.speed);
            let speed_hover = ui::hovered(cursor, x, y, 32.0, btn_h);
            style::button(
                batch,
                atlas,
                [x, y, 32.0, btn_h],
                &Text::literal(&speed_str),
                (speed_hover, true, false),
            );
        }

        if let Some(x) = layout.freecam_x {
            let freecam_label = Text::translate(
                if batch.width < 520.0 {
                    if freecam.active {
                        "horizon.studio.timeline.freecam_active_short"
                    } else {
                        "horizon.studio.timeline.freecam_short"
                    }
                } else if freecam.active {
                    "horizon.studio.timeline.freecam_on"
                } else {
                    "horizon.studio.timeline.freecam_off"
                },
                &[],
            );
            let freecam_hover = ui::hovered(cursor, x, y, layout.freecam_width, btn_h);
            style::button(
                batch,
                atlas,
                [x, y, layout.freecam_width, btn_h],
                &freecam_label,
                (freecam_hover, true, freecam.active),
            );
        }

        if let Some(mut x) = layout.keyframe_x {
            let add_w = 40.0;
            let add_hover = ui::hovered(cursor, x, y, add_w, btn_h);
            style::button(
                batch,
                atlas,
                [x, y, add_w, btn_h],
                &Text::translate("horizon.studio.timeline.add_keyframe_short", &[]),
                (add_hover, true, false),
            );
            x += add_w + 3.0;
            let has_sel = self.selected_keyframe.is_some();
            let del_w = 38.0;
            let del_hover = ui::hovered(cursor, x, y, del_w, btn_h);
            style::button(
                batch,
                atlas,
                [x, y, del_w, btn_h],
                &Text::translate("horizon.studio.timeline.delete_keyframe_short", &[]),
                (del_hover, has_sel, has_sel),
            );
            x += del_w + 3.0;
            let split_w = 38.0;
            let split_hover = ui::hovered(cursor, x, y, split_w, btn_h);
            style::button(
                batch,
                atlas,
                [x, y, split_w, btn_h],
                &Text::translate("horizon.studio.timeline.split", &[]),
                (split_hover, true, false),
            );
        }
    }

    fn render_ruler(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        _cursor: (f32, f32),
        y: f32,
        start_x: f32,
        w: f32,
    ) {
        let ruler_rect = [start_x, y, w, 16.0];
        style::well(batch, atlas, ruler_rect);

        // Draw second tick marks and labels
        let sec_step = 1.0; // 1s ticks
        let total_secs = (self.duration_ms as f32 / 1000.0).max(1.0);
        let px_per_sec = w / total_secs;

        let mut sec = 0.0;
        while sec <= total_secs {
            let tx = start_x + sec * px_per_sec;
            if tx <= start_x + w {
                // Major tick line
                style::fill(batch, atlas, [tx, y + 8.0, 1.0, 8.0], style::HEAD_RULE);
                // Label every 2 or 5 seconds depending on width
                let is_labeled = if total_secs > 60.0 {
                    (sec as u32).is_multiple_of(10)
                } else if total_secs > 20.0 {
                    (sec as u32).is_multiple_of(5)
                } else {
                    (sec as u32).is_multiple_of(2)
                };

                if is_labeled {
                    let seconds = (sec as u32).to_string();
                    let label = Text::translate(
                        "horizon.studio.timeline.seconds",
                        &[Text::literal(seconds)],
                    );
                    batch.text_styled(atlas, &label, tx + 2.0, y + 1.0, style::SOFT, true);
                }
            }
            sec += sec_step;
        }
    }

    /// Render the unified big master sequence bar with scene blocks, splits, and keyframes.
    fn render_master_sequence_bar(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        _cursor: (f32, f32),
        freecam: &StudioFreecam,
        y: f32,
        start_x: f32,
        track_w: f32,
    ) {
        let bar_h = 24.0;

        // Track label column
        let num_scenes = self.split_points.len() + 1;
        let track_lbl = if num_scenes > 1 {
            Text::translate(
                "horizon.studio.timeline.scene_count",
                &[Text::literal(num_scenes.to_string())],
            )
        } else {
            Text::translate("horizon.studio.timeline.sequence", &[])
        };
        batch.text_styled(atlas, &track_lbl, 6.0, y + 6.0, style::WHITE, true);

        // Base lane well
        let lane_rect = [start_x, y, track_w, bar_h];
        style::fill(batch, atlas, lane_rect, 0xFF14_171F);
        style::ring(batch, atlas, lane_rect, style::ROW_RULE);

        let total_dur = self.duration_ms.max(1) as f32;

        // Render scene blocks between split points
        let mut segments = Vec::new();
        let mut prev_t = 0u64;
        for &split_t in &self.split_points {
            if split_t > prev_t && split_t < self.duration_ms {
                segments.push((prev_t, split_t));
                prev_t = split_t;
            }
        }
        segments.push((prev_t, self.duration_ms));

        for (idx, (t0, t1)) in segments.iter().enumerate() {
            let norm0 = (*t0 as f32 / total_dur).clamp(0.0, 1.0);
            let norm1 = (*t1 as f32 / total_dur).clamp(0.0, 1.0);
            let seg_x = start_x + norm0 * track_w;
            let seg_w = ((norm1 - norm0) * track_w).max(2.0);

            // Alternating clip background shades
            let bg_color = if idx % 2 == 0 {
                0xFF22_2735
            } else {
                0xFF2A_3040
            };
            style::fill(
                batch,
                atlas,
                [seg_x + 1.0, y + 1.0, seg_w - 2.0, bar_h - 2.0],
                bg_color,
            );

            // Highlight border for selected scene
            if self.selected_scene == idx {
                style::ring(
                    batch,
                    atlas,
                    [seg_x + 1.0, y + 1.0, seg_w - 2.0, bar_h - 2.0],
                    0xFFFF_C83B,
                );
            }

            // Scene name badge inside clip
            if seg_w > 45.0 {
                let scene_title = Text::translate(
                    "horizon.studio.timeline.scene_title",
                    &[Text::literal((idx + 1).to_string())],
                );
                batch.text_styled(
                    atlas,
                    &scene_title,
                    seg_x + 5.0,
                    y + 5.0,
                    style::MUTED,
                    true,
                );
            }

            // Split line / cut marker between scenes
            if idx > 0 {
                // Vertical cut line
                style::fill(batch, atlas, [seg_x - 1.0, y, 2.0, bar_h], 0xFF8A_98B8);
                // Top and bottom cut notch tick marks
                style::fill(batch, atlas, [seg_x - 3.0, y, 6.0, 2.0], 0xFFB4_C4E6);
                style::fill(
                    batch,
                    atlas,
                    [seg_x - 3.0, y + bar_h - 2.0, 6.0, 2.0],
                    0xFFB4_C4E6,
                );
            }
        }

        // Render camera keyframe diamonds directly on this big master bar
        for (idx, kf) in freecam.track.keyframes.iter().enumerate() {
            let norm = (kf.time_ms as f32 / total_dur).clamp(0.0, 1.0);
            let kx = start_x + norm * track_w;
            let ky = y + (bar_h * 0.5) - 5.0;

            let is_sel = self.selected_keyframe == Some(idx);
            let node_color = if is_sel {
                0xFFFF_C83B // Bright gold/amber selected keyframe
            } else {
                0xFF9E_7BFF // Violet accent unselected keyframe
            };

            // Selected keyframe halo
            if is_sel {
                self.draw_diamond(batch, atlas, kx, ky - 1.0, 12.0, 0x88FF_C83B);
            }

            // Draw keyframe diamond ◆
            self.draw_diamond(batch, atlas, kx, ky, 10.0, node_color);
        }

        for marker in &self.markers {
            let norm = (marker.time_ms as f32 / total_dur).clamp(0.0, 1.0);
            let x = start_x + norm * track_w;
            self.draw_diamond(batch, atlas, x, y + bar_h - 5.0, 7.0, 0xFFFF_C83B);
        }
    }

    fn render_playhead(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        y: f32,
        h: f32,
        start_x: f32,
        track_w: f32,
    ) {
        let total_dur = self.duration_ms.max(1) as f32;
        let norm = (self.current_time_ms as f32 / total_dur).clamp(0.0, 1.0);
        let px = start_x + norm * track_w;

        // Scrubber line from ruler through bottom of tracks
        style::fill(batch, atlas, [px - 1.0, y, 2.0, h], style::ACCENT);

        // Inverted triangle marker at ruler top
        let tri_color = style::WHITE;
        for i in 0..5 {
            let w = (5 - i) * 2 - 1;
            let ox = px - (w as f32 * 0.5);
            style::fill(batch, atlas, [ox, y + i as f32, w as f32, 1.0], tri_color);
        }
    }

    fn draw_diamond(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cx: f32,
        cy: f32,
        size: f32,
        color: u32,
    ) {
        let half = (size * 0.5).round();
        for i in 0..=(half as i32) {
            let w = (i * 2 + 1) as f32;
            let ox = cx - (w * 0.5).floor();
            let oy_top = cy + i as f32;
            let oy_bot = cy + (size - i as f32 - 1.0);
            style::fill(batch, atlas, [ox, oy_top, w, 1.0], color);
            if oy_bot != oy_top {
                style::fill(batch, atlas, [ox, oy_bot, w, 1.0], color);
            }
        }
    }

    /// Handle mouse click down inside the timeline bounds.
    pub fn handle_mouse_down(
        &mut self,
        cursor: (f32, f32),
        screen_w: f32,
        screen_h: f32,
        freecam: &StudioFreecam,
    ) -> TimelineAction {
        if !self.visible {
            return TimelineAction::None;
        }

        let panel_h = self.height();
        let panel_y = screen_h - panel_h;
        if cursor.1 < panel_y || cursor.1 > screen_h {
            return TimelineAction::None;
        }

        let y = panel_y + 3.0;
        let btn_w = 18.0;
        let btn_h = 16.0;

        // Transport buttons
        let layout = TimelineHeaderLayout::new(screen_w);
        if ui::hovered(cursor, layout.transport_x[0], y, btn_w, btn_h) {
            return TimelineAction::SeekStart;
        }
        if ui::hovered(cursor, layout.transport_x[1], y, btn_w, btn_h) {
            return TimelineAction::StepBack;
        }
        if ui::hovered(cursor, layout.transport_x[2], y, btn_w, btn_h) {
            return TimelineAction::TogglePlay;
        }
        if ui::hovered(cursor, layout.transport_x[3], y, btn_w, btn_h) {
            return TimelineAction::StepForward;
        }
        if ui::hovered(cursor, layout.transport_x[4], y, btn_w, btn_h) {
            return TimelineAction::SeekEnd;
        }

        // Speed button
        if layout
            .speed_x
            .is_some_and(|x| ui::hovered(cursor, x, y, 32.0, btn_h))
        {
            let next_speed = match (self.speed * 10.0).round() as i32 {
                2 => 0.5,
                5 => 1.0,
                10 => 2.0,
                20 => 5.0,
                _ => 1.0,
            };
            return TimelineAction::SetSpeed(next_speed);
        }

        // Freecam button
        if layout
            .freecam_x
            .is_some_and(|x| ui::hovered(cursor, x, y, layout.freecam_width, btn_h))
        {
            return TimelineAction::ToggleFreecam;
        }

        // Keyframe controls: [+ Key] [- Del] [Split]
        if let Some(mut kf_x) = layout.keyframe_x {
            let add_w = 40.0;
            if ui::hovered(cursor, kf_x, y, add_w, btn_h) {
                return TimelineAction::AddCameraKeyframe;
            }
            kf_x += add_w + 3.0;

            let del_w = 38.0;
            if ui::hovered(cursor, kf_x, y, del_w, btn_h)
                && let Some(sel) = self.selected_keyframe
            {
                return TimelineAction::RemoveCameraKeyframe(sel);
            }
            kf_x += del_w + 3.0;

            let split_w = 38.0;
            if ui::hovered(cursor, kf_x, y, split_w, btn_h) {
                return TimelineAction::SplitClip(self.current_time_ms);
            }
        }

        // Check keyframe nodes selection in the master sequence bar
        if let Some(idx) = self.keyframe_at_cursor(cursor, screen_w, screen_h, freecam) {
            self.selected_keyframe = Some(idx);
            self.dragging_keyframe = Some(idx);
            return TimelineAction::SelectKeyframe(idx);
        }

        // Check click on Ruler or Master Sequence Bar to seek playhead
        let (track_start_x, track_w) = timeline_track_bounds(screen_w);
        let total_dur = self.duration_ms.max(1) as f32;
        let ruler_y = panel_y + 24.0;
        let bar_y = ruler_y + 18.0;
        let bar_h = 24.0;

        if cursor.0 >= track_start_x && cursor.0 <= track_start_x + track_w {
            let norm = ((cursor.0 - track_start_x) / track_w).clamp(0.0, 1.0);
            let click_time = (norm * total_dur).round() as u64;

            // Click on Master Sequence Bar: snap slider to selected scene start
            if cursor.1 >= bar_y && cursor.1 <= bar_y + bar_h {
                let scene = self.scene_at_time(click_time);
                self.selected_scene = scene;
                let scene_start = self.scene_start_time(scene);
                self.current_time_ms = scene_start;
                self.dragging_playhead = true;
                self.pending_seek_ms = Some(scene_start);
                return TimelineAction::PauseForSeek;
            }

            // Click on Ruler: scrub playhead smoothly
            if cursor.1 >= ruler_y && cursor.1 <= screen_h {
                self.selected_scene = self.scene_at_time(click_time);
                self.current_time_ms = click_time;
                self.dragging_playhead = true;
                self.pending_seek_ms = Some(click_time);
                return TimelineAction::PauseForSeek;
            }
        }

        TimelineAction::None
    }

    /// Handle mouse drag movement while dragging playhead or keyframe.
    pub fn handle_mouse_drag(
        &mut self,
        cursor: (f32, f32),
        screen_w: f32,
        freecam: &mut StudioFreecam,
    ) -> TimelineAction {
        let (track_start_x, track_w) = timeline_track_bounds(screen_w);
        let total_dur = self.duration_ms.max(1) as f32;
        let norm = ((cursor.0 - track_start_x) / track_w).clamp(0.0, 1.0);
        let time_ms = (norm * total_dur).round() as u64;

        if self.dragging_playhead {
            self.current_time_ms = time_ms;
            self.selected_scene = self.scene_at_time(time_ms);
            self.pending_seek_ms = Some(time_ms);
            return TimelineAction::None;
        }

        if let Some(idx) = self.dragging_keyframe
            && idx < freecam.track.keyframes.len()
        {
            let target_time = time_ms.min(u32::MAX as u64) as u32;
            freecam.track.keyframes[idx].time_ms = target_time;
            // Keep keyframes sorted by time
            freecam.track.keyframes.sort_by_key(|k| k.time_ms);
            // Re-find the selected keyframe index
            if let Some(pos) = freecam
                .track
                .keyframes
                .iter()
                .position(|k| k.time_ms == target_time)
            {
                self.selected_keyframe = Some(pos);
                self.dragging_keyframe = Some(pos);
            }
            return TimelineAction::KeyframeMoved;
        }

        TimelineAction::None
    }

    /// Release mouse drag state.
    pub fn handle_mouse_up(&mut self) -> TimelineAction {
        let seek = self.pending_seek_ms.take().map(TimelineAction::Seek);
        self.dragging_playhead = false;
        self.dragging_keyframe = None;
        seek.unwrap_or(TimelineAction::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeline_ui_defaults_and_split() {
        let mut ui = TimelineUi::new();
        assert!(ui.visible);
        assert_eq!(ui.current_time_ms, 0);
        assert_eq!(ui.speed, 1.0);
        assert_eq!(ui.height(), 76.0);
        assert!(ui.split_points.is_empty());

        // Split at 5000ms
        ui.split_at(5000);
        assert_eq!(ui.split_points, vec![5000]);

        // Split at 12000ms
        ui.split_at(12000);
        assert_eq!(ui.split_points, vec![5000, 12000]);

        // Splitting close to 5000ms toggles/removes it
        ui.split_at(5100);
        assert_eq!(ui.split_points, vec![12000]);
    }

    #[test]
    fn test_scene_click_snaps_playhead() {
        let mut ui = TimelineUi::new();
        ui.duration_ms = 30_000;
        ui.split_at(10_000);
        ui.split_at(20_000);

        assert_eq!(ui.scene_at_time(0), 0);
        assert_eq!(ui.scene_at_time(5_000), 0);
        assert_eq!(ui.scene_at_time(10_000), 1);
        assert_eq!(ui.scene_at_time(15_000), 1);
        assert_eq!(ui.scene_at_time(20_000), 2);
        assert_eq!(ui.scene_at_time(25_000), 2);

        let screen_w = 1000.0;
        let screen_h = 700.0;
        let freecam = StudioFreecam::default();

        let track_start_x = 90.0;
        let track_w = screen_w - track_start_x - 10.0; // 900.0
        let panel_h = ui.height(); // 76.0
        let panel_y = screen_h - panel_h; // 624.0
        let ruler_y = panel_y + 24.0; // 648.0
        let bar_y = ruler_y + 18.0; // 666.0
        let click_y = bar_y + 10.0; // Inside master sequence bar

        // Click in Scene 2 (halfway through timeline = 15_000ms = norm 0.5)
        let click_x = track_start_x + 0.5 * track_w;
        let action = ui.handle_mouse_down((click_x, click_y), screen_w, screen_h, &freecam);

        // Slider/playhead snaps to start of Scene 2 (10_000ms)
        assert_eq!(action, TimelineAction::PauseForSeek);
        assert_eq!(ui.current_time_ms, 10_000);
        assert_eq!(ui.selected_scene, 1);
        assert_eq!(ui.handle_mouse_up(), TimelineAction::Seek(10_000));

        // Click in Scene 3 (80% through timeline = 24_000ms = norm 0.8)
        let click_x3 = track_start_x + 0.8 * track_w;
        let action3 = ui.handle_mouse_down((click_x3, click_y), screen_w, screen_h, &freecam);

        // Slider/playhead snaps to start of Scene 3 (20_000ms)
        assert_eq!(action3, TimelineAction::PauseForSeek);
        assert_eq!(ui.current_time_ms, 20_000);
        assert_eq!(ui.selected_scene, 2);
        assert_eq!(ui.handle_mouse_up(), TimelineAction::Seek(20_000));
    }

    #[test]
    fn dragging_a_keyframe_moves_it_without_seeking_playback() {
        use super::super::spline::CameraKeyframe;

        let mut ui = TimelineUi::new();
        ui.duration_ms = 30_000;
        let mut freecam = StudioFreecam::default();
        freecam.track.keyframes.push(CameraKeyframe {
            time_ms: 10_000,
            position: [1.0, 2.0, 3.0],
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov: 70.0,
        });
        let screen_w = 1000.0;
        let screen_h = 700.0;
        let (start_x, track_w) = timeline_track_bounds(screen_w);
        let y = screen_h - ui.height() + 24.0 + 18.0 + 12.0;
        let start_x = start_x + track_w / 3.0;
        assert_eq!(
            ui.handle_mouse_down((start_x, y), screen_w, screen_h, &freecam),
            TimelineAction::SelectKeyframe(0)
        );
        let moved_x = timeline_track_bounds(screen_w).0 + track_w * 0.6;
        assert_eq!(
            ui.handle_mouse_drag((moved_x, y), screen_w, &mut freecam),
            TimelineAction::KeyframeMoved
        );
        assert_eq!(freecam.track.keyframes[0].time_ms, 18_000);
        assert_eq!(ui.handle_mouse_up(), TimelineAction::None);
    }

    #[test]
    fn playhead_drag_previews_then_commits_one_seek_on_release() {
        let mut ui = TimelineUi::new();
        ui.duration_ms = 30_000;
        let freecam = StudioFreecam::default();
        let screen_w = 1000.0;
        let screen_h = 700.0;
        let (start_x, track_w) = timeline_track_bounds(screen_w);
        let ruler_y = screen_h - ui.height() + 24.0 + 4.0;
        assert_eq!(
            ui.handle_mouse_down(
                (start_x + track_w * 0.2, ruler_y),
                screen_w,
                screen_h,
                &freecam
            ),
            TimelineAction::PauseForSeek
        );
        assert_eq!(
            ui.handle_mouse_drag(
                (start_x + track_w * 0.75, ruler_y),
                screen_w,
                &mut StudioFreecam::default()
            ),
            TimelineAction::None
        );
        assert_eq!(ui.current_time_ms, 22_500);
        assert_eq!(ui.handle_mouse_up(), TimelineAction::Seek(22_500));
    }

    #[test]
    fn timeline_header_controls_fit_small_logical_widths() {
        for width in [240.0, 280.0, 320.0, 459.0, 640.0] {
            let layout = TimelineHeaderLayout::new(width);
            let end = layout.keyframe_x.map_or_else(
                || {
                    layout.freecam_x.map_or_else(
                        || layout.speed_x.map_or(104.0, |x| x + 32.0),
                        |x| x + layout.freecam_width,
                    )
                },
                |x| x + 122.0,
            );
            assert!(end <= width, "header controls overflow at width {width}");
        }
    }
}
