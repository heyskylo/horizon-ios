use super::*;
use crate::{font::Font, hud::Rect};
use std::{collections::HashMap, path::Path};

/// The Chat option's default: no restriction.
fn full() -> ChatAbilities {
    ChatAbilities::new(ChatVisibility::Full)
}

#[test]
fn accepted_suggestions_report_utf16_ranges_and_tab_cycles() {
    use winit::keyboard::KeyCode as K;
    let font = font();
    let m = Measure(&font);
    let registries = Registries {
        players: vec!["Steve".into(), "Steven".into()],
        ..Registries::default()
    };
    let c = Commands {
        dispatcher: crate::command::vanilla::game_dispatcher(),
        registries: &registries,
    };
    let mut screen = ChatScreen::new(&m, &c, "", false, 0, 640, 360, full());
    screen.type_text(&m, &c, "😀 hi st");
    assert!(screen.take_suggestion_completion().is_none());
    screen.key(&m, &c, &[], K::Tab, false, false);
    assert!(
        screen.take_suggestion_completion().is_none(),
        "opening is not accepting"
    );
    for (shift, text) in [(false, "Steve"), (false, "Steven"), (true, "Steve")] {
        screen.key(&m, &c, &[], K::Tab, shift, false);
        assert_eq!(screen.value(), format!("😀 hi {text}"));
        assert_eq!(
            screen.take_suggestion_completion(),
            Some(SuggestionCompletion {
                original: "😀 hi st".into(),
                range: (6, 8),
                text: text.into(),
                input: format!("😀 hi {text}"),
            })
        );
        assert!(
            screen.take_suggestion_completion().is_none(),
            "reported once"
        );
    }
    screen.key(&m, &c, &[], K::ArrowDown, false, false);
    assert!(
        screen.take_suggestion_completion().is_none(),
        "selection is not accepting"
    );
    screen.key(&m, &c, &[], K::Escape, false, false);
    assert!(screen.take_suggestion_completion().is_none());
}

#[test]
fn clicked_suggestion_reports_the_actual_length_limited_input() {
    use winit::keyboard::KeyCode as K;
    let font = font();
    let m = Measure(&font);
    let registries = Registries {
        players: vec!["Steve".into()],
        ..Registries::default()
    };
    let c = Commands {
        dispatcher: crate::command::vanilla::game_dispatcher(),
        registries: &registries,
    };
    let mut screen = ChatScreen::new(&m, &c, "", false, 0, 640, 360, full());
    screen.input.max_length = 6;
    screen.type_text(&m, &c, "hi st");
    screen.key(&m, &c, &[], K::Tab, false, false);
    let [x, y, _, _] = screen.suggestions.as_ref().unwrap().rect;
    assert!(screen.click(&m, &c, f64::from(x + 1), f64::from(y + 1)));
    assert_eq!(screen.value(), "hi Ste");
    assert_eq!(
        screen.take_suggestion_completion(),
        Some(SuggestionCompletion {
            original: "hi st".into(),
            range: (3, 5),
            text: "Steve".into(),
            input: "hi Ste".into(),
        })
    );
    assert!(screen.take_suggestion_completion().is_none());
}

fn font() -> Font {
    // The missing-glyph metrics let these tests run without game assets.
    Font::load(
        Path::new("/nonexistent-horizon-chat-test-resources"),
        &HashMap::new(),
        &[],
        (1, 1),
        (0, 0),
        Rect {
            u0: 0.0,
            v0: 0.0,
            u1: 1.0,
            v1: 1.0,
            w: 1,
            h: 1,
        },
    )
}

fn add(chat: &mut Chat, font: &Font, content: Text) {
    for content in font.wrap_components(&content, chat.max_width()) {
        chat.lines.insert(
            0,
            Line {
                added: 0,
                content,
                tag: None,
                message: "".into(),
            },
        );
    }
}

#[test]
fn smooth_chat_slides_a_line_in_and_can_hide_the_indicator() {
    let mut chat = Chat {
        smooth: true,
        ..Chat::default()
    };
    chat.lines.push(Line {
        added: 0,
        content: Text::literal("hi"),
        tag: Some(std::sync::Arc::new(Tag::system_single_player(
            SYSTEM_INDICATOR,
        ))),
        message: "hi".into(),
    });
    let text_alpha = |draws: &[GuiDraw]| {
        draws.iter().find_map(|d| match d {
            GuiDraw::Text { color, .. } => Some(color >> 24),
            _ => None,
        })
    };
    // The slide is the negative translate the line carries while it moves.
    let slide = |draws: &[GuiDraw]| {
        draws.iter().find_map(|d| match d {
            GuiDraw::Push(ops) => ops.iter().find_map(|op| match op {
                PoseOp::Translate(x, _) if *x < 0.0 => Some(*x),
                _ => None,
            }),
            _ => None,
        })
    };
    // The tick it arrives: fully faded out, so nothing is drawn.
    assert_eq!(text_alpha(&chat.layout(361, 0.0, false)), None);
    // Halfway through the intro: left of its place and still faint.
    let halfway = chat.layout(361, INTRO_TICKS / 2.0, false);
    assert!(slide(&halfway).is_some_and(|x| (-INTRO_WIDTH..0.0).contains(&x)));
    let faded = text_alpha(&halfway).expect("the line shows halfway through");
    // Settled: in place and as opaque as vanilla's line.
    let settled = chat.layout(361, INTRO_TICKS, false);
    assert_eq!(slide(&settled), None);
    assert!(text_alpha(&settled).expect("settled") > faded);
    // The extra off draws the line whole on the tick it arrives, as vanilla.
    chat.smooth = false;
    let plain = chat.layout(361, 0.0, false);
    assert_eq!(slide(&plain), None);
    assert_eq!(text_alpha(&plain), text_alpha(&settled));
    // The indicator bar is the -4 to -2 fill at the left of the line.
    let bar = |draws: &[GuiDraw]| {
        draws
            .iter()
            .any(|d| matches!(d, GuiDraw::Fill { x0: -4, x1: -2, .. }))
    };
    assert!(bar(&plain));
    chat.hide_indicator = true;
    assert!(!bar(&chat.layout(361, 0.0, false)));
}

