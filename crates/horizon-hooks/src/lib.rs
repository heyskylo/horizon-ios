//! The game's extension points for code above the engine (docs/layers.md, "How a Horizon crate
//! plugs in"). A platform or Horizon crate implements [`Hooks`] and [`register`]s it before the
//! game starts; the engine calls every registered hook at the points below. This crate names no
//! type from above, so the engine can offer it; `horizon-platform` re-exports it as
//! `horizon_platform::hooks`, which is where the Horizon layer uses it from.
//!
//! Registration happens once, before the game runs: the list is sealed the first time the engine
//! dispatches, and a hook registered after that is refused. With nothing registered a dispatch
//! is one load and an empty loop.
//!
//! A new hook is a new method with an empty default body, so existing implementations keep
//! compiling, and a dispatch function in [`engine`] that the engine calls at the place vanilla
//! does the matching thing.
//!
//! The platform registries (docs/layers.md, "Platform registries") have the same shape:
//! [`settings`] for the settings each crate keeps in its own lines, [`pages`] for its settings
//! pages, [`hud`] for what it draws on the HUD.
#![forbid(unsafe_code)]

pub mod hud;
pub mod pages;
pub mod players;
mod registry;
pub mod settings;
pub mod tunnel;

use registry::Registry;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// What a crate above the engine hears from the game. Every method has an empty default:
/// implement only those you need. Hooks run on the game's thread (the render thread) unless a
/// method says otherwise; they must return quickly and must not panic.
pub trait Hooks: Sync {
    /// The game started for someone playing it: once, after the log and the crash handler, before
    /// the first frame. Not for a dedicated server, a benchmark, an offscreen run or a run that
    /// quits by itself or that horizon-ctl drives (those start in a source checkout).
    fn started(&self, _game: &Started<'_>) {}

    /// The world this window plays was opened to LAN or to friends (`true`), or no longer is
    /// (`false`). Called when it changes, not every frame.
    fn published(&self, _open: bool) {}

    /// The game is ending normally: its window closed and the world is saved. Not after a crash.
    fn shutdown(&self) {}

    /// Once a frame, before drawing: `sprites` takes the pictures the crate's
    /// [`players::Sprites`] lacked. Only a window's game calls it (each splitscreen player in
    /// turn, [`seat`]), never the integrated server's thread or a dedicated server; so are the
    /// player methods below (not [`Hooks::tunnel`]).
    fn frame(&self, _frame: &players::Frame, _sprites: &mut dyn players::SpriteStore) {}

    /// Once a client tick while a world is shown: the player models in view.
    fn tick(&self, _models: &mut dyn players::PlayerModels) {}

    /// What goes by a player model's name over its head, each frame its name tag is drawn. The
    /// first hook that answers is used.
    fn name_decoration(
        &self,
        _model: &players::PlayerModel,
        _sprites: &dyn players::Sprites,
    ) -> Option<players::NameDecoration> {
        None
    }

    /// The mark before the name of the player `uuid` in the player list, each frame it shows.
    /// The first hook that answers is used.
    fn player_mark(&self, _uuid: u128) -> Option<players::Mark> {
        None
    }

    /// How the camera stands in third person, each frame. The first hook that answers is used.
    fn camera(&self, _own: &players::CameraView) -> Option<players::CameraOverride> {
        None
    }

    /// Draws on a player model, after its vanilla layers, each time it is drawn.
    fn player_layers(&self, _model: &players::PlayerModel, _draw: &mut dyn players::PlayerDraw) {}

    /// The resolver of `friend:` addresses this crate offers ([`tunnel`]); the first hook that
    /// offers one is used. Asked on the network thread when joining and on the game thread when
    /// an address is shown, never per frame.
    fn tunnel(&self) -> Option<&dyn tunnel::Resolver> {
        None
    }
}

/// What [`Hooks::started`] gets.
#[non_exhaustive]
pub struct Started<'a> {
    /// The build: the release tag (`0.2.31`), or `dev <crate version>` for a build from source.
    pub version: &'a str,
    /// The player's game folder: servers.dat, options.txt and saves (the working directory unless
    /// `--game-dir` names another).
    pub game_dir: &'a std::path::Path,
}

impl<'a> Started<'a> {
    pub fn new(version: &'a str, game_dir: &'a std::path::Path) -> Self {
        Self { version, game_dir }
    }
}

