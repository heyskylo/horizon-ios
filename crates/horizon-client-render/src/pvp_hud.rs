//! The PvP & HUD page's HUD widgets (`docs/pvp.md`): boxes of lines in the screen corners,
//! placed by `extras_hud::Corners` beside mods' boxes. Each widget keeps its lines, their
//! text and widths, and rebuilds them only when what it shows changes, so a frame that
//! changes nothing allocates nothing.
use crate::text::Text;

/// The icon at a line's left: an item the game draws, or an atlas sprite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Icon {
    None,
    /// The armour slot (`Game::armor`: 0 head to 3 feet).
    Armour(u8),
    /// An inventory slot's item (`OFFHAND_SLOT` for the off hand), drawn without its count.
    Slot(usize),
    /// A sprite by its atlas name (`mob_effect/speed`), 18 pixels square.
    Sprite(String),
}
impl Icon {
    pub fn size(&self) -> f32 {
        match self {
            Icon::None => 0.0,
            Icon::Armour(_) | Icon::Slot(_) => 16.0,
            Icon::Sprite(_) => 18.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub icon: Icon,
    pub text: Text,
    /// A second, grey text under the first (an effect's time left).
    pub detail: Option<Text>,
    /// The widest of its texts, in GUI pixels.
    pub text_width: f32,
    pub colour: u32,
}
/// Padding inside a widget's box.
pub const PAD: f32 = 2.0;
const TEXT_HEIGHT: f32 = 8.0;
/// Space between an icon and its text.
const ICON_GAP: f32 = 2.0;
impl Line {
    pub fn new(icon: Icon, text: String, width: &dyn Fn(&str) -> f32) -> Self {
        Self {
            icon,
            text_width: width(&text),
            text: Text::literal(text),
            detail: None,
            colour: 0xFFFF_FFFF,
        }
    }
    pub fn with_detail(mut self, detail: String, width: &dyn Fn(&str) -> f32) -> Self {
        self.text_width = self.text_width.max(width(&detail));
        self.detail = Some(Text::literal(detail));
        self
    }
    pub fn height(&self) -> f32 {
        let text = if self.detail.is_some() {
            2.0 * TEXT_HEIGHT + 2.0
        } else {
            TEXT_HEIGHT
        };
        text.max(self.icon.size()) + 2.0
    }
    /// Where the text starts, from the line's left.
    pub fn text_x(&self, icon_column: f32) -> f32 {
        if icon_column > 0.0 {
            icon_column + ICON_GAP
        } else {
            0.0
        }
    }
}

/// One widget's lines, rebuilt from `K` (what they show) only when it changes. The caller
/// fills [`Widget::next`] each frame (reusing its storage), then calls [`Widget::refresh`].
#[derive(Default)]
pub struct Widget<K> {
    shown: Option<K>,
    /// What the widget shows this frame, filled by the caller.
    pub next: K,
    pub lines: Vec<Line>,
    /// The box's size, padding included.
    pub size: (f32, f32),
}
impl<K: PartialEq + Default> Widget<K> {
    /// Rebuilds the lines with `build` when `next` differs from what they show; true if so.
    pub fn refresh(&mut self, build: impl FnOnce(&K, &mut Vec<Line>)) -> bool {
        if self.shown.as_ref() == Some(&self.next) {
            return false;
        }
        self.lines.clear();
        build(&self.next, &mut self.lines);
        self.size = size(&self.lines);
        let next = std::mem::take(&mut self.next);
        // The old key's storage comes back as `next` for the following frames.
        self.next = self.shown.replace(next).unwrap_or_default();
        true
    }
    /// Forget the lines (the widget was turned off).
    pub fn clear(&mut self) {
        self.shown = None;
        self.lines.clear();
        self.size = (0.0, 0.0);
    }
}
/// The width of the icons' column: the widest icon of the lines.
pub fn icon_column(lines: &[Line]) -> f32 {
    lines.iter().map(|l| l.icon.size()).fold(0.0, f32::max)
}
/// A box around `lines`, one under the other.
pub fn size(lines: &[Line]) -> (f32, f32) {
    if lines.is_empty() {
        return (0.0, 0.0);
    }
    let icons = icon_column(lines);
    let width = lines
        .iter()
        .map(|l| l.text_x(icons) + l.text_width)
        .fold(0.0, f32::max);
    let height = lines.iter().map(Line::height).sum::<f32>();
    (width + 2.0 * PAD, height + 2.0 * PAD - 2.0)
}

/// How armour durability shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// Uses left.
    Number,
    /// Uses left in percent of the item's maximum, rounded.
    Percent,
}
/// An item's durability text: none for an item without durability.
pub fn durability(damage: u32, max: u32, mode: Durability) -> Option<String> {
    if max == 0 {
        return None;
    }
    let left = max.saturating_sub(damage);
    Some(match mode {
        Durability::Number => left.to_string(),
        Durability::Percent => format!("{}%", (left * 100 + max / 2) / max),
    })
}
/// The colour of a durability text: `ItemStack.getBarColor`'s hue, green full to red empty.
pub fn durability_colour(damage: u32, max: u32) -> u32 {
    if max == 0 {
        return 0xFFFF_FFFF;
    }
    let left = max.saturating_sub(damage) as f32 / max as f32;
    // Mth.hsvToRgb(f / 3, 1, 1): from red (0) through yellow to green (1/3).
    let (r, g) = if left >= 0.5 {
        ((2.0 - 2.0 * left) * 255.0, 255.0)
    } else {
        (255.0, 2.0 * left * 255.0)
    };
    0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8
}

/// `Direction.fromYRot`: the horizontal direction a yaw faces, as F3's "Facing" names it.
pub fn facing(yaw: f32) -> &'static str {
    match (f64::from(yaw) / 90.0 + 0.5).floor() as i64 & 3 {
        0 => "south",
        1 => "west",
        2 => "north",
        _ => "east",
    }
}