#[test]
fn clicking_seed_copies_the_raw_value_only_inside_the_link() {
    let font = font();
    for seed in [1, -42, i64::MIN, i64::MAX] {
        let mut chat = Chat::default();
        let seed_text = Text::copyable(seed.to_string()).with_color(text::GOLD);
        assert_eq!(seed_text.plain(), format!("[{seed}]"));
        assert_eq!(seed_text.spans[0].style.color, Some(text::GOLD));
        assert_eq!(seed_text.spans[1].style.color, Some(text::GREEN));
        assert_eq!(seed_text.spans[2].style.color, Some(text::GOLD));
        add(&mut chat, &font, Text::literal("Seed: ").append(seed_text));
        let expected = Some(ClickEvent::CopyToClipboard(seed.to_string()));
        let start = 4.0 + f64::from(font.width("Seed: ["));
        let end = start + f64::from(font.width(&seed.to_string()));
        assert_eq!(chat.click_at(&font, 360, start, 315.0), expected);
        assert_eq!(chat.click_at(&font, 360, end - 0.5, 315.0), expected);
        for (x, y) in [
            (start - 0.5, 315.0),
            (end, 315.0),
            (3.0, 315.0),
            (start, 320.0),
            (start, 310.0),
        ] {
            assert_eq!(chat.click_at(&font, 360, x, y), None, "{x}, {y}");
        }
    }
}

#[test]
fn click_targets_match_rendered_spans_when_scaled_wrapped_and_scrolled() {
    let font = font();
    let screenshot = ClickEvent::OpenFile("/tmp/My screenshots/世界.png".into());
    for scale in [0.5, 0.75, 1.0] {
        for line_spacing in [0.0, 0.5, 1.0] {
            let mut chat = Chat::default();
            chat.options.scale = scale;
            chat.options.width = 0.0;
            chat.options.line_spacing = line_spacing;
            for _ in 0..20 {
                add(
                    &mut chat,
                    &font,
                    Text::literal("Saved screenshot as ").append(Text::styled(
                        "2026-10-14_18.55.14.png",
                        Style {
                            click: Some(screenshot.clone()),
                            bold: Some(true),
                            underlined: Some(true),
                            ..Style::default()
                        },
                    )),
                );
            }
            for scroll in [0, 3, 12] {
                chat.scroll = scroll;
                let mut links = 0;
                for draw in chat.layout(361, 0.0, true) {
                    if let GuiDraw::Text { text, x, y, .. } = draw {
                        let mut at = x as f64;
                        for (c, style) in text.chars() {
                            let width = f64::from(font.advance(c, style));
                            let cursor_x = (4.0 + at + width / 2.0) * scale;
                            let cursor_y = (y as f64 + 3.0) * scale;
                            assert_eq!(
                                chat.click_at(&font, 361, cursor_x, cursor_y),
                                style.click,
                                "scale={scale}, spacing={line_spacing}, scroll={scroll}, char={c}"
                            );
                            links += usize::from(style.click.is_some());
                            at += width;
                        }
                    }
                }
                assert!(links > 0);
                // The history above the focused viewport is not clickable.
                let top = ((361 - 40) as f64 / scale).floor()
                    - chat.lines_per_page(true) as f64 * chat.options.line_height() as f64;
                assert_eq!(
                    chat.click_at(&font, 361, 10.0 * scale, (top - 1.0) * scale),
                    None
                );
            }
        }
    }
}

#[test]
fn locate_coordinates_fill_chat_with_teleport_without_submitting() {
    let font = font();
    let m = Measure(&font);
    let registries = Registries::default();
    let commands = Commands {
        dispatcher: crate::command::vanilla::game_dispatcher(),
        registries: &registries,
    };
    // Structures and POIs use relative Y; biome results include absolute Y.
    for (y, expected) in [
        (None, "/tp @s -1136 ~ 848"),
        (Some(73), "/tp @s -1136 73 848"),
    ] {
        let link = Text::teleport_link(-1136, y, 848);
        assert_eq!(link.component()["click_event"]["action"], "suggest_command");
        assert_eq!(link.component()["click_event"]["command"], expected);
        let mut chat = Chat::default();
        add(&mut chat, &font, Text::literal("Found at ").append(link));
        let mut screen = ChatScreen::new(&m, &commands, "draft", false, 0, 640, 360, full());
        let x = 4.0 + f64::from(font.width("Found at "));
        assert!(!screen.click(&m, &commands, x, 315.0));
        let Some(ClickEvent::SuggestCommand(command)) = chat.click_at(&font, 360, x, 315.0) else {
            panic!("locate coordinates must suggest a command");
        };
        screen.suggest_command(&m, &commands, &command);
        assert_eq!(screen.value(), expected);
        assert_eq!(screen.input.cursor, expected.chars().count());
        assert_eq!(screen.input.highlight, screen.input.cursor);
        // Submission is a separate, explicit Enter press.
        assert_eq!(
            screen.key(
                &m,
                &commands,
                &[],
                winit::keyboard::KeyCode::Enter,
                false,
                false
            ),
            ChatAction::Submit(expected.to_owned())
        );
    }
}

