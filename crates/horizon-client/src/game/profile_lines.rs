//! What `/profiler` says: a chat line when there is a next step, an action bar notice for progress.
use horizon_client::client_perf::request::ProfileSide;
use horizon_client::mark_sprite::SPRITE as MARK_SPRITE;
use horizon_client::text::{
    ClickEvent, DARK_GRAY, GOLD, GRAY, InlineObject, RED, Style, Text, WHITE, YELLOW,
};
use std::path::Path;

/// Where a message goes: the chat history, or the action bar above the hotbar.
#[derive(Debug, PartialEq)]
pub(super) enum Notice {
    Chat(Text),
    Bar(Text),
}

/// Spark's chat shape: a dim tag round Horizon's mark, then the line in `color`.
fn tagged(body: Text, color: u32) -> Notice {
    let mark = Style {
        color: Some(WHITE),
        inline_object: Some(InlineObject::Atlas {
            atlas: "minecraft:gui".into(),
            sprite: MARK_SPRITE.into(),
            fallback: "H".into(),
        }),
        ..Style::default()
    };
    Notice::Chat(
        Text::colored("[", DARK_GRAY)
            .append(Text::styled("\u{fffc}", mark))
            .append(Text::colored("] ", DARK_GRAY))
            .append(body.with_style(&Style::color(color))),
    )
}

fn say(body: Text) -> Notice {
    tagged(body, GRAY)
}

fn fail(body: Text) -> Notice {
    tagged(body, RED)
}

fn bar(key: &str, args: &[Text]) -> Notice {
    Notice::Bar(Text::translate(key, args))
}

fn tr(key: &str, args: &[Text]) -> Text {
    Text::translate(key, args)
}

fn button(label: &str, hover: Text, click: ClickEvent) -> Text {
    tr(label, &[]).with_style(&Style {
        color: Some(GOLD),
        click: Some(click),
        hover: Some(Box::new(hover)),
        ..Style::default()
    })
}

fn command(label: &str, hover: &str, command: String) -> Text {
    button(label, tr(hover, &[]), ClickEvent::RunCommand(command))
}

/// Buttons side by side, a single space between.
fn row(buttons: Vec<Text>) -> Text {
    let mut out = Text::empty();
    for (i, one) in buttons.into_iter().enumerate() {
        if i > 0 {
            out.push(" ");
        }
        out.push(one);
    }
    out
}

fn stop() -> Text {
    command(
        "horizon.perf.stop_button",
        "horizon.perf.stop_hover",
        "/profiler stop".into(),
    )
}

fn start() -> Text {
    command(
        "horizon.perf.start_button",
        "horizon.perf.start_hover",
        "/profiler start".into(),
    )
}

fn folder(file: &Path, shown: &str) -> Text {
    let dir = file.parent().unwrap_or(file).to_string_lossy().into_owned();
    button(
        "horizon.perf.folder_button",
        tr("horizon.perf.folder_hover", &[Text::literal(shown)]),
        ClickEvent::OpenFile(dir),
    )
}

fn seconds(secs: f64) -> Text {
    Text::literal(format!("{secs:.0}"))
}

/// The shared address as a white, underlined link that drops the scheme from view.
fn link(url: &str) -> Text {
    let shown = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    Text::styled(
        shown,
        Style {
            color: Some(WHITE),
            underlined: Some(true),
            click: Some(ClickEvent::OpenUrl(url.to_owned())),
            hover: Some(Box::new(tr("horizon.perf.link_hover", &[]))),
            ..Style::default()
        },
    )
}

pub(super) fn started(side: Option<ProfileSide>) -> Notice {
    let key = match side {
        None => "horizon.perf.started",
        Some(ProfileSide::Client) => "horizon.perf.started.client",
        Some(ProfileSide::Server) => "horizon.perf.started.server",
    };
    say(tr(key, &[stop()]))
}