/// The combo counter: hits in a row on what this player attacked, each counted when the
/// server reports that entity hurt (`ClientboundDamageEventPacket`) soon after the attack;
/// back to 0 when this player is hurt or no hit lands for [`Combo::EXPIRE`] seconds.
#[derive(Default)]
pub struct Combo {
    count: u32,
    /// The entity attacked last and when.
    target: Option<(i32, f64)>,
    last_hit: f64,
}
impl Combo {
    /// How long after an attack the server's hurt report still counts it.
    pub const CONFIRM: f64 = 1.0;
    pub const EXPIRE: f64 = 3.0;
    pub fn attacked(&mut self, id: i32, now: f64) {
        self.target = Some((id, now));
    }
    /// The server reports entity `id` hurt. `by_me`: whether this player is
    /// the damage's direct entity, when the report names one (melee and
    /// spear hits do; Horizon's LAN hub names none for player victims). A
    /// report that names nobody counts only as the answer to the last swing.
    pub fn hurt(&mut self, id: i32, by_me: Option<bool>, now: f64) {
        let confirmed = match by_me {
            Some(mine) => mine,
            None => self
                .target
                .is_some_and(|(t, at)| t == id && now - at <= Self::CONFIRM),
        };
        if confirmed {
            self.target = None;
            self.count += 1;
            self.last_hit = now;
        }
    }
    /// This player was hurt.
    pub fn broken(&mut self) {
        self.count = 0;
        self.target = None;
    }
    pub fn count(&mut self, now: f64) -> u32 {
        if now - self.last_hit > Self::EXPIRE {
            self.count = 0;
        }
        self.count
    }
}

/// Clicks per second: the presses of the last second, kept for the widgets that are on.
#[derive(Default)]
pub struct Clicks {
    /// Press times in seconds, oldest first; at most [`Clicks::KEEP`].
    times: std::collections::VecDeque<f64>,
}
impl Clicks {
    /// More than any hand clicks in a second: the ring never grows past it.
    const KEEP: usize = 64;
    pub fn press(&mut self, now: f64) {
        if self.times.len() == Self::KEEP {
            self.times.pop_front();
        }
        self.times.push_back(now);
    }
    /// The presses in the second up to `now`.
    pub fn per_second(&mut self, now: f64) -> u32 {
        while self.times.front().is_some_and(|&t| now - t >= 1.0) {
            self.times.pop_front();
        }
        self.times.len() as u32
    }
}

