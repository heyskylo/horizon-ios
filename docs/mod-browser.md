# The launcher's mod browser

Search Modrinth (CurseForge support is coming soon) for Java mods, modpacks, resource packs and data packs, and Horizon's
registry for Horizon's own mods, modpacks, resource packs and shader packs; see how well each one runs before installing it, and install it with its checksum checked. This is item 16 of the roadmap and issue #577. The code is in
`launcher/src/browser.rs` and `launcher/src/browser/`.

## What it offers

Horizon doesn't run Java code ([mods.md](mods.md)). It ports what a Java mod
carries as data (blocks, items, recipes, loot tables, tags, worldgen, textures, names) with `horizon-port-pack`, and
says before install which of the mod's features that leaves out. Horizon's own mods (content mods, `.hmod`, and native
mods, `.hnmod`) aren't on Modrinth or CurseForge yet (Modrinth has no `horizon` loader).

| Kind | Goes to | How well it runs |
|---|---|---|
| Java mods (Fabric, NeoForge, Forge, Quilt) | ported into `horizon-mods/<mod id>/` and turned on | Checked from its jar before install: **Works**, **Partly** or **Doesn't run** ([below](#java-mods)) |
| Resource packs | `resourcepacks/` beside `options.txt` | **Works** when made for 26.3 ([resource-packs.md](resource-packs.md) lists what Horizon reads). Older packs are **Partly**: they load with vanilla's compatibility warning |
| Modpacks (`.mrpack`, CurseForge zip) | the Modpacks page's import | **Partly**: Horizon runs the mods it can port, from the player's own downloads ([modpacks.md](modpacks.md)) |
| Shader packs (`.hshader` and `shaders/`-format zips, Horizon's registry only) | `shaderpacks/` beside `options.txt` | **Works** when made for 26.3: checked against the registry's index before install (a `.hshader` by the game's own check, [shaders.md](shaders.md)). The `shaders/`-format packs on Modrinth and CurseForge are not offered |
| Data packs (Horizon's registry, Modrinth) | `datapacks/` of the world picked on the **Data packs** tab (`mods install <pack> --world <world>`); not in the Installed list: each world's list on Home has them | **Partly**: Horizon runs a data pack's functions, function tags, predicates and item modifiers, not yet its recipes, loot tables, advancements, other tags or worldgen. The install reads the zip and says what of it runs; a pack with none of those (worldgen only, such as Tectonic) is refused |

Mods, resource packs and shader packs go into the game folder of the instance selected in the sidebar, and the
Installed list, updates and Remove are that instance's: picking another instance reads its list. The page says which
instance it installs into when there is more than one. The CLI's `mods list|install|update|remove|enable|disable` take
`--instance <name>` (default: the selected one). A modpack becomes an instance of its own.

By default a search shows only what runs: Java mods made for 26.3, resource packs made for 26.3, and every modpack. **Show everything** lists the rest of the kind too, each marked with how well it runs; what doesn't run is
never offered for install.

## Java mods

Before a mod is offered for install, the launcher reads its jar. For each result it picks the version it would
install (the newest for 26.3; with **Show everything**, else the newest for 1.21 or later, the versions port-pack
upgrades from), downloads that file with the checks below, and runs `horizon-port-pack check` on it
([modpacks.md](modpacks.md#the-pipeline-horizon-port-pack)): the jar is carried over into a folder content mod and its
class files are read, not run, for what the carry can't take. Two checks run at once; the row says **Checking** until
its answer comes.

- **Works**: content only. Its blocks, items, recipes, loot, tags and worldgen port, and its code only registers
  them. Blocks take the closest vanilla block's shape, hardness and sounds (a block's numbers live in Java code; the
  NeoForge oracle that reads them for modpacks is a developer tool).
- **Partly**: the content ports; the row lists what doesn't: blocks with no vanilla block to copy, recipes of the
  mod's own types, block entities (machines, containers), menus, mobs and entities, blocks and items with behaviour
  of their own, worldgen written in Java, Mixins, the game events it reacts to.
- **Doesn't run**: nothing to port (performance mods, libraries, everything in code), a jar made for 1.20 or older,
  or a mod id with characters Horizon mods can't use (`fabric-api`). Never offered for install.

The answer is kept in `mod-browser/checks-1/<checksum>/` under the game folder, with the carry when something
ports; the jar itself isn't kept. A file is checked once. On ten 26.3 jars a check took 0.2 to 3 s (24 s for an
18 MB jar with 9,360 recipes).

**Install** copies the carry into `horizon-mods/<mod id>/`, turns it on in `horizon-mods.txt` (the game's Options >
Mods lists it) and records it in `mod-browser.json` with its verdict and what didn't port, which the Installed list
shows. Its required mods that port are installed with it; one with nothing to port (a library such as Cloth Config
or GeckoLib) is listed as not ported. A mod that shares its id or a namespace with an installed one is refused. The
game reads mod content when it starts: start it again after installing. **Remove** deletes the folder and turns it
off.

### End-to-end check

`tools/mod_browser_harness.py [--mod SLUG] [--query TEXT] [--keep]` runs the whole path a player takes, in a scratch
game folder: the launcher's `mods search --kind mod`, `mods check` and `mods install` (Modrinth), then the game
opens a survival world with the mod offscreen (horizon-ctl), places one of its blocks, and crafts one of its recipes
from vanilla ingredients on a crafting table and takes the result. It times the world's load with and without the
mod and prints a JSON summary; it exits 1 when a step fails. It needs the launcher, the game and horizon-ctl built
and the prepared 26.3 resources in `.local`.

On the Mac mini (M4 Pro, offscreen) with Macaw's Fences and Walls (the default): check 4.0 s, install 3.1 s, world
load 5.4 s without mods and 7.3 s with it (resources 1.1 s and 2.6 s: the mod content folder is built on the first
start); the hedge places and its recipe crafts 4.

## Checks on every download

- The file must come from Modrinth's or CurseForge's file servers (`cdn.modrinth.com`, `edge.forgecdn.net`,
  `mediafilez.forgecdn.net`), over HTTPS.
- Its name must be one plain file name (no folders, `..`, hidden names or reserved characters).
- It is written as `.<name>.part`, hashed as it arrives, and renamed into place only when its size and checksum match
  what the source lists: SHA-512 on Modrinth, SHA-1 on CurseForge (the strongest hash CurseForge publishes). Anything
  else removes the part file and installs nothing.
- A file whose source lists no checksum is refused.

## The Mods page

The launcher's **Mods** page has the five kinds as tabs: **Mods**, **Modpacks**, **Resource packs**, **Shader packs**
and **Data packs**, each with a search field (Enter searches). Mods, Modpacks, Resource packs and Data packs pick their
source: Horizon's registry or Modrinth, with **CurseForge (coming soon)** beside them, disabled. Mods start on
Horizon's registry; **Shader packs** search it only (Horizon's own packs); data packs install into the world picked
under the search. On Horizon's registry, **Filters** lists the kind's categories (`horizon_mods::index::page::categories`):
a result has every category picked; the search words also match a project's tags. A project's page shows its
categories. A Java instance's tabs always search Modrinth for its version and
loader, with the version and loader filters. A Horizon instance searching Modrinth's Java mods sees them all, each
marked **Works**, **Partly** or **Doesn't run in Horizon** once its jar is checked; **Show everything** adds what was
made for other Minecraft versions. Each result is an index row: its summary, author,
downloads, source and licence, how well it runs (and why), **Page** (the project's page on its source) and
**Install** (**Import** for a modpack). The Modpacks tab lists the imported packs first, with their coverage
report, **Port** and **Play** ([modpacks.md](modpacks.md)). Installed packs are listed above the results with their version, source,
file and the checksum they were checked against, with **Update to …** when a newer version runs and **Remove**
(asked twice). **Update all (N)** above the list updates every pack a newer version was found for, of every kind,
one after the other; one that fails is named and doesn't stop the others.

**Automatic updates** (Settings > Game > **Update registry mods automatically**, on by default): when the launcher
starts (every Horizon instance) and before Play (the instance it starts), what was installed from Horizon's registry
moves to its newest version for the Minecraft that version is for and the Horizon that plays it
(`Index::installable`), checked as an install is (`browser::horizon::auto`). Native mods update at the start only, and
the next Play first builds the game again with them (the Mods page's Build, `build_native`), then plays it. Never
updated automatically: a modpack's profile (its `.hpack` pins each mod by SHA-256; the pack updates as a whole when the
player asks), native mods of an instance that keeps a Nightly or a native build made on one, data packs (they live in a
world), a version for another Minecraft, and anything older than what is installed. A content mod (`.hmod`) whose new version no longer has a block, item, entity, effect, menu or creative tab
of the installed one waits for the player, and the page names what it would drop. The file an update replaces is kept
in `mod-browser/replaced/` until the next start, which puts it back if the update was cut short.

### Project pages, filters and icons

A result's title opens its page inside the launcher, not in a web browser: its icon, authors, downloads, followers,
last update, licence and source, how well it runs (for a Java mod, what ports and what doesn't, from its check) with
**Install** or **Import**, the project's links (its page on the source, source code, issues, wiki, Discord,
donations; `https://` only, opened in the system browser), then three tabs: **Description** (the project's Markdown,
drawn by the launcher's renderer), **Gallery** (one image large, a click shows the next; the arrows and the strip of
thumbnails under it pick another; Modrinth's gallery, or the screenshots a registry project lists, which the registry
mirrors beside its icons; no tab when there are none) and **Versions** (each version's date, loaders, game versions,
file and size, and the verdict of any version already checked). The data is `browser::details` (`Source::details`; on
Modrinth `GET /project/{id}` and `/project/{id}/members`, then `/project/{id}/version`).

**Filters** sit above the results: the order (best match, relevance, downloads, follows, newest, updated) and, under
**Filters**, the loader, game version, environment (client, server) and categories, with the values the source lists
for the kind (Modrinth's `/tag/category`, `/tag/loader`, `/tag/game_version`, asked once per run). A loader or version
picked replaces the default (Java mod loaders and 26.3 for mods); a version other than 26.3 checks and installs the
newest file for 1.21 or later, as **Show everything** does. Changing tabs clears the filters. CurseForge support
is coming soon; its source already maps the order and the game version.

**Icons and gallery images** are the projects' own, fetched at run time from the source's image server
(`browser::images`): HTTPS from `cdn.modrinth.com` (or CurseForge's `media.forgecdn.net` / `edge.forgecdn.net`) only,
at most 8 MB each, decoded (PNG, JPEG, WebP, GIF, at most 8192 pixels a side), scaled (icons 128 px, list rows 80 px,
gallery 960 px) and kept as PNGs in `mod-browser/images/` under the launcher's data folder, which is kept under 128 MB
by dropping the oldest files. After a week an image is asked for again with its server's `ETag` and `Last-Modified`:
an unchanged one costs a 304 and is kept; offline, the cached one stays. A plate with the project's initial stands in
until the icon arrives. Nothing of a project's art is in the launcher or the repository.

**Installed mods' icons** (Home's Mods tab, the Mods page's installed list): Modrinth's project icon for what the
browser installed from it, the URLs asked for in one request (`GET /projects?ids=[…]`) and kept in
`mod-browser/project-icons.json` so they show offline; else the icon a jar names for itself
(`fabric.mod.json`/`quilt.mod.json` `icon`, `mods.toml` `logoFile`), read from the jar off the UI thread and scaled to
the row; else a generic glyph. The tab builds only the rows near the view, so a pack of hundreds costs what a few do.

**Vanilla Minecraft** is marked with Horizon's own grass block (`launcher/assets/logos/grass-block.png`, drawn by
`tools/launcher_grass_block.py` from our palette, not from Mojang's textures): the game-version filter, the Java Edition
tab and plain Minecraft instances show it. No Mojang file or logo is in the launcher or the repository.

**Source logos.** Modrinth's logo is not shown: Modrinth's own repository says its branding is all rights reserved and
"the use of Modrinth branding elements, including but not limited to the wrench-in-labyrinth logo, ... is strictly
prohibited without explicit written permission from Rinth, Inc." (`packages/assets/COPYING.md` in
[modrinth/code](https://github.com/modrinth/code/blob/main/packages/assets/COPYING.md)), and its
[terms](https://modrinth.com/legal/terms) ask for prior written permission to use its marks. Results name their source
in text ("from Modrinth") until Rinth, Inc. agrees. CurseForge support, and so its logo, is coming soon.

## Installing, updating, removing

`mod-browser.json` beside `options.txt` lists every file the browser installed: its source, project, version, file
and the checksum it was checked against. The browser replaces or removes only the files it lists; a file of the
same name the player put there is never overwritten.

- **Install** picks the newest version that runs, then its required dependencies, checked before any download: a
  dependency that isn't a pack Horizon loads (a Java mod) stops the install. Dependencies go in first. A resource
  pack still has to be turned on in the game's Resource Packs screen, as in vanilla, and a shader pack picked in
  the game's Shader Packs page.
- **Update** installs the newer version and removes the old file.
- **Update all** does the same for each pack with a newer version (`horizon-launcher mods update` with no pack name).
- **Remove** deletes the file and its entry. Packs installed as dependencies stay.
- **A modpack** is downloaded (checked the same way) into `ports/downloads/`, imported, and ported straight away
  for the selected version when it is prepared (`modpacks::import_and_port`): the jars of the mods Horizon can carry
  over come from the pack's links or Modrinth, and the message says how many of its files play. **Play** on the
  Modpacks tab starts it with every ported mod on together. A ported mod that owns a namespace an earlier one owns
  (the game would refuse it) is left off before the game starts and listed under the pack ("Left off: ..."); the
  coverage list says why each mod that didn't port didn't. A port that fails leaves the pack imported, with **Port**
  to try again. Measured on three packs in [modpacks.md](modpacks.md#three-packs-through-the-mod-browser-measured).

From a terminal (or an agent):

```sh
horizon-launcher mods search faithful                 # --kind mod|modpack|resource|shader, --everything, --source
horizon-launcher mods check macaws-fences-and-walls   # a Java mod's verdict, what ports and what doesn't (--json)
horizon-launcher mods install faithful-32x            # id or slug; a Java mod is ported, a modpack imported and ported
horizon-launcher mods list                            # --json
horizon-launcher mods update [faithful-32x] [--check]
horizon-launcher mods remove faithful-32x
```

## Sources

**Horizon's registry.** Content mods (`.hmod`), native mods (`.hnmod`), modpacks (`.hpack`), resource packs and shader packs (`.hshader`) made for Horizon, reviewed as pull requests in
[horizon-rust/horizon-mods](https://github.com/horizon-rust/horizon-mods) ([api/mod-index.md](api/mod-index.md)). The
launcher reads its `index.json` (`browser/horizon.rs`; `HORIZON_MOD_INDEX=<file or https address>` reads a test
registry instead, whose files may then be on disk) at most every ten minutes, and keeps the last copy in
`mod-browser/horizon-index.json`. Before a result is installed it shows whether its files are signed and, for a mod,
what it may do, in the words of the game's Options > Mods. A download goes to `mod-browser/downloads/` and is moved
into place only after `horizon_mods::index::check_file` passes (size, SHA-256, the author's signature when the project
has a key, the file's own manifest against the index). A content mod becomes `horizon-mods/<id>.hmod`, turned on; a native mod becomes
`horizon-native-mods/<id>.hnmod`, and the launcher builds it into the game ([native-mods.md](native-mods.md));
`mods disable` and `mods enable` switch it in `horizon-mods.txt`. When the index takes a file down, the launcher turns
it off the next time it reads the index or starts the game, says why, and refuses to install it or turn it back on.
A Horizon modpack (`.hpack`) installs as its own profile (`browser/horizon/pack.rs`): `horizon-packs/<id>/` holds the
checked pack, `pack.json` and `home/`, the profile's game folder, where each of the pack's mods is downloaded and checked
like any registry mod and turned on or off as the pack says; its overrides (settings, resource packs, a mod's saved
settings) are copied in on the first install only, so the player's changes survive updates. A pack whose mod was taken
down installs nothing. It is listed on the Modpacks tab with **Play** and **Remove**, and plays like an imported pack
(`modpacks::apply_profile`). A shader pack goes to `shaderpacks/<file>` once `check_file` has run the game's own
check on it (`horizon_shader_api::check`); a taken-down one is named, for the player to remove.
A registry `.hmod` with any capability besides `registries` is neither listed nor installed alone. A modpack that
requires one still installs, and the launcher lists the mod as not running, with that reason, and leaves out its code.

```sh
horizon-launcher mods search --source horizon --kind mod <words>   # with whether it is signed and what it may do
horizon-launcher mods install --source horizon <id>
horizon-launcher mods disable <id>
horizon-launcher mods install --source horizon cozy               # a modpack: its own profile
horizon-launcher modpack play cozy
```


**Modrinth.** The public API (`api.modrinth.com/v2`) needs no key. The launcher names itself in its user agent, as
Modrinth asks.

### CurseForge (coming soon)

CurseForge support is coming soon. Its API (`api.curseforge.com/v1`) needs a paid key, so for now the launcher
doesn't use it: the Mods page shows **CurseForge (coming soon)** beside the Modrinth switch, disabled, and no search
or install goes to CurseForge (`browser::sources()` is Modrinth only; `horizon-launcher mods ... --source curseforge`
says it is coming soon). The CurseForge source (`browser/curseforge.rs`) is kept for when it comes.

A CurseForge modpack zip or instance the player already has still imports, since that needs no key: its jars come
from the player's installed instance or, when Modrinth has the same file, from Modrinth. A jar only CurseForge serves
is listed under the pack as "CurseForge downloads coming soon".
