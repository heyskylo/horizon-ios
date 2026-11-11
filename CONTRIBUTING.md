# Contributing to Horizon

Thanks for helping. Horizon is Minecraft: Java Edition 26.3 rewritten in Rust, and the one rule that shapes
everything is that **it plays exactly like vanilla**. This page is the short version of how we work; questions are
welcome on [Discord](https://discord.gg/playhorizon).

This repository holds the game and the launcher, plus the tools, tests and docs that build, check and release them.
The website (playhorizon.dev) is built in a separate repository from this one's README, `docs/api/` and roadmap. Anything
else (videos, Discord and other community material) lives elsewhere.

**Where to start: [playhorizon.dev/hub](https://playhorizon.dev/hub).** It shows what is being done now and next
([ROADMAP.md](ROADMAP.md) in the repository), the work board, good first tasks, and the forms to report a bug or
suggest something. A pull request for a roadmap item carries its `roadmap:<id>` label; ideas go to the suggestion
queue (the hub, or `/suggest` on Discord) rather than to issues, which are for bugs.

**Mod-related pull requests go to [arismoko/horizon](https://github.com/arismoko/horizon)**, the fork that carries the
mod API and reviews and merges that work. That covers the mod API, native hooks and events for mods, native mods,
content mods (`.hmod`), mixins, wasm mods, `cargo horizon-mod`, `horizon-mods`, mod builds and the registry, and
their docs ([mods](docs/mods.md), [native mods](docs/native-mods.md)). Open the pull request there from your usual
branch. Mod-related pull requests opened here are closed with a pointer to it. Engine extension points that Horizon's
own crates register (the [layer split](docs/layers.md)), resource and shader pack formats, and gameplay fixes stay
here.

## Build it

- **Prerequisites:** Rust (stable), Python 3.10+, JDK 25, and on Linux the ALSA and udev headers
  (`libasound2-dev libudev-dev`).
- **Resources:** Horizon never downloads the game; it prepares its resources from your own copy of Minecraft 26.3, as
  described in [Build from source](README.md#build-from-source).
- **Commands:**

  ```sh
  cargo run --release -p horizon-prepare -- --prepare --server-jar ~/Downloads/server.jar
  cargo run --release -p horizon-client
  ```

- **Launcher:** its own Cargo workspace in `launcher/` (`cd launcher && cargo run`; on NixOS, `nix-shell` first).

## Vanilla first

Wherever vanilla has a feature, vanilla is the specification, down to the order of random calls. "Parity" here means
Horizon gives the same results as the pinned 26.3 client and server for the same inputs, checked by a **harness**: a
`tools/test_*_parity.py` script that runs a small Java oracle against the real game and a Rust probe against Horizon,
and compares their outputs.

- **Read the vanilla code before writing Horizon's.** `tools/rea_vanilla.py` queries the pinned game bytecode:
  `python3 tools/rea_vanilla.py method net.minecraft.world.level.block.DispenserBlock dispenseFrom`.
  Say in your pull request which vanilla classes and methods you followed.
- **One version: 26.3.** Parity work targets 26.3 until its parity is good, redstone first. Don't open pull requests
  that move Horizon to a newer Minecraft version for now; to join servers on a newer version, put a Via proxy
  (ViaVersion, ViaProxy) in front of them.
- **Gameplay changes come with a harness**, new or extended, so the next change can't silently break them.
  [The parity workflow](docs/parity-workflow.md) shows how harnesses run; copy a nearby one.
- **Horizon's own features** (the launcher, the new menus) follow [the design system](docs/design-system.md) and are
  off by default wherever they would change the vanilla look.
- **Three layers: engine, platform, Horizon.**
  - Vanilla code never depends on the platform (mods, shader packs, HDR, the settings system) or on Horizon
    (its extras, cosmetics, the shop, official servers, telemetry).
  - The platform never depends on Horizon.
  - Horizon uses only the platform, never the engine: it will live in its own repository.

  [docs/layers.md](docs/layers.md) says where a new feature goes; CI checks the dependencies.
- **Everything players run is Rust.** Python and Java are fine for dev tools and harness drivers in `tools/`, never
  for player-facing code.
- **No game files in the repo.** Never commit game files, decompiled source or prepared resources (`.local/`):
  `tools/check_public_tree.py` rejects them (`git config core.hooksPath .githooks` runs it before every commit).
  The pre-push hook checks the full history reachable from the branches and tags being pushed. Run
  `python3 tools/check_public_tree.py --history` to audit the history of all local refs.

## Only original code, or code whose license allows it

Horizon is MIT. Its code is written from Minecraft 26.3 itself and from public documentation. Other projects'
source may be read for research and review; incorporating their code depends on its license:

- **Allowed:** MIT, MIT-0, BSD-2-Clause, BSD-3-Clause, 0BSD, ISC, zlib, Apache-2.0, Unlicense, and the CC0-1.0
  public-domain dedication. That code may be read, copied or adapted.
- **Read, but do not copy or adapt:** GPL, LGPL (Iris), AGPL, MPL, EPL, Polyform (Sodium), the Creative Commons
  licenses (CC BY, CC BY-SA, CC BY-NC and the others; CC0 is a public-domain dedication, not one of them), custom
  or "source available" licenses, all rights reserved (OptiFine), and projects with no license or an unclear one.
  You may read their source for research and review, but do not copy, translate, paraphrase or incorporate it into
  Horizon, and do not submit code generated by an AI tool from it. Reading does not grant permission to redistribute
  source or assets; respect access restrictions and applicable licenses.
- **Java's own library is GPL too.** OpenJDK's source (`java.lang.Math`, `StrictMath`, `FdLibm`, `HashMap`, the
  sorts, `Double.toString`, HotSpot's intrinsics...) is GPL-2.0: it may be read, but not copied or adapted into
  Horizon. Reproduce Java's behaviour from its API documentation (the Javadoc), from what a JVM returns, or from a
  permissive original such as Sun's fdlibm C sources (see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)); see #6741.
- **Check the license yourself.** Read the LICENSE file at the commit you use and the header of each file you take:
  some repositories mix licenses, and a file under another license may not be copied or adapted. A license that
  changed: the one at your commit counts. In doubt, don't copy or adapt it: ask in #contributors first.
- **Say it in the pull request:** the project, a link to the commit, its license, and what you read or copied. A
  pull request that used another project's source without saying so is closed.
- **Copied or adapted code** keeps its copyright line and license notice at the top of the file, and gets an entry
  in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Apache-2.0 code also keeps its NOTICE text and says what
  was changed.

User documentation and file formats are fine to read, and packs and mods are fine to run and observe, whatever their
license. Shader packs are a clean-room rewrite of the pack format, produced from a written description of it (see
#1561): the [specification and its provenance log](docs/cleanroom/README.md) are public. They carry no foreign
source, and the rules above still apply to anything you read while working on them. A shader-pack pull request
names its sources: the sections of that specification, the documentation pages and the packs it ran.

## Vendor libraries and Java mods

- **Upscalers and latency tools.** AMD FSR (MIT) may go in the repository, like any code under an allowed license.
  NVIDIA's DLSS, Reflex and any other library under an NVIDIA license may not: they ship only as an optional native mod
  that downloads NVIDIA's files itself, on the player's machine. No NVIDIA binaries, headers or SDK files in the
  repository.
- **The JVM bridge** (running Java mods) is only ever a separate, optional download, never installed or on by
  default. The game may offer to turn it on when it finds a Java mod; the player decides. It is promoted out of beta
  only after MSPT and RAM benchmarks against the Java client with the same mods ([#13914](https://github.com/horizon-rust/horizon/issues/13914)).

## Before asking for review

Run the self-check and paste the block it prints into your pull request:

```sh
python3 tools/pr_ready.py --base upstream/main   # or origin/main if you work on the main repository
```

It looks at what your branch changes and runs what matters: rustfmt, clippy with warnings denied, the tests of the
crates you touched (and the launcher's), generated files, `Cargo.lock`, the Python tests, and the vanilla harnesses
your change can affect. Without the vanilla data on your machine it lists those harnesses instead; CI runs them.
`--quick` skips the harnesses. It exits non-zero when something fails.

For the game workspace, Clippy checks all targets of the touched crates and their transitive workspace dependents,
including examples. The dependency scope includes optional, platform, build and development dependencies. Tests run
library, binary, integration and doc tests without building unrelated example executables. Shared Cargo
configuration and toolchain changes select the whole workspace; CI also keeps its workspace-wide checks.

### Translations

Most of Horizon's translations were written by machine and stay marked unreviewed until someone who speaks the
language checks them. To report a wrong string or review your language, see [docs/translations.md](docs/translations.md)
and #translations on Horizon's Discord. Never mark machine-written text as reviewed.

## Claim before you start

Work is tracked in GitHub issues, and an issue that is assigned is taken (so is one with the `in progress` label:
claimed on Discord with Kira's `/claim`): pick another, or ask its holder in the thread. Before you start, comment
**`/claim`** on the issue; you are assigned at once, no role needed. No issue for your work yet? Open one (a line is
enough), then claim it. `/unclaim` gives it back. Issues labelled `tracker`, `proposal` or `not claimable` aren't
claimed whole: claim one of the issues a tracker lists, or open one for the part you take.

A claim with no open pull request linking the issue (`Fixes #123` in the description, or a draft) is released after
5 days, with a comment: open a draft early. You can hold 8 open issues at a time, not counting the ones your
pull requests already fix. Agents claim the same way.

When your pull request changes the same functions as another open one, or closes the same issue, a bot comment names
it (edited on every push, never repeated): talk to its author in the thread before you both finish the same work.

## Pull requests

- **Small.** One change per pull request, under about 500 lines of code where you can; split bigger work into
  pieces that each make sense alone. Small pull requests get reviewed and merged the same day. Bigger than that:
  [large pull requests](#large-pull-requests).
- **Fill in the template**: what and why, the vanilla reference, the pr_ready block, screenshots for anything you
  can see.
- **Tick "Allow edits by maintainers".** When something small is left (a lint, a rebase, a nit) we push the fixup
  ourselves instead of sending it back to you; only design questions come back.
- **Once we've pushed to your branch, merge main into it; don't rebase or force-push.** A rebase or force-push
  replaces our commits with yours and erases the fixes we added (it happened on #5188 and #5203). Tell your agent
  too: `git merge origin/main`, never `git rebase` or `git push --force` on a branch a maintainer has touched. If
  our fixes get erased anyway, we finish the work in our own pull request that keeps your commits and credits you.
- **Tick its kind** in the template: `parity` (Horizon now does what vanilla does), `bug` (a crash or breakage),
  `perf`, `feature` (new for players), `extra` (Horizon-only, off by default), `tooling` or `docs`. A workflow sets
  that label, and area labels (`launcher`, `mobs`, `worldgen`, `rendering`...) from the paths you change.
  `label:parity` is the list of real progress toward vanilla.
- **Release labels** (we set them if you can't): `feature` also puts it under "New" in the release notes,
  `skip-release` when players see no change (tooling, docs, tests), nothing for a fix. `release-now` is for crash
  fixes that shouldn't wait. pr_ready suggests one.

Who reviews and merges: the Horizon Team, on CI for ordinary pull requests; risky ones (saves, network, world
generation, the launcher's updates) wait for a local proof first ([merging](docs/release-flow.md#merging)).

Every merge to main is released automatically ([release flow](docs/release-flow.md)), so main always stays playable.
How agents, checks, releases and signed updates keep this safe: [security](docs/security.md).

### Large pull requests

500 lines stays the default. A pull request that can't be split below it starts from a spec:

1. **Open an issue with the spec** before the code: what changes, the files it touches, the behaviour players and
   other code will see, and the tests or harnesses that prove it.
2. **A maintainer approves the spec** in the issue. Code written before that may have to change or wait.
3. **The pull request links the approved issue** (`Spec: #123` in the description) and stays within it; a change of
   plan goes back to the issue first.
4. **An agent check compares the code with the spec**, run by the maintainers' agents: anything the spec doesn't
   cover, or that it promises and the code doesn't do, comes back to you.
5. **Then the normal review**, as for any pull request.

## Parity checks on forks

The `verify` check (formatting, lints, tests) runs on every pull request. The `parity` check runs the affected
harnesses on a maintainer's machine, which holds the game files, so it never runs a fork's code on its own: a
maintainer reads your change, then adds the `run-parity` label, which runs it once on your current commit. After
new pushes, we add it again.

## Talk to us

[Discord](https://discord.gg/playhorizon) for questions and ideas, [issues](https://github.com/horizon-rust/horizon/issues)
for bugs. To take something on, `/claim` its issue ([above](#claim-before-you-start)).

How Horizon is run, and how people join the Team or lead a group: [GOVERNANCE.md](GOVERNANCE.md).
