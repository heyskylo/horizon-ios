# Mods from inside the game, loaded and unloaded in seconds

Asked by Ludovic on 2026-11-05: players add mods from inside the game, and turn them on and off in a few seconds,
without a restart wherever one can be avoided. This note says what can change live, what needs a world reload or a
restart, how the in-game browser installs, and which native-mod path gets a code change from "minutes, then
restart" to seconds. It builds on the registry v2 ([registry-v2.md](registry-v2.md)), the content mods of
[mods.md](../mods.md), the native mods of [native-mods.md](../native-mods.md) and the plan in
[#7386](https://github.com/horizon-rust/horizon/issues/7386).

## Where things stand

| | Install | Turn on or off | Change |
|---|---|---|---|
| Content mod (`.hmod`) | launcher, or by hand in `horizon-mods/` | Options > Mods, then **restart** | rebuild the `.hmod`, restart |
| Native mod (`.hnmod`) | launcher only | off: at once, if the mod checks `native_on`; on: **rebuild** | **rebuild** (minutes), restart |

Three facts from the code shape the plan:

- A pack reload already runs `mods::content_root` with the mods that are on (`stage_pack`), so a switch followed by
  a resource reload already swaps the mods' **assets** (models, textures, sounds, lang). Nothing else follows.
- The server's data tables (recipes, tags, loot tables, advancements, functions, data maps) are rebuilt by `/reload`
  (`reload_world_tables`), from `options.resources`, and sent to the players (`lan::reload_sync_data`). A mod switch
  never reaches them.
- The content root is one folder, `.horizon-cache/mod-content`, rebuilt **in place** when the mod set changes. A
  running game whose `Resources` were built from the old root then reads tables of the new one. Today that happens
  only between a switch and the restart the page asks for; with live switching it would happen every time.

## Content mods: what can change live

A content mod brings four sorts of thing. They differ in who holds them and whether a save names them by number.

| What | Held by | Saves name them | Live? |
|---|---|---|---|
| Assets: block and item models, textures, sounds, lang, fonts, particles, entity models | the client's `PackResources`, atlas, audio, `lang` | no | **live**: the pack reload (1 to 3 s) |
| Data: recipes, tags, loot tables, advancements, functions, predicates, item modifiers, data maps | the server's `WorldTables` | no | **live**: `/reload`'s path, players resynced |
| Worldgen: biome modifiers, structure set, pool and processor additions, features | the world's generator, built when it opens | no, but chunks already generated keep what was there | **world reload**; only new chunks change |
| Registries: blocks and their states, items, mob effects, entity types, menu types, component types | `Resources` (`BlockFacts`, `Items`), process-wide tables (`MOD_EFFECTS`, `THROWN_TYPES`, mod menu types, `item::custom`) | **yes**: chunk palettes, inventories, effects, entities | **world reload**, later; **restart** today |

Vanilla draws the same line. `/reload` reloads a server's data packs (`ReloadableServerResources`), and F3+T reloads
the client's resources. A change to the registries a world was created with (a new feature flag, a data pack with
worldgen) goes through the world's **Data Packs** screen and reopens the world.

### Live: assets and data

A switch of a mod whose content is only assets and data applies at once, in the world or on the title screen:

1. **A root per mod set.** `mod_content::resource_root` builds into `mod-content/<stamp hash>/` instead of one
   folder. A running game keeps reading the root it was given, and a switch never changes files under it. Roots
   that are not the current one and are older than a day are removed when the game starts, so the cache holds one
   root, or two after a switch.
2. **Classify the switch** before applying it: build the new mod list (`accept_mod`, as `content_root` does) on a
   worker, and compare what each side **registers** (the ids of blocks and their declared state properties, items,
   mob effects, entity types, menu types, and the native component types). Equal registries mean the switch is live.
   Otherwise it needs a world reload (below), and the page says so on the mod's row before anything changes.
3. **Client.** The pack reload already builds the new root and swaps the assets. It runs under its progress overlay;
   the world keeps drawing.
4. **Server.** The server thread gets the new root (`options.resources`) and runs `reload_world_tables`, as `/reload`
   does: tables rebuilt, recipes and tags sent to the players, advancements reloaded. A LAN host's guests and the
   window's own client get the same packets they get after `/reload`.
