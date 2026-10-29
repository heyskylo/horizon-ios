# Version ports: from a new Minecraft release to Horizon

Goal: every Minecraft Java release is turned into Rust by Horizon with as little human work as possible. A new
release is detected, downloaded and prepared on the Mac mini, compared with the version Horizon pins, everything
data-driven is regenerated, every parity harness runs against it, and what is left is a ranked list of agent-sized
work items, each a GitHub issue and a Kira card. This page is the design; `tools/version_port.py` implements it
(status at the end).

The downloads are for development only, on the Mac mini: nothing from them is ever committed, published or shipped
to players. Players still prepare their own copy from their official launcher (`horizon-prepare`), as today.

## How Horizon pins and prepares 26.3 today

- **The pin.** `research-lock.json` `minecraft`: the version id, the sha1 of Mojang's version JSON (and of the
  republished ones it accepts), the client and server jar sha1s, the Java major version. Vineflower, REA and JADX
  are pinned beside it.
- **The vanilla data.** `tools/lab.py prepare` imports the client jar, the server jar and the client libraries from
  the user's own launcher install, unpacks the server bundler into `server-game.jar`, and writes the class
  inventories; `tools/lab.py decompile` runs Vineflower on both jars. On the Mac mini all of this lives once in
  `~/code/horizon-parity/shared-local/` (`minecraft/26.3/{jars,libraries,sources,reports}`, 366 MB), and every
  worktree links to it from `.local/`. Release builds never do this: `horizon-prepare` imports the user's copy.
- **The data generator.** `horizon-prepare` runs `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar
  --reports` into `.local/datagen/out/reports/`: `blocks.json`, `registries.json`, `packets.json`,
  `commands.json`, `datapack.json`, item default components, biome parameters. It also runs Horizon's Java
  exporters (`mods/horizon/src/test/java/dev/horizonlab/parity/Vanilla*Exporter.java`) against the jar for block
  behaviours, worldgen bundles and the like. CI does the same through `tools/ci/vanilla-data.sh`, keyed by the
  version in `research-lock.json`.
