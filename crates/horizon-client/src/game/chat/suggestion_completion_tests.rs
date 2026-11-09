use super::*;
use crate::{
    game::{Audio, Options, Progress, Settings, load_resources},
    mod_api::{self, ChatSuggestionAppliedEvent, Hooks, Host},
};
use std::sync::{Arc, Mutex};

/// Exercise the actual Game keyboard/mouse handlers and generated mod dispatch.
#[test]
#[ignore = "needs prepared resources and a fresh HORIZON_HOME; run in isolation"]
fn chat_completion_reaches_mods_from_keyboard_and_mouse() {
    let root = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let scratch = PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap()).join("suggestion-game");
    std::fs::create_dir_all(&scratch).unwrap();
    struct RestoreDir(PathBuf);
    impl Drop for RestoreDir {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }
    let _restore = RestoreDir(std::env::current_dir().unwrap());
    std::env::set_current_dir(&scratch).unwrap();
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&scratch),
        None,
    )
    .unwrap();

    struct Listener(Arc<Mutex<Vec<String>>>);
    impl Hooks for Listener {
        fn chat_suggestion_applied(
            &self,
            _host: &dyn Host,
            e: &mut ChatSuggestionAppliedEvent<'_>,
        ) {
            assert_eq!(e.original, "😀 hi st");
            assert_eq!((e.start, e.end), (6, 8));
            assert_eq!(e.input, format!("😀 hi {}", e.suggestion));
            self.0.lock().unwrap().push(e.input.to_owned());
        }
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Listener(calls.clone()))];
    assert!(
        mod_api::MODS.set(mods.into()).is_ok(),
        "run this test alone"
    );
    game.screen = Screen::Chat;
    game.ui.registries.players = vec!["Steve".into(), "Steven".into()];
    game.ui.chat_screen = Some(ChatScreen::new(
        &Measure(&game.gui.atlas.font),
        &Commands {
            dispatcher: chat_tree(&game.remote),
            registries: &game.ui.registries,
        },
        "",
        false,
        0,
        640,
        360,
        ChatAbilities::new(ChatVisibility::Full),
    ));
    game.chat_type("😀 hi st");
    game.chat_key_code(KeyCode::Tab, None);
    assert!(calls.lock().unwrap().is_empty(), "only opened the list");
    game.chat_key_code(KeyCode::Tab, None);
    game.chat_key_code(KeyCode::Tab, None);
    assert_eq!(*calls.lock().unwrap(), ["😀 hi Steve", "😀 hi Steven"]);

    // Click the suggestion's rendered text rather than assuming its coordinates.
    let draws =
        game.ui
            .chat_screen
            .as_mut()
            .unwrap()
            .draw(&Measure(&game.gui.atlas.font), (-1, -1), 0, 0);
    let at = draws
        .into_iter()
        .find_map(|d| match d {
            horizon_client::screen_draw::Draw::Text { text, x, y, .. }
                if text.plain() == "Steve" =>
            {
                Some(((x + 1) as f32, (y + 1) as f32))
            }
            _ => None,
        })
        .expect("Steve is a visible suggestion");
    game.cursor = at;
    game.chat_click();
    assert_eq!(
        *calls.lock().unwrap(),
        ["😀 hi Steve", "😀 hi Steven", "😀 hi Steve"]
    );
    assert!(
        game.ui
            .chat_screen
            .as_mut()
            .unwrap()
            .take_suggestion_completion()
            .is_none()
    );
    game.chat_key_code(KeyCode::ArrowDown, None);
    assert_eq!(calls.lock().unwrap().len(), 3, "navigation does not apply");
}