#[test]
fn history_arrows_recall_messages_and_restore_the_draft() {
    use winit::keyboard::KeyCode::{ArrowDown, ArrowUp};

    let font = font();
    let m = Measure(&font);
    let registries = Registries::default();
    let commands = Commands {
        dispatcher: crate::command::vanilla::game_dispatcher(),
        registries: &registries,
    };
    let history = vec!["first message".to_owned(), "latest message".to_owned()];
    let mut screen = ChatScreen::new(
        &m,
        &commands,
        "unfinished draft",
        false,
        history.len(),
        640,
        360,
        full(),
    );
    for (key, expected) in [
        (ArrowUp, "latest message"),
        (ArrowUp, "first message"),
        (ArrowUp, "first message"),
        (ArrowDown, "latest message"),
        (ArrowDown, "unfinished draft"),
        (ArrowDown, "unfinished draft"),
    ] {
        assert_eq!(
            screen.key(&m, &commands, &history, key, false, false),
            ChatAction::None
        );
        assert_eq!(screen.value(), expected);
    }
}

/// `ChatScreen.isDraft`: the box opens on the draft kept from last time
/// (`ChatComponent.createScreen` passes `true`, which `ChatScreen.init`
/// stores), so the text draws grey italic through `ChatScreen.formatChat`
/// and the first Backspace clears the whole draft instead of one character.
/// Editing the draft (`onEdited`) ends both.
#[test]
fn a_restored_draft_is_grey_italic_and_one_backspace_clears_it() {
    use winit::keyboard::KeyCode::Backspace;

    let font = font();
    let m = Measure(&font);
    let registries = Registries::default();
    let c = Commands {
        dispatcher: crate::command::vanilla::game_dispatcher(),
        registries: &registries,
    };
    // The input's own text as the game draws it: x 4, y height - 12.
    let input_text = |screen: &mut ChatScreen| -> Text {
        screen
            .draw(&m, (0, 0), 0, i32::MIN)
            .into_iter()
            .find_map(|draw| match draw {
                Draw::Text {
                    text, x: 4, y: 348, ..
                } => Some(text),
                _ => None,
            })
            .expect("the input draws its text")
    };
    let draft_style = |t: &Text| {
        t.spans
            .iter()
            .all(|s| s.style.color == Some(text::GRAY) && s.style.is_italic())
    };
    let mut screen = ChatScreen::new(&m, &c, "half-written", true, 0, 640, 360, full());
    assert_eq!(screen.value(), "half-written");
    assert!(
        draft_style(&input_text(&mut screen)),
        "the restored draft draws grey italic"
    );
    // Typing into the draft (`onEdited`) ends the draft style, and Backspace
    // eats one character again.
    screen.type_text(&m, &c, "!");
    assert_eq!(screen.value(), "half-written!");
    let typed = input_text(&mut screen);
    assert!(
        !typed.spans.iter().any(|s| s.style.is_italic()),
        "an edited draft is not grey italic"
    );
    assert_eq!(
        screen.key(&m, &c, &[], Backspace, false, false),
        ChatAction::None
    );
    assert_eq!(screen.value(), "half-written");
    // A draft left alone clears whole on its first Backspace.
    let mut screen = ChatScreen::new(&m, &c, "half-written", true, 0, 640, 360, full());
    assert_eq!(
        screen.key(&m, &c, &[], Backspace, false, false),
        ChatAction::None
    );
    assert_eq!(screen.value(), "", "the first Backspace clears the draft");
    // The flag is gone with it: typing is ordinary text, and Backspace eats
    // one character.
    screen.type_text(&m, &c, "hi");
    assert_eq!(screen.value(), "hi");
    let typed = input_text(&mut screen);
    assert!(!typed.spans.iter().any(|s| s.style.is_italic()));
    assert_eq!(
        screen.key(&m, &c, &[], Backspace, false, false),
        ChatAction::None
    );
    assert_eq!(screen.value(), "h");
    // A box opened on ordinary text deletes one character at a time.
    let mut screen = ChatScreen::new(&m, &c, "half-written", false, 0, 640, 360, full());
    assert!(!draft_style(&input_text(&mut screen)));
    assert_eq!(
        screen.key(&m, &c, &[], Backspace, false, false),
        ChatAction::None
    );
    assert_eq!(screen.value(), "half-writte");
    // A command draft is grey italic as a whole: `formatChat` is registered
    // before `CommandSuggestions.formatText`, so it wins over the argument
    // colours.
    let mut screen = ChatScreen::new(&m, &c, "/give @s stone", true, 0, 640, 360, full());
    assert!(draft_style(&input_text(&mut screen)));
}

