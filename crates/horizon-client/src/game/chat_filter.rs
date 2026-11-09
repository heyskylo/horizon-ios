//! The dedicated server's chat filter, game side (docs/chat-filter.md): what the hub refused is
//! written to `logs/chat-filter.log` and told to horizon-server's event stream as `chat_filtered`
//! (never as `chat`), a temporary ban goes into banned-players.json, and operators mute and unmute
//! players by hand with `/mute`, `/tempmute`, `/mutelist` and `/unmute` (the same words on the console).
use super::{Game, dedicated, lan::LanHost};
use horizon_chat_filter::Action;
use horizon_client::lan::hub::{HostEvent, chat_filter::Place};
use horizon_client::text::Text;
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_protocol::text::{literal, translatable};
use horizon_server_files::user_lists::{BanEntry, NameAndId};
use horizon_world::nbt::Tag;

/// A moderation reply retains its translation component for each client's language.
#[derive(Debug, PartialEq)]
pub(super) struct Reply(Text);

impl Reply {
    pub(super) fn translated(key: &str, args: &[Text]) -> Self {
        Self(Self::translation(key, args))
    }

    /// Vanilla clients lack Horizon's keys, so preserve a readable fallback too.
    pub(super) fn translation(key: &str, args: &[Text]) -> Text {
        if key.starts_with("horizon.") {
            static ENGLISH: std::sync::OnceLock<horizon_client::lang::Language> =
                std::sync::OnceLock::new();
            let english =
                ENGLISH.get_or_init(|| horizon_client::lang::Language::load(&[], "en_us"));
            Text::translate_with_fallback(key, english.get(key).unwrap_or(key), args)
        } else {
            Text::translate(key, args)
        }
    }

    pub(super) fn no_player() -> Self {
        Self::translated("argument.entity.notfound.player", &[])
    }

    pub(super) fn already_banned() -> Self {
        Self::translated("commands.ban.failed", &[])
    }

    pub(super) fn duration(millis: i64) -> Text {
        use horizon_server_files::duration::{DurationUnit, duration_parts};
        let (count, unit) = duration_parts(millis);
        let key = match (unit, count == 1) {
            (DurationUnit::Minute, true) => "horizon.moderation.duration.minute",
            (DurationUnit::Minute, false) => "horizon.moderation.duration.minutes",
            (DurationUnit::Hour, true) => "horizon.moderation.duration.hour",
            (DurationUnit::Hour, false) => "horizon.moderation.duration.hours",
            (DurationUnit::Day, true) => "horizon.moderation.duration.day",
            (DurationUnit::Day, false) => "horizon.moderation.duration.days",
        };
        Self::translation(key, &[Text::literal(count.to_string())])
    }

    pub(super) fn with_reason(self, reason: Option<&str>) -> Self {
        match reason {
            Some(reason) => Self::translated(
                "horizon.moderation.reason",
                &[self.0, Text::literal(reason)],
            ),
            None => self,
        }
    }

    pub(super) fn tag(&self) -> Tag {
        horizon_client::lan::data::component_nbt(&self.0.component())
    }

    pub(super) fn text(&self) -> Text {
        self.0.clone()
    }
}

/// The operator level the mute commands need, as `/ban`'s.
const MUTE_LEVEL: u8 = 3;

/// The commands of `mute_command`.
pub(super) const MUTE_COMMANDS: [&str; 4] = ["mute", "tempmute", "mutelist", "unmute"];

/// The mute commands as operators' clients get them in the command tree (`command_tree`'s usages).
pub(super) const MUTE_USAGES: [&str; 4] = [
    "mute <player:player> [duration_and_reason:text]",
    "tempmute <player:player> <duration:word> [reason:text]",
    "mutelist",
    "unmute <player:player>",
];

/// `/mute <player> [duration] [reason]` (`words`, the command first): how long, when the word after
/// the name is a duration (`10m`, `2h`, `1d`; none: until `/unmute`), and the index the reason starts at.
fn mute_time(words: &[&str]) -> (Option<i64>, usize) {
    match words
        .get(2)
        .and_then(|w| horizon_server_files::duration::parse_duration(w))
    {
        Some(ms) => (Some(ms), 3),
        None => (None, 2),
    }
}

/// The reason a temporary ban gives at login.
const BAN_REASON: &str = "Hate speech in chat (Horizon chat filter)";