static HOOKS: Registry<dyn Hooks> = Registry::new();

/// Why [`register`] refused a hook.
#[derive(Debug, PartialEq, Eq)]
pub struct TooLate;

impl std::fmt::Display for TooLate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the game already started: hooks register before it runs")
    }
}

impl std::error::Error for TooLate {}

/// Adds `hooks` to those the game calls. Call it before the game starts (from the executable's
/// `main`, before it runs the game); once the engine has dispatched, the list is fixed and this
/// returns [`TooLate`]. Hooks are called in the order they registered.
pub fn register(hooks: &'static dyn Hooks) -> Result<(), TooLate> {
    HOOKS.register(hooks)
}

/// The registered hooks, sealing the list on the first call.
fn all() -> &'static [&'static dyn Hooks] {
    HOOKS.all()
}

thread_local! {
    static SEAT: Cell<usize> = const { Cell::new(0) };
}

/// The splitscreen player whose turn it is: 0 for P1 (and without splitscreen), 1 for P2...
/// Splitscreen players are one process, each `Game` in turn on the game's thread, so a crate's
/// statics are shared by them: a crate that keeps per-player state (settings, pages, HUD) indexes
/// it by this. A player keeps its seat while it plays. Other threads always see 0.
pub fn seat() -> usize {
    SEAT.get()
}

/// The engine's side: where it calls the hooks. Only the engine calls these.
pub mod engine {
    use super::*;

    /// The player whose turn starts on this thread ([`seat`]).
    pub fn set_seat(seat: usize) {
        SEAT.set(seat);
    }

    /// Seals every list (hooks and the registries); their first use does it anyway. Call where
    /// registration ends.
    pub fn seal() {
        all();
        crate::settings::all();
        crate::pages::engine::all();
        crate::hud::engine::all();
    }

    /// [`Hooks::started`].
    pub fn started(game: &Started<'_>) {
        for hooks in all() {
            hooks.started(game);
        }
    }

    /// Whether the world was open to LAN or friends at the last call.
    static PUBLISHED: AtomicBool = AtomicBool::new(false);

    /// [`Hooks::published`], when `open` differs from the last call's. Cheap enough to call
    /// every frame.
    pub fn published(open: bool) {
        let hooks = all();
        if !hooks.is_empty() && PUBLISHED.swap(open, Ordering::Relaxed) != open {
            for h in hooks {
                h.published(open);
            }
        }
    }

    /// [`Hooks::shutdown`].
    pub fn shutdown() {
        for hooks in all() {
            hooks.shutdown();
        }
    }

    /// Whether any hook is registered: without one the engine builds no view to hand them.
    pub fn any() -> bool {
        !all().is_empty()
    }

    /// [`Hooks::frame`].
    pub fn frame(frame: &players::Frame, sprites: &mut dyn players::SpriteStore) {
        for hooks in all() {
            hooks.frame(frame, sprites);
        }
    }

    /// [`Hooks::name_decoration`].
    pub fn name_decoration(
        model: &players::PlayerModel,
        sprites: &dyn players::Sprites,
    ) -> Option<players::NameDecoration> {
        all().iter().find_map(|h| h.name_decoration(model, sprites))
    }

    /// [`Hooks::player_mark`].
    pub fn player_mark(uuid: u128) -> Option<players::Mark> {
        all().iter().find_map(|h| h.player_mark(uuid))
    }

    /// [`Hooks::player_layers`].
    pub fn player_layers(model: &players::PlayerModel, draw: &mut dyn players::PlayerDraw) {
        for hooks in all() {
            hooks.player_layers(model, draw);
        }
    }

    /// [`Hooks::camera`].
    pub fn camera(own: &players::CameraView) -> Option<players::CameraOverride> {
        all().iter().find_map(|h| h.camera(own))
    }

    /// [`Hooks::tick`].
    pub fn tick(models: &mut dyn players::PlayerModels) {
        for hooks in all() {
            hooks.tick(models);
        }
    }

    /// [`Hooks::tunnel`]: the first registered hook's resolver.
    pub fn tunnel() -> Option<&'static dyn tunnel::Resolver> {
        all().iter().find_map(|hooks| hooks.tunnel())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use players::PlayerModels as _;
    use std::sync::Mutex as StdMutex;