#[test]
fn mod_commands_are_coloured_and_complete_like_vanilla_ones() {
    let font = font();
    let m = Measure(&font);
    // Allow Commands off: a mod's command needs no level.
    let registries = Registries::default();
    let vanilla = crate::command::vanilla::game_dispatcher();
    let modded = vanilla.with_mod_commands(["where", "coords"]);
    let red = |screen: &ChatScreen| {
        screen
            .format(screen.value(), 0)
            .spans
            .iter()
            .any(|s| s.style.color == Some(text::RED))
    };
    let typed = |dispatcher: &crate::command::Dispatcher, line: &str| {
        let c = Commands {
            dispatcher,
            registries: &registries,
        };
        let mut screen = ChatScreen::new(&m, &c, "", false, 0, 640, 360, full());
        screen.type_text(&m, &c, line);
        screen
    };
    // Unknown without the mod's tree (#456).
    assert!(red(&typed(vanilla, "/coords")));
    for line in ["/coords", "/where", "/where me and you"] {
        assert!(!red(&typed(&modded, line)), "{line}");
    }
    let c = Commands {
        dispatcher: &modded,
        registries: &registries,
    };
    let mut screen = typed(&modded, "/coo");
    screen.key(&m, &c, &[], winit::keyboard::KeyCode::Tab, false, false);
    assert_eq!(screen.value(), "/coords");
}

#[test]
fn chat_completes_the_servers_custom_completions_with_the_players() {
    // CommandSuggestions.updateCommandInfo completes chat from
    // ClientSuggestionProvider.getCustomTabSuggestions: the online players
    // and what ClientboundCustomChatCompletionsPacket added.
    use crate::command::{CustomCompletionAction as Action, modify_custom_completions};
    let font = font();
    let m = Measure(&font);
    let mut registries = Registries {
        players: vec!["Steve".into()],
        ..Registries::default()
    };
    let entries = |e: &[&str]| e.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let set = &mut registries.custom_completions;
    modify_custom_completions(set, Action::from_id(0), &entries(&["Alex", "#trade"]));
    modify_custom_completions(set, Action::Add, &entries(&["Notch", "Steve"]));
    modify_custom_completions(set, Action::Remove, &entries(&["Alex"]));
    // An id past the last action is ADD (OutOfBoundsStrategy.ZERO).
    modify_custom_completions(set, Action::from_id(7), &entries(&["Zuri"]));
    assert_eq!(
        registries
            .custom_tab_suggestions()
            .into_iter()
            .collect::<Vec<_>>(),
        ["#trade", "Notch", "Steve", "Zuri"]
    );
    let completed = |registries: &Registries, line: &str| {
        let dispatcher = crate::command::vanilla::game_dispatcher();
        let c = Commands {
            dispatcher,
            registries,
        };
        let mut screen = ChatScreen::new(&m, &c, "", false, 0, 640, 360, full());
        screen.type_text(&m, &c, line);
        // Chat shows no list by itself: Tab shows it, Tab again uses it.
        for _ in 0..2 {
            screen.key(&m, &c, &[], winit::keyboard::KeyCode::Tab, false, false);
        }
        screen.value().to_owned()
    };
    assert_eq!(completed(&registries, "hi no"), "hi Notch");
    assert_eq!(completed(&registries, "sell #tr"), "sell #trade");
    assert_eq!(completed(&registries, "hi st"), "hi Steve");
    assert_eq!(completed(&registries, "hi al"), "hi al");
    // SET replaces them all; the players stay.
    let set = &mut registries.custom_completions;
    modify_custom_completions(set, Action::Set, &entries(&["Alex"]));
    assert_eq!(completed(&registries, "hi no"), "hi no");
    assert_eq!(completed(&registries, "hi al"), "hi Alex");
    assert_eq!(completed(&registries, "hi st"), "hi Steve");
}

#[test]
fn ask_server_nodes_wait_for_the_servers_completions() {
    // ClientSuggestionProvider.customSuggestion: an `ask_server` node at the
    // cursor sends the command so far, and the list is done with the reply.
    let font = font();
    let m = Measure(&font);
    let registries = Registries {
        permission: 4,
        ..Registries::default()
    };
    let mut d = crate::command::vanilla::game_dispatcher().clone();
    let child = |d: &crate::command::Dispatcher, n: usize, name: &str| {
        d.nodes[n]
            .children
            .iter()
            .copied()
            .find(|&c| d.nodes[c].name() == name)
            .expect(name)
    };
    let id = child(&d, child(&d, child(&d, 0, "bossbar"), "get"), "id");
    d.nodes[id].suggests = Some(crate::command::Suggests::AskServer);
    let c = Commands {
        dispatcher: &d,
        registries: &registries,
    };
    let tab = |screen: &mut ChatScreen| {
        screen.key(&m, &c, &[], winit::keyboard::KeyCode::Tab, false, false);
    };
    let mut screen = ChatScreen::new(&m, &c, "", false, 0, 640, 360, full());
    screen.type_text(&m, &c, "/bossbar get pt");
    let (request, sent) = screen.take_request().expect("the server is asked");
    assert_eq!(sent, "/bossbar get pt");
    assert_eq!(screen.take_request(), None);
    // Nothing to complete until the reply.
    tab(&mut screen);
    assert_eq!(screen.value(), "/bossbar get pt");
    // A reply to an older request is dropped; the range is in UTF-16 units.
    let reply = |text: &str| vec![(text.to_owned(), None)];
    screen.server_suggestions(&m, &c, request - 1, (13, 2), reply("pt6:old"));
    tab(&mut screen);
    assert_eq!(screen.value(), "/bossbar get pt");
    screen.server_suggestions(&m, &c, request, (13, 2), reply("pt6:check"));
    assert!(screen.take_suggestion_completion().is_none());
    tab(&mut screen);
    assert_eq!(screen.value(), "/bossbar get pt6:check");
    assert_eq!(
        screen.take_suggestion_completion(),
        Some(SuggestionCompletion {
            original: "/bossbar get pt".into(),
            range: (13, 15),
            text: "pt6:check".into(),
            input: "/bossbar get pt6:check".into(),
        })
    );
    // Without the server's provider the local list is done at once.
    let local = crate::command::vanilla::game_dispatcher();
    let c = Commands {
        dispatcher: local,
        registries: &registries,
    };
    let mut screen = ChatScreen::new(&m, &c, "", false, 0, 640, 360, full());
    screen.type_text(&m, &c, "/bossbar get pt");
    assert_eq!(screen.take_request(), None);
}