/// The answer to a status, a start while one runs, and (idle) a stop or mark with nothing to stop.
pub(super) fn status(sides: &[ProfileSide], elapsed_s: Option<f64>) -> Notice {
    let Some(elapsed) = elapsed_s.filter(|_| !sides.is_empty()) else {
        return say(tr("horizon.perf.idle", &[start()]));
    };
    let key = match (
        sides.contains(&ProfileSide::Client),
        sides.contains(&ProfileSide::Server),
    ) {
        (true, false) => "horizon.perf.running.client",
        (false, true) => "horizon.perf.running.server",
        _ => "horizon.perf.running.both",
    };
    say(tr(key, &[seconds(elapsed), stop()]))
}

/// A profile that was asked for both sides and got the client's alone.
pub(super) fn client_only(why: &str) -> Notice {
    tagged(
        tr("horizon.perf.client_only", &[Text::literal(why)]),
        YELLOW,
    )
}

/// `file` is the saved profile, `shown` its path as the player sees it, `can_open_and_share` whether this build has the hooks.
pub(super) fn saved(secs: f64, file: &Path, shown: &str, can_open_and_share: bool) -> Notice {
    let id = file.file_stem().and_then(|s| s.to_str());
    let mut buttons = Vec::new();
    if let Some(id) = id.filter(|_| can_open_and_share) {
        buttons.push(command(
            "horizon.perf.open_button",
            "horizon.perf.open_hover",
            format!("/profiler open {id}"),
        ));
        buttons.push(share_button(id));
    }
    buttons.push(folder(file, shown));
    say(tr("horizon.perf.saved", &[seconds(secs), row(buttons)]))
}

fn share_button(id: &str) -> Text {
    command(
        "horizon.perf.share_button",
        "horizon.perf.share_hover",
        format!("/profiler share {id}"),
    )
}

pub(super) fn shared(url: &str) -> Notice {
    let copy = button(
        "horizon.perf.copy_button",
        tr("horizon.perf.copy_hover", &[]),
        ClickEvent::CopyToClipboard(url.to_owned()),
    );
    say(tr("horizon.perf.shared", &[link(url), copy]))
}

pub(super) fn share_missing() -> Notice {
    say(tr("horizon.perf.share_missing", &[start()]))
}

pub(super) fn not_found(name: &str) -> Notice {
    fail(tr("horizon.perf.not_found", &[Text::literal(name)]))
}

pub(super) fn unavailable() -> Notice {
    fail(tr("horizon.perf.unavailable", &[]))
}

pub(super) fn open_failed(error: &str, file: &Path, shown: &str) -> Notice {
    fail(tr(
        "horizon.perf.open_failed",
        &[Text::literal(error), folder(file, shown)],
    ))
}

/// The retry names the profile, so it shares this one and not whichever is latest.
pub(super) fn upload_failed(error: &str, file: &Path) -> Notice {
    let retry = match file.file_stem().and_then(|s| s.to_str()) {
        Some(id) => command(
            "horizon.perf.retry_button",
            "horizon.perf.retry_hover",
            format!("/profiler share {id}"),
        ),
        None => Text::empty(),
    };
    fail(tr(
        "horizon.perf.upload_failed",
        &[Text::literal(error), retry],
    ))
}

pub(super) fn error(error: &str) -> Notice {
    fail(tr("horizon.perf.error", &[Text::literal(error)]))
}

pub(super) fn saving() -> Notice {
    bar("horizon.perf.saving", &[])
}

pub(super) fn uploading() -> Notice {
    bar("horizon.perf.uploading", &[])
}

pub(super) fn opened() -> Notice {
    bar("horizon.perf.opened", &[])
}