- **Version facts in the code.** The client jar's own `version.json` states them (26.3: world version 5023,
  protocol 777, resource pack 97.1, data pack 121.0, Java 25). `tools/gen_version_consts.py` writes them into
  one generated file, `crates/horizon-core/src/version.rs` (`VANILLA_VERSION`, `PROTOCOL_VERSION`,
  `DATA_VERSION`, the pack formats, Java, `/version`'s build time), and every Rust site reads them from there
  (`horizon_protocol::PROTOCOL_VERSION`, `horizon_world::DATA_VERSION` and the save's version name are aliases
  of it; the status, the legacy ping, the crash report, `/version`, the title screens and the default resource
  folder use it). CI checks the file against the pinned jar (`--check`). Test fixtures that state a version
  (a level.dat to read, a relay message) keep their literals: they test parsing, not the pin.

- **Generated tables** (each has its generator in `tools/`):

  | Output | Generator | Input |
  | --- | --- | --- |
  | `crates/horizon-core/src/version.rs` | `gen_version_consts.py` | the client jar's `version.json` |
  | `crates/horizon-protocol/src/ids.rs` | `generate_packet_ids.py` | `packets.json` report |
  | `crates/horizon-client/src/lan/tracking.rs` | `generate_entity_tracking.py` | decompiled `EntityTypes` |
  | `crates/horizon-client/src/lan/command_suggestions.rs` | `generate_command_suggestions.py` | `VanillaCommandSuggestionsExporter` |
  | `crates/horizon-client/src/lan/command_order.rs` | `generate_lan_command_order.py` | vanilla's command tree |
  | `crates/horizon-command/src/vanilla_order.rs` | `generate_command_order.py` | command-parse harness export |
  | `crates/horizon-world/src/datafix/block_state_data.txt` | `gen_block_state_data.py` | decompiled `BlockStateData` |
  | `crates/horizon-world/src/datafix/flat_tables.rs` | `gen_flat_tables.py` | 1.13 flattening fixers (never change) |

  Everything else data-driven (block behaviours, items, worldgen, models, sounds, languages) is not committed: the
  game reads what `horizon-prepare` exports from the user's own jar, so it follows a new version by itself as long
  as the exporters still compile and the formats still parse.

- **The proof.** ~300 harnesses (`tools/test_*_parity.py`) each compare a Rust probe with a Java oracle that calls
  vanilla's own classes; `tools/parity.py` runs them and records each one's dependencies.

## The pipeline

```
detect ─► prepare NEW ─► diff OLD..NEW ─► regenerate ─► harnesses on NEW ─► port report ─► issues + cards
 (daily)   (download,     (data, protocol,  (generators   (candidate          (ranked work    (agents port,
           verify,         registries,       per kind)     worktree)           items)          pin bump PR,
           datagen,        assets, classes                                                     release)
           class index)    → Horizon map)
```

`tools/version_port.py` has one subcommand per stage and `port NEW` runs them all. Each stage writes its result
under the version store and can be rerun alone.

### The version store

`~/code/horizon-parity/versions/<id>/` (`HORIZON_VERSIONS` overrides it), next to `shared-local/` and never inside
it:

```
versions/26.4/
  version.json            Mojang's version JSON (sha1 checked against the manifest)
  jars/client.jar         sha1 checked against version.json
  jars/server.jar         the bundler; server-game.jar beside it, unpacked with sha256 checks
  datagen/reports/...     the data generator's --reports output
  classes.json            the class index: every class's methods, each with a normalized bytecode hash
  facts.json              world/protocol/pack/Java versions from the jar's own version.json
  prepared.json           what was made, with which tool versions
versions/diff-26.3-26.4/  the diff, the source diffs of changed classes, the port report
```

About 150 MB a version (jars 120 MB, reports 20 MB, index 10 MB): client libraries are not needed (the data
generator runs from the bundler, which carries its own) and the full Vineflower tree is not kept (only changed
classes are decompiled, below). The store keeps at most three versions, the pinned one (whose files already exist in
`shared-local/`, so it is only indexed, not copied), the previous one and the candidate; `version_port.py gc`
deletes the rest and every scratch directory. Snapshots, if prepared, are deleted when the next one is.

### Detect

`version_port.py detect` reads `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`, compares the
latest release (and, with `--snapshots`, the latest snapshot) with the pin and with
`versions/detect-state.json`, and prints what is new; exit status 0 means "nothing new", 10 "new release",
11 "new snapshot". It needs only the network, so it can run anywhere:

- **Mac mini, launchd (chosen):** the agent `dev.horizon.version-port` (`tools/ci/dev.horizon.version-port.plist`)
  runs `tools/ci/version-port-daily.sh` daily at 16:00 Zurich time (14:00 UTC in summer, 15:00 in winter; Mojang
  publishes on weekdays around 10:00-14:00 UTC), in a dedicated worktree of `origin/main`
  (`~/code/horizon-parity/port-main`), at nice 10 with low-priority I/O (the plist's `Nice` and
  `LowPriorityIO`; `~/fleet/load prio low` would lower the calling agent session instead). The whole port runs where the vanilla
  data, Java and the harnesses are. See "Turning on the daily agent" below.
- **GitHub Actions:** a scheduled job could run `detect` alone and open a "Minecraft X is out" issue; the Mac mini
  would still have to do the rest, so it adds a hop for nothing. Kept as the fallback if the Mac mini is down for
  long.

Snapshots are optional and only ever diffed (`port --snapshot`: prepare, diff, report, no issues), as an early
warning: agents can read what 26.4 changes weeks before it ships.

### Prepare

`version_port.py prepare NEW`:

1. Fetch the manifest, the version JSON (check its sha1 against the manifest), the client and server jars (check
   their sha1s against the version JSON); `lab.download` already refuses partial or mismatched files. Releases
   since 26.1 ship unobfuscated, so no mappings are fetched; an obfuscated version (`client_mappings` present)
   stops here, as `lab.py prepare` does.
2. Unpack the server bundler (`lab.unpack_server`: one version, sha256 per entry).
3. Read the jar's `version.json` into `facts.json`. If its Java major is newer than the JDK on the Mac mini, stop
   with a single work item, "install JDK N" (a Ludovic task: fleet toolchains).
4. Run the data generator (`--reports`) with that JDK into `datagen/reports/`.
5. Index the classes of `client.jar` and `server-game.jar` into `classes.json`: for each class its superclass,
   interfaces, fields, and methods; each method's hash is over its bytecode with every constant-pool reference
   replaced by what it names (class, member, descriptor, literal), so recompiling unchanged code with a shifted
   constant pool hashes the same, and lambda bodies (`lambda$name$N`) are folded into the method that declares them
   so renumbered lambdas don't show as changes. This is exact where a decompiled-source diff is noisy, and takes
   seconds instead of the 10 minutes of a full Vineflower run.

The pinned version is indexed from `shared-local/` the same way, without copying its jars.

### Diff

`version_port.py diff OLD NEW` writes `diff.json` (and readable sections of the report), one section per kind:

| Kind | From | What it lists |
| --- | --- | --- |
| Facts | `facts.json` | protocol, world (data) version, pack formats, Java |
| Packets | `packets.json` | per state and direction: added, removed, renumbered packets |
| Registries | `registries.json` | per registry: added and removed entries, protocol-id shifts, new registries |
| Blocks | `blocks.json` | added and removed blocks; per block: properties and values, state count, default state |
| Item components | `components/item` | per item: added, removed and changed default components |
| Commands | `commands.json` | added and removed nodes, changed arguments and executability |
| Data packs | jar `data/minecraft/**` | added, removed, changed files by kind (recipe, loot table, worldgen, tags...), with a JSON path diff for changed files |
| Assets | jar `assets/minecraft/**` | `en_us.json` keys added, removed, changed; models, blockstates, textures, sounds added, removed, changed (by hash) |
| Classes | `classes.json` | added, removed and changed classes; per changed class the methods added, removed and changed (name and descriptor), field changes, hierarchy changes |

Changed classes are then decompiled, only those, on both sides: Vineflower runs on a small jar holding just those
classes (with the full jar as a library, so types resolve), and a unified diff per class goes into
`diff-OLD-NEW/sources/`. These diffs stay on the Mac mini: decompiled game code is never committed or posted
(`tools/check_public_tree.py`).

### From vanilla classes to Horizon modules

The diff matters only through what it changes in Horizon. `version_port.py map` builds `vanilla_map.json`, from
the repository alone, in seconds:

1. **Oracles → classes.** Each Java oracle and exporter under `mods/horizon/src/test/java/dev/horizonlab/parity/`
   names the vanilla classes it imports and uses (`net.minecraft.*` imports and fully qualified names).
2. **Harnesses → oracles and Rust.** Each `tools/test_*_parity.py` names its oracle class; its probe (a crate's
   example or binary named in the script) and its `PARITY_RUST_SCOPE` give the Rust files it proves.
3. **Rust → classes.** Horizon's doc comments name the vanilla code they port (`/// SharedConstants.getProtocolVersion()`,
   `//! Fixes of 1.21.6 to 26.3`, `EntityType.Builder clientTrackingRange`...): every CamelCase identifier in a
   comment that is the simple name of exactly one vanilla class (or `Outer.Inner`, or `Class.method`) maps that
   Rust file, and the method when named, to the class.