#[test]
fn legacy_codes_colour_the_line_or_vanish_without_chat_colours() {
    let mut chat = Chat::default();
    let red = Text::literal("\u{a7}cRED");
    let shown = chat.displayed(&red);
    assert_eq!(shown.plain(), "RED");
    assert!(shown.chars().all(|(_, s)| s.color == Some(0xFF5555)));
    chat.options.colors = false;
    let shown = chat.displayed(&red);
    assert_eq!(shown.plain(), "RED");
    assert!(shown.chars().all(|(_, s)| s.color.is_none()));
    // A lone section sign or an unknown code stays as written.
    assert_eq!(
        chat.displayed(&Text::literal("a\u{a7}zb")).plain(),
        "a\u{a7}zb"
    );
}

#[test]
fn a_server_component_keeps_its_click_and_hover_events() {
    let font = font();
    let json = serde_json::json!({
        "text": "CLICK", "color": "green", "bold": true,
        "click_event": {"action": "suggest_command", "command": "/say clicked"},
        "hover_event": {"action": "show_text", "value": {"text": "Tip", "color": "aqua"}},
    });
    let line = crate::component::to_text(&json, &mut |s| Text::literal(s));
    let mut chat = Chat::default();
    add(&mut chat, &font, line);
    let x = 4.0 + f64::from(font.width("CL"));
    let Some(ClickEvent::SuggestCommand(command)) = chat.click_at(&font, 360, x, 315.0) else {
        panic!("the line must suggest its command");
    };
    assert_eq!(command, "/say clicked");
    let hover = chat.hover_at(&font, 360, x, 315.0).expect("a hover text");
    assert_eq!(hover.plain(), "Tip");
    assert_eq!(hover.spans[0].style.color, Some(text::AQUA));
    // Past the (bold, so wider) text, nothing.
    let past = 4.0 + f64::from(font.width("CLICK")) + 20.0;
    assert!(chat.hover_at(&font, 360, past, 315.0).is_none());
}

/// `ChatVisiblity`: Commands Only keeps the chat out, Hidden takes the
/// commands too (`ChatAbilities`), and Enter sends nothing the option hides.
#[test]
fn the_chat_option_decides_what_may_be_sent() {
    let font = font();
    let m = Measure(&font);
    let registries = Registries::default();
    let c = Commands {
        dispatcher: crate::command::vanilla::game_dispatcher(),
        registries: &registries,
    };
    let enter = |screen: &mut ChatScreen| {
        screen.key(&m, &c, &[], winit::keyboard::KeyCode::Enter, false, false)
    };
    for (visibility, chat, command) in [
        (ChatVisibility::Full, true, true),
        (ChatVisibility::System, false, true),
        (ChatVisibility::Hidden, false, false),
    ] {
        let abilities = ChatAbilities::new(visibility);
        assert_eq!(abilities.allows("hi"), chat, "{visibility:?}");
        assert_eq!(abilities.allows("/time set day"), command, "{visibility:?}");
        for (line, allowed) in [("hi", chat), ("/time set day", command)] {
            let mut screen = ChatScreen::new(&m, &c, "", false, 0, 640, 360, abilities);
            screen.type_text(&m, &c, line);
            match enter(&mut screen) {
                ChatAction::Submit(sent) => {
                    assert!(allowed, "{visibility:?} sent {sent}");
                    assert_eq!(sent, line);
                }
                ChatAction::None => assert!(!allowed, "{visibility:?} kept {line}"),
                ChatAction::Close => panic!("Enter does not close the chat"),
            }
        }
    }
}

/// The restriction the option sets is a usage line: red, under the input, for
/// a command the Chat option takes and for a message it takes.
#[test]
fn restricted_input_says_why_it_will_not_send() {
    let font = font();
    let m = Measure(&font);
    let registries = Registries::default();
    let c = Commands {
        dispatcher: crate::command::vanilla::game_dispatcher(),
        registries: &registries,
    };
    let usage = |visibility, line: &str| {
        let mut screen = ChatScreen::new(
            &m,
            &c,
            "",
            false,
            0,
            640,
            360,
            ChatAbilities::new(visibility),
        );
        screen.type_text(&m, &c, line);
        screen.usage.iter().map(|t| t.plain()).collect::<Vec<_>>()
    };
    assert_eq!(usage(ChatVisibility::Full, "hi"), Vec::<String>::new());
    // The font has no language files here, so the line is its key.
    assert_eq!(
        usage(ChatVisibility::System, "hi"),
        ["chat_screen.messages_not_allowed"]
    );
    // A command the option allows says nothing about the restriction.
    assert!(
        !usage(ChatVisibility::System, "/time set day")
            .iter()
            .any(|line| line == "chat_screen.commands_not_allowed")
    );
    assert!(
        usage(ChatVisibility::Hidden, "/time set day")
            .iter()
            .any(|line| line == "chat_screen.commands_not_allowed")
    );
}

