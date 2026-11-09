//! The chat screen's input for tools/test_chat_screen_parity.py: reads one
//! JSON case per line (GUI size, history, options, typed text, keys,
//! clicks, mouse), runs `horizon_client::chat::ChatScreen` on the vanilla
//! command tree and writes the input's state and the screen's GUI calls in
//! the oracle's JSON form.
//!
//! usage: chat_screen_fixture RESOURCES REGISTRIES CASES OUTPUT
use crate::pack_screen_fixture;
use horizon_client::{
    chat::{ChatAbilities, ChatAction, ChatScreen, ChatVisibility, Commands},
    command::vanilla,
    hud::HudAtlas,
    lang,
    screen_draw::Measure,
};
use serde_json::{Value, json};
use std::{fmt::Write as _, fs, path::PathBuf};
use winit::keyboard::KeyCode as K;
#[path = "chat_screen_fixture/registries.rs"]
mod registries;

pub fn main() -> horizon_client::Result<()> {
    let args: Vec<PathBuf> = crate::args_os().skip(1).map(PathBuf::from).collect();
    let [root, registries, inputs, output] = args.as_slice() else {
        return Err("usage: chat_screen_fixture RESOURCES REGISTRIES CASES OUTPUT".into());
    };
    let atlas = HudAtlas::build(root)?;
    lang::set(lang::Language::load(&[root], lang::DEFAULT));
    let m = Measure(&atlas.font);
    let reg = registries::registries(&serde_json::from_slice(&fs::read(registries)?)?, 4);
    let c = Commands {
        dispatcher: vanilla::release_dispatcher(),
        registries: &reg,
    };
    let mut out = String::new();
    for line in fs::read_to_string(inputs)?
        .lines()
        .filter(|l| !l.is_empty())
    {
        let case: Value = serde_json::from_str(line)?;
        let int = |v: &Value| v.as_i64().unwrap_or(0) as i32;
        let history: Vec<String> = case["history"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect();
        let (w, h) = (int(&case["width"]), int(&case["height"]));
        let mut screen = ChatScreen::new(
            &m,
            &c,
            case["initial"].as_str().unwrap_or(""),
            false,
            history.len(),
            w,
            h,
            ChatAbilities::new(ChatVisibility::Full),
        );
        screen.auto_suggestions = case["autoSuggest"] == true;
        let mut closed = false;
        for a in case["actions"].as_array().into_iter().flatten() {
            match a["type"].as_str().unwrap_or_default() {
                "type" => screen.type_text(&m, &c, a["text"].as_str().unwrap_or_default()),
                "click" => {
                    screen.click(
                        &m,
                        &c,
                        a["x"].as_f64().unwrap_or(0.0),
                        a["y"].as_f64().unwrap_or(0.0),
                    );
                }
                "key" => {
                    let mut name = a["key"].as_str().unwrap_or_default();
                    let (mut shift, mut ctrl) = (false, false);
                    loop {
                        if let Some(n) = name.strip_prefix("shift+") {
                            shift = true;
                            name = n;
                        } else if let Some(n) = name.strip_prefix("ctrl+") {
                            ctrl = true;
                            name = n;
                        } else {
                            break;
                        }
                    }
                    let key = match name {
                        "backspace" => K::Backspace,
                        "delete" => K::Delete,
                        "home" => K::Home,
                        "end" => K::End,
                        "right" => K::ArrowRight,
                        "left" => K::ArrowLeft,
                        "down" => K::ArrowDown,
                        "up" => K::ArrowUp,
                        "tab" => K::Tab,
                        "escape" => K::Escape,
                        _ => K::KeyA,
                    };
                    // InputQuirks.EDIT_SHORTCUT_KEY_MODIFIER: Cmd, not Ctrl, on macOS.
                    let ctrl = ctrl && !cfg!(target_os = "macos");
                    if screen.key(&m, &c, &history, key, shift, ctrl) != ChatAction::None {
                        closed = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        if closed {
            writeln!(out, "{}", json!({"closed": true}))?;
            continue;
        }
        // Options.getBackgroundColor(Integer.MIN_VALUE).
        let background = if case["chatOnly"] == true {
            i32::MIN
        } else {
            let a = (case["opacity"].as_f64().unwrap_or(0.0) as f32 * 255.0).floor() as i32;
            a << 24
        };
        let mouse = (int(&case["mouse"][0]), int(&case["mouse"][1]));
        let draws = screen.draw(&m, mouse, case["blink"].as_u64().unwrap_or(0), background);
        let input = &screen.input;
        writeln!(
            out,
            "{}",
            json!({"value": input.value, "cursor": input.cursor, "highlight": input.highlight, "display": input.display,
                   "draws": pack_screen_fixture::encode(&draws)})
        )?;
    }
    fs::write(output, out)?;
    Ok(())
}