5. **Multiplayer.** On someone else's server, the client never changes data: only assets switch live, and a mod that
   adds registries can't be turned on while connected ("Leave the server to turn this mod on"). On a LAN host, a
   live switch resyncs the guests; a registry change is refused while guests are connected, as it would disconnect
   them.

What a player keeps: their world, position, inventory and open screen. What they see: the overlay of a resource
reload, then the new textures and recipes. A mod turned off leaves its recipes out of the recipe book at once; its
items stay where they are only if a mod still registers them (otherwise the switch was not live).

### World reload: registries and worldgen

Blocks and items have numbers. Chunks store block states as palette entries, inventories store item ids, and the
client meshes sections against `BlockFacts`. Adding a block shifts nothing (mod ids come after vanilla's), but a
running world's tables, caches and connections all hold the old numbering, so a registry change reopens the world:

1. Save and leave the world (as **Save and Quit**).
2. Build the new `Resources` on a worker (`load_resources` from the new root), as the game does at start.
3. Swap `Game.res` and reload the client's resources.
4. Reopen the same world, at the same place.

The cost is a world's load, about what the player waits for when opening it from the title screen. Two things keep
this step out of the first PRs:

- **Process-wide tables are set once.** Mob effects (`potions::MOD_EFFECTS`), thrown entity types
  (`projectiles::THROWN_TYPES`), mod menu types, living entity types and native component types
  (`horizon_protocol::item::custom::install`) are `OnceLock`s, filled at start and read without a lock on hot
  paths. They become `ArcSwap`-style tables replaced between worlds, never while one is open.
- **A mod turned off whose blocks are in the world.** Opening a world without a mod already drops its effects and
  keeps its entities frozen ([mods.md](../mods.md)), and its blocks get what opening the world without it does today. The world reload asks
  first, with the count of the mod's blocks and items it found in the loaded chunks, and makes a backup (the world's
  `backups/` copy) before it reopens without them.

Until then, a registry change keeps today's rule: the row says "Restart the game to apply", and the game asks
whether to restart now. A restart from the game is the launcher starting it again with `--world <name>`, so the
player lands back in the world.

### What "a few seconds" means here, measured

Measured in game on the ThinkPad (i7-7500U, Intel HD 620, Vulkan, `profile.fast`; the desktop can't run the game
over ssh) with the fixture mod of #10217: a recipe, an item tag, two functions and a language file. Four runs, on
2026-11-05:

| Step | Time |
|---|---|
| Turn on the first time: build its content root, then reload the client's resources | 3.2 to 5.6 s |
| Turn off, or on again (the root is reused) | 1.1 to 1.7 s |
| Then the server's data reload (`reload_datapacks`, as `/reload`), on its own thread | 0.9 to 1.7 s |

So a switch lands in about 2 to 3 s, and the first time a mod set is used in about 4 to 7 s. The root alone, on
the desktop (i7-13700KF, NVMe, under a load of 30 to 40), in #10217's test: built in 5.1 to 5.4 s, reused in 0 to
19 ms, over three runs. Most of a first switch is building the root; a set used before costs only the reloads.

## The in-game mod browser

Options > Mods gets a **Browse mods** button beside **Open mod folder**. It opens Horizon's own menu pages, drawn
with the options screen's widgets (`options_ui`), every string through `Text::tr` keys in
`assets/horizon/lang/*.json`.

- **The index.** The same file the launcher reads (`horizon-mods` `index.json`, `HORIZON_MOD_INDEX` for a test
  registry), fetched on a worker with the game's `ureq` when the page opens (a Pages file, not the API: no rate
  limit), kept in `mod-browser/horizon-index.json` beside `options.txt`. The launcher and the game share that copy,
  so an offline game still lists what the launcher saw last. Parsed with `horizon_mods::index::Index::parse`, which
  refuses an index with a bad entry as the launcher does.
- **The list.** A search box and the projects this game can install, sorted by downloads: name, author, summary,
  its categories, and **Installed**, **Update** or nothing. A search matches the name, id, summary, tags and
  categories.