/// Horizontal speed in blocks per second, from the player's own position sampled at most
/// every [`Speed::SAMPLE`] seconds (nothing is asked of the server).
#[derive(Default)]
pub struct Speed {
    last: Option<([f64; 2], f64)>,
    /// Tenths of a block per second.
    tenths: u32,
}
impl Speed {
    pub const SAMPLE: f64 = 0.25;
    pub fn sample(&mut self, x: f64, z: f64, now: f64) -> u32 {
        match self.last {
            Some((_, at)) if now - at < Self::SAMPLE => {}
            Some(([lx, lz], at)) => {
                let d = ((x - lx).powi(2) + (z - lz).powi(2)).sqrt();
                // A teleport or respawn isn't speed.
                let v = d / (now - at);
                self.tenths = if v > 200.0 {
                    0
                } else {
                    (v * 10.0).round() as u32
                };
                self.last = Some(([x, z], now));
            }
            None => self.last = Some(([x, z], now)),
        }
        self.tenths
    }
}

/// The reach display: how far the last hit this player made was, measured from the eyes to
/// the point the crosshair's ray met the target's box. It measures what the client already
/// computed for the attack and changes nothing about it.
#[derive(Default)]
pub struct Reach {
    last: Option<(f64, f64)>,
}
impl Reach {
    /// How long the last hit's distance stays shown.
    pub const SHOWN: f64 = 3.0;
    pub fn hit(&mut self, distance: f64, now: f64) {
        self.last = Some((distance, now));
    }
    /// Hundredths of a block, while the last hit is recent.
    pub fn hundredths(&self, now: f64) -> Option<u32> {
        let (d, at) = self.last?;
        (now - at <= Self::SHOWN).then(|| (d * 100.0).round() as u32)
    }
}
/// `n` hundredths as "3.05".
pub fn hundredths(n: u32) -> String {
    format!("{}.{:02}", n / 100, n % 100)
}
/// `n` tenths as "4.3".
pub fn tenths(n: u32) -> String {
    format!("{}.{}", n / 10, n % 10)
}
/// The local time as "14:05".
pub fn clock(hour: u8, minute: u8) -> String {
    format!("{hour:02}:{minute:02}")
}

/// The keystrokes widget's keys, in drawing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Forward,
    Left,
    Back,
    Right,
    Attack,
    Use,
    Jump,
}
impl Key {
    pub const ALL: [Key; 7] = [
        Key::Forward,
        Key::Left,
        Key::Back,
        Key::Right,
        Key::Attack,
        Key::Use,
        Key::Jump,
    ];
    /// The key mapping it shows.
    pub fn mapping(self) -> &'static str {
        match self {
            Key::Forward => "key.forward",
            Key::Left => "key.left",
            Key::Back => "key.back",
            Key::Right => "key.right",
            Key::Attack => "key.attack",
            Key::Use => "key.use",
            Key::Jump => "key.jump",
        }
    }
}
/// The keystrokes' key size and the gap between keys, in GUI pixels.
pub const KEY: f32 = 22.0;
pub const KEY_GAP: f32 = 2.0;
/// The space bar's height.
pub const SPACE: f32 = 12.0;
/// Where each key of the keystrokes widget sits in its box: W above A S D, the two mouse
/// buttons under them, the space bar at the bottom, as `(x, y, w, h)`.
pub fn key_rect(key: Key) -> (f32, f32, f32, f32) {
    let step = KEY + KEY_GAP;
    let full = 3.0 * KEY + 2.0 * KEY_GAP;
    let half = (full - KEY_GAP) / 2.0;
    match key {
        Key::Forward => (step, 0.0, KEY, KEY),
        Key::Left => (0.0, step, KEY, KEY),
        Key::Back => (step, step, KEY, KEY),
        Key::Right => (2.0 * step, step, KEY, KEY),
        Key::Attack => (0.0, 2.0 * step, half, KEY),
        Key::Use => (half + KEY_GAP, 2.0 * step, half, KEY),
        Key::Jump => (0.0, 3.0 * step, full, SPACE),
    }
}
/// The keystrokes widget's size.
pub fn keys_size() -> (f32, f32) {
    let (_, y, w, h) = key_rect(Key::Jump);
    (w, y + h)
}

