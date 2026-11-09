//! `/tempban <player> <duration> [reason]`, a Horizon extra for dedicated servers: a ban that ends by
//! itself. It is a `banned-players.json` entry with `expires` set, as the chat filter's automatic temporary
//! ban makes (game/chat_filter.rs), so the login check, `/banlist` and vanilla's `/pardon` already treat it as
//! they treat any ban, and it survives a restart. Operators of level 3 (`/ban`'s level) run it as a player; the
//! console takes the same words without the slash. `/tempbanlist` lists the bans that end by themselves, with
//! the time left. Vanilla has no such commands: they reach operators' command trees (completions included)
//! as a native mod's commands do (`moderation_commands`), and the game runs them itself.
use super::{Game, chat_filter::Reply, dedicated, lan::LanHost};
use horizon_client::{mod_api::NativeCommand, text::Text};
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_protocol::text::translatable;
use horizon_server_files::duration::{duration_text, now_millis, parse_duration};
use horizon_server_files::user_lists::{BanEntry, BanList, NameAndId, format_date};

/// The operator level `/tempban` needs, as `/ban`'s.
const TEMPBAN_LEVEL: u8 = 3;

/// The commands of `tempban_command`: `/tempbanlist` lists the bans that end by themselves.
pub(super) const TEMPBAN_COMMANDS: [&str; 2] = ["tempban", "tempbanlist"];

/// Horizon's moderation commands in operators' command trees (`command_tree`'s usages, for operators of
/// `/ban`'s level): the mute commands when the server filters chat (`chat_filter`), `/tempban` and
/// `/tempbanlist` on a dedicated server (`dedicated`), where the game runs them.
pub(super) fn moderation_commands(chat_filter: bool, dedicated: bool) -> Vec<NativeCommand> {
    let mut usages = Vec::new();
    if chat_filter {
        usages.extend(super::chat_filter::MUTE_USAGES);
    }
    if dedicated {
        usages.extend([
            "tempban <player:player> <duration:word> [reason:text]",
            "tempbanlist",
        ]);
    }
    usages
        .into_iter()
        .map(|usage| NativeCommand::op(usage, TEMPBAN_LEVEL))
        .collect()
}

/// `/tempbanlist`: the bans that end by themselves and haven't yet (`now`), the soonest to end first,
/// with who banned, the time left and the reason.
fn timed_bans(bans: &[&BanEntry<NameAndId>], now: i64) -> Vec<Reply> {
    let mut timed: Vec<(&BanEntry<NameAndId>, i64)> = bans
        .iter()
        .filter_map(|b| b.expires.filter(|&e| e >= now).map(|e| (*b, e)))
        .collect();
    if timed.is_empty() {
        return vec![Reply::translated("horizon.tempban.list.empty", &[])];
    }
    timed.sort_by_key(|&(b, e)| (e, b.user.name.clone()));
    let mut lines = vec![Reply::translated(
        "horizon.tempban.list.count",
        &[Text::literal(timed.len().to_string())],
    )];
    for (ban, expires) in timed {
        lines.push(
            Reply::translated(
                "horizon.tempban.list.entry",
                &[
                    Text::literal(&ban.user.name),
                    Text::literal(&ban.source),
                    Reply::translation(
                        "horizon.moderation.remaining",
                        &[Reply::duration(expires - now)],
                    ),
                    Text::literal(format_date(expires)),
                ],
            )
            .with_reason(ban.reason.as_deref()),
        );
    }
    lines
}

/// What `/tempban` was asked: the player's name, how long, and the reason, or the line to answer.
#[derive(Debug, PartialEq)]
struct Request<'a> {
    name: &'a str,
    millis: i64,
    reason: Option<String>,
}

