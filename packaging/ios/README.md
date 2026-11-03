# The iOS app shell: what Horizon's simulation says on an iPhone

Three files and one crate make the app shell, and a fourth puts the game itself in a bundle beside it.

- `project.yml`: the Xcode project, as an [XcodeGen](https://github.com/yonaskolb/XcodeGen) spec. Run
  `brew install xcodegen && xcodegen generate` in this folder to write `HorizonIOS.xcodeproj`, which is
  not checked in. Its paths are relative to this directory, so generate it from here.
- `Info.plist`: the bundle's plist, filled in by Xcode from `project.yml`'s `MARKETING_VERSION` and
  `CURRENT_PROJECT_VERSION`. Bundle id `dev.horizon.ios`, iOS 16 or later, iPhone and iPad.
- `Sources/main.m`: the app. One `AppDelegate`, one view controller, and no storyboard: it calls
  `horizon_probe()` in `viewDidLoad` and shows what came back in a monospaced text view.
- `GameInfo.plist`: a second bundle's plist, for the `.ipa` whose executable is the cross-compiled
  `horizon-client` rather than an app written here. Bundle id `dev.horizon.game`, and literal versions because no
  Xcode substitutes anything. See "The second bundle" below.

The Rust half is [`crates/horizon-ios`](../../crates/horizon-ios), whose header is
`crates/horizon-ios/include/horizon_ios.h`. The project links it by name out of
`target/aarch64-apple-ios/release`, so nothing here pretends a fresh clone is buildable: the archive
has to be cross-compiled first.

## Building it, as the workflow does

```sh
cargo rustc --release --locked --target aarch64-apple-ios -p horizon-ios --lib --crate-type staticlib
cd packaging/ios && xcodegen generate && cd ../..
xcodebuild -project packaging/ios/HorizonIOS.xcodeproj -target Horizon -configuration Release \
  -sdk iphoneos -arch arm64 \
  CODE_SIGNING_ALLOWED=NO CODE_SIGN_IDENTITY= CODE_SIGNING_REQUIRED=NO \
  CONFIGURATION_BUILD_DIR="$PWD/build-ios" build
mkdir -p ipa/Payload && cp -a build-ios/Horizon.app ipa/Payload/
( cd ipa && zip -qry ../Horizon-unsigned.ipa Payload )
```

`.github/workflows/ios.yml` does exactly this on a Mac runner, checks the bundle's binary is arm64
iOS machine code with `_horizon_probe` really linked into it, and uploads `Horizon-unsigned.ipa`.

## What the .ipa is, and what it is not

It is **unsigned**: iOS refuses to install it, on your own phone included. What it is for is proof of
the chain — Cargo's arm64 objects, an Objective-C caller, a linker, a bundle, a Payload archive — so
that the only thing standing between it and a hand is a signing certificate, which is an account and
not a build problem. With any Apple ID added to Xcode, open the generated project, pick a team, and
run it on the device.

It is **not the game**: this bundle holds the probe crate, not `horizon-client`. The client now *compiles*
for `aarch64-apple-ios` -- the Mac runner measured that on 2026-10-24, with `--no-default-features`, and the
clipboard that used to drag the Wayland stack into a phone build is gated to the four targets `arboard` has
a backend for (`crates/horizon-client/Cargo.toml`). Compiling is the entry fee and not the port. What stands
between this bundle and a playable one:

| Gap | Why it is structural | Decided |
|---|---|---|
| Assets | `horizon-prepare` runs a JDK against the player's own Minecraft install to produce `.local/horizon-client/26.3`; there is no Java on iOS | Prepare off-device, download to the phone |
| Mods | `horizon-mods` runs them through wasmtime's cranelift JIT, and Apple forbids generating executable code at runtime (`docs/mods.md`) -- note the runner says the `mods` *feature compiles*, which is about the compiler and not about running | Disabled on iOS |
| Input | the client's winit loop is desktop-shaped: cursor-warp mouse look, no touch | A touch layer, in no crate yet |
| Sound, controllers | `hidapi`/`gilrs` need libudev or IOKit, which is what `--no-default-features` drops and why the phone build asks for that flag; `cpal` (the `audio` feature) has a CoreAudio backend for iOS too, through RemoteIO | `--features audio` on the phone, with the launcher setting up the audio session; `GCController` for pads |

So this app runs the half that was already portable before the client was: `horizon-core`'s Java-accurate RNG
and HotSpot-exact `Math`, its paletted block storage, and `horizon-world`'s NBT codec -- the four probes whose
answers you can read on the screen, all of them computed on the phone rather than shipped to it.

## The second bundle: the game, with no Xcode in its build

`.github/workflows/ios.yml` uploads two `.ipa` files, because two things are being measured. `Horizon-unsigned.ipa`
is the shell above. `Horizon-game-unsigned.ipa` is `horizon-client` itself:

```sh
cargo build --release --locked --target aarch64-apple-ios -p horizon-client --no-default-features --features audio
rm -rf game-payload && mkdir -p game-payload/Payload/Horizon.app
cp target/aarch64-apple-ios/release/horizon-client game-payload/Payload/Horizon.app/Horizon
cp GameInfo.plist game-payload/Payload/Horizon.app/Info.plist
printf 'APPL????' > game-payload/Payload/Horizon.app/PkgInfo
( cd game-payload && zip -qry ../Horizon-game-unsigned.ipa Payload )
```

No `.xcodeproj` and no `xcodegen`: rustc produced an executable, and the rest is a folder layout iOS loads. The
bundle id is `dev.horizon.game`, so the two install side by side. `GameInfo.plist` carries literal versions where
`Info.plist` carries `$(MARKETING_VERSION)` for Xcode to fill in -- keep the two in step with `project.yml`.

What that bundle cannot do yet, in the order it will be found: it has **no assets** (CI has no Minecraft jar to run
`horizon-prepare` against, and this repository must not ship one; `UIFileSharingEnabled` in the plist is the
deliberate door for the player to put the prepared folder where the game looks), and **no touch input** -- the first
frame may be the whole show until something turns a tap into a key. The game starts in `$HOME/Documents`, which is
why `saves/`, `options.txt` and `.horizon-cache` are where a player can reach them; see `crates/horizon-client/src/main.rs`
`into_app_sandbox`.

## Running it on a phone

An unsigned `.ipa` installs nowhere, so the route that works today is the one Xcode signs for you. `project.yml`
has two application targets and two schemes:

| Scheme | Bundle id | Executable | What it is |
|---|---|---|---|
| **Horizon shell** | `dev.horizon.ios` | Objective-C + `libhorizon_ios.a` | the four probes, printed on screen |
| **Horizon game** | `dev.horizon.game` | Cargo's `horizon-client` | the game |

```sh
rustup target add aarch64-apple-ios
brew install xcodegen
cd packaging/ios && xcodegen generate && open HorizonIOS.xcodeproj
```

Pick **Horizon game**, choose your device, set a signing team on the target, and press Run. The target's first
build phase runs `cargo build --release --target aarch64-apple-ios -p horizon-client --no-default-features --features audio`;
Xcode then links a placeholder `GameShell/main.m` so the bundle has an executable, and a phase after it
overwrites that file with Cargo's binary -- which is what the signature then covers. rustc cannot emit a library
from a bin target and `Game` is bin-only code, so this is the only shape that gets the game into a signed bundle
without moving 30k lines into the library. `.github/workflows/ios.yml` builds the same target with
`CODE_SIGNING_ALLOWED=NO` and `cmp`s the bundle's executable against Cargo's, so "the copy stuck" is a checked
claim and not an assumption.

**If you open the shell and see the probe report, that is the other target.** The report text is
`crates/horizon-ios`, printed by `Sources/main.m`; nothing about it is the game.

### Landscape, which two places have to say

The game is landscape-only on a phone, and both halves are needed:

- `UISupportedInterfaceOrientations` in `Info.plist` and `GameInfo.plist` lists the two landscape directions and
  no portrait. This is what the launch screen and the bundle's own metadata are judged by.
- `ValidOrientations::Landscape` on the winit window attributes (`crates/horizon-client/src/game.rs`, both
  `resumed` implementations). Without it the plist alone does not hold: winit implements
  `supportedInterfaceOrientations` on its own view controller from its default
  (`LandscapeAndPortrait`, and it never reads the plist), and a phone asks the root view controller rather than
  the bundle, so the game would rotate into portrait under it.

Both landscape directions, not one: a handheld gets to be held either way, and the window size is the same in
each, so nothing in the render path has to care.

### Where the game looks, on the phone

- It works in `$HOME/Documents`, which on iOS is the app's data container (`main.rs` `into_app_sandbox`). Saves
  land in `Documents/saves`, settings in `Documents/options.txt`, pack caches in `Documents/.horizon-cache`.
- The log is `Library/Application Support/horizon/logs/latest.log` -- the container Xcode's *Devices and
  Simulators* window downloads, and the way to read what happened when nothing appeared.
- `Options::read` looks for `horizon-resources.json` in three places, in this order: a `resources` folder
  **beside the executable**, which on a phone means inside the bundle; then `Documents/resources`, which is
  where an import unpacks; then `Documents/.local/horizon-client/26.3`, the desktop fallback, which the Files
  app neither shows nor lets you create.
- The first stop is a developer's own: in Xcode, add your prepared `.local/horizon-client/26.3` to the
  **Horizon game** target as a folder reference with a path in bundle of `resources` (produce folders: keep
  their name or rename it there). It is fine to sign for yourself and it is the fastest debug loop, but a
  bundle built that way is not publishable -- see the next section -- which is why the game does not stop
  looking at the bundle.
- `UIFileSharingEnabled` in `GameInfo.plist` is what makes `Documents` visible to the Files app and to a cable
  at all, and is the reason the second stop is a folder a player can reach.

### Bringing the game files across

A published `.ipa` carries no assets: `horizon-resources.json` in a bundle is a refusal in
`.github/workflows/ios.yml`, and the bundle a phone installs is read-only while the game runs anyway. So the
files come from the computer that prepared them, as one ZIP, through one of two doors.

On the computer, with the same `horizon-client` a player already has:

```sh
horizon-client --pack-resources .local/horizon-client/26.3 --out Horizon-26.3.zip
```

The archive's one top folder is the directory you pointed at, which is exactly what compressing the folder by
hand produces: `crates/horizon-client/src/import.rs` reads both shapes, and refuses an archive that is none of
its three kinds before it writes a single file. Roughly 589 MB unpacked, so allow for the archive and the tree
both being on the phone at once.

**Over Wi-Fi, while the phone is asking.** A game that cannot start does not exit; it keeps its window, shows the
loader's own reason, and asks for its files at `http://<address>:<port>/` with six digits beside it
(`crates/horizon-client/src/game/import_screen.rs`, drawn with the one atlas that exists before Mojang's files
do). Open that address on the computer, choose the archive, type the digits, and watch the bar on the phone. Or,
from a terminal:

```sh
curl --upload-file Horizon-26.3.zip "http://<address>:<port>/upload?name=Horizon-26.3&pin=<digits>"
```

`name` is what a world or a pack is filed under, so it is worth passing; the game files land in
`Documents/resources` whatever it says.

The listener is up only while that screen is up, it takes one archive at a time, and the digits -- never in the
page it serves -- are what stops anyone else on the network from writing into the app: after thirty wrong codes
the session answers nobody, right code included, and only a relaunch asks with new ones. When the archive lands,
the port is closed, the game files are in `Documents/resources`, and the loader starts again on them. A wrong
code answers `403` and writes nothing. If what arrived still is not enough to start the game, the screen comes
back with the new reason and asks again on a new port with new digits, so a bad archive is one more send away
rather than a dead end.

**By cable, with no network involved.** Copy the same `.zip` into the app's `Documents` folder -- Finder's
*Files* tab, or iTunes' *File Sharing*, or any manager that gets there -- and launch. `main.rs` unpacks what it
finds there before the game looks for its resources, then deletes the archive.

Both doors route by the archive's root, so one file format carries three things: `horizon-resources.json` is
the game files, `level.dat` is a world (into `Documents/saves`, never over one that exists), `pack.mcmeta` is a
resource pack, which stays a `.zip` because the game reads those where they are. An archive that will not
unpack is renamed `<name>.zip.failed` rather than retried every launch, and an unpacking that a flat battery
interrupts is finished, or its old tree put back, on the next start -- `import::recover`, before anything reads
the folder it was working on.

What is measured of this, on Linux rather than on a phone: a game started with no files stayed open, refused a
wrong code with `403` and wrote nothing, took a real archive posted to the address it logged, unpacked it into
`Documents/resources`, closed the port and started the loader again -- then asked again on a fresh port and code
when that archive turned out to be a three-file stub. `HORIZON_IMPORT_SCREEN=1` is the same decision the iOS
build makes on its own, which is how that run was possible without a phone.

What is left to see on a device: the screen's text is drawn from `horizon-theme`'s embedded font, so it needs
nothing installed, but nobody has read it on a real phone yet. Nobody has run the game on a device either; what
is measured there is in the workflow's summary -- the game compiles for the target, links, and comes out of
Xcode's bundle as the same bytes Cargo wrote.

## Why the game, when it comes, is the Cargo executable and not another static library

`crates/horizon-ios` is a library, so `cargo rustc --crate-type staticlib` works for it. The game is not a
library: `Game` and `game::run()` live in the **bin** target (`crates/horizon-client/src/main.rs`, `game.rs`,
`src/game/`), and the lib cannot see them. rustc will not emit a library from a bin -- both
`crate-type = ["staticlib"]` in a `[[bin]]` and `cargo rustc --bin … --crate-type staticlib` are hard errors
("crate types can only be specified for libraries and example libraries"). winit 0.30's iOS backend removes the
need for that detour anyway: `EventLoop::run_app` calls `UIApplicationMain` itself, so a Rust executable built
by `cargo build --target aarch64-apple-ios` is a legitimate `Payload/Horizon.app/Horizon`, which is what
`.github/workflows/ios.yml` now produces beside this shell.

### Unsigned builds in GitHub Releases

After this workflow is merged, each successful `main` push in `heyskylo/horizon-ios`
creates a versioned prerelease containing an unsigned IPA and its `.sha256` checksum.
The app's build number is the workflow run number and attempt (for example `42.1`),
and its source commit is recorded in `Info.plist`. Release tags use `ios-build-42.1`;
assets are named `Horizon-iOS-<version>-build42.1-unsigned.ipa`.

PRs and fork builds only upload temporary Actions artifacts. A manual run on `main`
can publish by selecting **publish_release**; other manual runs remain artifact-only.
The publishing job alone receives `contents: write`, uses the same successful run's
artifact, verifies its source commit, and audits every Mach-O binary for signatures.
It also rejects provisioning profiles, certificates, private files and prepared game
data. No extra publishing token or signing certificate is required.

Files upload to a draft release before it becomes public. A release-only retry can
resume a partial draft, but cannot overwrite a published build or reuse a tag pointing
at another commit. Downloaded IPAs still need the user's own signing method before
installation. Prereleases are not marked as the stable `latest` release.
