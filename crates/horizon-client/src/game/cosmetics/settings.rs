//! The cosmetics' settings (spec #14091 step 5a): their lines of `horizon-extras.txt`, the ones
//! the launcher's Wardrobe shares (`horizon_cosmetics::prefs`), through the settings store
//! ([`STORE`]), and their rows in the Extras page's Cosmetics group ([`SECTION`]). Each
//! splitscreen player has their own, as their own settings file: they live in the player's
//! [`Seat`].

use super::{Seat, service};
use horizon_cosmetics::prefs::{self, Others};
use horizon_hooks::pages::{Act, Label, Page, Place, Row};
use horizon_hooks::settings::Store;

/// My cosmetics in single player too.
const SOLO: &str = "cosmeticsSolo";

/// The settings, each with its line in `horizon-extras.txt`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Settings {
    /// Show my cosmetics (the account's, told to the service; `prefs::MINE`).
    pub mine: bool,
    /// How much of other players' cosmetics shows: no line until the player chooses
    /// (`Seat::others`).
    pub others: Option<Others>,
    /// Other players' trails and auras hidden.
    pub hide_fx: bool,
    /// My cosmetics in single player too: the window's own server names the player by their
    /// offline profile, which the service doesn't know, so they show only with this on.
    pub solo: bool,
    /// Horizon's mark before Horizon players' names (name tags, player list).
    pub marks: bool,
    /// The service may say this player plays Horizon.
    pub listed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mine: true,
            others: None,
            hide_fx: false,
            solo: false,
            marks: true,
            listed: true,
        }
    }
}

const KEYS: [&str; 6] = [
    prefs::MINE,
    prefs::OTHERS,
    prefs::HIDE_FX,
    SOLO,
    prefs::MARKS,
    prefs::LISTED,
];

impl Settings {
    /// One line; a value it can't read keeps the setting.
    fn load(&mut self, key: &str, value: &str) {
        let flag = |was: bool| match value {
            "true" => true,
            "false" => false,
            _ => was,
        };
        match key {
            prefs::MINE => self.mine = flag(self.mine),
            // `Others::parse` reads the old two-way switch too.
            prefs::OTHERS => self.others = Others::parse(value).or(self.others),
            prefs::HIDE_FX => self.hide_fx = flag(self.hide_fx),
            SOLO => self.solo = flag(self.solo),
            prefs::MARKS => self.marks = flag(self.marks),
            prefs::LISTED => self.listed = flag(self.listed),
            _ => {}
        }
    }

    /// Their lines; others' only once chosen.
    fn save(&self, line: &mut dyn FnMut(&str, &str)) {
        let text = |on: bool| if on { "true" } else { "false" };
        line(prefs::MINE, text(self.mine));
        if let Some(others) = self.others {
            line(prefs::OTHERS, others.name());
        }
        line(prefs::HIDE_FX, text(self.hide_fx));
        line(SOLO, text(self.solo));
        line(prefs::MARKS, text(self.marks));
        line(prefs::LISTED, text(self.listed));
    }
}

/// The settings' lines of `horizon-extras.txt`, the player's own file in splitscreen: the game
/// loads and saves it on the player's turn, so they are that seat's.
pub(super) static STORE: Lines = Lines;

pub(super) struct Lines;

impl Store for Lines {
    fn file(&self) -> &'static str {
        prefs::FILE
    }
    fn owns(&self, key: &str) -> bool {
        KEYS.contains(&key)
    }
    fn load(&self, key: &str, value: &str) {
        service(|s| s.settings.load(key, value));
    }
    fn save(&self, line: &mut dyn FnMut(&str, &str)) {
        service(|s| s.settings).save(line);
    }
    fn is_default(&self) -> bool {
        service(|s| s.settings) == Settings::default()
    }
}

/// The rows, top to bottom.
const ROWS: [&str; 6] = [
    "horizon.extras.cosmetics_mine",
    "horizon.extras.cosmetics_solo",
    "horizon.extras.cosmetics_others",
    "horizon.extras.cosmetics_hide_fx",
    "horizon.extras.horizon_marks",
    "horizon.extras.horizon_me",
];

const TOOLTIPS: [&str; 6] = [
    "horizon.extras.cosmetics_mine.tooltip",
    "horizon.extras.cosmetics_solo.tooltip",
    "horizon.extras.cosmetics_others.tooltip",
    "horizon.extras.cosmetics_hide_fx.tooltip",
    "horizon.extras.horizon_marks.tooltip",
    "horizon.extras.horizon_me.tooltip",
];

/// The rows in the Extras page's Cosmetics group, while cosmetics are offered
/// ([`super::enabled`]).
pub(super) static SECTION: Section = Section;

pub(super) struct Section;