/// `ChatComponent.setVisibleMessageFilter`: a message the Chat option hides
/// on arrival is dropped; one already in the chat leaves it while the option
/// hides its kind and comes back when it shows it again.
#[test]
fn the_chat_option_filters_the_lines_both_ways() {
    let font = font();
    let mut chat = Chat::default();
    let says = |chat: &mut Chat, text: &str, source| {
        chat.add_message(&font, Text::literal(text), None, source, 0, false);
    };
    let shown =
        |chat: &Chat| -> Vec<String> { chat.lines.iter().map(|l| l.content.plain()).collect() };
    says(&mut chat, "player", ChatSource::Player);
    says(&mut chat, "server", ChatSource::Server);
    chat.set_abilities(&font, ChatAbilities::new(ChatVisibility::System));
    assert_eq!(shown(&chat), ["server"]);
    says(&mut chat, "dropped", ChatSource::Player);
    chat.set_abilities(&font, ChatAbilities::new(ChatVisibility::Hidden));
    says(&mut chat, "client", ChatSource::Client);
    assert_eq!(shown(&chat), ["client"]);
    chat.set_abilities(&font, ChatAbilities::new(ChatVisibility::Full));
    assert_eq!(shown(&chat), ["client", "server", "player"]);
}

/// A server's own chat: a guest's line is a player message, so the
/// Chat option's player-message permission hides it at Commands Only and
/// Hidden; command feedback (`/list`, `/tp`) is a system message and stays
/// at Commands Only.
#[test]
fn a_guests_line_follows_the_player_message_permission() {
    let font = font();
    let shown = |visibility, source| {
        let mut chat = Chat::default();
        chat.set_abilities(&font, ChatAbilities::new(visibility));
        chat.add_message(&font, Text::literal("line"), None, source, 0, false);
        chat.messages.len()
    };
    for (visibility, player, system) in [
        (ChatVisibility::Full, 1, 1),
        (ChatVisibility::System, 0, 1),
        (ChatVisibility::Hidden, 0, 0),
    ] {
        assert_eq!(
            shown(visibility, ChatSource::Player),
            player,
            "{visibility:?}"
        );
        assert_eq!(
            shown(visibility, ChatSource::Server),
            system,
            "{visibility:?}"
        );
    }
}

/// `ChatComponent.deleteMessageOrDelay`: a deleted message shows for 60
/// ticks from when it was added, then becomes the grey italic
/// `chat.deleted_marker` line with the system bar; the other lines stay.
#[test]
fn a_deleted_message_shows_60_ticks_then_becomes_the_marker() {
    let font = font();
    let mut chat = Chat::default();
    let secret = [7; 256];
    chat.add_signed_message(
        &font,
        Text::literal("<Steve> secret"),
        Some(secret),
        100,
        false,
    );
    chat.add_signed_message(
        &font,
        Text::literal("<Alex> hi"),
        Some([8; 256]),
        110,
        false,
    );
    let lines =
        |chat: &Chat| -> Vec<String> { chat.lines.iter().map(|l| l.content.plain()).collect() };
    let marker = Chat::deleted_marker().plain();
    chat.delete_message_with(&font, &secret, 130);
    assert_eq!(
        lines(&chat),
        ["<Alex> hi", "<Steve> secret"],
        "too new: delayed"
    );
    chat.process_deletions(&font, 159);
    assert_eq!(lines(&chat), ["<Alex> hi", "<Steve> secret"]);
    chat.process_deletions(&font, 160);
    assert_eq!(lines(&chat), ["<Alex> hi".to_owned(), marker.clone()]);
    assert!(chat.deletions.is_empty());
    let line = &chat.lines[1];
    assert_eq!(line.tag.as_ref().map(|t| t.color), Some(SYSTEM_INDICATOR));
    assert_eq!(line.added, 100, "the marker keeps the message's time");
    assert!(
        line.content
            .spans
            .iter()
            .all(|s| s.style.color == Some(text::GRAY) && s.style.is_italic())
    );
    assert_eq!(
        chat.messages[1].content.translation_key(),
        Some("chat.deleted_marker")
    );
    // A message shown 60 ticks already is replaced at once; an unknown or
    // already deleted signature changes nothing.
    chat.delete_message_with(&font, &[8; 256], 170);
    assert_eq!(lines(&chat), [marker.clone(), marker.clone()]);
    chat.delete_message_with(&font, &secret, 200);
    chat.delete_message_with(&font, &[9; 256], 200);
    assert_eq!(lines(&chat), [marker.clone(), marker]);
    assert!(chat.deletions.is_empty());
}