A changed vanilla class then resolves to the Horizon files that port it and the harnesses that prove them; a
changed method resolves more precisely when a comment names it. A class with Horizon code but no harness, or with
neither, is reported as such.

### Regenerate

`version_port.py regen NEW` (`tools/port_regen.py`) makes the candidate and regenerates every table in it:

1. **The candidate**: a worktree of the branch `port/NEW` (from `--base`, default `origin/main`) in
   `~/code/horizon-parity/ports/NEW` (`HORIZON_PORTS`), with `research-lock.json` pinned to NEW: version, the
   version JSON's sha1, the jars' sha1s, Java. Every tool reads the version from the lock (`lab.VERSION`,
   `lab.GAME`, `lab.RESOURCES`), so nothing else in the tree names the old one.
2. **Its `.local/`**, a real directory: `minecraft/NEW` is prepared by `tools/lab.py prepare` itself, from a
   launcher-shaped copy of the download in the version store (`versions/NEW/launcher`: the version JSON, the client
   jar and its libraries, verified by sha1) into `versions/NEW/game`, so the candidate's jars, libraries and
   manifest are exactly what a player's install gives. `datagen/out/reports` links to NEW's reports, `tools` to the
   shared tools. Nothing is written into `shared-local/`.
3. **Decompiled sources**: only the classes a generator parses (`EntityTypes`, `BlockStateData`), each decompiled
   alone with the full jar and its libraries on Vineflower's path, into `versions/NEW/sources/` (seconds instead of
   a 10-minute full run). The source is not the full run's text (inner classes of the jar's other classes are
   imported, chained calls are not wrapped), but what the generators read from it is: from 26.3's they write the
   committed tables byte for byte.