pub(super) fn marked(name: &str) -> Notice {
    bar("horizon.perf.marked", &[Text::literal(name)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::lang::{self, Language};
    use std::path::PathBuf;

    /// Builds notices in en_us and fails if any key they use is missing from it.
    fn english<R>(build: impl FnOnce() -> R) -> R {
        let (built, missed) = lang::scoped(Language::load(&[], "en_us"), build);
        assert!(missed.is_empty(), "keys missing from en_us: {missed:?}");
        built
    }

    fn chat(notice: Notice) -> Text {
        match notice {
            Notice::Chat(text) => text,
            Notice::Bar(text) => panic!("went to the action bar: {}", text.plain()),
        }
    }

    fn bar_text(notice: Notice) -> Text {
        match notice {
            Notice::Bar(text) => text,
            Notice::Chat(text) => panic!("went to chat: {}", text.plain()),
        }
    }

    /// The span that reads exactly `text`.
    fn span<'a>(line: &'a Text, text: &str) -> &'a horizon_client::text::Span {
        line.spans
            .iter()
            .find(|s| s.text == text)
            .unwrap_or_else(|| panic!("no span {text:?} in {:?}", line.plain()))
    }

    fn click<'a>(line: &'a Text, text: &str) -> &'a ClickEvent {
        span(line, text)
            .style
            .click
            .as_ref()
            .expect("a click event")
    }

    fn run(command: &str) -> ClickEvent {
        ClickEvent::RunCommand(command.into())
    }

    fn file() -> PathBuf {
        PathBuf::from("/home/p/reports/profile-20261110-233142Z.hzperf")
    }

    #[test]
    fn every_chat_line_starts_with_the_dim_tag_and_the_mark_sprite() {
        let line = chat(english(|| started(None)));
        assert_eq!(line.plain(), "[H] Profiling the client and server. [Stop]");
        assert_eq!(span(&line, "[").style.color, Some(DARK_GRAY));
        assert_eq!(span(&line, "] ").style.color, Some(DARK_GRAY));
        let Some(InlineObject::Atlas { sprite, .. }) = &span(&line, "\u{fffc}").style.inline_object
        else {
            panic!("the tag has no sprite");
        };
        assert_eq!(sprite, MARK_SPRITE);
    }

    #[test]
    fn a_start_names_the_sides_and_offers_stop() {
        let (both, client, server) = english(|| {
            (
                chat(started(None)),
                chat(started(Some(ProfileSide::Client))),
                chat(started(Some(ProfileSide::Server))),
            )
        });
        assert_eq!(both.plain(), "[H] Profiling the client and server. [Stop]");
        assert_eq!(client.plain(), "[H] Profiling the client. [Stop]");
        assert_eq!(server.plain(), "[H] Profiling the server. [Stop]");
        let stop = span(&both, "[Stop]");
        assert_eq!(stop.style.color, Some(GOLD));
        assert_eq!(stop.style.underlined, None);
        assert_eq!(stop.style.click, Some(run("/profiler stop")));
        assert_eq!(
            stop.style.hover.as_ref().unwrap().plain(),
            "Stop profiling and save the profile."
        );
        assert_eq!(
            span(&both, "Profiling the client and server. ").style.color,
            Some(GRAY)
        );
    }

    #[test]
    fn a_status_is_one_of_three_lines_and_idle_offers_start() {
        let line = |sides: &[ProfileSide], elapsed| chat(english(|| status(sides, elapsed)));
        use ProfileSide::{Client, Server};
        assert_eq!(
            line(&[Server, Client], Some(41.6)).plain(),
            "[H] Profiling the client and server for 42 s. [Stop]"
        );
        assert_eq!(
            line(&[Client], Some(3.0)).plain(),
            "[H] Profiling the client for 3 s. [Stop]"
        );
        assert_eq!(
            line(&[Server], Some(3.0)).plain(),
            "[H] Profiling the server for 3 s. [Stop]"
        );
        let idle = line(&[], None);
        assert_eq!(idle.plain(), "[H] Not profiling. [Start]");
        assert_eq!(click(&idle, "[Start]"), &run("/profiler start"));
    }

    #[test]
    fn the_saved_line_carries_the_length_and_the_buttons_for_this_profile() {
        let line = chat(english(|| {
            saved(
                42.2,
                &file(),
                "reports/profile-20261110-233142Z.hzperf",
                true,
            )
        }));
        assert_eq!(
            line.plain(),
            "[H] Profile saved (42 s). [Open] [Share] [Folder]"
        );
        assert_eq!(
            click(&line, "[Open]"),
            &run("/profiler open profile-20261110-233142Z")
        );
        assert_eq!(
            click(&line, "[Share]"),
            &run("/profiler share profile-20261110-233142Z")
        );
        assert_eq!(
            click(&line, "[Folder]"),
            &ClickEvent::OpenFile("/home/p/reports".into())
        );
        assert_eq!(
            span(&line, "[Folder]")
                .style
                .hover
                .as_ref()
                .unwrap()
                .plain(),
            "Show reports/profile-20261110-233142Z.hzperf in its folder."
        );
        assert!(line.spans.iter().all(|s| s.style.underlined.is_none()));
        let bare = chat(english(|| saved(42.0, &file(), "x", false)));
        assert_eq!(bare.plain(), "[H] Profile saved (42 s). [Folder]");
    }

    #[test]
    fn a_shared_profile_is_one_line_with_an_underlined_link_and_copy() {
        let url = "https://playhorizon.dev/perf/c/55bwef5st6";
        let line = chat(english(|| shared(url)));
        assert_eq!(
            line.plain(),
            "[H] Shared: playhorizon.dev/perf/c/55bwef5st6 [Copy]"
        );
        let link = span(&line, "playhorizon.dev/perf/c/55bwef5st6");
        assert_eq!(link.style.color, Some(WHITE));
        assert_eq!(link.style.underlined, Some(true));
        assert_eq!(link.style.click, Some(ClickEvent::OpenUrl(url.into())));
        assert_eq!(
            link.style.hover.as_ref().unwrap().plain(),
            "Open the shared profile in your browser."
        );
        let copy = span(&line, "[Copy]");
        assert_eq!(copy.style.color, Some(GOLD));
        assert_eq!(
            copy.style.click,
            Some(ClickEvent::CopyToClipboard(url.into()))
        );
        assert_eq!(copy.style.underlined, None);
    }

    #[test]
    fn a_failed_share_retries_that_profile_by_name() {
        let line = chat(english(|| upload_failed("no network", &file())));
        assert_eq!(
            line.plain(),
            "[H] Could not share the profile: no network. [Retry]"
        );
        assert_eq!(
            click(&line, "[Retry]"),
            &run("/profiler share profile-20261110-233142Z")
        );
        assert_eq!(
            span(&line, "Could not share the profile: ").style.color,
            Some(RED)
        );
    }

    #[test]
    fn failures_are_red_and_say_what_failed_in_words() {
        let reds = english(|| {
            [
                (error("boom"), "[H] Profiler: boom"),
                (not_found("old"), "[H] No saved profile named old."),
                (
                    unavailable(),
                    "[H] Opening and sharing profiles is not available in this build.",
                ),
                (
                    open_failed("no browser", &file(), "p"),
                    "[H] Could not open the profile: no browser. [Folder]",
                ),
            ]
        });
        for (notice, words) in reds {
            let line = chat(notice);
            assert_eq!(line.plain(), words);
            let body = &line.spans[3];
            assert_eq!(body.style.color, Some(RED), "{words}");
        }
    }

    #[test]
    fn a_profile_that_could_not_share_both_sides_is_yellow() {
        let line = chat(english(|| {
            client_only("a server profile needs operator level 4")
        }));
        assert_eq!(
            line.plain(),
            "[H] Profiling the client only: a server profile needs operator level 4"
        );
        assert_eq!(line.spans[3].style.color, Some(YELLOW));
    }

    #[test]
    fn nothing_saved_yet_is_a_gray_line_with_start() {
        let line = chat(english(share_missing));
        assert_eq!(
            line.plain(),
            "[H] No profile saved in this session yet. [Start]"
        );
        assert_eq!(click(&line, "[Start]"), &run("/profiler start"));
        assert_eq!(line.spans[3].style.color, Some(GRAY));
    }

    #[test]
    fn progress_goes_to_the_action_bar_without_a_tag() {
        let (saving, uploading, opened, marked) =
            english(|| (saving(), uploading(), opened(), marked("boss fight")));
        for (notice, words) in [
            (saving, "Saving the profile…"),
            (uploading, "Uploading the profile…"),
            (opened, "Opened in your browser"),
            (marked, "Marked boss fight"),
        ] {
            let text = bar_text(notice);
            assert_eq!(text.plain(), words);
            assert!(
                text.spans
                    .iter()
                    .all(|s| s.style.inline_object.is_none() && s.style.click.is_none())
            );
        }
    }
}