/// `SuggestionsList.select` says an entry once (`lastNarratedEntry`), as
/// "suggestion N out of M", with its tooltip when it has one.
#[test]
fn a_suggestion_is_narrated_by_its_place_once() {
    let suggestion = |text: &str, tooltip: Option<&str>| Suggestion {
        range: (1, 1),
        text: text.into(),
        tooltip: tooltip.map(str::to_owned),
        int_value: None,
    };
    let mut l = SuggestionList {
        rect: [0; 4],
        original: "/".into(),
        list: vec![
            suggestion("give", None),
            suggestion("gamemode", Some("Mode")),
        ],
        offset: 0,
        current: 1,
        last_mouse: (0, 0),
        tab_cycles: false,
        last_narrated: 0,
    };
    let said = l.narration_message().component();
    assert_eq!(l.last_narrated, 1);
    assert_eq!(said["translate"], "narration.suggestion.tooltip");
    assert_eq!(said["with"].as_array().map(Vec::len), Some(4));
    l.current = 0;
    let said = l.narration_message().component();
    assert_eq!(said["translate"], "narration.suggestion");
    assert_eq!(said["with"].as_array().map(Vec::len), Some(3));
}

#[test]
fn a_chat_message_is_trimmed_and_its_whitespace_runs_collapsed() {
    // ChatScreen.normalizeChatMessage: String.trim, then StringUtils
    // .normalizeSpace, so a command gets the arguments the player meant.
    assert_eq!(normalize_chat_message("  hello   there  "), "hello there");
    assert_eq!(normalize_chat_message("/msg  Alex\thi"), "/msg Alex hi");
    assert_eq!(normalize_chat_message("a\n\nb\r\nc"), "a b c");
    assert_eq!(normalize_chat_message("\t \n"), "");
    assert_eq!(normalize_chat_message(""), "");
    // Character.isWhitespace is not str::trim's Unicode whitespace: U+00A0 is
    // no Java whitespace, so it is neither trimmed nor collapsed, while the
    // ideographic space U+3000 and Java's U+001C separators are.
    assert_eq!(normalize_chat_message(" \u{a0} hi"), "\u{a0} hi");
    assert_eq!(normalize_chat_message("a\u{a0}\u{a0}b"), "a\u{a0}\u{a0}b");
    assert_eq!(normalize_chat_message("a\u{3000}\u{3000}b"), "a b");
    assert_eq!(normalize_chat_message("a\u{1c}b"), "a b");
}

#[test]
fn a_chat_message_is_cut_to_the_256_utf16_units_the_packet_holds() {
    // StringUtil.trimChatMessage counts String.length(), UTF-16 code units:
    // 256 ASCII characters are all kept, 257 are cut to 256.
    assert_eq!(normalize_chat_message(&"a".repeat(256)).len(), 256);
    assert_eq!(normalize_chat_message(&"a".repeat(300)).len(), 256);
    // An emoji is two units, so 128 of them fill the packet and the 129th is
    // dropped: chars().take(256) would keep all 129 and the server would
    // reject the message.
    let emoji = normalize_chat_message(&"\u{1f600}".repeat(129));
    assert_eq!(emoji, "\u{1f600}".repeat(128));
    assert_eq!(emoji.encode_utf16().count(), 256);
    // A cut between the halves of a pair keeps the character below it.
    let mixed = format!("{}\u{1f600}", "b".repeat(255));
    assert_eq!(normalize_chat_message(&mixed), "b".repeat(255));
}

/// PvP & HUD's chat options: the time before each message, and a repeated message counted on
/// its own lines instead of filling the chat.
#[test]
fn timestamps_and_compact_chat_change_the_lines_not_the_messages() {
    let font = font();
    let mut chat = Chat {
        clock: Some((9, 5)),
        ..Chat::default()
    };
    chat.qol.compact = true;
    let says = |chat: &mut Chat, text: &str| {
        chat.add_message(
            &font,
            Text::literal(text),
            None,
            ChatSource::Server,
            0,
            false,
        );
    };
    let shown =
        |chat: &Chat| -> Vec<String> { chat.lines.iter().map(|l| l.content.plain()).collect() };
    says(&mut chat, "gg");
    says(&mut chat, "gg");
    says(&mut chat, "gg");
    assert_eq!(shown(&chat), ["gg (x3)"]);
    says(&mut chat, "wp");
    says(&mut chat, "gg");
    assert_eq!(shown(&chat), ["gg", "wp", "gg (x3)"]);
    assert_eq!(chat.received, 5, "every message still counts as received");
    chat.qol.timestamps = true;
    chat.rewrap(&font);
    assert_eq!(
        shown(&chat),
        ["[09:05] gg", "[09:05] wp", "[09:05] gg (x3)"]
    );
    // Copying takes the message as it came: no time, no count.
    assert_eq!(&*chat.lines[2].message, "gg");
    // Turned off, the history shows each repeat as it was counted.
    chat.qol = ChatQol::default();
    chat.rewrap(&font);
    assert_eq!(shown(&chat), ["gg", "wp", "gg (x3)"]);
}

