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
}
