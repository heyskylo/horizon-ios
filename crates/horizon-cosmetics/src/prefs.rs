//! The cosmetics switches: three lines of the game's `horizon-extras.txt` (in the game's home,
//! beside `options.txt`), so the launcher's Wardrobe and the game's settings read and write the
//! same ones.
//!
//! "Show my cosmetics" is the account's, kept by the cosmetics service: its line here is what the
//! player last asked for, and `cosmetics-visible.txt` what the service confirmed per account.
//! While the two differ, the game (or the launcher) sends the wish again.
//!
//! "Other players' cosmetics" has no line until the player picks one: then it follows whether the
//! signed-in account owns a cosmetic (`effective`), as the service last said (`cosmetics-owned.txt`
//! per account). A player who owns none sees a vanilla game, as CONTRIBUTING.md asks of Horizon-only
//! features that change the look (Ludovic, 2026-11-04).
use std::{fs, io, path::Path};

/// The game's extras file.
pub const FILE: &str = "horizon-extras.txt";
/// Other players' cosmetics: `shown`, `name_tags` or `hidden` (`true` and `false` from before);
/// no line, not chosen yet (`effective`).
pub const OTHERS: &str = "cosmeticsOthers";
/// Other players' auras and trails: `true` or `false` (the default).
pub const AURAS: &str = "cosmeticsAuras";
/// Show my cosmetics: `true` (the default) or `false`.
pub const MINE: &str = "cosmeticsMine";
/// Horizon's mark by other Horizon players' names: `true` (the default) or `false`. The game's.
pub const MARKS: &str = "horizonMarks";
/// Show me as a Horizon player: `true` (the default) or `false`. The game's; the launcher reads it
/// to draw your name tag as others see it.
pub const LISTED: &str = "horizonListed";
/// What the service confirmed, a `<uuid hex>:<true|false>` line per account.
pub const CONFIRMED: &str = "cosmetics-visible.txt";
/// Whether the account owns a cosmetic, as the service last said: a `<uuid hex>:<true|false>`
/// line per account.
pub const OWNED: &str = "cosmetics-owned.txt";

/// How much of other players' cosmetics this player sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Others {
    Shown,
    /// Only their name tag styles: no models on their bodies.
    NameTags,
    /// None: others look like plain players, and the game never asks what they wear.
    Hidden,
}

impl Others {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "shown" | "true" => Some(Self::Shown),
            "name_tags" => Some(Self::NameTags),
            "hidden" | "false" => Some(Self::Hidden),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Shown => "shown",
            Self::NameTags => "name_tags",
            Self::Hidden => "hidden",
        }
    }

    /// The next one in the game's button: shown, name tags only, hidden.
    pub fn next(self) -> Self {
        match self {
            Self::Shown => Self::NameTags,
            Self::NameTags => Self::Hidden,
            Self::Hidden => Self::Shown,
        }
    }

    /// What the player sees: what they `chose`, else everything when the account `owns` a
    /// cosmetic and nothing when it doesn't (a vanilla game, no request about other players).
    pub fn effective(chose: Option<Self>, owns: bool) -> Self {
        chose.unwrap_or(if owns { Self::Shown } else { Self::Hidden })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefs {
    /// Show my cosmetics, as last asked for.
    pub mine: bool,
    /// None until the player chooses (`Others::effective`).
    pub others: Option<Others>,
    pub auras: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            mine: true,
            others: None,
            auras: false,
        }
    }
}

impl Prefs {
    /// The switches in an extras file's `text`; a missing or unknown line keeps its default.
    pub fn parse(text: &str) -> Self {
        let mut prefs = Self::default();
        for (key, value) in text.lines().filter_map(|l| l.split_once(':')) {
            match key.trim() {
                OTHERS => prefs.others = Others::parse(value).or(prefs.others),
                AURAS => prefs.auras = value.trim() == "true",
                MINE => prefs.mine = value.trim() != "false",
                _ => {}
            }
        }
        prefs
    }

    /// The switches of the game whose home is `home`.
    pub fn load(home: &Path) -> Self {
        fs::read_to_string(home.join(FILE)).map_or_else(|_| Self::default(), |t| Self::parse(&t))
    }

    /// `text` with these lines set (others' only once chosen), every other line kept as it was.
    pub fn write_into(&self, text: &str) -> String {
        let mut out: String = text
            .lines()
            .filter(|l| {
                l.split_once(':')
                    .is_none_or(|(k, _)| ![MINE, OTHERS, AURAS].contains(&k.trim()))
            })
            .flat_map(|l| [l, "\n"])
            .collect();
        out.push_str(&format!("{MINE}:{}\n", self.mine));
        if let Some(others) = self.others {
            out.push_str(&format!("{OTHERS}:{}\n", others.name()));
        }
        out.push_str(&format!("{AURAS}:{}\n", self.auras));
        out
    }