4. **The generators**, every row of the table above, each recorded in `diff-OLD-NEW/regen.json` as unchanged,
   regenerated (with its `--numstat`) or failed (with the end of its output and, when an exporter or oracle no
   longer compiles, javac's errors one per line: they name the vanilla API that changed). Rust outputs are
   rustfmt'ed. The pin and what regenerated are committed on `port/NEW`, locally; nothing is pushed.

The report then lists what was regenerated in its summary and makes each failed generator a rank-1 item, instead of
guessing which tables the diff made stale. `--only NAME...` reruns some generators; `port NEW` runs `regen` after
`diff` (`--no-regen` for snapshots).

Three generators run vanilla through Horizon's Java oracles (`VanillaCommandSuggestionsExporter`,
`VanillaCommandParseOracle`) and one builds `command_parse_fixture` (Horizon's command tree in its own registration
order, before `vanilla_order.rs` reorders it), so `regen` needs Java and Cargo. `horizon-prepare` (the resources the
harnesses read) is the next stage's first step.

### Run every harness against the new version

`version_port.py harnesses NEW [NAME...] --baseline STATE` (`tools/port_harness.py`), in the candidate `regen`
made:

1. **horizon-prepare**, built from the candidate and run there: `.local/horizon-client/NEW` and its exporters'
   outputs. Its `sounds` and `panorama` steps read the version store's launcher-shaped copy, whose asset index and the
   objects those steps copy (about 370 MB of sounds) are downloaded once per version from Mojang's resource server,
   each checked against its sha1. Regen's link to the version store's data generator reports is dropped first:
   horizon-prepare runs the generator itself, into the candidate. When the exporters no longer compile against NEW,
   the stage goes on and the report gets a rank-1 item with javac's errors (read from the log horizon-prepare names).