impl Section {
    fn rows_of(seat: &Seat, rows: &mut Vec<Row>) {
        let s = &seat.settings;
        let shown = |on: bool| {
            Label::Key(if on {
                "horizon.extras.cosmetics.shown"
            } else {
                "horizon.extras.cosmetics.hidden"
            })
        };
        let others = Label::Key(match seat.others() {
            Others::Shown => "horizon.extras.cosmetics.shown",
            Others::NameTags => "horizon.extras.cosmetics.name_tags",
            Others::Hidden => "horizon.extras.cosmetics.hidden",
        });
        let toggle = |label, on| Row::Toggle { label, on };
        let cycle = |label, value| Row::Cycle { label, value };
        rows.extend([
            cycle(ROWS[0], shown(s.mine)),
            toggle(ROWS[1], s.solo),
            cycle(ROWS[2], others),
            toggle(ROWS[3], s.hide_fx),
            toggle(ROWS[4], s.marks),
            toggle(ROWS[5], s.listed),
        ]);
    }

    /// A press on row `row`; `back` turns the three-way row the other way.
    fn act_on(seat: &mut Seat, row: usize, back: bool) {
        let s = &mut seat.settings;
        match row {
            0 => s.mine = !s.mine,
            1 => s.solo = !s.solo,
            2 => {
                // From what the row shows; the result is the player's choice from now on.
                let o = seat.others();
                seat.settings.others = Some(if back { o.next().next() } else { o.next() });
            }
            3 => s.hide_fx = !s.hide_fx,
            4 => s.marks = !s.marks,
            5 => s.listed = !s.listed,
            _ => {}
        }
    }
}

impl Page for Section {
    fn id(&self) -> &'static str {
        "horizon-cosmetics.extras"
    }
    fn title(&self) -> &'static str {
        "horizon.extras.header.cosmetics"
    }
    fn place(&self) -> Place {
        Place::Extras
    }
    fn rows(&self, rows: &mut Vec<Row>) {
        if super::enabled() {
            service(|seat| Self::rows_of(seat, rows));
        }
    }
    fn act(&self, row: usize, act: Act) {
        let back = match act {
            Act::Press => false,
            Act::PressBack => true,
            _ => return,
        };
        service(|seat| Self::act_on(seat, row, back));
    }
    fn tooltip(&self, row: usize) -> Option<Label> {
        TOOLTIPS.get(row).copied().map(Label::Key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(text: &str) -> Settings {
        let mut s = Settings::default();
        for (key, value) in text.lines().filter_map(|l| l.split_once(':')) {
            s.load(key.trim(), value.trim());
        }
        s
    }

    fn saved(s: &Settings) -> String {
        let mut text = String::new();
        s.save(&mut |k, v| text.push_str(&format!("{k}:{v}\n")));
        text
    }

    #[test]
    fn the_lines_round_trip_and_the_wardrobe_reads_them() {
        let s = Settings {
            mine: false,
            others: Some(Others::NameTags),
            hide_fx: true,
            solo: true,
            marks: false,
            listed: false,
        };
        let text = saved(&s);
        assert_eq!(loaded(&text), s);
        // The launcher's Wardrobe reads the same lines.
        let wardrobe = prefs::Prefs::parse(&text);
        assert_eq!(
            (wardrobe.mine, wardrobe.others, wardrobe.hide_fx),
            (false, Some(Others::NameTags), true)
        );
        // No choice made: no `cosmeticsOthers` line; the old switch still reads.
        assert!(!saved(&Settings::default()).contains(prefs::OTHERS));
        assert_eq!(
            loaded("cosmeticsOthers:false\n").others,
            Some(Others::Hidden)
        );
        // A value it can't read keeps the setting.
        assert!(loaded("cosmeticsMine:maybe\n").mine);
    }

    #[test]
    fn others_cycle_three_ways_from_what_the_row_shows() {
        let mut seat = Seat::default();
        // Not chosen, nothing owned: Hidden, and forward wraps to Shown.
        assert_eq!(seat.others(), Others::Hidden);
        Section::act_on(&mut seat, 2, false);
        assert_eq!(seat.settings.others, Some(Others::Shown));
        Section::act_on(&mut seat, 2, true);
        assert_eq!(seat.settings.others, Some(Others::Hidden));
        Section::act_on(&mut seat, 2, true);
        assert_eq!(seat.settings.others, Some(Others::NameTags));
        // Owning one shows them by default, until the player chooses.
        let mut owner = Seat::default();
        owner.owner.owns = true;
        assert_eq!(owner.others(), Others::Shown);
        let mut rows = Vec::new();
        Section::rows_of(&owner, &mut rows);
        assert_eq!(
            rows[2],
            Row::Cycle {
                label: ROWS[2],
                value: Label::Key("horizon.extras.cosmetics.shown")
            }
        );
        assert_eq!(rows.len(), TOOLTIPS.len());
    }
}