    /// Saves these switches for the game in `home`, keeping the file's other settings.
    pub fn save(&self, home: &Path) -> io::Result<()> {
        let path = home.join(FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        fs::create_dir_all(home)?;
        fs::write(path, self.write_into(&text))
    }
}

/// Whether the game in `home` lets the service say its player plays Horizon (on unless off).
pub fn listed(home: &Path) -> bool {
    fs::read_to_string(home.join(FILE)).map_or(true, |t| {
        !t.lines()
            .filter_map(|l| l.split_once(':'))
            .any(|(k, v)| k.trim() == LISTED && v.trim() == "false")
    })
}

/// Whether the service last confirmed `uuid`'s cosmetics shown; None: never told this game.
pub fn confirmed(home: &Path, uuid: &str) -> Option<bool> {
    read_account(home, CONFIRMED, uuid)
}

/// Notes what the service confirmed for `uuid`, keeping the other accounts' lines.
pub fn set_confirmed(home: &Path, uuid: &str, visible: bool) -> io::Result<()> {
    write_account(home, CONFIRMED, uuid, visible)
}

/// Whether the service last said `uuid` owns a cosmetic; None: never asked from this game.
pub fn owned(home: &Path, uuid: &str) -> Option<bool> {
    read_account(home, OWNED, uuid)
}

/// Notes whether `uuid` owns a cosmetic, keeping the other accounts' lines.
pub fn set_owned(home: &Path, uuid: &str, owns: bool) -> io::Result<()> {
    write_account(home, OWNED, uuid, owns)
}

/// `uuid`'s line of the per-account file `name`.
fn read_account(home: &Path, name: &str, uuid: &str) -> Option<bool> {
    let text = fs::read_to_string(home.join(name)).ok()?;
    text.lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(u, _)| u.trim() == uuid)
        .map(|(_, v)| v.trim() == "true")
}

fn write_account(home: &Path, name: &str, uuid: &str, value: bool) -> io::Result<()> {
    let path = home.join(name);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut out: String = text
        .lines()
        .filter(|l| l.split_once(':').is_some_and(|(u, _)| u.trim() != uuid))
        .flat_map(|l| [l, "\n"])
        .collect();
    out.push_str(&format!("{uuid}:{value}\n"));
    fs::create_dir_all(home)?;
    fs::write(path, out)
}

/// Whether the wish for `uuid` still has to reach the service: the player asked for something
/// it never confirmed (shown is its default for an account it never heard about).
pub fn unsent(home: &Path, uuid: &str) -> Option<bool> {
    let wish = Prefs::load(home).mine;
    (confirmed(home, uuid).unwrap_or(true) != wish).then_some(wish)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_old_switch_and_keeps_the_other_settings() {
        assert_eq!(Prefs::parse(""), Prefs::default());
        assert!(!Prefs::default().auras);
        // The game's earlier on/off line still means what it meant.
        assert_eq!(
            Prefs::parse("cosmeticsOthers:false").others,
            Some(Others::Hidden)
        );
        assert_eq!(
            Prefs::parse("cosmeticsOthers:true").others,
            Some(Others::Shown)
        );
        let text = "zoom:true\ncosmeticsOthers:false\nnoAi:true\n";
        let prefs = Prefs {
            mine: false,
            others: Some(Others::NameTags),
            auras: true,
        };
        let written = prefs.write_into(text);
        assert_eq!(
            written,
            "zoom:true\nnoAi:true\ncosmeticsMine:false\ncosmeticsOthers:name_tags\ncosmeticsAuras:true\n"
        );
        assert_eq!(Prefs::parse(&written), prefs);
    }

    #[test]
    fn others_stay_unchosen_until_the_player_picks() {
        // No line, or one this game doesn't know: not chosen, and saving writes none.
        assert_eq!(Prefs::parse("zoom:true\n").others, None);
        assert_eq!(Prefs::parse("cosmeticsOthers:maybe").others, None);
        let written = Prefs::default().write_into("zoom:true\n");
        assert_eq!(
            written,
            "zoom:true\ncosmeticsMine:true\ncosmeticsAuras:false\n"
        );
        assert_eq!(Prefs::parse(&written), Prefs::default());
        // Not chosen: everything for an owner, a vanilla game for anyone else.
        assert_eq!(Others::effective(None, true), Others::Shown);
        assert_eq!(Others::effective(None, false), Others::Hidden);
        // A choice wins either way.
        for owns in [false, true] {
            for o in [Others::Shown, Others::NameTags, Others::Hidden] {
                assert_eq!(Others::effective(Some(o), owns), o);
            }
        }
    }

    #[test]
    fn the_game_button_cycles_through_the_three() {
        let mut o = Others::Shown;
        let seen: Vec<Others> = (0..3)
            .map(|_| {
                o = o.next();
                o
            })
            .collect();
        assert_eq!(seen, [Others::NameTags, Others::Hidden, Others::Shown]);
    }

    #[test]
    fn a_wish_waits_until_the_service_confirms_it() {
        let home = std::env::temp_dir().join(format!("cosmetics-prefs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let (alex, steve) = ("a".repeat(32), "b".repeat(32));
        // Nothing asked: shown, which the service assumes too.
        assert_eq!(unsent(&home, &alex), None);
        let hide = Prefs {
            mine: false,
            ..Prefs::default()
        };
        hide.save(&home).unwrap();
        assert_eq!(unsent(&home, &alex), Some(false));
        set_confirmed(&home, &steve, true).unwrap();
        set_confirmed(&home, &alex, false).unwrap();
        assert_eq!(unsent(&home, &alex), None);
        // Each account keeps its own line; Steve's account still shows its cosmetics.
        assert_eq!(unsent(&home, &steve), Some(false));
        assert_eq!(confirmed(&home, &steve), Some(true));
        // Ownership is kept the same way, per account, in a file of its own.
        assert_eq!(owned(&home, &alex), None);
        set_owned(&home, &alex, true).unwrap();
        set_owned(&home, &steve, false).unwrap();
        set_owned(&home, &alex, false).unwrap();
        assert_eq!(
            (owned(&home, &alex), owned(&home, &steve)),
            (Some(false), Some(false))
        );
        assert_eq!(confirmed(&home, &alex), Some(false));
        let _ = fs::remove_dir_all(&home);
    }
}
