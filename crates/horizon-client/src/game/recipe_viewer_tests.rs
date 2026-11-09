//! Run alone with private prepared resources: installs process-wide trait mods.
use super::super::*;
use crate::mod_api::{
    self, Hooks, Host, NativeRecipeCategory, NativeRecipeLookup, NativeStack,
    RecipeCategoriesEvent, RecipeLookupEvent,
};
use std::sync::Mutex;

/// A recipe viewer that takes every lookup but `Uses`, then a second that
/// hears what the first left; and a mod with one category.
#[derive(Default)]
struct Viewer {
    heard: Mutex<Vec<(String, NativeRecipeLookup)>>,
    takes_uses: bool,
}
impl Hooks for Arc<Viewer> {
    fn recipe_lookup(&self, _: &mut dyn Host, event: &mut RecipeLookupEvent<'_>) -> mod_api::Flow {
        self.heard
            .lock()
            .unwrap()
            .push((event.stack.item.clone(), event.lookup.clone()));
        if self.takes_uses || *event.lookup != NativeRecipeLookup::Uses {
            mod_api::Flow::Cancel
        } else {
            mod_api::Flow::Continue
        }
    }
    fn recipe_categories(&self, _: &mut dyn Host, event: &mut RecipeCategoriesEvent<'_>) {
        if !self.takes_uses {
            event.categories.push(NativeRecipeCategory {
                id: "test:sifting".into(),
                title: "Sifting".into(),
                ..Default::default()
            });
        }
    }
}

fn stack(item: &str) -> NativeStack {
    NativeStack {
        item: item.into(),
        count: 1,
        components: String::new(),
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and an isolated mod registry"]
fn lookups_wait_for_the_next_frame_and_stop_at_the_viewer_that_takes_them() {
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
    let first = Arc::new(Viewer::default());
    let second = Arc::new(Viewer {
        takes_uses: true,
        ..Default::default()
    });
    assert!(
        mod_api::MODS
            .set(Arc::from(vec![
                Box::new(first.clone()) as Box<dyn Hooks>,
                Box::new(second.clone()) as Box<dyn Hooks>,
            ]))
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

    // Asked from a shared borrow, as a screen hook asks: nobody hears yet.
    let shared: &Game<'_> = &game;
    shared.native_show_recipes(stack("minecraft:flint"), NativeRecipeLookup::Recipes);
    shared.native_show_recipes(stack("minecraft:gravel"), NativeRecipeLookup::Uses);
    shared.native_show_recipes(
        stack("minecraft:brush"),
        NativeRecipeLookup::Key("key.keyboard.a".into()),
    );
    assert!(first.heard.lock().unwrap().is_empty());

    game.recipe_lookups_now();
    assert_eq!(
        *first.heard.lock().unwrap(),
        [
            ("minecraft:flint".into(), NativeRecipeLookup::Recipes),
            ("minecraft:gravel".into(), NativeRecipeLookup::Uses),
            (
                "minecraft:brush".into(),
                NativeRecipeLookup::Key("key.keyboard.a".into())
            ),
        ]
    );
    // The second viewer hears only the one the first left.
    assert_eq!(
        *second.heard.lock().unwrap(),
        [("minecraft:gravel".into(), NativeRecipeLookup::Uses)]
    );
    // Heard once.
    game.recipe_lookups_now();
    assert_eq!(first.heard.lock().unwrap().len(), 3);

    let version = game.native_recipe_categories_version();
    let categories = game.native_collect_recipe_categories();
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].id, "test:sifting");
    (&game as &Game<'_>).native_recipe_categories_changed();
    assert_ne!(game.native_recipe_categories_version(), version);
}