impl Game<'_> {
    /// The hub's events from a filtered text: `ChatFiltered` carried out here, the others logged.
    pub(super) fn chat_filter_events(&mut self, host: &mut LanHost, events: Vec<HostEvent>) {
        for event in events {
            match event {
                HostEvent::ChatFiltered {
                    sender,
                    name,
                    place,
                    text,
                    count,
                    action,
                } => self.chat_filtered(host, sender, &name, place, &text, count, action),
                event => self.lan_log_event(event),
            }
        }
    }

    /// One refused text: the log, the event, and a temporary ban when it came to that.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn chat_filtered(
        &mut self,
        host: &mut LanHost,
        uuid: u128,
        name: &str,
        place: Place,
        text: &str,
        count: u32,
        action: Action,
    ) {
        let now = horizon_server_files::duration::now_millis();
        let summary = format!(
            "[chat-filter] Refused {name}'s {} (hit {count}: {})",
            place.name(),
            action.name()
        );
        if self.options.dedicated.is_none() {
            log::info!("{summary}");
            return;
        }
        // The text only goes to the filter's own log, for operators: never to the console or the stream.
        dedicated::log("INFO", &summary);
        if let Some(path) = host.hub.chat_filter_log() {
            let line =
                horizon_chat_filter::log_line(now, name, uuid, place.name(), count, action, text);
            if let Err(e) = horizon_chat_filter::append_log(std::path::Path::new(path), &line) {
                dedicated::log("WARN", &format!("[chat-filter] {path}: {e}"));
            }
        }
        dedicated::events::record(
            "chat_filtered",
            name,
            uuid,
            serde_json::json!({ "place": place.name(), "hit": count, "action": action.name() }),
        );
        let Action::Banned { until } = action else {
            return;
        };
        if let Some(lists) = &self.server_lists {
            lists.lock().unwrap().bans.add(BanEntry::new(
                NameAndId {
                    id: uuid,
                    name: name.to_owned(),
                },
                Some("Server".into()),
                Some(until),
                Some(BAN_REASON.into()),
            ));
        }
        let hours = (until - now + 3_599_999) / 3_600_000;
        dedicated::log(
            "INFO",
            &format!("[chat-filter] Banned {name} for {hours} hours: {BAN_REASON}"),
        );
        let reason = literal(&format!(
            "You're banned for {hours} hours: slurs aren't allowed here."
        ));
        let conns: Vec<u32> = host
            .hub
            .guests
            .iter()
            .filter(|g| g.uuid == uuid)
            .map(|g| g.conn)
            .collect();
        for conn in conns {
            host.hub.kick(conn, reason.clone());
            dedicated::events::record(
                "kick",
                name,
                uuid,
                serde_json::json!({ "reason": "chat filter: temporary ban" }),
            );
        }
    }

    /// The message arguments of a guest's command (`/msg`, `/me`, `/say`, `/teammsg`...) through the
    /// filter: false when it refused them.
    pub(super) fn chat_filter_command(
        &mut self,
        host: &mut LanHost,
        i: usize,
        command: &str,
    ) -> bool {
        if !host.hub.chat_filter_on() {
            return true;
        }
        let permission = host.hub.guests[i].permission;
        let arguments = self.signable_arguments(permission, command);
        if arguments.is_empty() {
            return true;
        }
        let texts: Vec<&str> = arguments.iter().map(|(_, t)| t.as_str()).collect();
        let mut events = Vec::new();
        let allowed = host
            .hub
            .chat_allowed(i, &texts, Place::Command, &mut events);
        self.chat_filter_events(host, events);
        allowed
    }

    /// A guest's text for a sign, an anvil or a book through the filter (`conn`'s guest).
    pub(super) fn chat_filter_text(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        texts: &[&str],
        place: Place,
    ) -> bool {
        if !host.hub.chat_filter_on() {
            return true;
        }
        let Some(i) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return true;
        };
        let mut events = Vec::new();
        let allowed = host.hub.chat_allowed(i, texts, place, &mut events);
        self.chat_filter_events(host, events);
        allowed
    }

    /// A mute command from a guest: operators of level 3 (`/ban`'s); vanilla's unknown command for
    /// the others.
    pub(super) fn guest_mute_command(&mut self, host: &mut LanHost, conn: u32, words: &[&str]) {
        let Some(g) = host.hub.guest(conn) else {
            return;
        };
        let (permission, by) = (g.permission, g.name.clone());
        let replies = if permission < MUTE_LEVEL {
            vec![translatable("command.unknown.command", &[])]
        } else {
            let words: Vec<&str> = words.iter().copied().filter(|w| !w.is_empty()).collect();
            self.mute_command(host, &words, &by)
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

    /// `/mute <player> [reason]`, `/tempmute <player> <duration> [reason]`, `/mutelist` and
    /// `/unmute <player>` (`words`, the command first) by `by`: the lines to answer.
    pub(super) fn mute_command(
        &mut self,
        host: &mut LanHost,
        words: &[&str],
        by: &str,
    ) -> Vec<Reply> {
        let reason = |from: usize| (words.len() > from).then(|| words[from..].join(" "));
        match words {
            ["mutelist"] => {
                let now = horizon_server_files::duration::now_millis();
                let mutes = host.hub.mutes();
                if mutes.is_empty() {
                    return vec![Reply::translated("horizon.moderation.mutelist.empty", &[])];
                }
                let mut lines = vec![Reply::translated(
                    "horizon.moderation.mutelist.count",
                    &[Text::literal(mutes.len().to_string())],
                )];
                for m in mutes {
                    let by = m.by.as_deref().map_or_else(
                        || Reply::translation("horizon.moderation.chat_filter", &[]),
                        Text::literal,
                    );
                    let until = m.until.map_or_else(
                        || Reply::translation("horizon.moderation.until_unmuted", &[]),
                        |u| {
                            Reply::translation(
                                "horizon.moderation.remaining",
                                &[Reply::duration(u - now)],
                            )
                        },
                    );
                    lines.push(
                        Reply::translated(
                            "horizon.moderation.mutelist.entry",
                            &[Text::literal(m.name), by, until],
                        )
                        .with_reason(m.reason.as_deref()),
                    );
                }
                lines
            }
            ["unmute", name] => vec![self.unmute(host, name, by)],
            ["mute", name, ..] => {
                let (time, from) = mute_time(words);
                vec![self.mute(host, name, time, reason(from), by)]
            }
            ["tempmute", name, time, ..] => {
                match horizon_server_files::duration::parse_duration(time) {
                    Some(ms) => vec![self.mute(host, name, Some(ms), reason(3), by)],
                    None => vec![Reply::translated(
                        "horizon.moderation.invalid_duration",
                        &[Text::literal(*time)],
                    )],
                }
            }
            ["mute", ..] => vec![Reply::translated("horizon.moderation.usage.mute", &[])],
            ["tempmute", ..] => vec![Reply::translated("horizon.moderation.usage.tempmute", &[])],
            ["unmute", ..] => vec![Reply::translated("horizon.moderation.usage.unmute", &[])],
            _ => vec![Reply::translated("horizon.moderation.usage.mutelist", &[])],
        }
    }

    /// Mutes `name` for `time` milliseconds (`None`: until `/unmute`), logged; what to answer.
    fn mute(
        &mut self,
        host: &mut LanHost,
        name: &str,
        time: Option<i64>,
        reason: Option<String>,
        by: &str,
    ) -> Reply {
        // A player here, else one the server has seen (the user cache).
        let user = host
            .hub
            .guests
            .iter()
            .find(|g| g.name.eq_ignore_ascii_case(name))
            .map(|g| (g.uuid, g.name.clone()))
            .or_else(|| self.resolve_name(name).map(|u| (u.id, u.name)));
        let Some((uuid, name)) = user else {
            return Reply::no_player();
        };
        let until = time.map(|ms| horizon_server_files::duration::now_millis() + ms);
        let what = time.map_or_else(
            || "until unmuted".into(),
            |ms| format!("for {}", horizon_server_files::duration::duration_text(ms)),
        );
        let why = reason
            .as_deref()
            .map(|r| format!(": {r}"))
            .unwrap_or_default();
        host.hub.mute(uuid, &name, until, by, reason.clone());
        if self.options.dedicated.is_some() {
            dedicated::log(
                "INFO",
                &format!("[chat-filter] {by} muted {name} {what}{why}"),
            );
        }
        let until = time.map_or_else(
            || Reply::translation("horizon.moderation.until_unmuted", &[]),
            |ms| Reply::translation("horizon.moderation.for_duration", &[Reply::duration(ms)]),
        );
        Reply::translated("horizon.moderation.muted", &[Text::literal(name), until])
            .with_reason(reason.as_deref())
    }

    /// Lifts `name`'s mute, logged; what to answer.
    pub(super) fn unmute(&mut self, host: &mut LanHost, name: &str, by: &str) -> Reply {
        match host.hub.unmute(name) {
            Some(name) => {
                if self.options.dedicated.is_some() {
                    dedicated::log("INFO", &format!("[chat-filter] {by} unmuted {name}"));
                }
                Reply::translated("horizon.moderation.unmuted", &[Text::literal(name)])
            }
            None => Reply::translated("horizon.moderation.not_muted", &[Text::literal(name)]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanillas_messages_go_out_as_its_translation_keys() {
        for (reply, key) in [
            (Reply::no_player(), "argument.entity.notfound.player"),
            (Reply::already_banned(), "commands.ban.failed"),
        ] {
            assert_eq!(reply.tag(), translatable(key, &[]));
            assert_eq!(reply.text().translation_key(), Some(key));
        }
    }

    #[test]
    fn horizons_replies_keep_nested_translation_components() {
        let reply = Reply::translated(
            "horizon.moderation.muted",
            &[
                Text::literal("Steve"),
                Reply::translation(
                    "horizon.moderation.for_duration",
                    &[Reply::duration(61_000)],
                ),
            ],
        )
        .with_reason(Some("literal reason"));
        let component = reply.text().component();
        assert_eq!(component["translate"], "horizon.moderation.reason");
        assert_eq!(
            component["with"][0]["translate"],
            "horizon.moderation.muted"
        );
        assert_eq!(
            component["with"][0]["with"][1]["with"][0]["translate"],
            "horizon.moderation.duration.minutes"
        );
        assert_eq!(component["with"][1]["text"], "literal reason");
        assert_eq!(
            reply.tag(),
            horizon_client::lan::data::component_nbt(&component)
        );
    }

    #[test]
    fn clients_can_translate_nested_replies_or_use_the_fallback() {
        use horizon_client::lang::{self, Language};
        lang::scoped(Language::load(&[], "en_us"), || {
            let reply = Reply::translated(
                "horizon.moderation.muted",
                &[
                    Text::literal("Steve"),
                    Reply::translation(
                        "horizon.moderation.for_duration",
                        &[Reply::duration(61_000)],
                    ),
                ],
            )
            .with_reason(Some("reason %s"));
            let wire = reply.tag();
            assert_eq!(
                horizon_protocol::text::plain(&wire, &Default::default()),
                "Muted Steve for 2 minutes: reason %s"
            );
            let language = [
                (
                    "horizon.moderation.reason".to_owned(),
                    "%1$s (%2$s)".to_owned(),
                ),
                (
                    "horizon.moderation.muted".to_owned(),
                    "M %2$s %1$s".to_owned(),
                ),
                (
                    "horizon.moderation.for_duration".to_owned(),
                    "D %s".to_owned(),
                ),
                (
                    "horizon.moderation.duration.minutes".to_owned(),
                    "%s min_local".to_owned(),
                ),
            ]
            .into_iter()
            .collect();
            assert_eq!(
                horizon_protocol::text::plain(&wire, &language),
                "M D 2 min_local Steve (reason %s)"
            );
        });
    }

    #[test]
    fn bundled_languages_resolve_the_reply_on_the_receiving_client() {
        use horizon_client::lang::{self, Language};
        lang::scoped(Language::load(&[], "en_us"), || {
            let reply = Reply::translated(
                "horizon.moderation.muted",
                &[
                    Text::literal("Steve"),
                    Reply::translation(
                        "horizon.moderation.for_duration",
                        &[Reply::duration(61_000)],
                    ),
                ],
            )
            .with_reason(Some("reason %s"));
            for (code, expected) in [
                (
                    "fr_fr",
                    "Steve a été réduit au silence pour 2 minutes : reason %s",
                ),
                (
                    "de_de",
                    "Steve wurde für 2 Minuten stummgeschaltet: reason %s",
                ),
                (
                    "es_es",
                    "Se ha silenciado a Steve durante 2 minutos: reason %s",
                ),
            ] {
                let language = Language::load(&[], code);
                assert_eq!(
                    horizon_protocol::text::plain(&reply.tag(), language.map()),
                    expected,
                    "{code}"
                );
            }
        });
    }

    #[test]
    fn mute_takes_a_duration_after_the_name_or_mutes_until_unmuted() {
        assert_eq!(mute_time(&["mute", "Steve"]), (None, 2));
        assert_eq!(mute_time(&["mute", "Steve", "spamming", "chat"]), (None, 2));
        assert_eq!(
            mute_time(&["mute", "Steve", "1h", "spam"]),
            (Some(3_600_000), 3)
        );
        assert_eq!(mute_time(&["mute", "Steve", "30m"]), (Some(1_800_000), 3));
        // Not a duration: the word starts the reason.
        assert_eq!(mute_time(&["mute", "Steve", "0m", "spam"]), (None, 2));
    }
}