2. **`tools/parity.py --all`** there (or the harnesses named), from scratch.
3. **Each harness classified**:
   - **pass**: Horizon already matches NEW there;
   - **oracle**: its log has javac errors, so vanilla's API changed under the oracle (javac names the symbol);
   - **fail**: behaviour changed (or Horizon's data reading broke); the log has the first differing case;

   and **newly failing** unless it fails in the baseline too: `--baseline` is a `tools/parity.py` `state.json` from
   a run on the pinned version (the nightly audit's, or any worktree of `main`), so the known failures of `main` don't
   count. `--no-run` classifies the last run again against another baseline.

The result is `diff-OLD-NEW/harnesses.json`, with the logs of the harnesses that don't pass copied beside it
(`harness-logs/`, local only). `report` reads it: a newly failing harness marks the rank-2 item that names it as
failing (or "oracle no longer compiles"), a newly failing harness no item names gets its own rank-2 item, and the
summary counts pass / oracle / fail / newly failing.

### The port report

`diff-OLD-NEW/report.md` and `report.json`:

1. Summary: facts changed, counts per kind, generators (clean / regenerated / failed), harnesses (pass / oracle
   broken / fail).
2. Work items, ranked. An item is one Horizon area, not one vanilla class: changed classes are grouped by the
   Horizon module (or, for unported code, by vanilla package) that owns them, so each item is a PR-sized task. Rank:
   1. blocks the switch: protocol or save format (packets, registries the protocol syncs, data version and the new
      DataFixer schemas), a failed generator, an exporter that no longer compiles;
   2. a harness that fails or whose oracle no longer compiles;
   3. a changed class Horizon ports but no harness covers (the risky kind: nothing will catch a difference);
   4. new content in data packs and assets the game reads at runtime (usually nothing to do: listed so it gets a
      look);
   5. changed vanilla code Horizon doesn't port yet (informational, no issue).

   Within a rank, by the number of changed methods. Each item lists the vanilla classes and methods, the Horizon
   files and harnesses, the data-diff excerpts that concern it, and the local path of its source diffs.
3. Everything regenerated cleanly, so the reader knows what not to look at.

`version_port.py publish OLD NEW` (`tools/port_publish.py`) files the report as GitHub issues. **It is a dry run
by default**: it prints every issue it would create or update and the first bodies (`--show N`; `--check-github`
reads the open issues and labels, read-only, to tell creates from updates). `--yes` does it; the first `--yes` run
and the `version-port` label are Ludovic's call.

- **One issue per item** of rank 1, of rank 2 when a harness newly fails on NEW (or its oracle no longer compiles),
  and of rank 3 when Horizon's comments name one of the changed methods: title `[NEW] <item>`, labels
  `version-port` and `port-NEW`. An open issue with the same title is updated, and only its generated part
  (between `<!-- version-port: generated -->` markers) is replaced: what people wrote around it stays.
- **One tracking issue**, `[NEW] Port to Minecraft NEW`: the facts, the harness counts, a checklist of the item
  issues (made after them, so it has their numbers), and every other item: rank-2 leads no harness confirmed, rank-3
  classes Horizon names but none of whose changed methods, new content, unported changes.
- **Kira**: the board already shows every open issue as a card in To do, with its labels, so contributors and agents
  claim them there (`task_claim`); `publish` adds one Kira card for the port itself, linking the tracking issue
  (`~/code/horizon-parity/tools/kira.mjs card_add`, skipped if the board has it).
- **No decompiled code, ever**: bodies hold names and signatures only (vanilla classes, methods with their JVM
  descriptors, fields, packets, registry entries, harness names, javac's one-line errors about Horizon's own oracles,
  Horizon paths, and the local path of the diff). Every vanilla-derived string is quoted, and `assert_no_code`
  refuses a body with a code block, or a line or quoted span that reads like Java (declarations, statements, braces).

For 26.2 → 26.3 the dry run would file 48 issues (5 of rank 1, 43 of rank 3) and the tracking issue.

### The switch and the release

Work items are fixed on `port/NEW` branches stacked on the candidate (or as normal PRs into it). The pin bump PR is
the candidate branch itself: `research-lock.json` (version, sha1s, Java), the regenerated tables and
`version_consts`, the oracle fixes, and the ports. It merges when the report says no rank 1 or 2 item is left: every
harness passes against NEW, CI's vanilla data (`tools/ci/vanilla-data.sh`) and the Mac mini's `shared-local/` have
been switched to NEW (the version store's files are copied in, then `lab.py decompile` runs once for the full
sources), and a playtest shows a world loads and a vanilla NEW client can join a Horizon LAN world.

A Horizon release then ships as usual (`docs/release-flow.md`), labelled `feature` with "Minecraft NEW" in its
title. Its `research-lock.json` makes `horizon-prepare` ask for NEW, so players must have played NEW once in the
official launcher; the launcher already reads each release's game version from its `research-lock.json`, so players
who haven't can keep the previous Horizon release. Supporting two Minecraft versions in one build is out of scope.

## Turning on the daily agent

Not installed: it runs Mojang downloads, Cargo, Java and every harness on the Mac mini unattended, and its
`publish` dry run prepares issues under a new `version-port` label, so both need Ludovic's approval. What it does
on each run (`tools/ci/version-port-daily.sh`):

1. update its worktree of `origin/main` (`~/code/horizon-parity/port-main`, made on the first run, `.local/` linking
   the shared data as a parity worktree does);
2. `version_port.py detect --dry-run` (`--snapshots` with `HORIZON_PORT_SNAPSHOTS=1`): nothing new ends the run;
3. for a new release: `port NEW` (prepare both, diff, regen into `ports/NEW` on the local branch `port/NEW`, report),
   `tools/parity.py` on main as the baseline (only what never passed or changed reruns), `harnesses NEW` against
   that baseline, `report OLD NEW`, and `publish OLD NEW --check-github` (the dry run: the log lists the issues it
   would file). A snapshot is only prepared, diffed and reported;
4. `detect` again, so the version is remembered only once its port went through (a failed run retries the next day),
   and a note on Kira card 28 (`HORIZON_PORT_KIRA_CARD`) saying the report is ready.

It never runs `publish --yes`, never pushes and never changes `shared-local/`. `HORIZON_PORT_VERSION=V` ports V at
once (by hand, or to replay a version); `HORIZON_PORT_REF` checks out another ref than `origin/main`.

To turn it on, on the Mac mini (as Ludovic, once approved):

```sh
# 1. The label publish files issues under (once):
gh label create version-port --repo horizon-rust/horizon --color 5319e7 \
  --description "Work to port Horizon to a new Minecraft release (tools/version_port.py publish)"
# 2. The script, as a copy from main (it updates its own worktree, so it never runs from a checkout):
curl -fsSL https://raw.githubusercontent.com/horizon-rust/horizon/main/tools/ci/version-port-daily.sh \
  -o ~/code/horizon-parity/version-port-daily.sh
# 3. The agent:
curl -fsSL https://raw.githubusercontent.com/horizon-rust/horizon/main/tools/ci/dev.horizon.version-port.plist \
  | sed "s|@HOME@|$HOME|g" > ~/Library/LaunchAgents/dev.horizon.version-port.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.horizon.version-port.plist
# A run now, to see it work (log: ~/Library/Logs/horizon-version-port.log):
launchctl kickstart gui/$(id -u)/dev.horizon.version-port
```

To turn it off: `launchctl bootout gui/$(id -u)/dev.horizon.version-port` and delete the plist. When the report is
in, publishing is still by hand: `python3 tools/version_port.py publish OLD NEW` (dry run), then `--yes`.

## What stays manual, and how small it gets

Automatic: detecting, downloading, verifying, the data generator, every diff, the class-to-module map, regenerating
data-driven tables, rerunning every harness, the report, issues and cards, and, once the report is clean, the pin
bump branch.

Manual, by agents (and Ludovic's review):

- **New mechanics** (a new mob, block behaviour, worldgen feature, UI screen): someone has to port the new code. The
  pipeline hands them the exact classes and methods, the decompiled diff, the Horizon module and the harness to
  extend, so the agent starts at the code instead of searching for it.
- **Changed behaviour under a failing harness**: the harness names the case, the class diff names the method.
- **Oracles whose vanilla API moved**: javac names the symbol; usually a short fix.
- **New harnesses** for rank-3 items, where Horizon ports code that no harness checks.
- **New JDK**: a fleet toolchain install, once per Java bump.
- **Judgment on the switch**: merging the pin bump and releasing stay Ludovic's call.

## Replay: 26.2 → 26.3

Horizon was written against 26.3 from its first commit, so its code is the ground truth of what porting 26.2 to 26.3
takes. On the Mac mini, `prepare 26.2` took 9 s (136 MB), `diff 26.2 26.3` 6 s and `report` under a second.

| What 26.3 needed (in Horizon's code) | What the report found |
| --- | --- |
| Protocol 777, DataVersion 5023, pack formats | rank 1: protocol 776 → 777, world version 4903 → 5023, resource pack 88.0 → 97.1, data pack 107.1 → 121.0, with every site stating them (`PROTOCOL_VERSION`, both `DATA_VERSION`s, the `"26.3"` names) |
| `ids.rs`: `ADD_TRANSIENT_BLOCK`, `POST_EFFECTS` (configuration and play), `SWING_ANIMATION`, serverbound `PUNCH` instead of `SWING`; handlers in `packets/play.rs`, `lan/hub.rs` | rank 1: exactly those packets, 131 renumbered, those files, and "regenerate `ids.rs`". Regenerating from 26.3's report reproduces the committed file byte for byte (after rustfmt); from 26.2's it differs by 274 lines |
| DataFixers 4996-5016 in `datafix/fixes_c.rs` | rank 1: schemas V4996, V4996_1, V4997, V5000 and 16 new fixes (`MergeTerrainChunkStatusFix`, `ExplorerMapItemFix`, `SwingAnimationComponentSplitFix`...), with `datafix/` |
| `swing_animation` split into `attack_animation` and `interact_animation` (`first_person.rs`, `game/hand.rs`, `item_model.rs`) | rank 1: data component types +13 -2, those files among the ones naming them; per component, 1,537 items affected |
| `lan/tracking.rs` and the command tables | rank 1: regenerate `tracking.rs`, `command_suggestions.rs`, `command_order.rs`, `vanilla_order.rs` |
| 133 methods that exist only in 26.3 and that Horizon's comments name (`Minecraft.startAttack`, `MultiPlayerGameMode.piercingAttack`, `SkyRenderer.buildEndSky`, `GameRenderer.render3dHud`...) | all 133: 132 in rank 2-3 items with the method named, on the file that ports it; 1 in the save-format item |
| 49 classes that exist only in 26.3 and that Horizon names (`ComputeCommand`, `PostEffectCommand`, `LivingEntity.SwingState`, `BlockTransformer`, `StrawBedBlock`...) | all 49, in rank 4 "New in <package>" items |

The report has 499 items: 10 of rank 1, 311 of rank 2, 96 of rank 3, 80 of rank 4, and 2 rank-5 summaries. Rank 1 is
exact. Ranks 2 and 3 are leads, not all work: 173 of their 407 items have a changed method that Horizon's comments
name. The rest are classes whose changed methods Horizon doesn't name. Many of those are mechanical (26.3 renamed
`GuiGraphics` to `GuiGraphicsExtractor` and reworked the render pipeline), and 105 are "widely named" buckets
(classes such as `Entity` that many files mention in passing). Running every harness against the new version is what turns a
rank-2 lead into a confirmed failure; since the oracles are written against 26.3, the replay runs that stage both ways
(below).

### `regen`

`regen 26.2` from a branch pinned to 26.3, then `regen 26.3 --base port/26.2`, both from versions downloaded from
Mojang (`prepare --download`):

| Generator | 26.3 → 26.2 | 26.2 → 26.3 |
| --- | --- | --- |
| `version_consts` | regenerated (+8 -8: protocol 776, data version 4903, packs 88.0 / 107.1, build time) | regenerated, back to the committed file |
| `packet_ids` | regenerated (+134 -142) | regenerated (+142 -134), back to the committed file |
| `entity_tracking` | regenerated (+10 -13) | regenerated (+13 -10), back to the committed file |
| `block_state_data` | regenerated (+1 -1: the version in its header; `BlockStateData` is unchanged) | regenerated, back to the committed file |
| `lan_command_order`, `vanilla_command_order` | failed: `VanillaCommandParseOracle.java:81: no suitable constructor found for CommandSourceStack(...)` and 2 more | unchanged |
| `command_suggestions` | failed: `ParityData.java:45: cannot find symbol: variable WORLD (class RegistryLayer)`, `WORLD_REGISTRIES` | unchanged |

After the round trip, `port/26.3` equals the branch it started from except `research-lock.json`'s five lines of
republished 26.3 version JSONs it accepts (which a port cannot know). So every table Horizon commits is what the
generators make from vanilla's data, and going 26.3 → 26.2 fails exactly where Horizon's oracles use 26.3-only API,
with javac naming it. The report for 26.2 → 26.3 then has 5 rank-1 items instead of 10: the 5 "regenerate" guesses
are replaced by the generators' results in its summary. Both regens took under three minutes, most of it building
`command_parse_fixture`.

### `harnesses`

The 17 harnesses closest to what 26.3 changed (block_properties, chunk_generation, command_exec, command_parse,
component_patch, datafix, entity_components, first_person, food, hotbar, item_model, lan, light, options,
resource_pack, save_roundtrip, sound_event; 22 runs with datafix's 6 shards), on the Mac mini at 2 to 3 jobs while CI
and other sessions ran:

| | 26.3 → 26.2 (`ports/26.2`) | 26.2 → 26.3 (`ports/26.3`) |
| --- | --- | --- |
| horizon-prepare | failed: `VanillaHeightmapOracle.java:33: cannot find symbol: variable FULL_CODEC (class BlockState)` (rank 1) | ok, sounds and panorama from the downloaded assets |
| pass | 1 (`food`) | 17 |
| oracle no longer compiles | 4: `chunk_generation` (`ServerChunkCache` constructor), `command_exec` (`com.mojang.authlib.services`), `command_parse` and `hotbar` (`RegistryLayer.WORLD`) | 0 |
| fail | 12 (Horizon's 26.3 packet tables and resources don't build or match 26.2) | 0 |
| newly failing, against the other direction's run as baseline | 16 | 0 |
| time | about an hour, most of it datafix's vanilla upgrades | about 85 min, 10 of them horizon-prepare |

So on a real port the stage separates the three cases the report needs: what already works on NEW, oracles whose
vanilla API moved (javac names the symbol, as for regen), and behaviour that changed; and a candidate that is right
passes everything it ran. The 26.2 → 26.3 report's summary now reads "harnesses on 26.3: 17 pass, 0 whose oracle no
longer compiles, 0 fail; 0 newly failing".

## Implementation status

```sh
python3 tools/version_port.py detect --snapshots     # exit 10: new release, 11: new snapshot
python3 tools/version_port.py prepare 26.3           # the pinned version: indexes .local, downloads nothing
python3 tools/version_port.py prepare 26.4           # downloads, verifies, runs the data generator (about 10 s)
python3 tools/version_port.py diff 26.3 26.4         # summary; the full diff in versions/diff-26.3-26.4/diff.json
python3 tools/version_port.py regen 26.4            # ports/26.4 on port/26.4: pinned, every table regenerated
python3 tools/version_port.py report 26.3 26.4 [--results .local/parity-runner/state.json]   # report.md, report.json
python3 tools/version_port.py harnesses 26.4 --baseline .local/parity-runner/state.json   # in ports/26.4
python3 tools/version_port.py publish 26.3 26.4     # dry run: the issues it would file (--yes files them)
python3 tools/version_port.py port 26.4             # prepare both, diff, regen, report
python3 tools/version_port.py gc                     # keep the pinned version and the newest two others
python3 tools/vanilla_map.py versions/26.3/classes.json net.minecraft.world.food.FoodData   # its Rust files and harnesses
```

| Slice | Status |
| --- | --- |
| This design | this page |
| `detect`, `prepare`, `diff` (facts, packets, registries, blocks, items, commands, data, assets), `gc` | done |
| Class index (`tools/class_index.py`), class diff with moves, `vanilla_map.json` (`tools/vanilla_map.py`) | done |
| Port report (`tools/port_report.py`, `report`), `port` | done |
| `version_consts`: one generated file for the version facts (`tools/gen_version_consts.py`) | done |
| Tools read the version from the lock (`lab.VERSION`, `lab.GAME`, `lab.RESOURCES`, `lab.version_info()`) | done |
| `regen` (`tools/port_regen.py`): candidate worktree, its `.local/`, the generators' classes decompiled alone, every generator, `regen.json` in the report | done |
| Candidate harness run (`tools/port_harness.py`, `harnesses`): horizon-prepare with sounds, every harness, pass / oracle / fail, newly failing against a baseline, in the report | done |
| `publish` (`tools/port_publish.py`): dry run by default, issues, tracking issue, Kira card | done (never run with `--yes`) |
| Daily launchd agent (`tools/ci/version-port-daily.sh`, `tools/ci/dev.horizon.version-port.plist`) | written, not installed: needs Ludovic's approval |
