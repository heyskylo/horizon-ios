//! Run alone with private prepared resources: installs process-wide trait mods.
use super::super::*;
use crate::mod_api::{self, Hooks, Host, ToastAddEvent};
use horizon_client::text::Text;
use std::sync::Mutex;

/// Hears every toast and cancels the mod toasts whose id ends in `/cancelled`.
#[derive(Default)]
struct Probe {
    heard: Mutex<Vec<(String, String)>>,
}
impl Hooks for Arc<Probe> {
    fn toast_add(&self, _: &mut dyn Host, event: &mut ToastAddEvent<'_>) -> mod_api::Flow {
        self.heard
            .lock()
            .unwrap()
            .push((event.kind.to_owned(), event.id.to_owned()));
        if event.id.ends_with("/cancelled") {
            mod_api::Flow::Cancel
        } else {
            mod_api::Flow::Continue
        }
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_mod_toast_queues_unless_a_mod_cancels_it() {
    crate::mod_api::in_own_process(
        "game::advancements::mod_toast_tests::a_mod_toast_queues_unless_a_mod_cancels_it",
        a_mod_toast_queues_unless_a_mod_cancels_it_in_own_process,
    );
}

fn a_mod_toast_queues_unless_a_mod_cancels_it_in_own_process() {
    let root = PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = execution_role::ExecutionRole::Client;
    let probe = Arc::new(Probe::default());
    assert!(
        mod_api::MODS
            .set(Arc::from(vec![Box::new(probe.clone()) as Box<dyn Hooks>]))
            .is_ok()
    );
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    let before = game.adv.toasts.len();

    assert!(game.native_add_toast(
        "test:quest/first",
        "minecraft:diamond",
        Text::literal("Quest Complete!"),
        Text::literal("Shiny"),
        false,
    ));
    assert_eq!(game.adv.toasts.len(), before + 1);
    let toast = game.adv.toasts.back().unwrap();
    assert!(!toast.is_recipe());

    // Cancelled by a mod: not queued.
    assert!(!game.native_add_toast(
        "test:quest/cancelled",
        "minecraft:diamond",
        Text::literal("Quest Complete!"),
        Text::literal("Never seen"),
        true,
    ));
    assert_eq!(game.adv.toasts.len(), before + 1);
    assert_eq!(
        *probe.heard.lock().unwrap(),
        [
            ("mod".to_owned(), "test:quest/first".to_owned()),
            ("mod".to_owned(), "test:quest/cancelled".to_owned()),
        ]
    );

    // A server's game has no toasts to show.
    game.options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    assert!(!game.native_add_toast(
        "test:quest/server",
        "minecraft:diamond",
        Text::literal("Quest Complete!"),
        Text::literal("Nobody looks"),
        false,
    ));
    assert_eq!(game.adv.toasts.len(), before + 1);
    assert_eq!(probe.heard.lock().unwrap().len(), 2);
}

#[test]
fn a_mod_toast_is_an_advancement_toast_with_the_mods_label() {
    let toast = super::Toast::of_mod(
        "minecraft:diamond",
        Text::literal("Quest Complete!"),
        Text::literal("Shiny"),
        true,
    );
    assert!(!toast.is_recipe());
    assert_eq!(toast.frame, super::Frame::Challenge);
    assert_eq!(
        toast.label.as_ref().map(Text::plain),
        Some("Quest Complete!".into())
    );
    assert_eq!(toast.title.plain(), "Shiny");
    assert!(toast.slot.is_none());
    let plain = super::Toast::of_mod(
        "minecraft:diamond",
        Text::literal("a"),
        Text::literal("b"),
        false,
    );
    assert_eq!(plain.frame, super::Frame::Task);
}