/// The HUD widgets, for their places on screen ([`Layout`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetId {
    Armour,
    Effects,
    Held,
    Ping,
    Coords,
    Server,
    Combo,
    Fps,
    Cps,
    Keys,
    Clock,
    Reach,
    Speed,
}
impl WidgetId {
    pub const ALL: [WidgetId; 13] = [
        WidgetId::Armour,
        WidgetId::Effects,
        WidgetId::Held,
        WidgetId::Ping,
        WidgetId::Coords,
        WidgetId::Server,
        WidgetId::Combo,
        WidgetId::Fps,
        WidgetId::Cps,
        WidgetId::Keys,
        WidgetId::Clock,
        WidgetId::Reach,
        WidgetId::Speed,
    ];
    pub fn name(self) -> &'static str {
        match self {
            WidgetId::Armour => "armour",
            WidgetId::Effects => "effects",
            WidgetId::Held => "held",
            WidgetId::Ping => "ping",
            WidgetId::Coords => "coords",
            WidgetId::Server => "server",
            WidgetId::Combo => "combo",
            WidgetId::Fps => "fps",
            WidgetId::Cps => "cps",
            WidgetId::Keys => "keys",
            WidgetId::Clock => "clock",
            WidgetId::Reach => "reach",
            WidgetId::Speed => "speed",
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Where a widget sits: in its corner, stacked with the others there (`at` is `None`), or at a
/// place the player dragged it to, as fractions of the room the screen leaves around the box
/// (0 at the left or top edge, 1 at the right or bottom), so it stays on screen at any size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub at: Option<(f32, f32)>,
    /// Its size in percent.
    pub scale: u8,
}
impl Default for Place {
    fn default() -> Self {
        Self {
            at: None,
            scale: 100,
        }
    }
}
pub const SCALES: (u8, u8) = (50, 200);
/// Every widget's place.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layout(pub [Place; 13]);
impl Layout {
    pub fn get(&self, id: WidgetId) -> Place {
        self.0[id.index()]
    }
    pub fn get_mut(&mut self, id: WidgetId) -> &mut Place {
        &mut self.0[id.index()]
    }
    /// One `hudPlace` value: `armour,0.25,0.5,150`, or `armour,corner,,150`.
    pub fn parse(&mut self, value: &str) {
        let mut parts = value.split(',');
        let Some(id) = parts
            .next()
            .and_then(|n| WidgetId::ALL.into_iter().find(|w| w.name() == n))
        else {
            return;
        };
        let (x, y, scale) = (parts.next(), parts.next(), parts.next());
        let frac = |v: Option<&str>| {
            v.and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(0.0, 1.0))
        };
        let place = self.get_mut(id);
        place.at = frac(x).zip(frac(y));
        if let Some(s) = scale.and_then(|s| s.parse::<u8>().ok()) {
            place.scale = s.clamp(SCALES.0, SCALES.1);
        }
    }
    /// The `hudPlace` lines of the widgets that left their corner or their size.
    pub fn serialize(&self, out: &mut String) {
        use std::fmt::Write;
        for id in WidgetId::ALL {
            let p = self.get(id);
            if p == Place::default() {
                continue;
            }
            let _ = match p.at {
                Some((x, y)) => writeln!(out, "hudPlace:{},{x:.4},{y:.4},{}", id.name(), p.scale),
                None => writeln!(out, "hudPlace:{},corner,,{}", id.name(), p.scale),
            };
        }
    }
}
/// A box `w` x `h` placed at fractions `(fx, fy)` of the room left on a `screen`.
pub fn from_fractions((fx, fy): (f32, f32), (w, h): (f32, f32), screen: (f32, f32)) -> (f32, f32) {
    (
        (fx * (screen.0 - w).max(0.0)).round(),
        (fy * (screen.1 - h).max(0.0)).round(),
    )
}
/// The fractions that put a `w` x `h` box at `(x, y)` on a `screen`, kept on screen.
pub fn to_fractions((x, y): (f32, f32), (w, h): (f32, f32), screen: (f32, f32)) -> (f32, f32) {
    let frac = |v: f32, room: f32| {
        if room > 0.0 {
            (v / room).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    (frac(x, screen.0 - w), frac(y, screen.1 - h))
}
/// How near, in GUI pixels, an edge must come to another to snap to it.
pub const SNAP: f32 = 4.0;
/// Snaps a box's start `x` of length `w` along one axis of length `screen`: to the screen's
/// edges and middle, and to the edges of the `others` boxes (`(start, length)`), where one
/// is within [`SNAP`]. Returns the new start and the line it snapped to, if any.
pub fn snap(x: f32, w: f32, screen: f32, others: &[(f32, f32)]) -> (f32, Option<f32>) {
    let mut best: Option<(f32, f32, f32)> = None;
    let mut consider = |start: f32, line: f32| {
        let d = (start - x).abs();
        if d <= SNAP && best.is_none_or(|(b, _, _)| d < b) {
            best = Some((d, start, line));
        }
    };
    // The screen's edges, with the corners' margin, and its middle.
    consider(2.0, 0.0);
    consider(screen - 2.0 - w, screen);
    consider((screen - w) / 2.0, screen / 2.0);
    for &(o, ow) in others {
        // Side by side, or lined up on the same edge.
        consider(o + ow, o + ow);
        consider(o - w, o);
        consider(o, o);
        consider(o + ow - w, o + ow);
    }
    match best {
        Some((_, start, line)) => (start.clamp(0.0, (screen - w).max(0.0)), Some(line)),
        None => (x.clamp(0.0, (screen - w).max(0.0)), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(s: &str) -> f32 {
        6.0 * s.chars().count() as f32
    }

    #[test]
    fn durability_shows_uses_left_as_a_number_or_a_percent() {
        // A diamond chestplate (528 uses) after 100 hits.
        assert_eq!(
            durability(100, 528, Durability::Number).as_deref(),
            Some("428")
        );
        assert_eq!(
            durability(100, 528, Durability::Percent).as_deref(),
            Some("81%")
        );
        assert_eq!(
            durability(0, 528, Durability::Percent).as_deref(),
            Some("100%")
        );
        assert_eq!(
            durability(528, 528, Durability::Number).as_deref(),
            Some("0")
        );
        assert_eq!(
            durability(0, 0, Durability::Number),
            None,
            "a pumpkin on the head"
        );
        assert_eq!(durability_colour(0, 100), 0xFF00_FF00);
        assert_eq!(durability_colour(100, 100), 0xFFFF_0000);
        assert_eq!(durability_colour(50, 100), 0xFFFF_FF00);
    }

    #[test]
    fn facing_follows_direction_from_y_rot() {
        // Yaw 0 looks toward +Z (south); turning right (yaw up) goes west, north, east.
        let names: Vec<_> = [0.0, 44.9, 45.0, 90.0, 180.0, -90.0, 270.0, -180.0, 720.0]
            .map(facing)
            .to_vec();
        assert_eq!(
            names,
            [
                "south", "south", "west", "west", "north", "east", "east", "north", "south"
            ]
        );
    }

    #[test]
    fn combos_count_confirmed_hits_until_this_player_is_hurt() {
        let mut c = Combo::default();
        c.attacked(7, 0.0);
        c.hurt(7, None, 0.1);
        c.attacked(7, 0.6);
        c.hurt(7, None, 0.7);
        assert_eq!(c.count(0.8), 2);
        // Another entity hurt, or a report too late after the swing, adds nothing.
        c.attacked(7, 1.0);
        c.hurt(9, None, 1.1);
        c.hurt(7, None, 2.5);
        assert_eq!(c.count(2.5), 2);
        // One report per swing: a second one (thorns, fire) for the same attack doesn't count.
        c.attacked(7, 3.0);
        c.hurt(7, None, 3.1);
        c.hurt(7, None, 3.2);
        assert_eq!(c.count(3.2), 3);
        c.broken();
        assert_eq!(c.count(3.3), 0);
        c.attacked(7, 4.0);
        c.hurt(7, None, 4.1);
        assert_eq!(c.count(7.0), 1);
        assert_eq!(c.count(7.2), 0, "more than 3 s since the last hit");
        // A report that names who hit decides alone: a spear jab (no swing
        // remembered) counts, a teammate's hit right after a swing doesn't.
        c.hurt(7, Some(true), 8.0);
        c.attacked(7, 8.5);
        c.hurt(7, Some(false), 8.6);
        assert_eq!(c.count(8.7), 1);
    }

    #[test]
    fn a_widget_rebuilds_only_when_what_it_shows_changes() {
        let mut w: Widget<Vec<u32>> = Widget::default();
        let mut builds = 0;
        for frame in 0..10 {
            w.next.clear();
            w.next.push(if frame < 5 { 7 } else { 8 });
            w.refresh(|k, lines| {
                builds += 1;
                lines.push(Line::new(Icon::None, k[0].to_string(), &width));
            });
        }
        assert_eq!(builds, 2);
        assert_eq!(w.lines[0].text.plain(), "8");
        w.clear();
        w.next.clear();
        w.next.push(8);
        assert!(w.refresh(|_, lines| lines.push(Line::new(Icon::None, "x".into(), &width))));
    }

    #[test]
    fn the_box_fits_its_widest_line_and_icons_line_up() {
        let lines = [
            Line::new(Icon::Armour(3), "428".into(), &width),
            Line::new(Icon::None, "12345".into(), &width),
            Line::new(
                Icon::Sprite("mob_effect/speed".into()),
                "Speed II".into(),
                &width,
            )
            .with_detail("01:30".into(), &width),
        ];
        assert_eq!(icon_column(&lines), 18.0);
        // Texts start after the 18-pixel column and a 2-pixel gap: the widest is "Speed II"
        // (48 pixels).
        let (w, h) = size(&lines);
        assert_eq!(w, 2.0 + 18.0 + 2.0 + 48.0 + 2.0);
        // 18 + 2 for the icon lines (the effect's two texts take 18), 8 + 2 for the text one.
        assert_eq!(h, 18.0 + 10.0 + 20.0 + 2.0 * PAD - 2.0);
        assert!(size(&[]) == (0.0, 0.0));
    }

    #[test]
    fn clicks_count_the_presses_of_the_last_second() {
        let mut c = Clicks::default();
        for t in [0.0, 0.1, 0.5, 0.9] {
            c.press(t);
        }
        assert_eq!(c.per_second(0.95), 4);
        // 1.0 s after the first press it has left the window.
        assert_eq!(c.per_second(1.0), 3);
        assert_eq!(c.per_second(1.6), 1);
        assert_eq!(c.per_second(5.0), 0);
        // The ring never keeps more than 64 presses.
        for i in 0..200 {
            c.press(10.0 + f64::from(i) * 0.001);
        }
        assert_eq!(c.per_second(10.2), 64);
    }

    #[test]
    fn speed_is_horizontal_blocks_per_second_between_samples() {
        let mut s = Speed::default();
        assert_eq!(s.sample(0.0, 0.0, 0.0), 0);
        // Sprinting: 5.612 blocks per second along x, with a jump (y is ignored).
        assert_eq!(s.sample(0.5, 0.0, 0.1), 0, "too soon for a new sample");
        assert_eq!(s.sample(1.403, 0.0, 0.25), 56);
        // 3 east and 4 south in a second: 5 blocks per second.
        assert_eq!(s.sample(1.403 + 3.0, 4.0, 1.25), 50);
        // A teleport reads as standing still.
        assert_eq!(s.sample(1000.0, 4.0, 1.5), 0);
        assert_eq!(tenths(56), "5.6");
    }

    #[test]
    fn reach_shows_the_last_hit_for_three_seconds() {
        let mut r = Reach::default();
        assert_eq!(r.hundredths(0.0), None);
        r.hit(2.954, 1.0);
        assert_eq!(r.hundredths(1.5), Some(295));
        assert_eq!(hundredths(295), "2.95");
        assert_eq!(hundredths(300), "3.00");
        assert_eq!(r.hundredths(4.1), None);
    }

    #[test]
    fn the_clock_reads_hours_and_minutes() {
        assert_eq!(clock(14, 5), "14:05");
        assert_eq!(clock(0, 0), "00:00");
    }

    #[test]
    fn keystrokes_keys_tile_their_box_without_overlap() {
        let (w, h) = keys_size();
        assert_eq!((w, h), (70.0, 3.0 * 24.0 + 12.0));
        for (i, a) in Key::ALL.iter().enumerate() {
            let (x, y, kw, kh) = key_rect(*a);
            assert!(
                x >= 0.0 && y >= 0.0 && x + kw <= w && y + kh <= h,
                "{a:?} inside"
            );
            for b in &Key::ALL[i + 1..] {
                let (bx, by, bw, bh) = key_rect(*b);
                let apart = x + kw <= bx || bx + bw <= x || y + kh <= by || by + bh <= y;
                assert!(apart, "{a:?} and {b:?} overlap");
            }
        }
    }

    #[test]
    fn places_round_trip_and_only_moved_widgets_are_written() {
        let mut l = Layout::default();
        let mut text = String::new();
        l.serialize(&mut text);
        assert_eq!(text, "", "a vanilla layout writes nothing");
        *l.get_mut(WidgetId::Keys) = Place {
            at: Some((0.25, 1.0)),
            scale: 150,
        };
        l.get_mut(WidgetId::Fps).scale = 75;
        l.serialize(&mut text);
        assert_eq!(
            text,
            "hudPlace:fps,corner,,75\nhudPlace:keys,0.2500,1.0000,150\n"
        );
        let mut back = Layout::default();
        for line in text.lines() {
            back.parse(line.split_once(':').unwrap().1);
        }
        assert_eq!(back, l);
        // Out of range values are clamped, unknown widgets and bad numbers ignored.
        back.parse("speed,-3,7,250");
        assert_eq!(back.get(WidgetId::Speed).at, Some((0.0, 1.0)));
        assert_eq!(back.get(WidgetId::Speed).scale, 200);
        back.parse("radar,0.5,0.5,100");
        back.parse("clock,NaN,0.5,100");
        assert_eq!(back.get(WidgetId::Clock).at, None);
    }

    #[test]
    fn fractions_keep_a_box_on_screen_at_any_size() {
        let screen = (480.0, 270.0);
        let at = to_fractions((100.0, 50.0), (80.0, 20.0), screen);
        assert_eq!(from_fractions(at, (80.0, 20.0), screen), (100.0, 50.0));
        // A box at the right edge stays at the right edge on a wider screen.
        let right = to_fractions((400.0, 0.0), (80.0, 20.0), screen);
        assert_eq!(right, (1.0, 0.0));
        assert_eq!(
            from_fractions(right, (80.0, 20.0), (960.0, 540.0)),
            (880.0, 0.0)
        );
        // Dragged past the edge: kept inside.
        assert_eq!(
            to_fractions((-30.0, 900.0), (80.0, 20.0), screen),
            (0.0, 1.0)
        );
    }

    #[test]
    fn boxes_snap_to_screen_edges_middle_and_each_other() {
        // Near the left margin.
        assert_eq!(snap(4.5, 50.0, 480.0, &[]), (2.0, Some(0.0)));
        // Centred: the middle of 480 for a 50-wide box starts at 215.
        assert_eq!(snap(213.0, 50.0, 480.0, &[]), (215.0, Some(240.0)));
        // Beside another box (100..160): its start goes to 160.
        assert_eq!(
            snap(163.0, 50.0, 480.0, &[(100.0, 60.0)]),
            (160.0, Some(160.0))
        );
        // Lined up with its left edge.
        assert_eq!(
            snap(98.0, 50.0, 480.0, &[(100.0, 60.0)]),
            (100.0, Some(100.0))
        );
        // Too far from anything: stays, but never off screen.
        assert_eq!(snap(300.0, 50.0, 480.0, &[(100.0, 60.0)]), (300.0, None));
        assert_eq!(snap(470.0, 50.0, 480.0, &[]), (430.0, None));
    }
}