- **A project's page.** Name, authors, licence, version and date, categories and tags, the summary and description
  (the index's markdown read as plain text: headings, lists and links shown as text), its changelog, and buttons:
  **Install** or **Update**, the mod's on and off switch once it is installed, and **Open page** (the site's page in
  the browser). **Remove** stays in the launcher for now. Screenshots come in a second step: the registry mirrors
  them as PNG and JPEG under its own Pages ([registry-v2.md](registry-v2.md)); the page will draw them through the
  same dynamic-texture path as player skins (`skin_slots`), one at a time, loaded when the page opens.
- **Install.** As the launcher's `install_version`: the newest version for this game that was not taken down, into
  `mod-browser/downloads/`, then `horizon_mods::index::check_file` (size, SHA-256, the author's signature, the
  file's manifest against the index), then moved to `horizon-mods/<id>.hmod`, turned on in `horizon-mods.txt`, and
  recorded in `mod-browser.json` with the launcher's own fields, so the launcher lists it, updates it and removes
  it. A file the index took down (`revoked.toml`) is refused. A `.hmod` with code (`needs_port`) is not offered.
- **Kinds.** Content mods first. Resource packs, shader packs and data packs follow with the same page; each installs
  where the launcher puts it.
- **Native mods.** Listed with the launcher's warning, word for word: they are native code with the player's rights,
  not sandboxed, and the red **Install** button asks first, as the launcher's trust notice does. The game can't
  compile, so it installs the `.hnmod` into `horizon-native-mods/`, and the launcher builds it before the next play
  (#9901). The registry review stays the gate for every kind.

## Native mods: the fast path

A native mod's code is in the game's binary. What can be made fast:

| Option | A mod change takes | On and off | Unload | Risk |
|---|---|---|---|---|
| **Today**: the launcher rebuilds the game (`profile.fast`, incremental, warm `native/target`) | see measurements | off at once (if the mod checks `native_on`), on: rebuild | none | none |
| **A. Every installed mod built in, on and off at run time** | rebuild only to install, update or remove | **at once**, for trait mods by dispatch | the mod's hooks stop; its state stays until restart | low: one check per dispatched mod |
| **B. Background build while playing, then a quick swap** | build while playing, then a restart into the same world | as A | restart | low; a second process and a hand-over |
| **C. Rust `dylib`s** (#7386 stages 3 and 4, arismoko's #797) | that mod only, under 30 s (the spike's pass mark) | restart | **never**: a Rust `dylib` can't be unloaded soundly | medium: exact-build pinning, Windows export limit |
| **D. A C ABI or `abi_stable` plugin layer over `native-hooks.json`** | that mod only, seconds | at run time | possible, with care | high: a second API to keep beside `mod_api`, and still not sandboxed |
| **E. wasm mods** (arismoko's fork: `horizon-mod-wasm`, wasmtime, one API with native) | that mod only, seconds | at run time | **yes**: drop the instance | medium: performance of the hooks it covers, API coverage |

### What each costs, and the choice

- **Unloading native code.** Rust code loaded with `dlopen` can't be unloaded soundly: a `'static` reference, a
  thread it spawned, a thread-local destructor or a `Box<dyn Fn>` it handed the game outlives `dlclose`, and the
  next call jumps into unmapped memory. A sound "unload" of native code is "stop calling it, keep it mapped, free it
  at exit". Only a sandbox frees a mod's code and state at once: wasm's instance is dropped with its memory, and the
  game never holds a pointer into it because every call goes through the host's table.
- **D is not worth a second API.** A stable C ABI over the hook catalog duplicates what arismoko's fork already
  generates for wasm from the same catalog (the `mod_api` crate builds for `wasm32-unknown-unknown`, and its
  interface is generated at build time). It would give native speed and run-time loading, but no sandbox and no
  sound unload. We don't build it.
- **C and E are arismoko's work, and we build on it.** The fork has the `dylib` loader (#797), its acceptance harness
  (#798) and the wasm runtime. They merge upstream through their own PRs; Horizon's side is to keep the hook catalog
  and `Hooks` dispatch shaped so both can sit behind it.
- **A is the biggest win for the least risk, now.** Turning a native mod on or off is the common case (trying a mod,
  finding a conflict, a modpack switch), and it needs no compiler at all while the mod is in the build. Step one
  (#10218): the game skips a turned-off trait mod in `Hooks` dispatch (`mod_api`), so a mod that was on at launch
  stops at once without its author doing anything, and starts again at once when turned back on. `RenderHooks`
  run every frame over a plain list, so they are not switched yet, and mixin mods keep honouring `native_on` (they
  paste code into the game's own functions, so only they can check). Step two: the launcher builds every installed
  mod, whatever its `mod.<id>` line says, so a mod that was off at launch also turns on without a build; it waits
  for step one to settle, because a build with every installed mod costs players who keep many mods turned off.
- **B comes next.** With A in, a build is needed only to install, update or remove. The launcher already rebuilds
  before play (#9901); B lets the player keep playing during it, and then restarts into the same world.

### Measured

On the desktop (i7-13700KF, `-j 8`, Linux, `rustc` 1.99, the launcher's `profile.fast`, incremental), building
`horizon-client` as the launcher does, on 2026-11-05 while other builds kept the machine at a load of about 30 to 40
(an idle machine is faster; the ratios are the point):

| Build | Time |
|---|---|
| Cold, empty target | 812 s |
| No change | 55 s |
| One line changed in gameplay (`game/mods.rs`), as a mixin into gameplay does | 570 s (the revert: 668 s) |
| One line changed in `horizon-world`, as a mixin into a lower crate does | 427 s (the revert: 469 s) |
| The gameplay change, linked with mold | 612 s |

Mold changes nothing: the time is spent compiling the gameplay crate again, not linking it. A native mod's change
costs most of a cold build, so the run-time switch (A) is what makes on and off fast, and only a smaller gameplay
crate or code outside the game's binary (C, E) makes a change fast.

## Security

Nothing in this note widens what a mod can do.

- Content mods have no code. The game checks every downloaded file against the index exactly as the launcher does,
  and keeps the content fence ([mods.md](../mods.md#namespaces-and-limits)).
- Native mods stay full-trust native code. The in-game browser shows the launcher's trust notice and red warning
  before installing one, and installing never builds or runs it: the launcher does, behind its own warning.
- The registry review stays the gate for listing a project, `revoked.toml` turns off taken-down files in the game
  as in the launcher, and SHA-256 pinning and signatures are checked before a file reaches `horizon-mods/`.
- The browser fetches only the index, the registry's release assets (`horizon_mods::index::FILE_HOST`) and, later,
  the registry's own mirrored images. It never follows a project's own links.

## Plan, small PRs

1. This note.
2. The in-game browser and install, for content mods (#10104).
3. Live switching of content mods (#10217): a root per mod set, classification, the client and server reloads,
   tests that load, unload and load again a fixture mod and check the recipes, tags and lang it adds. Measured in
   game on the ThinkPad, the root alone on the desktop.
4. Native step A, first half (#10218): a trait mod built in turns off and on at run time through `Hooks` dispatch. Then
   the launcher builds every installed mod, and `RenderHooks` get the same switch.

Later: the world reload for registry changes (replaceable process-wide tables), screenshots on the project page,
the other kinds in the browser, and B.

## From the contributors' plans

What this note takes from them:

- From arismoko's fork ("deslopping the mod API"): the `dylib` loader (#797, #798) and the wasm runtime are the
  run-time loading paths; mod-defined data pack registries reloaded on `/reload` (their #811) fit the live data
  reload above as they are; one dependency model for native and wasm.
- From #7386: no unload for Rust `dylib`s, the spike's pass mark (under 30 s for a one-line mod change), and the
  static build kept for a settled mod set.
- From levzzz5154's horizon-mixins: mixins stay for what has no hook; option A keeps every mixin mod working,
  turned off only through `native_on`.
- From Vaspyyy's #9866: the browser says before install whether a native mod builds on the player's Horizon; the
  in-game list shows the same verdict.

What we'd ask them:

- **arismoko:** Should option A's dispatch gate (skip a turned-off mod's `Hooks`) go in the generated dispatch
  upstream now, or does your fork's dispatch already have a per-mod switch we should take instead? When the wasm
  runtime lands upstream, can a wasm mod's instance be dropped while a world is open, so its switch is live too?
- **levzzz5154:** Could horizon-mixins wrap each mixin's injected code in a generated `native_on(<mod id>)` check,
  so a mixin mod turns off at run time without its author writing the check?
- **Both:** Is a content root per mod set (`mod-content/<hash>/`) in the way of anything you load from it?