    /// Records what it heard. One process-wide list, as the game has: this test is the only
    /// one that registers or dispatches.
    struct Recorder(StdMutex<Vec<String>>);

    impl Hooks for Recorder {
        fn started(&self, game: &Started<'_>) {
            self.0
                .lock()
                .unwrap()
                .push(format!("started {}", game.version));
        }
        fn published(&self, open: bool) {
            self.0.lock().unwrap().push(format!("published {open}"));
        }
        fn frame(&self, frame: &players::Frame, sprites: &mut dyn players::SpriteStore) {
            self.0
                .lock()
                .unwrap()
                .push(format!("frame {}", frame.connection));
            if !sprites.has(1) {
                sprites.put(1, vec![0; 64 * 64 * 4]);
            }
        }
        fn name_decoration(
            &self,
            model: &players::PlayerModel,
            sprites: &dyn players::Sprites,
        ) -> Option<players::NameDecoration> {
            let badge = sprites.get(1).map(|uv| players::Badge::new(uv, 0xFF0000));
            (model.kind == players::Kind::Player)
                .then(|| players::NameDecoration::new(Some(MARK), badge, Some(2.0)))
        }
        fn player_mark(&self, uuid: u128) -> Option<players::Mark> {
            (uuid == 9).then_some(MARK)
        }
        fn camera(&self, own: &players::CameraView) -> Option<players::CameraOverride> {
            Some(players::CameraOverride::new(own.age / 10.0))
        }
        /// A hat on the head, its brim turned by the model's age.
        fn player_layers(&self, _: &players::PlayerModel, draw: &mut dyn players::PlayerDraw) {
            let (Some(head), Some(uv)) = (draw.bone(players::Bone::Head), draw.sprite(1)) else {
                return;
            };
            let turn = draw.ticks() / 100.0;
            draw.draw(&HAT, head, uv, &mut |p| p.pose("brim")[4] = turn);
        }
        fn tick(&self, models: &mut dyn players::PlayerModels) {
            let mut ids = Vec::new();
            models.for_each(&mut |m| ids.push(m.id));
            for id in ids {
                models.set_body_pose(id, Some(std::sync::Arc::new(Still)));
            }
        }
    }

    #[derive(Debug)]
    struct Still;
    impl players::BodyPose for Still {
        fn pose(&self, _: f32, _: &[[f32; 9]; 6], _: &mut [[f32; 9]; 6]) -> bool {
            false
        }
    }

    /// Two models, poses by id.
    struct Two(std::collections::BTreeMap<u32, std::sync::Arc<dyn players::BodyPose>>);
    impl players::PlayerModels for Two {
        fn connection(&self) -> u64 {
            1
        }
        fn for_each(&self, f: &mut dyn FnMut(players::PlayerModel)) {
            for id in [3, 7] {
                f(players::PlayerModel::new(
                    id,
                    players::Kind::Player,
                    None,
                    0,
                    false,
                ));
            }
        }
        fn body_pose(&self, id: u32) -> Option<&std::sync::Arc<dyn players::BodyPose>> {
            self.0.get(&id)
        }
        fn set_body_pose(&mut self, id: u32, pose: Option<std::sync::Arc<dyn players::BodyPose>>) {
            match pose {
                Some(p) => self.0.insert(id, p),
                None => self.0.remove(&id),
            };
        }
    }

    const MARK: players::Mark = &[(0, 0, 8, 8, 0xFF00_00FF)];

    /// One sprite slot, as the game's atlas has many.
    #[derive(Default)]
    struct Atlas(Option<(u64, usize)>);
    impl players::SpriteStore for Atlas {
        fn has(&self, id: u64) -> bool {
            self.0.is_some_and(|(i, _)| i == id)
        }
        fn put(&mut self, id: u64, rgba: Vec<u8>) {
            self.0 = Some((id, rgba.len()));
        }
    }
    impl players::Sprites for Atlas {
        fn get(&self, id: u64) -> Option<[f32; 4]> {
            players::SpriteStore::has(self, id).then_some([0.0, 0.0, 0.5, 0.5])
        }
    }

