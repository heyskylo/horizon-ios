# Layers: engine, platform, Horizon

Horizon is three layers:

- a **vanilla engine**;
- a **platform** that makes the vanilla game easier to change, port and run;
- **Horizon's own layer**: its extras and its product (cosmetics, the shop, telemetry, sponsors, official servers,
  the launcher...).

**The target** (Ludovic, 2026-11-09). This repository holds the engine and the platform. Built alone, it is a
complete vanilla-like game. The Horizon layer moves to its own repository, public if its license needs it, and
ships with every official launcher download (see [The Horizon layer in its own repository](#the-horizon-layer-in-its-own-repository)).
The split comes first, inside this repository, one checked step at a time. The repositories separate after that.

This page says:

- what belongs to each layer and which layer may depend on which;
- where a new feature goes;
- where every built-in outside address lives;
- the plan.

CI checks the rules, so review doesn't have to. Contributors fixing vanilla behaviour never have to touch
cosmetics, the shop or a server address. Mods and desktop-class ports (iOS, Android) build on the platform without
editing engine files; ports to machines the desktop client can't run on (a handheld console, a browser) build on
the [portable crates](#portable-crates).

Credits:

- **NikitaMGrimm** (DuckyBertDuck on Discord): the three layers, the dependency rule, the address rule and the plan
  ("3-layer split: engine / platform / Horizon", #contributors, 2026-11-08), the portable crates (#12997, #13002),
  and the review that made the Horizon layer linked crates rather than native mods.
- **arismoko:** the mod API owns its types and a CI property check keeps engine types out of it
  ([arismoko/horizon](https://github.com/arismoko/horizon), arismoko/horizon#1608). Also the idea of shipping
  Horizon's own parts from another repository as a launcher plugin.
- **Martipops** (controllers, splitscreen): the compositor's place in the engine, and where controllers,
  splitscreen, Replay Studio, PvP & HUD and a UI page API go.
- **Contributors at the 2026-11-08 stage:** the file-size rule, the single list of addresses, sponsors in the
  Horizon layer, a platform that exposes what vanilla already uses, and extras as native mods.

## The rule

| Layer | May depend on |
|---|---|
| engine | engine |
| platform | engine, platform |
| Horizon | platform, Horizon |
| tool (dev tools) | anything; no shipped crate depends on a tool |

The engine never depends on the platform or on Horizon. The platform never depends on Horizon. **The Horizon layer
never depends on the engine:** it uses only the platform's public API, so it can build from another repository.
Normal, build and optional dependencies all count; dev-dependencies don't, so tests may use anything.

When lower code needs something from above, the lower layer offers an extension point and the higher layer plugs
into it. The extension point is a trait, a callback list, an event or a registry, and it names no type from above.
When Horizon needs something from the engine, the platform exposes it as public API.

## Engine

Vanilla Minecraft: Java Edition 26.3, and nothing else.

- **Belongs:** gameplay, world data and generation, block behaviour, mobs, items, commands, the network
  protocol, rendering, sound, vanilla's screens, saves, the integrated and dedicated servers. It also holds
  optimizations that keep vanilla's behaviour, and the extension points the platform plugs into.
- **Also the render pipeline's own capabilities that vanilla's path doesn't use**, when a higher layer could only
  get them by rebuilding the pipeline. The one today is the **compositor**: drawing a view into an offscreen target
  and composing several targets into a window's views. With one view (vanilla, and splitscreen with one player)
  the renderer takes vanilla's path and the compositor does nothing, so the engine still plays 1:1 vanilla.
  Splitscreen, the feature that uses it, is platform (below).
- **Examples:** `horizon-world` (`worldgen/`, `level/redstone.rs`), `horizon-protocol` (`packets/`),
  `horizon-items`, `horizon-command`, `horizon-core`, `game/creatures.rs`, `game/integrated.rs`, `gfx.rs`,
  `mesher.rs`, `streaming.rs`, `lan/`.
- **Never:** a mod hook's implementation, a shader or pack format vanilla doesn't read, a setting or screen vanilla
  doesn't have, a network address vanilla doesn't contact, Horizon branding.
- **Network:** only what vanilla contacts: Mojang and Microsoft services, and the servers the player joins.

### Portable crates

`horizon-core`, `horizon-world`, `horizon-protocol`, `horizon-items`, `horizon-command`, `horizon-client-core`
and `horizon-assets` hold vanilla's rules and data with no desktop in them: no window, GPU, sound, gamepad or
clipboard library, no HTTP client, and no engine crate outside this list. The platform sits on the desktop engine
(`wgpu`, `winit`, `cpal`), which a handheld console or a browser can't run, so a port there builds on these crates
instead. They stay that way: the layer check fails when one takes `winit`, `wgpu`, `cpal`, `gilrs`, `arboard` or
`ureq`, or an engine crate outside the list (#13002). Gameplay itself still lives in `horizon-client`; step P of
the [plan](#plan) moves it here over time.

## Platform

What makes vanilla nicer to change, port and run, with no Horizon branding and no address of its own. It is the
public API that mods, desktop-class ports and the Horizon layer build on.

- **Belongs:** the mod API and mod formats, the native mod host, the shader API and pack formats beyond vanilla
  (OptiFine, Polytone, `shaders/`), HDR, controllers, splitscreen, replay recording and playback, the HUD widget
  framework, UI pages and the settings system, agent control (`horizon-ctl`, MCP), the dedicated server's anticheat and chat
  filter, modpack porting, the design tokens of non-vanilla screens.
- **Exposes what vanilla already uses.** FOV changes, potion effects, the player's state, screens, rendering
  hooks and so on become public API with stable types (arismoko/horizon#1608's patterns). Then a mod, a port or a
  Horizon feature builds on them instead of editing or copying engine code.
- **Examples:** `horizon-shader-api`, `horizon-shaders`, `horizon-optifine` (`cem/`), `horizon-polytone`,
  `horizon-hdr`, `horizon-dualsense`, `horizon-mods`, `cargo-horizon-mod`, `horizon-anticheat`, `horizon-mcp`,
  `horizon-theme`. Still in engine crates today: `game/mod_api_host.rs`, `game/native_*.rs`, `shader_ui.rs`,
  `hdr_calibration.rs`.
- **Never:** a built-in address (every address comes from the player, a file or a setting), anything that names
  a Horizon crate, cosmetics, the shop, official or partner servers, uploads to playhorizon.dev.
- **Network:** allowed, but never to a built-in address.

### Controllers, splitscreen, replays and UI pages

Vanilla has none of these, and the engine stays 1:1 vanilla, so they are platform features on hooks the engine
exposes. Ports to consoles and handhelds need them, and other projects can build on them.

- **Controllers:** the pad backend (gilrs), bindings, deadzones and the look curve, button glyphs, the pad's menu
  navigation and cursor, the controller settings, DualSense output. Today in `horizon-client-input/src/gamepad.rs`,
  `game/pad.rs`, `game/pad_*.rs` and `game/pad/`, `pad_cursor.rs`, `pad_glyph.rs`, `controller_ui.rs`, and
  `horizon-dualsense`. The engine's hooks:
  - **Actions.** Every input the game reads is an `Action`, vanilla's key mappings (`horizon-client-input/src/actions.rs`),
    and code asks whether an action is held (`held.rs`), never a key. A pad presses and holds actions like a
    keyboard does. This hook exists today.
  - **Movement and look.** An analog move vector and a look delta that the player's movement reads instead of
    keys (today `game/pad.rs` calls `gamepad::move_vector` and `Game::look_turn` itself).
  - **Menu focus.** Screens say which widget has focus and take "move focus" and "press" (today the pad's
    cursor drives vanilla's screens through mouse events).

  The last two become a registered input source, a trait in the engine that the platform's pad code implements,
  in M6. Until then the pad code stays in its own modules, so that move is a pure file move.
- **Splitscreen:** more local players, each its own `Game` joined to P1's server thread through memory; which
  device drives which player; the setup screen, window layouts and profiles. Today in `game/split.rs` and
  `game/split/`, `split_layout.rs`, `split_map.rs`, `split_profiles.rs`, `split_ui.rs`. It draws through the
  engine's compositor (see [Engine](#engine)), and its input comes through the input hooks above, one input
  source per player.

  The compositor stays in the engine, as Martipops (who builds splitscreen) and duckybertduck argued: it is part
  of how the renderer presents a frame (targets, submission, pacing, present). Moving it above the engine would
  mean exposing the whole frame pipeline as API or rebuilding it up there, for a feature that on vanilla's path
  costs nothing. What the engine must not hold is the feature: the router, the screens, the layouts.
- **Replays:** recording the session's packets as `.mcpr` (the format other replay tools read) and playing them
  back through the client (`studio/recorder.rs`, `mcpr.rs`, `replay.rs`) is platform: mods, tools and bug reports
  build on it. The Studio on top of it (camera paths, splines, the timeline and its bar: `studio/camera.rs`,
  `spline.rs`, `path_render.rs`, `timeline_ui.rs`, `topbar.rs`, `studio_ui.rs`) is a finished feature, so it
  goes to the Horizon layer or a native mod (rule 4), built on the platform's replay API.
- **UI pages:** a general API for pages a mod, a port or the Horizon layer adds (the settings shell, its rows,
  search and undo, today `settings_shell.rs` and `extras_ui.rs`'s pages). The engine exposes screens: open one, draw on
  the GUI layer, take input and focus. The platform gives the page API stable types (as #1608 does for the mod
  API). Horizon Settings, the controller pages and the PvP & HUD page all become clients of it.
- **PvP & HUD:** the HUD widget framework (placing, sizing and toggling HUD elements: `hud_kit.rs` and `hud_kit/`,
  `extras_hud.rs`, `game/hud_room.rs`, `game/hud_editor.rs`) is platform. `hud_layout.rs` is vanilla's `Hud` as draw
  calls, which the parity fixture compares with the vanilla client: engine, like `hud_overlays.rs`. The PvP presets and Horizon's own widgets (keystrokes, CPS, reach readout, `pvp.rs`, `pvp_hud.rs`)
  are Horizon extras, and candidates to become native mods.

## Horizon

Horizon's own extras and its product, and every built-in address of an outside service vanilla doesn't contact.

- **Belongs:** Horizon's extras (zoom, Mouse Tweaks, sorting, smooth chat, the PvP & HUD presets and widgets, the
  music page, keystrokes, auto-reconnect...), cosmetics, emotes, the wardrobe and the shop, sponsors, the official
  and partner servers in the list, Play with Friends and its relay, Discord, the "I'm playing" ping, telemetry and
  benchmark uploads, releases and updates, the hosting supervisor, the launcher.
- **Examples:** `horizon-addresses`, `horizon-cosmetics`, `horizon-friends`, `horizon-relay`, `horizon-telemetry`,
  `horizon-bench`, `horizon-release-sign`, `horizon-server`, `horizon-mod-registry`, `horizon-heartbeat`,
  `horizon-official`, `horizon-official-servers`, `horizon-discord`, `launcher/`. Still in engine crates today:
  `game/cosmetics.rs`, `game/emotes.rs`, `game/extras.rs`, `game/tweaks.rs`, `extras_ui.rs`, `pvp.rs`.
- **Rules:**
  - It uses the platform's public API only, never the engine.
  - Until it moves, every Horizon feature in the game sits behind the `horizon-extras` feature
    ([forking.md](forking.md)), so [horizon-plain](https://github.com/horizon-rust/horizon-plain) builds without it.

## Tools

Developer tools that no player runs: `xtask`, `horizon-bots`, `horizon-mod-api-gen-tests`, and the Python in
`tools/`. They may depend on anything, and no shipped crate depends on them.

## Where a new feature goes

The rule, in order (duckybertduck's):

1. Vanilla has it: **engine**. Hooks in the engine are fine: the platform needs them.
2. It is directly about Horizon (server addresses, the website, the name or branding): **Horizon**.
3. It helps other people build on the project (mods, ports, other clients; dev tooling excepted, see
   [Tools](#tools)): **platform**.
4. Everything else: **Horizon**, or a native mod that players add themselves, since it can be built on what's below.

A feature often splits along this rule: the part others build on is platform, and the finished feature on top of it
is Horizon (PvP & HUD and replays, [above](#controllers-splitscreen-replays-and-ui-pages)).

| I add... | Layer | Where |
|---|---|---|
| A vanilla behaviour, block, mob, packet, command or screen | engine | the crate that owns the area ([CLAUDE.md](../CLAUDE.md) crate table) |
| A speed-up that keeps vanilla's results | engine | next to the code it speeds up |
| A call to a Mojang or Microsoft service vanilla makes | engine | `auth.rs`, `realms/`, `skins.rs`, `horizon-server-files` |
| A place mods, ports or Horizon can hook into | engine + platform | an extension point in the engine crate, naming no type from above; its public face in the platform |
| A mod hook, host method or mod format | platform | `horizon-mods`, the mod API (today: `game/mod_api_host.rs`, `game/native_*.rs`) |
| A shader or resource pack format vanilla doesn't read | platform | `horizon-shaders`, `horizon-optifine`, `horizon-polytone`, or a new platform crate |
| A dedicated server option (anticheat, chat filter) | platform | `horizon-anticheat`, `horizon-chat-filter` |
| Controller support: a device, bindings, glyphs, pad menus | platform | the pad modules, on the engine's input hooks ([Controllers](#controllers-splitscreen-replays-and-ui-pages)) |
| Splitscreen or more players on one machine | platform | the `split` modules, on the engine's compositor |
| A render pipeline capability vanilla's path doesn't use but a higher layer can't add without rebuilding the pipeline (the compositor) | engine | `gfx.rs` and `gfx/`, doing nothing on vanilla's path |
| A UI page others can add to, a HUD widget framework, replay recording and playback | platform | the page API, `hud_kit.rs` and `hud_kit/`, `studio/recorder.rs`, `mcpr.rs`, `replay.rs` |
| A finished feature built on those (PvP presets, the replay editor) | Horizon, or a native mod | behind `horizon-extras` |
| A Horizon extra (a setting, screen or HUD element vanilla doesn't have) | Horizon | its own module behind `horizon-extras`, built on platform API; or an optional native mod that players add themselves |
| Anything that talks to playhorizon.dev, a partner or a third-party service by itself | Horizon | a Horizon crate, its address in `horizon-addresses` |
| Cosmetics, emotes, the shop, sponsors, Discord, telemetry, official servers | Horizon | `horizon-cosmetics`, `horizon-telemetry`..., behind `horizon-extras` |
| A launcher page | Horizon | `launcher/src/` |
| A harness, script or dev tool | tool | `tools/`, `xtask/` |
| A new crate | its layer | give it a layer in `tools/layers.json` in the same PR |

Code that belongs to another layer than the crate it has to live in today (a Horizon extra in
`horizon-client-render`, say) goes in its own module, never mixed into an engine module. Horizon code in the game
also goes behind `horizon-extras`. That keeps each later move a pure file move.

## The Horizon layer in its own repository

The target, after the split. It is not built yet: nothing moves before the split below is done.

**What a build of this repository gives.** A complete game: vanilla play, the platform (mods, shader packs,
controllers, splitscreen, replays, HDR, the dedicated server) and no Horizon layer. That means no extras, cosmetics, shop, telemetry,
sponsors or official server, no built-in outside address, and no launcher. Forks and builds from source get exactly
that. horizon-plain becomes a plain build of this repository.

**The seam.** The Horizon repository has to plug into the game without changing it:

1. No engine or platform code names a Horizon crate. The layer check refuses it.
2. The Horizon layer uses the platform's public API only. The layer check refuses its engine dependencies, and
   today's are on the exceptions list.
3. Horizon's in-game parts (extras, cosmetics, emotes, Discord, Play with Friends, the ping) are crates linked into
   the official game build in CI, plugged in through the platform's hooks. Players never compile them: a native
   mod is source the launcher compiles per instance, so it is not the way. The official executable is a thin
   binary crate in the Horizon repository that links the platform and the Horizon crates; this repository keeps
   its own plain executable. The game runs the same without them.
4. Every Horizon address is in `horizon-addresses`, which moves with the layer.
5. The launcher is Horizon's: it moves too. It installs the game from this repository's releases and adds the
   Horizon layer to it.

**Licence.** The project's licence is being decided (the poll ends around 2026-11-10); the Horizon repository's
licence follows from it. If this repository becomes GPL, the official game links the Horizon crates into the same
executable as the engine and the platform, so the Horizon layer must be GPL-compatible too: no closed native code in
it. Mods loaded as WebAssembly run as separate modules, not linked into the executable, so they could stay closed
(duckybertduck's reading; the licence decision settles it).

**Every official download includes it.** These paths must ship the Horizon layer:

| Path | Today |
|---|---|
| The launcher's release builds, every platform and format: Linux `.tar.gz` and AppImage, macOS `.zip` and signed `.dmg`, Windows `.zip` and `horizon-launcher-setup*.exe` | `.github/workflows/launcher.yml` |
| GitHub releases (`launcher@X.Y.Z`) | `launcher.yml`'s release job, `tools/release_train.py` |
| playhorizon.dev's download page | horizon-rust/horizon-site, reading `launcher/links.json` and the releases |
| The launcher's self-update | `launcher/src/backend.rs` (`stage_update`), `launcher/src/update_handoff.rs`, `launcher/src/nightly.rs` |
| The game builds the launcher installs, so the in-game part reaches every player | `.github/workflows/nightly-build.yml`, `release.yml`, `game-build.yml` |
| The links the launcher and the site read (Discord, sponsors, website, official server) | `launcher/links.json` |

These stay without it: builds of this repository from source, horizon-plain, and `packaging/server/install.sh`
and `install.ps1` (the dedicated server). The server's anticheat and chat filter are platform features.

## How a Horizon crate plugs in

The seam's points 1-3 above, as they work today (layer plan M5).

| Crate | Layer | Holds |
|---|---|---|
| `horizon-hooks` | engine | The `Hooks` trait, `register`, and the engine's dispatch (`horizon_hooks::engine`) |
| `horizon-platform` | platform | `horizon_platform::hooks`: `Hooks`, `Started`, `register`, without the dispatch |
| `horizon-game` | platform | The game for an executable to start: `horizon_game::main()`, the allocator, the mod API mark. The platform's own parts register here as they leave the engine (M6) |
| `horizon-official` | Horizon | The official executable: `register()` adds the Horizon crates' hooks, then it runs `horizon_game::main()` |

A Horizon crate that runs in the game:

1. depends on `horizon-platform` (never on an engine crate) and implements `Hooks` on a `static`, only the methods it
   needs: every method has an empty default;

   ```rust
   use horizon_platform::hooks::{Hooks, Started};

   pub static HOOKS: Ping = Ping;
   pub struct Ping;

   impl Hooks for Ping {
       fn started(&self, game: &Started<'_>) { /* someone is playing: game.version */ }
       fn published(&self, open: bool) { /* the world was opened to LAN or friends, or closed */ }
   }
   ```

2. is added to `horizon-official`'s dependencies and to its `register()`:
   `horizon_platform::hooks::register(&my_crate::HOOKS)`.

Registration happens before the game runs: the engine seals the list as it starts (`entry::main`), and a later
`register` returns `TooLate`. Hooks run in the order they registered, on the render thread unless the method says
otherwise, and must return quickly. With nothing registered a dispatch is one load and an empty loop, so the plain
build pays nothing.

The hooks today: `started` (once, when someone plays: not a dedicated server, a benchmark or an automated run),
`published` (the world opened to or closed from LAN or friends, on change), `shutdown` (the window closed, the
world saved) and `tunnel` (the resolver of `friend:` addresses, Play with Friends'). **A hook you need and don't find:** add a method with an empty default to `Hooks` and a function in
`horizon_hooks::engine` that the engine calls where vanilla does the matching thing; give it plain data or a type
the engine owns, never one from above. A hook that needs engine types the platform doesn't expose yet (a draw
context for HUD and chat overlays, a settings page) comes with that type's platform API (M6). An extension point
that belongs to one subsystem (an input source, the compositor) lives in the engine crate that owns it, with the
same shape: registered before the game runs, read-only after.

**What players get.** The release builds (`game-build.yml`, so the nightly and the release; `tools/ci/build-musl.sh`)
build `horizon-official` and ship it as `horizon-client`, the name the launcher and players know. The launcher's
native-mod builds of the game do the same, and its generated executable for trait mods calls
`horizon_official::register()` (`launcher/src/native_mods/api.rs`). The dedicated server build (`horizon-engine`)
stays horizon-client's, with no Horizon part. horizon-client's own executable, the one `cargo run -p
horizon-client`, the harnesses and horizon-plain build, has none either. The first part that moved this way is the
"I'm playing" ping (`horizon-heartbeat`).

## Platform registries

The spec for the three registries that let a platform or Horizon crate add settings, pages and HUD drawing without
an engine file naming it. Controllers, splitscreen, replays (M6) and Horizon's extras (M5) move out on them.
Status: each registry lands in its own PR (layer plan M5, seam): the settings store (#14036), pages (#14070:
`HorizonSettings` and every row but `Slider`; #14110: `Slider` and `tooltip`; `Title` and `Pause` next), the HUD
hook (#14081), `seat()` (#14096).

**One shape for all three.** As with `Hooks` above:

- The trait and its `register` are in `horizon-hooks` (engine; no dependencies, so it names no type from above).
  `horizon-platform` re-exports them as `horizon_platform::{settings, pages, hud}`. They take plain data
  (strings, numbers, translation keys) and no engine type. `horizon-platform` stays light because the launcher
  and the telemetry server build it.
- A crate registers a `&'static` value before the game runs (`horizon-official`'s `register()` for Horizon,
  `horizon_game::main()` for the platform). The engine seals every list as it starts (`entry::main`).
- A registrant keeps its own state: statics in its own crate, never a field of an engine struct. The engine
  only calls it.
- With nothing registered, the engine does no work: a load of an empty slice where it would dispatch. It makes
  no allocation, no draw call and no file. A vanilla build (horizon-client, horizon-plain) is unchanged.
- Visible text is a translation key the registrant ships in its lang files, never English in code. Values a
  player typed (a name, a path) are the exception.
- Splitscreen players are one process, one thread, each `Game` in turn (`game/split.rs`), so a crate's statics
  are shared by them. Every call (stores, pages, layers, hooks) happens on one player's turn, and
  `horizon_platform::hooks::seat()` (0 for P1) says whose: a crate keeps per-player state indexed by it.
- The enums are `#[non_exhaustive]`: a new row, act or place is an addition, not a break.

### Settings store (`horizon_platform::settings`)

Each crate keeps its own settings and its own lines in the game folder, next to `options.txt`, which stays
exactly vanilla's.

```rust
pub trait Store: Sync {
    /// Its file in the game folder: `horizon-extras.txt` (shared with the engine's extras until they all
    /// move) or its own `horizon-<name>.txt`.
    fn file(&self) -> &'static str;
    /// Whether a `key:value` line of that file is this store's (a fixed list, or a `<name>.` prefix).
    fn owns(&self, key: &str) -> bool;
    /// One of its lines, as the game loads its settings. Unknown values keep the default.
    fn load(&self, key: &str, value: &str);
    /// Its lines, as the game saves its settings.
    fn save(&self, line: &mut dyn FnMut(&str, &str));
    /// Whether everything is at its default: a file that doesn't exist yet is then not written.
    fn is_default(&self) -> bool;
}
```

- The engine routes lines in `Settings::load` and `Settings::save`: once per load or save, never per frame.
  Each file is read once. A store's lines come after the engine's own in a shared file. A line nobody owns
  is kept as it was.
- A setting that moves out of `extras.rs` keeps its key and file, so players keep their choice. The engine's
  `Extras` stops reading and writing the key in the same PR.
- A page that changes a store's setting (below) needs nothing else: the options screen saves on close, as
  it does for every setting.

### Pages (`horizon_platform::pages`)

A page lives in the options screen's shell, and its entry sits on one of the game's menus.

```rust
pub enum Place { HorizonSettings, Title, Pause }
pub trait Page: Sync {
    fn id(&self) -> &'static str;            // stable, for horizon-ctl and the page stack
    fn title(&self) -> &'static str;         // translation key
    fn place(&self) -> Place;                // where its entry is
    fn rows(&self, rows: &mut Vec<Row>);     // asked when the page is drawn, not every frame elsewhere
    fn act(&self, row: usize, act: Act);     // a click, a reverse click, a slider's value, a key
    fn tooltip(&self, row: usize) -> Option<Label> { None }   // hover text, may follow the value
}
pub enum Row {
    Heading(&'static str),                   // translation key
    Note(String),                            // grey text, a value (a path)
    Toggle { label: &'static str, on: bool },
    Cycle { label: &'static str, value: Label },
    Slider { label: &'static str, value: f32, text: Label },   // value in 0..=1
    Button { label: &'static str },
    Key { label: &'static str, key: String },                 // a binding: `key.keyboard.z`
}
pub enum Label { Key(&'static str), Value(String) }
pub enum Act { Press, PressBack, Slide(f32), Bind(String) }
```

- `HorizonSettings`: a row of the Horizon Settings page, after the engine's own pages. `Title` and `Pause`: a
  button on the title screen or the pause menu that opens the page. These buttons exist only when something
  registered, so the vanilla menus keep vanilla's look. The page is drawn with the same widgets, tooltips,
  narration, controller focus and `horizon-ctl` ids as the engine's pages.
- Rows are rebuilt when the page is drawn or acted on, like the engine's own pages: no cost while no page is
  open.
- The engine keeps native mods' pages (`Page::NativeMod`) as they are. Both kinds sit in one module that
  `options_ui.rs` calls: `options_ui.rs` is over 3000 lines and doesn't grow.

### HUD drawing (`horizon_platform::hud`)

```rust
pub enum Corner { TopLeft, TopRight, BottomLeft, BottomRight }
pub trait Canvas {
    fn size(&self) -> (f32, f32);                                  // GUI pixels
    fn tr(&self, key: &str) -> String;                             // the player's language
    fn text_width(&self, text: &str) -> f32;
    fn text(&mut self, x: f32, y: f32, text: &str, argb: u32, shadow: bool);
    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, argb: u32);
    /// A box of `w` × `h` in `corner`, stacked with the minimap, PvP widgets and mods' boxes;
    /// `None` while the debug screen covers that corner.
    fn place(&mut self, corner: Corner, w: f32, h: f32) -> Option<(f32, f32)>;
}
pub trait Layer: Sync {
    fn draw(&self, canvas: &mut dyn Canvas);
}
```

- Layers draw in the HUD pass where the extras do (`game/extras.rs`, `draw_extras`, beside the native mods' HUD
  boxes), in the order they registered. They are hidden with F1, and a corner the debug screen covers is
  `None`, as for mods' boxes.
- With no layer registered the engine builds no canvas.
- A layer reads its own state, which its crate keeps up to date through `Hooks` or its own threads. It never
  reads the engine's.
- Sprites, items and the player's state come with their platform API (M6). They are not in this first
  version.

### Look (`horizon_platform::look`)

What the engine draws differently for a crate above, as plain data: PvP & HUD's knobs today (#14217).

```rust
pub struct Look {       // #[non_exhaustive]; Default is vanilla's
    pub legacy_pose: bool, pub hide_attack_indicator: bool,
    pub mute_attack_sounds: bool, pub hide_attack_particles: bool,
    pub frame: Frame,   // outline, hurt overlay and glint colours
    pub shield_size: u8, pub totem_scale: f32, pub fire_lower: f32,
    pub clear_lava: bool, pub clear_water: bool, pub chat: Chat,
    pub hide_effect_icons: bool, pub time_of_day: Option<i64>, pub weather: Option<(f32, f32)>,
}
pub fn update(change: impl FnOnce(&mut Look));  // the player whose turn it is (`seat()`)
```

- Pushed, not asked: a crate changes its own fields when its settings load or change (several crates share the
  look, each keeping to its fields), and the engine reads it where it draws (`game/look.rs`, the only reader).
  Nothing is called per frame, and a seat nothing changed sees vanilla's.
- A knob is named by what it changes in the picture or the sound, never by the feature that uses it. A new knob
  is a field added with vanilla's value as its default.

### Container screens (`horizon_platform::containers`)

Input and drawing on an open container screen (the inventory, chests, furnaces, crafting tables...), for the
extras that act on one: Mouse Tweaks and sorting (M5).

```rust
/// The open screen's menu, for the length of one call. The engine implements it.
pub trait Container {
    fn menu(&self) -> &str;                     // "inventory", "minecraft:generic_9x3", "minecraft:furnace"...
    fn slots(&self) -> Vec<Slot>;               // every slot drawn, in the screen's order
    fn stack(&mut self, slot: SlotId) -> Option<Stack>;
    fn carried(&self) -> Option<Stack>;         // the stack on the cursor
    fn takes_one(&mut self, slot: SlotId, of: &Stack) -> bool;  // the menu's rules: mayPlace, the slot's limit
    fn landing(&mut self, slot: SlotId) -> Vec<SlotId>;         // where quickMoveStack would put its items
    fn click(&mut self, slot: SlotId, click: Click) -> bool;    // one ordinary click, as vanilla sends it
    fn spread(&self) -> usize;                  // the slots vanilla's quick craft drag covers now
    fn cursor(&self) -> (f32, f32);             // GUI pixels, from the panel's corner
    fn shift(&self) -> bool;
    fn order(&mut self, slot: SlotId) -> (u32, String);         // the creative tabs' order and the shown name
}
pub struct SlotId(pub u32);                     // opaque, stable while the screen is open
pub struct Slot { pub id: SlotId, pub role: Role, pub x: f32, pub y: f32 }  // top left, from the panel's corner
pub enum Role { Hotbar, Inventory, Storage, Input, Result, Equipment, Other }   // a crafting grid is Input
pub struct Stack { pub item: String, pub count: u32, pub max: u32, pub kind: u64 }  // kind: equal for the same item and components
pub enum Click { Pick { right: bool }, QuickMove, Put { one: bool } }               // Put: a one-slot QUICK_CRAFT drag

// In `Hooks`, each `false` (or nothing) by default, so vanilla's handling runs:
fn container_press(&self, c: &mut dyn Container, slot: Option<SlotId>, right: bool) -> bool;
fn container_drag(&self, c: &mut dyn Container, slot: Option<SlotId>) -> bool;  // the cursor moved, a button held
fn container_release(&self, c: &mut dyn Container);
fn container_wheel(&self, c: &mut dyn Container, slot: Option<SlotId>, steps: f32) -> bool;
fn container_key(&self, c: &mut dyn Container, slot: Option<SlotId>, key: &str) -> bool;  // `key.keyboard.r`, `key.mouse.middle`
fn container_draw(&self, c: &dyn Container, canvas: &mut dyn hud::Canvas);    // over the panel, from its corner
```

- `true` means the hook took the input: vanilla's press, drag or wheel doesn't run, and a press or drag taken
  ends vanilla's quick craft drag, as Mouse Tweaks' right-drag replaces it. Every hook hears `release`. The
  first hook that returns `true` wins.
- Each `click` is an ordinary slot click: the menu engine's on the player's own world, a `ContainerClick` on a
  server. So an extra built on it works wherever vanilla's clicks do, and anticheats see vanilla's packets.
- The engine offers every menu screen's slots; the crate decides which menus it acts in, by `menu()` (Mouse
  Tweaks stays out of those whose slots cost or trigger something: anvils, enchanting, merchants, beacons,
  crafters). Creative's tabs are no container screen.
- The reads that ask the menu's rules take `&mut self`, as the engine's do; drawing gets `&dyn Container`,
  so it reads the rest only.
- The engine builds a `Container` only when a hook is registered and a container screen gets input or draws.
  Without one there is no cost, and vanilla's handling is unchanged.
- `order` is the only text: Sorting ranks by it. The rest is ids and numbers, so the API names no engine type
  (`ItemStack` stays in the engine).

## Extras that may leave the game

Horizon's extras are Horizon-layer code. Each one is a candidate to become an optional native mod that players add
themselves, or stays a cfg-gated module linked into the official build. None moves yet; the list is here so new work doesn't add to them. See [extras.md](extras.md) for each one:

- zoom (OptiFine-style), keystrokes and FPS overlays, own name tag;
- Mouse Tweaks controls, sorting, scroll-wheel settings, smooth chat;
- auto-reconnect, UI-sound channels, the music page;
- the PvP & HUD presets and widgets, the replay editor (Replay Studio's camera paths and timeline), the AI agent page;
- the Horizon Settings menu itself, the advanced menu that hosts them;
- the renderer's extras: fullbright, dynamic lights, hidden block-break particles;
- chunk pregeneration.

The minimap already moved to a native mod ([extras.md](extras.md#moved-to-native-mods)). Splitscreen,
controllers and replay recording are not extras: they are platform ([above](#controllers-splitscreen-replays-and-ui-pages)).

## File size

A Rust file over 3000 lines may not grow compared with the base of the branch. `game.rs` (~21,700 lines),
`game/remote.rs` (~10,900) and `gfx.rs` (~10,100) are the worst; they slow incremental builds. Smaller files may
grow to 3000 lines, and a big file may shrink or stay as it is. There is no list of files, so no check breaks on
an unrelated edit. Generated files and `third_party/` are not checked. Test code doesn't count either, since a normal
build doesn't compile it: inline `#[cfg(test)] mod name { ... }` modules, and files named `tests.rs` or
`<name>_tests.rs`. Other `#[cfg(test)]` items count.

## The checks

- `python3 tools/check_layers.py` (#12983) reads the workspaces' `cargo metadata` (no build, about a second) and
  `tools/layers.json`, which gives every crate its layer and each layer what it may use. It fails on:
  - a crate with no layer;
  - a dependency its layer may not use that `allowed` doesn't list;
  - a branch adding an `allowed` entry for a crate that already had a layer (a crate newly split out of another
    may carry the old crate's exceptions);
  - shipped code depending on a tool.

  Each failure names the section of this page that applies. An `allowed` entry with no dependency left is only a
  note.
- `python3 tools/check_addresses.py` (#12990) checks the built-in addresses ([below](#built-in-addresses)).
- `python3 tools/check_file_size.py` (#13015) checks the file-size rule above.

All three run in CI's verify job and in `tools/pr_ready.py`.

### Today's exceptions

47, on main at d5504cd328 (2026-11-08):

| Crate | Uses, against the rule |
|---|---|
| `horizon-client` | platform: `horizon-anticheat`, `-chat-filter`, `-dualsense`, `-hdr`, `-mod-api-gen`, `-mods`, `-optifine`, `-shader-api`, `-shaders`, `-theme`; Horizon: `horizon-bench`, `-cosmetics`, `-friends`, `-telemetry` |
| `horizon-client-lib` | the same, plus `horizon-mcp`, minus `horizon-mod-api-gen` and `horizon-friends` (left through `Hooks::tunnel`) |
| `horizon-client-render` | platform: `horizon-hdr`, `-mod-api-gen`, `-optifine`, `-polytone`, `-shader-api`, `-shaders`, `-theme`; Horizon: `horizon-bench`, `-cosmetics`, `-telemetry` |
| `horizon-client-input` | platform: `horizon-dualsense`, `horizon-theme` |

No platform crate depends on Horizon.

## Built-in addresses

An address is built in when the code contacts it without the player typing it. Engine code may hold vanilla's own
endpoints. Every other built-in address belongs to the Horizon layer, in one list file:
`crates/horizon-addresses/src/lib.rs` (#12993). The game's Horizon crates and the launcher read it, and it moves
with the Horizon layer. Platform code gets its addresses passed in.

`tools/check_addresses.py` reads the string literals of shipped Rust and finds every host. It skips tests,
placeholders (`example.com`, `*.test`) and local IPs. `tools/addresses.json` says where each host may be:

- `homes`: the list file;
- `vanilla`: vanilla's endpoints, allowed anywhere;
- `not_addresses`: strings that look like hosts but are not contacted, each with its reason;
- `allowed`: today's addresses outside the list file, as "path host".

`allowed` only shrinks. A branch may move an entry with its code, but it may not add a host or grow the list.

Vanilla's endpoints (engine, the same as Java Edition):

| Address | For | Where |
|---|---|---|
| `discovery.minecraftservices.com`, `api.minecraftservices.com` | services discovery, profile keys, player certificates | `auth.rs`, `horizon-server-files/src/profiles.rs` |
| `textures.minecraft.net` | skins and capes | `skins.rs` |
| `pc.realms.minecraft.net`, `java.frontend.realms.minecraft-services.net` | Realms | `realms/client.rs` |
| `aka.ms/…` | vanilla's help links (accessibility, moderation, EULA, Realms terms), opened only on a click | `game/options.rs`, `game/screens.rs`, `ban_notice.rs`, `realms.rs`, `eula.rs` |
| `libraries.minecraft.net`, `piston-meta.mojang.com`, `resources.download.minecraft.net` | the vanilla jars, libraries and assets | `horizon-prepare`, launcher |

Horizon's own and third-party addresses (Horizon layer):

| Address | For | Where today |
|---|---|---|
| `play.playhorizon.dev` (status `:24454`), previously `horizon.bunchy7s.dev` | the official server in the list, its status | `horizon-addresses` (#12993), `launcher/links.json` |
| `cosmetics.playhorizon.dev` | cosmetics, emotes, presence | `horizon-addresses` (#12993) |
| `telemetry.playhorizon.dev` | opt-in telemetry, the "I'm playing" ping, benchmark uploads | `horizon-addresses` (#12993) |
| `stun.cloudflare.com`, `stun.l.google.com` | Play with Friends NAT discovery | `horizon-addresses` (#12993) |
| `afri-network.co.za` | the withdrawn partner server, so the game can remove the entry it added | `horizon-addresses` |
| `horizon-rust.github.io/horizon-mods`, `github.com/horizon-rust/horizon-mods` | the mod registry index and files: a build of this repository alone asks the player for a registry | `mod_registry.rs`, `horizon-mods/src/index*.rs`, `cargo-horizon-mod` |
| `api.modrinth.com`, `cdn.modrinth.com` | the in-game resource pack browser, modpack porting | `pack_browser.rs`, `horizon-port-pack` |
| `github.com`, `raw.githubusercontent.com`, `gitlab.com`, `maven.neoforged.net` | modpack porting's sources | `horizon-port-pack` |
| `playhorizon.dev` | the download link shown with no resources | `no_resources.rs` |
| `api.github.com`, `discord.com` | hosting: updates, Discord webhooks | `horizon-server` |
| `api.github.com`, `uploads.github.com`, `playhorizon.dev` | the registry's submission service | `horizon-mod-registry` |
| Modrinth, CurseForge, Fabric, Quilt, Forge, NeoForge, GitHub, Microsoft and Xbox sign-in, Tailscale, `discord.gg`, `mclo.gs`, Sculk Hosting, `static.rust-lang.org`, GitHub Sponsors | the launcher's browsers, loaders, sign-in, logs, hosting, partner and sponsor pages | `launcher/src/`, `launcher/links.json` |

These are not addresses:

- `bluemap_detect.rs` lists dynamic-DNS suffixes, to guess a server's own map host;
- `horizon-server/src/discord.rs` lists the hosts a webhook URL may have;
- the Windows, Apple and SOAP schema URLs in manifests and requests, and the UPnP multicast group.

## Reviewer checklist

- The PR names its layer, and new code sits in a crate or module of that layer (the table above).
- No dependency its layer may not use. CI fails on one; a module in an engine crate that calls platform or Horizon
  code is just as wrong, and so is Horizon code that reaches into the engine instead of the platform.
- Horizon features in the game are behind `horizon-extras`, and nothing new names a Horizon crate below it.
- No new built-in address outside `horizon-addresses`.
- No file over 3000 lines grows.

## Plan

Review first. The design and this plan are open for review in the tracking issue for ~48 hours. The small
mechanical steps (checks, M1–M4) continue meanwhile. M5 and M6 started on 2026-11-09, after the review in #13011. Each step is a PR under ~500 lines. Each move:

- keeps the default build the same for players and costs nothing at run time;
- keeps `--no-default-features` and horizon-plain building;
- takes its entries out of the allowlists;
- runs the harnesses `tools/parity.py --changed-files … --list` selects when it touches gameplay.

| Step | What | Removes | State |
|---|---|---|---|
| 1 | This page, first version | | #12979, merged |
| 2 | Layer check in CI and pr_ready, with the PR template's checklist | | #12983, merged |
| 3 | Address check in CI and pr_ready | | #12990, merged |
| 4 | File-size check in CI and pr_ready | | #13015, merged |
| 2b | The portable crates' rule in the layer check ([Portable crates](#portable-crates)) | | #13002 (NikitaMGrimm), merged |
| M1 | `horizon-addresses`, the address list file; the Horizon crates read it | 7 address entries | #12993, merged |
| M2 | `horizon-system` (engine): the hardware probes leave `horizon-telemetry`, and the crash report and GPU info read them there; `horizon-platform`, the platform's public API, re-exports them for the Horizon layer | prepares M3 | #13058, merged |
| M3 | The renderer's telemetry module leaves `horizon-client-render` for `horizon-client-lib`, with Horizon's other in-game parts; the renderer offers the adapter (`gfx::adapter`) and a crash-report hook (`crash::on_report`) | render → telemetry, bench | #13060, merged |
| M4 | Cosmetics leave `horizon-client-render`: the tab list's mark through a name-decoration callback (#13094); the settings lines leave with `extras.rs` and `extras_ui.rs` in M5, as clients of the UI page API | render → cosmetics | #13094, merged; the rest in M5 |
| M5 | Horizon's in-game parts (extras, cosmetics, emotes, Discord, Friends, ping, benchmark upload, official server) leave `horizon-client` for Horizon crates linked into the official build through platform hooks | client, client-lib → cosmetics, friends, telemetry, bench | started 2026-11-09 |
| M6 | The engine crates stop depending on platform crates: the renderer's shader-pack, pack-format, HDR and theme code moves behind engine extension points; the mod host and agent control move to platform crates; controllers, splitscreen, replay recording and the UI page API move to platform crates on the engine's input-source, compositor and screen hooks; the launcher and `horizon-server` reach the engine through platform API (done, #13905) | the engine → platform and Horizon → engine exceptions | started 2026-11-09; HDR's part with NikitaMGrimm, below |
| P | Optional, longer: gameplay into the portable crates. `Game` holds the world and its presentation (renderer, screens, particles, sound): split its state module by module, turn gameplay's direct particle and sound calls into level events as vanilla's server sends them, then move each clean module into a portable crate | | any time, small PRs |
| R | The Horizon layer moves to its own repository and ships in every official download (the table above) | | after the split, Ludovic's call |

HDR in M6: moving HDR behind engine extension points moves the frame pipeline (float targets, tone mapping,
present). NikitaMGrimm leads HDR, so that part is planned with them and comes after HDR's step 5 (float targets on
by default), so the pipeline moves once. HDR off stays bit-exact.

The mod API work on arismoko/horizon lands upstream as part of the platform. The API owns its types,
`horizon-mod-runtime` gets no game dependency, and its CI property check stays as a stricter check inside the
platform.
