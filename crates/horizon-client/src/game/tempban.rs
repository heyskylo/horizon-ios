//! `/tempban <player> <duration> [reason]`, a Horizon extra for dedicated servers: a ban that ends by
//! itself. It is a `banned-players.json` entry with `expires` set, as the chat filter's automatic temporary
//! ban makes (game/chat_filter.rs), so the login check, `/banlist` and vanilla's `/pardon` already treat it as
//! they treat any ban, and it survives a restart. Operators of level 3 (`/ban`'s level) run it as a player; the
//! console takes the same words without the slash. Vanilla has no such command, so it is not in the command
//! tree (and has no completions), as the mute commands are not.
use super::{Game, dedicated, lan::LanHost};
use horizon_chat_filter::{duration_text, now_millis, parse_duration};
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_protocol::text::{literal, translatable};
use horizon_server_files::user_lists::{BanEntry, BanList, NameAndId, format_date};

/// The operator level `/tempban` needs, as `/ban`'s.
const TEMPBAN_LEVEL: u8 = 3;

/// The commands of `tempban_command`.
pub(super) const TEMPBAN_COMMANDS: [&str; 1] = ["tempban"];

const USAGE: &str = "/tempban <player> <duration> [reason]";

/// What `/tempban` was asked: the player's name, how long, and the reason, or the line to answer.
#[derive(Debug, PartialEq)]
struct Request<'a> {
    name: &'a str,
    millis: i64,
    reason: Option<String>,
}

/// `words` are the command and its arguments; the duration is `parse_duration`'s: a whole number and
/// `s`, `m`, `h`, `d` or `w` (`30m`, `12h`, `7d`).
fn parse<'a>(words: &[&'a str]) -> Result<Request<'a>, String> {
    let [_, name, time, reason @ ..] = words else {
        return Err(USAGE.into());
    };
    let Some(millis) = parse_duration(time) else {
        return Err(format!("{time} isn't a duration: 30m, 12h, 7d"));
    };
    Ok(Request {
        name,
        millis,
        reason: (!reason.is_empty()).then(|| reason.join(" ")),
    })
}

/// The ban entry for `/tempban`: ends `millis` after `now`.
fn entry(
    user: NameAndId,
    by: &str,
    now: i64,
    millis: i64,
    reason: Option<String>,
) -> BanEntry<NameAndId> {
    let mut ban = BanEntry::new(user, Some(by.to_owned()), Some(now + millis), reason);
    ban.created = now;
    ban
}

/// Puts the ban on the list, which saves it; `None` when the player is already banned (as vanilla's `/ban`,
/// it never replaces a ban: `/pardon` first). A ban that has ended is no ban.
fn add_ban(
    bans: &mut BanList,
    user: NameAndId,
    by: &str,
    now: i64,
    millis: i64,
    reason: Option<String>,
) -> Option<BanEntry<NameAndId>> {
    if bans.get(&user).is_some() {
        return None;
    }
    let ban = entry(user, by, now, millis, reason);
    bans.add(ban.clone());
    Some(ban)
}