/// PvP & HUD's chat search: only the messages holding the query show, in any case; clearing
/// it brings every line back.
#[test]
fn the_chat_search_keeps_the_matching_messages() {
    let font = font();
    let mut chat = Chat::default();
    for text in ["Alpha one", "beta", "ALPHABET soup"] {
        chat.add_message(
            &font,
            Text::literal(text),
            None,
            ChatSource::Server,
            0,
            false,
        );
    }
    let shown =
        |chat: &Chat| -> Vec<String> { chat.lines.iter().map(|l| l.content.plain()).collect() };
    chat.set_search_with(&font, Some("alpha"));
    assert!(chat.searching());
    assert_eq!(shown(&chat), ["ALPHABET soup", "Alpha one"]);
    // A new message that doesn't match stays out of sight until the search ends.
    chat.add_message(
        &font,
        Text::literal("gamma"),
        None,
        ChatSource::Server,
        0,
        false,
    );
    assert_eq!(shown(&chat).len(), 2);
    chat.set_search_with(&font, Some("  "));
    assert!(!chat.searching());
    assert_eq!(
        shown(&chat),
        ["gamma", "ALPHABET soup", "beta", "Alpha one"]
    );
}

/// Search and copy read a message as the chat shows it: legacy `§` codes format it, they
/// aren't text.
#[test]
fn the_chat_search_and_copy_skip_legacy_codes() {
    let font = font();
    let mut chat = Chat::default();
    for text in ["He§cllo", "§aplain"] {
        chat.add_message(
            &font,
            Text::literal(text),
            None,
            ChatSource::Server,
            0,
            false,
        );
    }
    assert_eq!(&*chat.lines[1].message, "Hello");
    chat.set_search_with(&font, Some("hello"));
    assert_eq!(chat.lines.len(), 1);
    // `c` and `a` are codes here, not letters of either message.
    chat.set_search_with(&font, Some("c"));
    assert_eq!(chat.lines.len(), 0);
}

/// A repeat merged by compact chat leaves a reader scrolled up where they were.
#[test]
fn a_compact_repeat_keeps_the_scrolled_view_still() {
    let font = font();
    let mut chat = Chat::default();
    chat.qol.compact = true;
    let says = |chat: &mut Chat, text: &str| {
        chat.add_message(
            &font,
            Text::literal(text),
            None,
            ChatSource::Server,
            0,
            true,
        );
    };
    for i in 0..60 {
        says(&mut chat, &format!("line {i}"));
    }
    says(&mut chat, "gg");
    chat.scroll_by(5, true);
    assert_eq!(chat.scroll, 5);
    says(&mut chat, "gg");
    assert_eq!(chat.lines[0].content.plain(), "gg (x2)");
    assert_eq!(chat.scroll, 5);
    // A new message still pushes the view up by its line.
    says(&mut chat, "new");
    assert_eq!(chat.scroll, 6);
}

/// `FilterMask.applyWithFormatting`: a pass-through mask keeps the message, a
/// fully filtered one hides it, and a partial one turns each masked UTF-16 unit
/// into a dark grey `#` whose hover reads `chat.filtered`.
#[test]
fn a_server_filter_mask_hides_or_masks_the_message() {
    let text = |v: &serde_json::Value| crate::component::to_text(v, &mut |s| Text::literal(s));
    let shown = |content: &str, mask: (i32, Vec<i64>)| {
        filtered_message(content, &mask).map(|v| text(&v).plain())
    };
    assert_eq!(shown("hello", (0, vec![])), Some("hello".to_owned()));
    assert_eq!(shown("hello", (1, vec![])), None);
    assert_eq!(shown("hello", (2, vec![0b0110])), Some("h##lo".to_owned()));
    assert_eq!(
        shown("hello", (2, vec![0b1_0001])),
        Some("#ell#".to_owned())
    );
    // An emoji is two UTF-16 units, so two bits and two `#`.
    assert_eq!(
        shown("a\u{1F600}b", (2, vec![0b0110])),
        Some("a##b".to_owned())
    );
    // Bit 65 is in the second word.
    let long = "x".repeat(70);
    let mut expected = long.clone();
    expected.replace_range(65..66, "#");
    assert_eq!(shown(&long, (2, vec![0, 0b10])), Some(expected));
    let masked = text(&filtered_message("hello", &(2, vec![0b0110])).unwrap());
    let hash = masked.spans.iter().find(|s| s.text == "##").unwrap();
    assert_eq!(hash.style.color, Some(text::DARK_GRAY));
    let hover = hash.style.hover.as_ref().unwrap();
    assert_eq!(hover.spans[0].text, "chat.filtered");
    assert!(
        masked
            .spans
            .iter()
            .filter(|s| s.text != "##")
            .all(|s| s.style.color.is_none())
    );
}

/// `SuggestionsList.extractRenderState`: the list's rectangle asks for the
/// pointing hand; outside it the cursor stays the arrow.
#[test]
fn the_suggestion_list_asks_for_the_pointing_hand() {
    let font = font();
    let m = Measure(&font);
    let registries = Registries::default();
    let c = Commands {
        dispatcher: crate::command::vanilla::game_dispatcher(),
        registries: &registries,
    };
    let mut screen = ChatScreen::new(&m, &c, "", false, 0, 640, 360, full());
    screen.type_text(&m, &c, "/");
    let rect = screen
        .suggestions
        .as_ref()
        .expect("the root commands are a suggestion list")
        .rect;
    let _ = crate::widgets::take_cursor();
    screen.draw(&m, (rect[0] + 2, rect[1] + 2), 0, 0);
    assert_eq!(
        crate::widgets::take_cursor(),
        crate::widgets::Cursor::PointingHand
    );
    let _ = crate::widgets::take_cursor();
    screen.draw(&m, (rect[0] - 1, rect[1] + 2), 0, 0);
    assert_eq!(crate::widgets::take_cursor(), crate::widgets::Cursor::Arrow);
}