/// `words` are the command and its arguments; the duration is `parse_duration`'s: a whole number and
/// `s`, `m`, `h`, `d` or `w` (`30m`, `12h`, `7d`).
fn parse<'a>(words: &[&'a str]) -> Result<Request<'a>, Reply> {
    let [_, name, time, reason @ ..] = words else {
        return Err(Reply::translated("horizon.tempban.usage", &[]));
    };
    let Some(millis) = parse_duration(time) else {
        return Err(Reply::translated(
            "horizon.tempban.invalid_duration",
            &[Text::literal(*time)],
        ));
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

fn success_reply(ban: &BanEntry<NameAndId>, millis: i64, expires: i64) -> Reply {
    let reason = ban.reason.as_ref().map_or_else(
        || Text::translate("multiplayer.disconnect.banned.reason.default", &[]),
        Text::literal,
    );
    Reply::translated(
        "horizon.tempban.success",
        &[
            Text::translate(
                "commands.ban.success",
                &[Text::literal(&ban.user.name), reason],
            ),
            Reply::duration(millis),
            Text::literal(format_date(expires)),
        ],
    )
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
                .map(Reply::tag)
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
    ) -> Vec<Reply> {
        if words.first() == Some(&"tempbanlist") {
            let Some(lists) = self.server_lists.clone() else {
                return vec![Reply::translated("horizon.tempban.dedicated_required", &[])];
            };
            let lists = lists.lock().unwrap();
            return timed_bans(&lists.bans.entries(), now_millis());
        }
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
    ) -> Reply {
        let Some(lists) = self.server_lists.clone() else {
            return Reply::translated("horizon.tempban.dedicated_required", &[]);
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
            return Reply::no_player();
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
            return Reply::already_banned();
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
        success_reply(&ban, millis, now + millis)
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
        for words in [&["tempban"][..], &["tempban", "Steve"][..]] {
            assert_eq!(
                parse(words).unwrap_err().text().component(),
                serde_json::json!({
                    "translate": "horizon.tempban.usage",
                    "fallback": "/tempban <player> <duration> [reason]"
                })
            );
        }
        for bad in [
            "soon",
            "0m",
            "-5m",
            "m",
            "1.5h",
            "10",
            "99999999999999999999d",
        ] {
            let reply = parse(&["tempban", "Steve", bad]).unwrap_err();
            let component = reply.text().component();
            assert_eq!(component["translate"], "horizon.tempban.invalid_duration");
            let argument = if bad == "10" {
                serde_json::json!(10)
            } else {
                serde_json::json!({"text": bad})
            };
            assert_eq!(component["with"], serde_json::json!([argument]), "{bad}");
            assert_eq!(
                horizon_client::scoreboard::tag_to_json(&reply.tag()),
                component
            );
        }
    }

    #[test]
    fn success_keeps_vanilla_reply_and_ban_data_nested_for_each_clients_language() {
        let now = 1_800_000_000_000;
        let millis = 12 * 3_600_000;
        for reason in [None, Some("griefing the farm".to_owned())] {
            let ban = entry(user(7, "Steve"), "Alex", now, millis, reason.clone());
            let reply = success_reply(&ban, millis, now + millis);
            let component = reply.text().component();
            assert_eq!(component["translate"], "horizon.tempban.success");
            assert_eq!(component["with"][0]["translate"], "commands.ban.success");
            assert_eq!(
                component["with"][0]["with"][0],
                serde_json::json!({"text": "Steve"})
            );
            let expected_reason = match &reason {
                Some(reason) => serde_json::json!({"text": reason}),
                None => {
                    serde_json::json!({"translate": "multiplayer.disconnect.banned.reason.default"})
                }
            };
            assert_eq!(component["with"][0]["with"][1], expected_reason);
            assert!(
                component["with"][1]["translate"]
                    .as_str()
                    .is_some_and(|k| k.starts_with("horizon."))
            );
            assert_eq!(
                component["with"][2],
                serde_json::json!({"text": format_date(now + millis)})
            );
            assert_eq!(
                horizon_client::scoreboard::tag_to_json(&reply.tag()),
                component
            );
            assert_eq!(ban.reason, reason);
            assert_eq!(ban.expires, Some(now + millis));
        }
    }
    #[test]
    fn initialized_console_language_resolves_the_vanilla_message_and_duration() {
        use horizon_client::lang::{self, Language, ResourceSource};
        struct Vanilla;
        impl ResourceSource for Vanilla {
            fn read(&self, path: &str) -> Option<Vec<u8>> {
                (path == "assets/minecraft/lang/en_us.json").then(|| {
                    br#"{
                    "commands.ban.success": "ban-local %1$s / %2$s",
                    "multiplayer.disconnect.banned.reason.default": "default-local"
                }"#
                    .to_vec()
                })
            }
        }
        lang::scoped(Language::load(&[&Vanilla], "en_us"), || {
            let ban = entry(user(7, "Steve"), "Alex", 0, 12 * 3_600_000, None);
            let reply = success_reply(&ban, 12 * 3_600_000, 0);
            assert_eq!(
                reply.text().plain(),
                "ban-local Steve / default-local for 12 hours, until 1970-01-01 00:00:00 +0000"
            );
        });
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

    fn ban(
        id: u128,
        name: &str,
        by: &str,
        expires: Option<i64>,
        reason: Option<&str>,
    ) -> BanEntry<NameAndId> {
        let mut b = BanEntry::new(
            user(id, name),
            Some(by.into()),
            expires,
            reason.map(Into::into),
        );
        b.created = 0;
        b
    }

    #[test]
    fn tempbanlist_lists_bans_still_running_soonest_first_with_time_left() {
        let now = 1_800_000_000_000;
        let hour = 3_600_000;
        let bans = [
            ban(1, "Forever", "rdbtclient", None, Some("x-ray")),
            ban(2, "Ended", "luzW", Some(now - 1), None),
            ban(
                3,
                "Week",
                "luzW",
                Some(now + 7 * 24 * hour),
                Some("griefing"),
            ),
            ban(4, "Hour", "Server", Some(now + hour), None),
        ];
        let text = |bans: &[BanEntry<NameAndId>]| -> Vec<String> {
            timed_bans(&bans.iter().collect::<Vec<_>>(), now)
                .iter()
                .map(|l| horizon_protocol::text::plain(&l.tag(), &Default::default()))
                .collect()
        };
        assert_eq!(
            text(&bans),
            [
                "There are 2 temporary ban(s):".to_owned(),
                format!(
                    "Hour was banned by Server for another 60 minutes (until {})",
                    format_date(now + hour)
                ),
                format!(
                    "Week was banned by luzW for another 7 days (until {}): griefing",
                    format_date(now + 7 * 24 * hour)
                ),
            ]
        );
        // Only permanent or ended bans: nothing to list.
        assert_eq!(text(&bans[..2]), ["There are no temporary bans"]);
    }

    #[test]
    fn moderation_commands_reach_only_operators_trees_and_only_where_the_game_runs_them() {
        let names = |chat_filter, dedicated| {
            let (tree, levels, problems) = super::super::native_server::command_tree(
                &moderation_commands(chat_filter, dedicated),
                |_| false,
            );
            assert!(problems.is_empty(), "{problems:?}");
            for level in levels.values() {
                assert_eq!(*level, 3);
            }
            if let Some(children) = tree["children"].as_object() {
                for (name, node) in children {
                    assert_eq!(
                        node["permissions"],
                        serde_json::json!({"type": "require", "permission": {"level": "admins"}}),
                        "{name}"
                    );
                }
            }
            let mut names: Vec<String> = levels.into_keys().collect();
            names.sort();
            names
        };
        assert_eq!(
            names(true, true),
            [
                "mute",
                "mutelist",
                "tempban",
                "tempbanlist",
                "tempmute",
                "unmute"
            ]
        );
        assert_eq!(names(false, true), ["tempban", "tempbanlist"]);
        assert_eq!(
            names(true, false),
            ["mute", "mutelist", "tempmute", "unmute"]
        );
        assert!(names(false, false).is_empty());
    }
}