    static HAT: std::sync::LazyLock<std::sync::Arc<players::Mesh>> =
        std::sync::LazyLock::new(|| {
            let brim = players::Mesh::new("brim".into(), [0.0; 9], Vec::new(), Vec::new());
            std::sync::Arc::new(players::Mesh::new(
                "hat".into(),
                [0.0; 9],
                Vec::new(),
                vec![brim],
            ))
        });

    /// A player drawn at age 50, its head where the camera is; logs each draw.
    #[derive(Default)]
    struct Canvas(Vec<(String, [[f32; 4]; 4], [f32; 4], f32)>);
    impl players::PlayerDraw for Canvas {
        fn pass(&self) -> players::Pass {
            players::Pass::World
        }
        fn ticks(&self) -> f32 {
            50.0
        }
        fn bone(&mut self, bone: players::Bone) -> Option<[[f32; 4]; 4]> {
            (bone == players::Bone::Head).then_some([[1.0, 0.0, 0.0, 0.0]; 4])
        }
        fn body_pose(&self) -> Option<&std::sync::Arc<dyn players::BodyPose>> {
            None
        }
        fn sprite(&self, id: u64) -> Option<[f32; 4]> {
            (id == 1).then_some([0.0, 0.0, 0.5, 0.5])
        }
        fn draw(
            &mut self,
            mesh: &std::sync::Arc<players::Mesh>,
            matrix: [[f32; 4]; 4],
            uv: [f32; 4],
            pose: &mut dyn FnMut(&mut dyn players::PartPoses),
        ) {
            struct Rest([f32; 9]);
            impl players::PartPoses for Rest {
                fn pose(&mut self, _: &str) -> &mut [f32; 9] {
                    &mut self.0
                }
            }
            let mut brim = Rest(mesh.children[0].pose);
            pose(&mut brim);
            self.0.push((mesh.name.clone(), matrix, uv, brim.0[4]));
        }
    }

    /// Implements nothing: the defaults do nothing.
    struct Silent;
    impl Hooks for Silent {}

    static RECORDER: Recorder = Recorder(StdMutex::new(Vec::new()));

    #[test]
    fn registered_hooks_hear_the_game_and_late_ones_are_refused() {
        register(&RECORDER).unwrap();
        register(&Silent).unwrap();
        engine::started(&Started::new("0.2.31", std::path::Path::new("game")));
        // Changes only: the same state every frame is said once.
        for open in [false, true, true, true, false, false] {
            engine::published(open);
        }
        engine::shutdown();
        assert!(engine::any());
        let mut atlas = Atlas::default();
        let model = |kind| players::PlayerModel::new(5, kind, Some(9), 0, false);
        // Before the frame put the sprite in: the decoration without its badge.
        let before = engine::name_decoration(&model(players::Kind::Player), &atlas).unwrap();
        assert!(before.badge.is_none() && before.top == Some(2.0));
        engine::frame(&players::Frame::new(4, false), &mut atlas);
        assert_eq!(atlas.0, Some((1, 64 * 64 * 4)));
        let after = engine::name_decoration(&model(players::Kind::Player), &atlas).unwrap();
        assert_eq!(
            after.badge.map(|b| (b.uv, b.line)),
            Some(([0.0, 0.0, 0.5, 0.5], 0xFF0000))
        );
        assert!(engine::name_decoration(&model(players::Kind::Mannequin), &atlas).is_none());
        assert_eq!(engine::player_mark(9), Some(MARK));
        assert_eq!(engine::player_mark(8), None);
        let camera = engine::camera(&players::CameraView::new(5.0));
        assert_eq!(camera.map(|c| c.distance), Some(0.5));
        let mut canvas = Canvas::default();
        engine::player_layers(&model(players::Kind::Player), &mut canvas);
        assert_eq!(
            canvas.0,
            [(
                "hat".to_owned(),
                [[1.0, 0.0, 0.0, 0.0]; 4],
                [0.0, 0.0, 0.5, 0.5],
                0.5
            )]
        );
        let mut models = Two(Default::default());
        engine::tick(&mut models);
        assert!(models.body_pose(3).is_some() && models.body_pose(7).is_some());
        assert_eq!(
            *RECORDER.0.lock().unwrap(),
            [
                "started 0.2.31",
                "published true",
                "published false",
                "frame 4"
            ]
        );
        assert_eq!(register(&Silent), Err(TooLate));
    }
}