impl Game<'_> {
    /// A guest's `/tempban`: operators of level 3 (`/ban`'s); vanilla's unknown command for the others.
    pub(super) fn guest_tempban_command(&mut self, host: &mut LanHost, conn: u32, words: &[&str]) {
        let Some(g) = host.hub.guest(conn) else {
            return;
        };
        let (permission, by) = (g.permission, g.name.clone());
        let replies = if permission < TEMPBAN_LEVEL {
            vec![translatable("command.unknown.command", &[])]
        } else {
            let words: Vec<&str> = words.iter().copied().filter(|w| !w.is_empty()).collect();
            self.tempban_command(host, &words, &by)
                .iter()
                .map(|line| literal(line))
                .collect()
        };
        for content in replies {
            host.hub.send_to(
                conn,
                &C::SystemChat(play::SystemChat {
                    content,
                    overlay: false,
                }),
            );
        }
    }

    /// `/tempban <player> <duration> [reason]` (`words`, the command first) by `by`: the lines to answer.
    pub(super) fn tempban_command(
        &mut self,
        host: &mut LanHost,
        words: &[&str],
        by: &str,
    ) -> Vec<String> {
        match parse(words) {
            Ok(r) => vec![self.tempban(host, r.name, r.millis, r.reason, by)],
            Err(line) => vec![line],
        }
    }

    /// Bans `name` for `millis`, kicks them if they are here, logs it; what to answer.
    fn tempban(
        &mut self,
        host: &mut LanHost,
        name: &str,
        millis: i64,
        reason: Option<String>,
        by: &str,
    ) -> String {
        let Some(lists) = self.server_lists.clone() else {
            return "Temporary bans need a dedicated server".into();
        };
        // A player here, else one the server has seen (the user cache).
        let user = host
            .hub
            .guests
            .iter()
            .find(|g| g.name.eq_ignore_ascii_case(name))
            .map(|g| NameAndId {
                id: g.uuid,
                name: g.name.clone(),
            })
            .or_else(|| self.resolve_name(name));
        let Some(user) = user else {
            return format!("No player was found named {name}");
        };
        let now = now_millis();
        let Some(ban) = add_ban(
            &mut lists.lock().unwrap().bans,
            user.clone(),
            by,
            now,
            millis,
            reason.clone(),
        ) else {
            return format!("{} is already banned", user.name);
        };
        let what = duration_text(millis);
        let why = reason
            .as_deref()
            .map(|r| format!(": {r}"))
            .unwrap_or_default();
        dedicated::log(
            "INFO",
            &format!("[tempban] {by} banned {} for {what}{why}", user.name),
        );
        let kick = dedicated::ban_message(&ban);
        let conns: Vec<u32> = host
            .hub
            .guests
            .iter()
            .filter(|g| g.uuid == user.id)
            .map(|g| g.conn)
            .collect();
        for conn in conns {
            host.hub.kick(conn, kick.clone());
        }
        dedicated::events::record(
            "ban",
            &user.name,
            user.id,
            serde_json::json!({
                "reason": reason,
                "by": by,
                "until": ban.expires.map(format_date),
            }),
        );
        format!(
            "Banned {} for {what}, until {}{why}",
            user.name,
            format_date(now + millis)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u128, name: &str) -> NameAndId {
        NameAndId {
            id,
            name: name.into(),
        }
    }
    fn list(tag: &str) -> BanList {
        let file =
            std::env::temp_dir().join(format!("horizon-tempban-{tag}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&file);
        BanList::new(file)
    }

    #[test]
    fn durations_are_the_mutes_and_a_reason_is_the_rest() {
        for (time, millis) in [
            ("30m", 30 * 60_000),
            ("12h", 12 * 3_600_000),
            ("7d", 7 * 86_400_000),
            ("45s", 45_000),
            ("2w", 14 * 86_400_000),
        ] {
            let r = parse(&["tempban", "Steve", time]).unwrap();
            assert_eq!((r.name, r.millis, r.reason), ("Steve", millis, None));
        }
        let r = parse(&["tempban", "Steve", "1d", "griefing", "the", "farm"]).unwrap();
        assert_eq!(r.reason.as_deref(), Some("griefing the farm"));
    }
    #[test]
    fn bad_words_get_a_usage_or_the_bad_duration() {
        assert_eq!(parse(&["tempban"]), Err(USAGE.into()));
        assert_eq!(parse(&["tempban", "Steve"]), Err(USAGE.into()));
        for bad in [
            "soon",
            "0m",
            "-5m",
            "m",
            "1.5h",
            "10",
            "99999999999999999999d",
        ] {
            assert_eq!(
                parse(&["tempban", "Steve", bad]),
                Err(format!("{bad} isn't a duration: 30m, 12h, 7d")),
                "{bad}"
            );
        }
    }
    #[test]
    fn the_ban_ends_after_its_duration_and_keeps_who_and_why() {
        let mut bans = list("ends");
        let now = 1_800_000_000_000;
        let ban = add_ban(
            &mut bans,
            user(7, "Steve"),
            "Alex",
            now,
            12 * 3_600_000,
            Some("griefing".into()),
        )
        .unwrap();
        assert_eq!(ban.expires, Some(now + 12 * 3_600_000));
        assert_eq!((ban.created, ban.source.as_str()), (now, "Alex"));
        assert_eq!(ban.reason.as_deref(), Some("griefing"));
        assert!(bans.contains(&user(7, "Steve")));
        // Banned while the ban lasts, whatever the name says.
        assert!(bans.get(&user(7, "Renamed")).is_some());
    }
    #[test]
    fn a_ban_in_force_is_not_replaced_but_an_ended_one_is() {
        let mut bans = list("replace");
        let now = now_millis();
        assert!(add_ban(&mut bans, user(1, "A"), "Op", now, 3_600_000, None).is_some());
        assert!(add_ban(&mut bans, user(1, "A"), "Op", now, 60_000, None).is_none());
        // Permanent ban: not replaced by a shorter one.
        assert!(bans.add(BanEntry::new(user(2, "B"), Some("Op".into()), None, None)));
        assert!(add_ban(&mut bans, user(2, "B"), "Op", now, 60_000, None).is_none());
        // An ended ban is dropped when asked about, so a new one can be made.
        assert!(bans.add(BanEntry::new(
            user(3, "C"),
            Some("Op".into()),
            Some(now - 1),
            None
        )));
        assert!(add_ban(&mut bans, user(3, "C"), "Op", now, 60_000, None).is_some());
    }
    #[test]
    fn the_file_keeps_the_end_and_pardon_removes_it() {
        let mut bans = list("file");
        let now = now_millis() + 3_600_000;
        let end = add_ban(&mut bans, user(9, "Dee"), "Op", now, 86_400_000, None)
            .unwrap()
            .expires
            .unwrap();
        let text = std::fs::read_to_string(&bans.file).unwrap();
        // The file writes `expires` to the second, as vanilla's ban list does.
        assert!(text.contains(&format_date(end)), "{text}");
        // A restart: the list reads it back, still banned, still ending then.
        let mut after = BanList::new(bans.file.clone());
        after.load().unwrap();
        let read = after.get(&user(9, "Dee")).unwrap();
        assert_eq!(read.expires, Some(end / 1000 * 1000));
        // `/pardon` is `remove`, as for any ban.
        assert!(after.remove(&user(9, "Dee")));
        assert!(after.get(&user(9, "Dee")).is_none());
        let _ = std::fs::remove_file(&bans.file);
    }
    #[test]
    fn a_ban_that_has_ended_lets_the_player_in_after_a_restart() {
        let mut bans = list("expired");
        let long_ago = now_millis() - 7_200_000;
        assert!(add_ban(&mut bans, user(4, "Eve"), "Op", long_ago, 3_600_000, None).is_some());
        let mut after = BanList::new(bans.file.clone());
        after.load().unwrap();
        assert!(after.get(&user(4, "Eve")).is_none());
        let _ = std::fs::remove_file(&bans.file);
    }
}
