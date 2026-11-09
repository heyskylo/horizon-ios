//! Native mods (docs/native-mods.md): mixin bundles that
//! levzzz5154's horizon-mixins (MIT, <https://github.com/horizon-rust/horizon-mixins>) applies to
//! Horizon's source, which the launcher then compiles into a game of its own with its portable Rust
//! ([`rust_toolchain`]). Unlike `.hmod` mods they are not sandboxed: their code runs with the
//! player's rights.
//!
//! A native mod pins the Horizon revision it was made for (a Nightly's or a release's commit).
//! Mods pinning different ones build on the newest of them, once the player agreed to try (they
//! often still apply and compile). A mod that hooks only Horizon's native hook points
//! (`game/native_hooks.rs`) names the version it needs instead (`hooks`): it builds on any Horizon
//! whose hook points have that version or a later one (they are only ever added to), the player's own
//! by default, so such mods always build together. The build compiles that revision's source, from
//! GitHub, never a checkout on this computer, and installs as its own version,
//! `native-<revision>-<key>` (the key hashes the revision, the mods' files, the tool's commit and
//! the profile, so the same set builds once). The instance the mods are installed in plays it, and
//! its next build stays on the same revision ([`base_for`]) until the player asks to update
//! ([`update_for`]), so a rebuild compiles only what the mods changed.
use crate::backend;
use crate::i18n::{tr, trf};
use crate::instances::{self, Instance};
use crate::rust_toolchain::{self, Toolchain};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod api;
mod crates;
mod dependencies;
mod diagnostics;
mod inject;
mod inject_cache;
mod isolate;

pub use isolate::LeftOut;

const PREFIX: &str = "native-";
/// horizon-mixins, for bundles that don't name the revision they were tested with.
const MIXINS_GIT: &str = "https://github.com/horizon-rust/horizon-mixins";
pub const MIXINS_REV: &str = "5a64af938ec86b00d53d16a408aabc0038fb3e57";
/// The features of the release's own game (game-build.yml).
const FEATURES: &str = "horizon-client/mods";

/// What a native build is compiled for. `Performance`, the default, is the workspace's `fast` (16
/// codegen units, incremental, thin-local LTO only), so changing a mod recompiles only `horizon-client`'s
/// changed parts; `Dev`, for developing a mod, is `fast` with `horizon-client` unoptimized: the
/// quickest to rebuild after an edit, for a game about twice as slow; `Release` is the release's
/// own (thin LTO, one codegen unit): slowest to build, slightly faster game. `Performance` and
/// `Dev` share `native/target/fast/`: every other crate is built alike, so switching between them
/// recompiles only `horizon-client`, and Cargo keeps both builds of it. Each profile has its own
/// key, so their builds never mix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    #[default]
    Performance,
    Dev,
    Release,
}

impl Profile {
    /// The player's choices (the Mods page, `settings native-profile`).
    pub const ALL: [Self; 3] = [Self::Performance, Self::Dev, Self::Release];

    pub fn name(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Dev => "dev",
            Self::Release => "release",
        }
    }

    /// The profile named `text`; `fast`, `Performance`'s name before `Dev`, still names it.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "performance" | "fast" => Some(Self::Performance),
            "dev" => Some(Self::Dev),
            "release" => Some(Self::Release),
            _ => None,
        }
    }

    /// The settings' profile (`native_profile`, or the older `native_release` switch for
    /// `Release`).
    pub fn chosen(settings: &crate::backend::Settings) -> Self {
        settings
            .native_profile
            .unwrap_or(if settings.native_release {
                Self::Release
            } else {
                Self::Performance
            })
    }

    /// The Cargo profile it compiles with (its folder in `native/target/`), and what it sets over
    /// it as `cargo --config` values: here rather than in `[profile.fast]`, which the workspace's
    /// own builds share.
    fn cargo(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Performance => ("fast", &[]),
            Self::Dev => ("fast", &["profile.fast.package.horizon-client.opt-level=0"]),
            Self::Release => ("release", &[]),
        }
    }
}

/// What a native build compiles: the game the player plays, or the headless engine a dedicated
/// server runs (`horizon-engine`, docs/server.md). Both build the same source with the same
/// mixins. The server uses `--no-default-features` (no audio or controllers), a separate cache,
/// and a standalone runtime folder outside the managed versions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    /// The game: `horizon-client` as the release ships it.
    #[default]
    Client,
    /// The headless engine: the same binary without the default features, installed and
    /// exported as `horizon-engine`.
    Server,
}

impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
        }
    }

    /// What the build compiles with besides the profile, in its key: the game the release's own
    /// features, the engine `--no-default-features` (the `mods` feature is added to both when
    /// the source has it, so it changes neither key).
    fn features(self) -> &'static str {
        match self {
            Self::Client => FEATURES,
            Self::Server => "--no-default-features",
        }
    }

    fn directory(self, version: &str) -> PathBuf {
        match self {
            Self::Client => backend::version_dir(version),
            Self::Server => root().join("server-builds").join(version),
        }
    }

    /// The name the built binary is installed under: the game's own, the engine's
    /// (`horizon-engine`, docs/server.md).
    fn binary(self) -> String {
        match self {
            Self::Client => backend::CLIENT.into(),
            Self::Server => format!("horizon-engine{}", std::env::consts::EXE_SUFFIX),
        }
    }
}

/// The platform the game's source will be compiled for. On Windows this follows the installed
/// game's executable, not the PC's native architecture; the launcher must have a matching managed
/// host toolchain before any compilation starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BuildPlatform {
    target: Option<&'static str>,
    machine: Option<u16>,
    /// Non-default target builds get their own Cargo cache and record namespace.
    cache_dir: Option<&'static str>,
}

impl BuildPlatform {
    fn cache_key(self) -> Option<&'static str> {
        self.cache_dir
    }
}

fn platform_for_game(base: &str) -> Result<BuildPlatform, String> {
    if std::env::consts::OS != "windows" {
        return Ok(BuildPlatform::default());
    }
    let executable = backend::version_dir(base).join(backend::CLIENT);
    let machine = pe_machine(&executable).map_err(|error| {
        trf(
            "Could not read {name}: {error}",
            &[
                ("name", &executable.display().to_string()),
                ("error", &error),
            ],
        )
    })?;
    let selected_target = rust_toolchain::windows_target_for_machine(machine).ok_or_else(|| {
        architecture_mismatch(&format!("PE machine 0x{machine:04x}"), "x64 or ARM64")
    })?;
    let Some(host) = rust_toolchain::host() else {
        return Err(architecture_mismatch(
            windows_architecture(selected_target),
            std::env::consts::ARCH,
        ));
    };
    windows_build_platform(machine, host.target)
}

fn windows_build_platform(machine: u16, host_target: &str) -> Result<BuildPlatform, String> {
    let target = rust_toolchain::windows_target_for_machine(machine).ok_or_else(|| {
        architecture_mismatch(&format!("PE machine 0x{machine:04x}"), "x64 or ARM64")
    })?;
    if rust_toolchain::windows_target_for_host(host_target, machine).is_none() {
        return Err(architecture_mismatch(
            windows_architecture(target),
            windows_architecture(host_target),
        ));
    }
    let cache_dir = (target == rust_toolchain::WINDOWS_ARM64_TARGET).then_some(target);
    Ok(BuildPlatform {
        target: Some(target),
        machine: Some(machine),
        cache_dir,
    })
}

fn architecture_mismatch(game: &str, toolchain: &str) -> String {
    trf(
        "Native mods require supported Windows architectures that match. Game: {game_arch}; build toolchain: {toolchain_arch}.",
        &[("game_arch", game), ("toolchain_arch", toolchain)],
    )
}

fn windows_architecture(target: &str) -> &'static str {
    match target {
        rust_toolchain::WINDOWS_X64_TARGET => "x64",
        rust_toolchain::WINDOWS_ARM64_TARGET => "ARM64",
        _ => unreachable!(),
    }
}

/// Reads the COFF machine from a Windows PE executable after validating its two signatures.
pub(crate) fn pe_machine(path: &Path) -> Result<u16, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)
        .map_err(|error| error.to_string())?;
    if &dos[..2] != b"MZ" {
        return Err("missing DOS MZ signature".into());
    }
    let pe_offset = u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap()) as u64;
    file.seek(SeekFrom::Start(pe_offset))
        .map_err(|error| error.to_string())?;
    let mut header = [0; 6];
    file.read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    if &header[..4] != b"PE\0\0" {
        return Err("missing PE signature".into());
    }
    Ok(u16::from_le_bytes([header[4], header[5]]))
}

/// What a native build was made from, kept per profile in `native/built.json` after it compiled:
/// what the next build is compared with to say what changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Record {
    pub horizon: String,
    pub mixins: String,
    /// Each mod's id and the hash of its files.
    pub mods: std::collections::BTreeMap<String, String>,
}

/// What a build is going to do, said before it starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The build already exists: nothing compiles.
    UpToDate,
    /// `native/target/` holds an earlier build of this profile: only what changed recompiles. The
    /// list says what changed.
    Incremental(Vec<String>),
    /// Nothing built yet in this profile: the whole game compiles.
    First,
}

impl Plan {
    pub fn label(&self) -> String {
        match self {
            Self::UpToDate => tr("Up to date (no build needed)").into(),
            Self::Incremental(changes) => trf(
                "Incremental rebuild: {changes}",
                &[("changes", &changes.join("; "))],
            ),
            Self::First => tr("First build: the whole game (several minutes)").into(),
        }
    }

    /// The word in "Built in 42 s (performance, incremental)".
    fn kind(&self) -> &'static str {
        match self {
            Self::UpToDate => "up to date",
            Self::Incremental(_) => "incremental",
            Self::First => "first build",
        }
    }
}

/// Which case a build is: `exists` (its version is installed), else whether an earlier build of
/// the profile (`before`) is there to rebuild from, and what differs from `now`.
pub fn classify(exists: bool, before: Option<&Record>, now: &Record) -> Plan {
    if exists {
        return Plan::UpToDate;
    }
    let Some(before) = before else {
        return Plan::First;
    };
    let mut changes = Vec::new();
    if before.horizon != now.horizon {
        let short = |r: &str| r.chars().take(7).collect::<String>();
        changes.push(format!(
            "Horizon commit {} to {}",
            short(&before.horizon),
            short(&now.horizon)
        ));
    }
    let changed: Vec<&str> = now
        .mods
        .iter()
        .filter(|(id, hash)| before.mods.get(*id) != Some(hash))
        .map(|(id, _)| id.as_str())
        .chain(
            before
                .mods
                .keys()
                .filter(|id| !now.mods.contains_key(*id))
                .map(String::as_str),
        )
        .collect();
    if !changed.is_empty() {
        changes.push(format!("mods {}", changed.join(", ")));
    }
    if before.mixins != now.mixins {
        changes.push("horizon-mixins".into());
    }
    if changes.is_empty() {
        changes.push("the build settings".into());
    }
    Plan::Incremental(changes)
}

fn records_path() -> PathBuf {
    root().join("built.json")
}

fn read_records(path: &Path) -> std::collections::BTreeMap<String, Record> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// A build record's key in `built.json`: the profile's folder in `native/target/` for a game
/// build (as it always was), the target's own name before it for a server one, so the two never
/// read each other's record. `Performance` and `Dev` share one: their last build in that folder.
fn record_key(target: Target, profile: Profile, platform: Option<&str>) -> String {
    let folder = profile.cargo().0;
    let key = match target {
        Target::Client => folder.into(),
        Target::Server => format!("{}-{folder}", target.name()),
    };
    match platform {
        Some(platform) => format!("{platform}-{key}"),
        None => key,
    }
}

/// The last build of `target` in `profile`'s folder in `native/target/`, if that folder (the
/// compiled crates both build from) is still there.
fn last_record(target: Target, profile: Profile, platform: Option<&str>) -> Option<Record> {
    let mut target_dir = root().join("target");
    if let Some(platform) = platform {
        target_dir.push(platform);
    }
    if !target_dir.join(profile.cargo().0).is_dir() {
        return None;
    }
    read_records(&records_path()).remove(&record_key(target, profile, platform))
}

fn save_record(target: Target, profile: Profile, platform: Option<&str>, record: &Record) {
    let mut all = read_records(&records_path());
    all.insert(record_key(target, profile, platform), record.clone());
    if let Ok(text) = serde_json::to_vec_pretty(&all) {
        let _ = fs::write(records_path(), text);
    }
}

/// The hash of a bundle's files.
fn mod_hash(b: &Bundle) -> Result<String, String> {
    let mut hash = Sha256::new();
    for (name, path) in bundle_files(b)? {
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(hash.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// What a cargo output line says about the compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cargo {
    /// `Compiling name vX`: a crate started.
    Compiling(String),
    /// Cargo's progress bar, `Building [===>   ] 12/430: names`: units done, and in all.
    Progress(usize, usize),
}

/// `line` without terminal escape sequences (`ESC [ ... letter`).
fn without_escapes(line: &str) -> String {
    let mut plain = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            plain.push(c);
        }
    }
    plain
}

/// Cargo's events in one line of its standard error. With `CARGO_TERM_PROGRESS_WHEN=always` the
/// progress bar is redrawn with carriage returns, so a line holds several pieces.
pub fn parse_cargo(line: &str) -> Vec<Cargo> {
    let mut out = Vec::new();
    for piece in without_escapes(line).split('\r') {
        let piece = piece.trim();
        if let Some(name) = piece.strip_prefix("Compiling ") {
            out.push(Cargo::Compiling(
                name.split_whitespace().next().unwrap_or(name).to_owned(),
            ));
        } else if let Some(bar) = piece.strip_prefix("Building [") {
            let counts = bar.split_once("] ").map(|(_, rest)| rest).unwrap_or("");
            let counts = counts.split(':').next().unwrap_or("");
            if let Some((done, total)) = counts.split_once('/') {
                if let (Ok(done), Ok(total)) = (done.trim().parse(), total.trim().parse()) {
                    out.push(Cargo::Progress(done, total));
                }
            }
        }
    }
    out
}

/// What of `line` the build log keeps: all of it but cargo's progress bar, whose redraws are
/// separated by carriage returns and may be followed on the same line by a real message
/// (`Building [=>] 3/9: a\r\x1b[Kerror[E0425]: ...`). None when nothing else is left.
fn log_text(line: &str) -> Option<String> {
    let kept: Vec<String> = without_escapes(line)
        .split('\r')
        .filter(|piece| {
            let piece = piece.trim();
            !piece.is_empty() && !piece.starts_with("Building [")
        })
        .map(str::to_owned)
        .collect();
    (!kept.is_empty()).then(|| kept.join("\n"))
}

/// "1 min 5 s" style time, for the build's status.
pub fn duration_label(secs: u64) -> String {
    if secs >= 60 {
        format!("{} min {} s", secs / 60, secs % 60)
    } else {
        format!("{secs} s")
    }
}

/// Whether an installed version is a native-mods build.
pub fn is_native(version: &str) -> bool {
    version.starts_with(PREFIX)
}

pub(crate) fn root() -> PathBuf {
    backend::build_dir().join("native")
}

/// The build's log, kept until the next build.
pub fn log_path() -> PathBuf {
    root().join("build.log")
}

/// Makes the build log fresh for a new attempt: its folder made, the last build's log
/// emptied. The error says why it couldn't, so the Build log page can show it instead of
/// the old file's contents.
pub fn clear_log() -> Result<(), String> {
    fs::create_dir_all(root()).map_err(|e| e.to_string())?;
    fs::write(log_path(), "").map_err(|e| format!("{}: {e}", log_path().display()))
}

/// Appends `message` (with a newline) to the build log, so a failure that happens before any
/// command runs is still recorded.
pub fn append_log(message: &str) -> Result<(), String> {
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
        .map_err(|e| format!("{}: {e}", log_path().display()))?;
    writeln!(log, "{message}").map_err(|e| e.to_string())
}

/// Records the whole attempt, including selection or export after compilation.
fn logged<T>(build: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    // Another build or a cleanup holds the lease on the same target/: refuse. Any other
    // failure (a filesystem without file locks, a `native` folder linked to another drive)
    // only stops the cleanup: the build goes ahead without the lease, as it did before.
    let (cache, unavailable) = match crate::native_cache::Cache::acquire(&backend::build_dir()) {
        Ok(cache) => (Some(cache), None),
        Err(crate::native_cache::Error::Busy) => {
            return Err(cache_error(crate::native_cache::Error::Busy));
        }
        Err(error) => (None, Some(cache_error(error))),
    };
    clear_log()?;
    if let Some(warning) = &unavailable {
        log_warning(warning);
    }
    let result = build();
    match &result {
        Ok(_) => {
            let _ = append_log("Build completed.");
        }
        Err(error) => {
            let _ = append_log(&format!("The build failed: {error}"));
        }
    }
    // The installed binary and post-build programs have left target/ before trimming. Keep
    // this lease through installation/export, including failed builds, and never wait on Cargo.
    let Some(cache) = cache else {
        return result;
    };
    let limit = backend::Settings::load()
        .native_cache_gib
        .unwrap_or(DEFAULT_CACHE_GIB);
    match cache.trim(limit.saturating_mul(1 << 30)) {
        Ok(bytes) if bytes > 0 => {
            let _ = append_log(&trf(
                "Cleared {bytes} bytes of native build cache. The next changed build will compile from scratch.",
                &[("bytes", &bytes.to_string())],
            ));
        }
        Err(error) => log_warning(&cache_error(error)),
        _ => {}
    }
    result
}

pub const DEFAULT_CACHE_GIB: u64 = 30;

fn cache_error(error: crate::native_cache::Error) -> String {
    match error {
        crate::native_cache::Error::Busy => {
            tr("A native build or cache cleanup is running. Try again when it finishes.").into()
        }
        crate::native_cache::Error::Unsafe(path) => trf(
            "Refusing native cache cleanup at an unexpected file or link: {path}",
            &[("path", &path.display().to_string())],
        ),
        crate::native_cache::Error::Io(error) => trf(
            "Could not access the native build cache: {error}",
            &[("error", &error.to_string())],
        ),
    }
}

#[derive(serde::Serialize)]
pub struct CacheInfo {
    pub path: PathBuf,
    pub bytes: u64,
    pub cleared_bytes: u64,
    pub limit_gib: u64,
}

/// Inspect or clear only Cargo's native fast/release outputs, while no build is using them.
/// This does not remove installed versions, instances, mods, saves, toolchains or downloads.
pub fn cache_info(clear: bool) -> Result<CacheInfo, String> {
    let cache = crate::native_cache::Cache::acquire(&backend::build_dir()).map_err(cache_error)?;
    let bytes = cache.bytes().map_err(cache_error)?;
    let cleared_bytes = if clear {
        cache.clear().map_err(cache_error)?
    } else {
        0
    };
    Ok(CacheInfo {
        path: root().join("target"),
        bytes,
        cleared_bytes,
        limit_gib: backend::Settings::load()
            .native_cache_gib
            .unwrap_or(DEFAULT_CACHE_GIB),
    })
}

const MANIFEST: &str = "horizon-mixin.toml";
/// A registry download's manifest (`.hnmod`, `horizon_mods::index::native`), beside `mixins/<id>/`.
const NATIVE_MANIFEST: &str = horizon_mods::index::native::MANIFEST;
/// Where an instance keeps the native mods installed from the registry, `<id>.hnmod`, each on or
/// off in `horizon-mods.txt` like any mod.
pub const INSTALLED: &str = "horizon-native-mods";

/// The native mods turned on in the game folder `home`, unpacked into a folder [`build`] takes.
/// Each `.hnmod` is checked again as the registry checks it (Rust and TOML under
/// `mixins/<id>/`, plus files under its `data/`, nothing run).
pub fn installed_mods(home: &Path) -> Result<PathBuf, String> {
    let out = root().join("installed.incoming");
    let _ = fs::remove_dir_all(&out);
    let mut paths: Vec<PathBuf> = fs::read_dir(home.join(INSTALLED))
        .map(|dir| dir.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.retain(|p| p.extension().is_some_and(|e| e == "hnmod"));
    paths.sort();
    let mut count = 0;
    for path in paths {
        let bundle = horizon_mods::index::native::Bundle::read(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !crate::browser::java::is_enabled(home, &bundle.manifest.id) {
            continue;
        }
        let dir = out.join(&bundle.manifest.id);
        for (name, bytes) in &bundle.files {
            let to = dir.join(name);
            fs::create_dir_all(to.parent().unwrap_or(&dir)).map_err(|e| e.to_string())?;
            fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
        }
        count += 1;
    }
    if count == 0 {
        return Err(format!(
            "No native mod is turned on in {}: install one from the Mods page first.",
            home.join(INSTALLED).display()
        ));
    }
    Ok(out)
}

/// A mixin bundle: a folder with a `horizon-mixin.toml`, and the revisions it pins.
#[derive(Clone, Debug, PartialEq)]
pub struct Bundle {
    pub id: String,
    pub version: String,
    pub dir: PathBuf,
    /// The Horizon commit it builds on (`[compatibility] horizon_revision`, or its `.hnmod`'s).
    pub horizon: Option<String>,
    /// The horizon-mixins commit it was tested with (its `.hnmod`'s).
    pub mixins: Option<String>,
    /// The native hook points' version it needs (its `.hnmod`'s `hooks`): then `horizon` is the
    /// revision it was tested on, and it builds on any with these hook points or later ones.
    pub hooks: Option<u32>,
    /// The bundle-relative trait package folder (`[api] path`).
    pub api: Option<String>,
    /// True when the manifest has content for horizon-mixins.
    pub mixins_content: bool,
    /// Its `[[injection]]` entries, for the build's warnings. Read leniently:
    /// an entry the launcher cannot make sense of is left out, horizon-mixins
    /// refuses it when it applies.
    pub injections: Vec<Injection>,
}

/// One `[[injection]]` of a bundle's manifest, read for the build's
/// warnings: where it sits and whether it can cancel.
#[derive(Clone, Debug, PartialEq)]
pub struct Injection {
    pub id: String,
    /// Where it injects: a native hook point by name, or a raw function path.
    pub site: InjectionSite,
    /// A named hook point's `hook_version`: with the name, what the revision's
    /// `native-hooks.json` must list for horizon-mixins to find it.
    pub hook_version: Option<u32>,
    /// Its `kind` (`HEAD`, `RETURN`, …) as written.
    pub kind: Option<String>,
    /// Its `selector` and `ordinal`, which with the kind tell two interior
    /// sites of one function apart.
    pub selector: Option<String>,
    pub ordinal: Option<i64>,
    /// The `[mod]` `priority` unless the entry overrides it. horizon-mixins
    /// defaults it to 0 (`#[serde(default)] priority: i32`).
    pub priority: i64,
    /// `cancellable = true` (a `HEAD`, `BEFORE_CALL`, `AFTER_CALL` or
    /// `AFTER_ASSIGN` may carry it): a `Break` skips the later injections
    /// at the site.
    pub cancellable: bool,
}

/// An injection's site: `hook = "frame"` names a native hook point from the
/// catalog, `target = "game::tick"` a raw function path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InjectionSite {
    Hook(String),
    Target(String),
}

impl InjectionSite {
    fn as_text(&self) -> String {
        match self {
            Self::Hook(name) => format!("hook `{name}`"),
            Self::Target(path) => format!("`{path}`"),
        }
    }
}

impl Bundle {
    /// The named hook points it injects at, with their versions: each must be in the
    /// `native-hooks.json` of the revision it builds on.
    fn named_hooks(&self) -> impl Iterator<Item = (&str, u32)> {
        self.injections
            .iter()
            .filter_map(|i| match (&i.site, i.hook_version) {
                (InjectionSite::Hook(name), Some(version)) => Some((name.as_str(), version)),
                _ => None,
            })
    }
}

impl Injection {
    /// The place it sits at, as the manifest tells it: the function, the
    /// kind, and the selector and ordinal of interior hooks. horizon-mixins
    /// resolves the same place against the source (`plan.rs` sorts by file,
    /// node, call site, constant site, then priority descending, mod id
    /// ascending, injection id ascending): same manifest place, and its
    /// tie-break tail is the run order.
    fn place(&self) -> (&InjectionSite, Option<&str>, Option<&str>, Option<i64>) {
        (
            &self.site,
            self.kind.as_deref(),
            self.selector.as_deref(),
            self.ordinal,
        )
    }
    /// The place as a message names it.
    fn place_as_text(&self) -> String {
        let mut text = self.site.as_text();
        if let Some(kind) = &self.kind {
            text.push(' ');
            text.push_str(kind);
        }
        if let Some(selector) = &self.selector {
            text.push_str(&format!(" selector `{selector}`"));
        }
        if let Some(ordinal) = self.ordinal {
            text.push_str(&format!(" ordinal {ordinal}"));
        }
        text
    }
}

/// The bundles in `folder`: the folder itself when it is one (a `horizon-mixin.toml`, or an
/// unpacked `.hnmod`: `horizon-native.json` and `mixins/<id>/`), else each of its subfolders that is.
pub fn bundles(folder: &Path) -> Result<Vec<Bundle>, String> {
    if let Some(found) = bundle_in(folder)? {
        return Ok(vec![found]);
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(folder)
        .map_err(|e| format!("Could not read {}: {e}", folder.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let mut found = Vec::new();
    for dir in dirs {
        found.extend(bundle_in(&dir)?);
    }
    if found.is_empty() {
        return Err(format!(
            "{} holds no native mod ({MANIFEST} or {NATIVE_MANIFEST}).",
            folder.display()
        ));
    }
    Ok(found)
}

pub(crate) fn bundle_in(dir: &Path) -> Result<Option<Bundle>, String> {
    if dir.join(MANIFEST).is_file() {
        return bundle(dir, None).map(Some);
    }
    let path = dir.join(NATIVE_MANIFEST);
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let native_json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let field = |name: &str| native_json[name].as_str().map(str::to_owned);
    let id = field("id").ok_or_else(|| format!("{} has no id.", path.display()))?;
    if !api::valid_id(&id) {
        return Err(format!("{}: invalid mod id {id:?}", path.display()));
    }
    let found = bundle(&dir.join("mixins").join(&id), field("mixins"))?;
    match (&found.horizon, field("horizon")) {
        (Some(pinned), Some(native)) if *pinned != native => Err(format!(
            "{id}: {NATIVE_MANIFEST} pins Horizon {native}, its {MANIFEST} {pinned}."
        )),
        (_, native) => Ok(Some(Bundle {
            horizon: found.horizon.clone().or(native),
            hooks: native_json["hooks"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok()),
            ..found
        })),
    }
}

fn bundle(dir: &Path, mixins: Option<String>) -> Result<Bundle, String> {
    let path = dir.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let field = |table: &str, name: &str| {
        value
            .get(table)
            .and_then(|m| m.get(name))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    let need = |name: &str| {
        field("mod", name).ok_or_else(|| format!("{} has no [mod] {name}.", path.display()))
    };
    // The `[mod]` `priority` the injections fall back to. horizon-mixins
    // defaults it to 0 (`#[serde(default)] priority: i32`), not to a README
    // example's 100.
    let mod_priority = value
        .get("mod")
        .and_then(|m| m.get("priority"))
        .and_then(|v| v.as_integer())
        .unwrap_or(0);
    // Read leniently: an entry without an id and a site (`hook` or `target`)
    // is left out — horizon-mixins refuses it when it applies.
    let injections = value
        .get("injection")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let text = |name: &str| entry.get(name).and_then(|v| v.as_str()).map(str::to_owned);
            let id = text("id")?;
            let site = match (text("hook"), text("target")) {
                (Some(hook), _) => InjectionSite::Hook(hook),
                (None, Some(target)) => InjectionSite::Target(target),
                (None, None) => return None,
            };
            Some(Injection {
                id,
                site,
                hook_version: entry
                    .get("hook_version")
                    .and_then(|v| v.as_integer())
                    .and_then(|v| u32::try_from(v).ok()),
                kind: text("kind"),
                selector: text("selector"),
                ordinal: entry.get("ordinal").and_then(|v| v.as_integer()),
                priority: entry
                    .get("priority")
                    .and_then(|v| v.as_integer())
                    .unwrap_or(mod_priority),
                cancellable: entry.get("cancellable").and_then(|v| v.as_bool()) == Some(true),
            })
        })
        .collect();
    let api = value
        .get("api")
        .map(|table| api::folder(dir, table))
        .transpose()?;
    let mixins_content = value
        .keys()
        .any(|key| !["mod", "compatibility", "api", "dependencies"].contains(&key.as_str()));
    // Reject links before copying the bundle into the incoming folder.
    files_with_api(dir, api.as_deref())?;
    let id = need("id")?;
    if !api::valid_id(&id) {
        return Err(format!("{}: invalid mod id {id:?}", path.display()));
    }
    Ok(Bundle {
        id,
        version: need("version")?,
        dir: dir.to_owned(),
        horizon: field("compatibility", "horizon_revision"),
        mixins,
        hooks: None,
        injections,
        api,
        mixins_content,
    })
}

fn is_revision(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The build's warnings about the mods' injections (#6490): two mods on one
/// place, mods wrapping one function at its head and its return, and a
/// cancelling injection on a native hook point. Neither stops the build —
/// they say what the player and the author could not otherwise see. The
/// places are the manifest's: whether two selectors pick the same call, or
/// a raw `target` names a hook point's function, only horizon-mixins
/// resolves, so the warnings stay to what the manifest says for sure.
fn warn_conflicts(mods: &[Bundle]) -> Vec<String> {
    let mut out = Vec::new();
    // One place (the function, the kind, the selector, the ordinal), the
    // injections of at least two mods on it, ordered as horizon-mixins runs
    // same-place injections: priority descending, mod id ascending,
    // injection id ascending — the tail of its own sort, which the place
    // prefixes already fixed.
    let mut places: BTreeMap<_, Vec<(&str, &Injection)>> = BTreeMap::new();
    for b in mods {
        for i in &b.injections {
            places.entry(i.place()).or_default().push((&b.id, i));
        }
    }
    for (_, list) in places {
        let mut list = list;
        list.sort_by_key(|(mod_id, i)| {
            (
                std::cmp::Reverse(i.priority),
                (*mod_id).to_owned(),
                i.id.clone(),
            )
        });
        let mut names: Vec<&str> = Vec::new();
        for (mod_id, _) in &list {
            if !names.contains(mod_id) {
                names.push(mod_id);
            }
        }
        if names.len() < 2 {
            continue;
        }
        let first = list[0].1;
        let both = if names.len() == 2 { "both" } else { "all" };
        out.push(format!(
            "mods {} {both} inject at {}; {} runs first (priority descending, mod id ascending).",
            names.join(", "),
            first.place_as_text(),
            names[0]
        ));
    }
    // A function's head and its return: two mods wrapping the whole
    // function, which run in that order whatever their priorities. Said
    // without a "runs first".
    let mut functions: BTreeMap<&InjectionSite, Vec<(&str, &Injection)>> = BTreeMap::new();
    for b in mods {
        for i in &b.injections {
            if i.kind.as_deref() == Some("HEAD") || i.kind.as_deref() == Some("RETURN") {
                functions.entry(&i.site).or_default().push((&b.id, i));
            }
        }
    }
    for (site, list) in functions {
        let kinds: std::collections::BTreeSet<&str> =
            list.iter().filter_map(|(_, i)| i.kind.as_deref()).collect();
        let mut names: Vec<&str> = Vec::new();
        for (mod_id, _) in &list {
            if !names.contains(mod_id) {
                names.push(mod_id);
            }
        }
        // Both sides, and more than one mod on the function: a single mod
        // wrapping its own function at both ends is no one's conflict.
        if kinds.len() < 2 || names.len() < 2 {
            continue;
        }
        let sides: Vec<String> = list
            .iter()
            .map(|(mod_id, i)| format!("{mod_id} ({})", i.kind.as_deref().unwrap_or("")))
            .collect();
        out.push(format!(
            "mods {} inject into {}: HEAD runs on entry, RETURN on return, whatever their priorities.",
            sides.join(", "),
            site.as_text()
        ));
    }
    // A cancelling `HEAD` on a hook point: a `Break` skips the later
    // injections at the place and the function's body. A raw `target` is
    // only known to be a hook point through the catalog, which the launcher
    // does not read; a `hook` name is one by definition. Only `HEAD` is
    // said: the interior kinds' cancellation skips less than the body.
    for b in mods {
        for i in &b.injections {
            if i.cancellable
                && i.kind.as_deref() == Some("HEAD")
                && matches!(i.site, InjectionSite::Hook(_))
            {
                out.push(format!(
                    "{}'s `{}` can cancel at {}: a Break skips the later injections and the function's body.",
                    b.id,
                    i.id,
                    i.place_as_text()
                ));
            }
        }
    }
    out
}

/// Appends `text` to the build log, the way a compiler warning reads.
fn log_warning(text: &str) {
    if let Ok(mut log) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = writeln!(log, "warning: {text}");
    }
}

/// The one value the bundles pin for `which` (each mod's `get`), else `fallback`; mods that pin
/// different ones can't be built together, and the error says which.
fn agreed(
    bundles: &[Bundle],
    which: &str,
    get: impl Fn(&Bundle) -> Option<&String>,
    fallback: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    let mut pins: Vec<(&String, &str)> = bundles
        .iter()
        .filter_map(|b| get(b).map(|r| (r, b.id.as_str())))
        .collect();
    pins.sort();
    let revision = match pins.first() {
        None => fallback()?,
        Some((first, _)) if pins.iter().all(|(r, _)| r == first) => (*first).clone(),
        Some(_) => {
            let list: Vec<String> = pins
                .iter()
                .map(|(r, id)| format!("{id} → {}", &r[..r.len().min(7)]))
                .collect();
            return Err(format!(
                "These mods were made for different {which} revisions and can't be built together: {}. Install versions made for the same Nightly.",
                list.join(", ")
            ));
        }
    };
    if !is_revision(&revision) {
        return Err(format!(
            "{revision} is not a {which} commit (40 hex digits)."
        ));
    }
    Ok(revision)
}

/// The revision `bundles` build on for `which` when their pins differ: the newest of the pins and
/// `also` (the launcher's own horizon-mixins), by its commit's time (`date`), so no mod gets an
/// older one than it was made for. With one pin or none, it is [`agreed`]'s.
fn newest(
    bundles: &[Bundle],
    which: &str,
    get: impl Fn(&Bundle) -> Option<&String>,
    fallback: impl FnOnce() -> Result<String, String>,
    also: Option<&str>,
    date: impl Fn(&str) -> Result<String, String>,
) -> Result<String, String> {
    let mut pins: Vec<&str> = bundles
        .iter()
        .filter_map(|b| get(b).map(String::as_str))
        .collect();
    pins.sort();
    pins.dedup();
    if pins.len() < 2 {
        return agreed(bundles, which, get, fallback);
    }
    if let Some(bad) = pins.iter().find(|p| !is_revision(p)) {
        return Err(format!("{bad} is not a {which} commit (40 hex digits)."));
    }
    pins.extend(also.filter(|a| !pins.contains(a)));
    let mut dated = Vec::new();
    for pin in pins {
        let when = date(pin).map_err(|e| {
            format!("Can't tell which of the mods' {which} revisions is newest: {e}")
        })?;
        dated.push((when, pin));
    }
    Ok(dated
        .into_iter()
        .max()
        .map(|(_, p)| p.to_owned())
        .unwrap_or_default())
}

/// When commit `revision` of GitHub's `repo` was made (ISO 8601, so it sorts), from the dates kept
/// in [`DATES`] when it was asked before: a commit's date never changes, so a build of mods made
/// for different revisions can be used again offline or past GitHub's rate limit.
fn committed(repo: &str, revision: &str) -> Result<String, String> {
    remembered(&root().join(DATES), repo, revision, ask_github)
}

/// The file in [`root`] keeping the commit dates GitHub gave, as `{"repo@revision": date}`.
const DATES: &str = "commit-dates.json";

/// The date `file` keeps for `repo`'s `revision`, else `ask`'s, then kept in `file`.
fn remembered(
    file: &Path,
    repo: &str,
    revision: &str,
    ask: impl FnOnce(&str, &str) -> Result<String, String>,
) -> Result<String, String> {
    let key = format!("{repo}@{revision}");
    let mut dates: serde_json::Map<String, serde_json::Value> = fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if let Some(date) = dates.get(&key).and_then(|d| d.as_str()) {
        return Ok(date.to_owned());
    }
    let date = ask(repo, revision)?;
    dates.insert(key, date.clone().into());
    if let Ok(text) = serde_json::to_vec_pretty(&dates) {
        let _ = fs::write(file, text);
    }
    Ok(date)
}

fn ask_github(repo: &str, revision: &str) -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{repo}/commits/{revision}");
    let commit = crate::github::json(&url)?;
    commit["commit"]["committer"]["date"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("GitHub doesn't know {repo} commit {revision}."))
}

/// How [`build`] starts its error when mods pin other Horizon revisions than the one it would
/// use and the player hasn't agreed to try (`anyway`); the rest lists them.
pub const MISMATCH: &str = "Made for other Horizon versions: ";

/// Each pinned mod whose Horizon revision isn't `revision`, as `id → pin`, and the build's.
fn mismatch(mods: &[Bundle], revision: &str) -> Option<String> {
    let list: Vec<String> = mods
        .iter()
        .filter(|b| b.hooks.is_none())
        .filter_map(|b| b.horizon.as_ref().map(|r| (b, r)))
        .filter(|(_, r)| *r != revision)
        .map(|(b, r)| format!("{} → {}", b.id, &r[..r.len().min(7)]))
        .collect();
    (!list.is_empty()).then(|| {
        format!(
            "{}; the build uses {}",
            list.join(", "),
            &revision[..revision.len().min(7)]
        )
    })
}

/// Where Horizon declares its native hook points' version, and the line that does.
const HOOKS_FILE: &str = "crates/horizon-client/src/game/native_hooks.rs";
const HOOKS_LINE: &str = "pub(crate) const NATIVE_HOOKS_API: u32 = ";

/// The native hook points' version in `HOOKS_FILE`'s text.
fn hooks_in(source: &str) -> Option<u32> {
    let line = source.lines().find_map(|l| l.strip_prefix(HOOKS_LINE))?;
    line.trim_end().strip_suffix(';')?.parse().ok()
}

/// The native hook points' version of Horizon at `revision`, from GitHub (None: it has none).
pub(crate) fn hooks_at(revision: &str) -> Option<u32> {
    let url = format!(
        "https://raw.githubusercontent.com/{}/{revision}/{HOOKS_FILE}",
        backend::REPO
    );
    let text = backend::get(&url).ok()?.into_string().ok()?;
    hooks_in(&text)
}

/// The named hook points (name, version) a `native-hooks.json` lists; None when it isn't one.
fn catalog_in(text: &str) -> Option<HookCatalog> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value["hooks"]
        .as_array()?
        .iter()
        .map(|h| {
            let version = u32::try_from(h["version"].as_u64()?).ok()?;
            Some((h["name"].as_str()?.to_owned(), version))
        })
        .collect()
}

/// The named hook points of Horizon at `revision`, from GitHub (None: it has no catalog, as
/// revisions from before d3e0774 that already had hook points v2).
fn catalog_at(revision: &str) -> Option<HookCatalog> {
    let url = format!(
        "https://raw.githubusercontent.com/{}/{revision}/{CATALOG}",
        backend::REPO
    );
    let text = backend::get(&url).ok()?.into_string().ok()?;
    catalog_in(&text)
}

/// Where Horizon lists its named hook points, at the workspace root (horizon-mixins reads it there).
const CATALOG: &str = "native-hooks.json";
type HookCatalog = std::collections::BTreeSet<(String, u32)>;

/// The revision to build `mods` on: `pinned` (the pins' agreement, else the installed version's)
/// when every mod naming a hook points' version finds it or a later one there (`at` reads a
/// revision's), and finds each named hook point it injects at in that revision's catalog
/// (`catalog`): hook points v2 came before `native-hooks.json`, so a pin on such a revision
/// would fail in horizon-mixins (E_HOOK_CATALOG). Else, when nothing pins it, the revision those mods were tested on. Else the
/// revision of the release or Nightly the build starts from (`game`), when they find theirs
/// there: a rebuild that stayed on an older Horizon, or mods pinning one, then move to the game
/// the player installed, and pinned mods wait for the player's Build anyway ([`mismatch`]).
fn with_hooks(
    mods: &[Bundle],
    pinned: String,
    free: bool,
    game: impl FnOnce() -> Option<String>,
    at: impl Fn(&str) -> Option<u32>,
    catalog: impl Fn(&str) -> Option<HookCatalog>,
) -> Result<String, String> {
    let hooked: Vec<&Bundle> = mods.iter().filter(|b| b.hooks.is_some()).collect();
    let named = hooked.iter().any(|b| b.named_hooks().next().is_some());
    // The revision's hook points' version, and its catalog when a mod names hook points.
    let points = |revision: &str| (at(revision), named.then(|| catalog(revision)).flatten());
    let fits_one = |b: &Bundle, (found, listed): &(Option<u32>, Option<HookCatalog>)| {
        found.is_some()
            && b.hooks <= *found
            && b.named_hooks().all(|(name, version)| {
                listed
                    .as_ref()
                    .is_some_and(|l| l.contains(&(name.to_owned(), version)))
            })
    };
    let fits = |revision: &str| {
        let found = points(revision);
        hooked.iter().all(|b| fits_one(b, &found))
    };
    if hooked.is_empty() || fits(&pinned) {
        return Ok(pinned);
    }
    let tested: Vec<Bundle> = hooked.iter().map(|b| (*b).clone()).collect();
    let fallback = agreed(
        &tested,
        "Horizon",
        |b| b.horizon.as_ref(),
        || Err(String::new()),
    );
    if let (true, Ok(revision)) = (free, fallback) {
        if fits(&revision) {
            return Ok(revision);
        }
    }
    if let Some(revision) = game().filter(|r| *r != pinned && fits(r)) {
        return Ok(revision);
    }
    // Only the mods that need more than `pinned` has: the others would build there.
    let found = points(&pinned);
    let list: Vec<String> = hooked
        .iter()
        .filter(|b| !fits_one(b, &found))
        .map(|b| format!("{} (hook points v{})", b.id, b.hooks.unwrap_or_default()))
        .collect();
    Err(format!(
        "Horizon {} doesn't have the native hook points these mods use: {}. Update Horizon or the mods.",
        &pinned[..pinned.len().min(7)],
        list.join(", ")
    ))
}

/// A bundle that names a hook points' version builds on another revision than the one it was
/// tested on: its `horizon_revision` (which horizon-mixins checks) becomes the build's.
fn repin(bundle: &Bundle, revision: &str) -> Result<(), String> {
    let path = bundle.dir.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut value: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(toml::Value::Table(compat)) = value.get_mut("compatibility") {
        compat.insert("horizon_revision".into(), revision.into());
    }
    let text = toml::to_string(&value).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether Horizon's source at `source` has the `mods` feature (the WebAssembly mods' runtime,
/// gone from later revisions).
fn has_mods_feature(source: &Path) -> bool {
    let path = source.join("crates/horizon-client/Cargo.toml");
    let text = fs::read_to_string(path).unwrap_or_default();
    let value: toml::Table = text.parse().unwrap_or_default();
    value.get("features").and_then(|f| f.get("mods")).is_some()
}

/// The profile the source at `source` builds with: `chosen`, except that one built on
/// `[profile.fast]` becomes `Release` (the release's own) for a revision whose Cargo.toml has none.
fn profile_of(source: &Path, chosen: Profile) -> Profile {
    if chosen == Profile::Release {
        return chosen;
    }
    let text = fs::read_to_string(source.join("Cargo.toml")).unwrap_or_default();
    let value: toml::Table = text.parse().unwrap_or_default();
    match value.get("profile").and_then(|p| p.get("fast")) {
        Some(_) => chosen,
        None => Profile::Release,
    }
}

/// The Horizon commit `version` (an installed release or Nightly) was built from, from GitHub.
pub fn revision_of(version: &str) -> Result<String, String> {
    let reference = match version.strip_prefix("nightly-") {
        // nightly-2026-10-28-0c02067: the commit is its last word.
        Some(rest) => rest.rsplit('-').next().unwrap_or(rest),
        None => version,
    };
    let url = format!(
        "https://api.github.com/repos/{}/commits/{reference}",
        backend::REPO
    );
    let commit = crate::github::json(&url)?;
    commit["sha"]
        .as_str()
        .filter(|s| is_revision(s))
        .map(str::to_owned)
        .ok_or_else(|| format!("GitHub doesn't know the commit of {version}."))
}

/// What a native build was made from: its `native-mods.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct About {
    /// The release or Nightly it started from.
    pub base: String,
    /// The Horizon commit it compiled.
    pub horizon: String,
    /// What it compiles: the game the player plays (also the only kind a `native-mods.json`
    /// from before the target was recorded builds), or the headless engine a server runs.
    pub target: Target,
    /// The commit of `base` itself, when the build knew it (not for builds made before it was
    /// recorded): `horizon` differs from it when mods pin another.
    pub base_horizon: Option<String>,
    /// The mods whose pin made the build use `horizon` instead of `base_horizon` (empty when the
    /// build is on `base`'s own commit).
    pub pinned_by: Vec<String>,
    /// The player chose to build on `base`'s own commit even where mods pin another: a rebuild
    /// from this build keeps doing so.
    pub on_base: bool,
    /// The mods turned on that it was built without, and why.
    pub left_out: Vec<LeftOut>,
}

impl About {
    /// A build from before `base_horizon` was recorded that compiled another commit than its
    /// Nightly's (the last word of its name): mods pinned it.
    pub fn off_its_nightly(&self) -> bool {
        let nightly = self.base.strip_prefix("nightly-").and_then(|r| r.get(11..));
        self.base_horizon.is_none()
            && nightly.is_some_and(|commit| !commit.is_empty() && !self.horizon.starts_with(commit))
    }
}

/// What installed native build `version` was made from; None for any other version.
pub fn about(version: &str) -> Option<About> {
    if !is_native(version) {
        return None;
    }
    about_in(&backend::version_dir(version))
}

/// The build record (`native-mods.json`) in version folder `dir`, if it parses. A build from
/// before targets were recorded is a client one, so it still reads.
fn about_in(dir: &Path) -> Option<About> {
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("native-mods.json")).ok()?).ok()?;
    let field = |name: &str| json[name].as_str().map(str::to_owned);
    Some(About {
        base: field("base")?,
        horizon: field("horizon").filter(|r| is_revision(r))?,
        target: match field("target").as_deref() {
            Some("server") => Target::Server,
            _ => Target::Client,
        },
        base_horizon: field("base_horizon").filter(|r| is_revision(r)),
        pinned_by: json["pinned_by"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m.as_str().map(str::to_owned))
            .collect(),
        on_base: json["on_base"].as_bool().unwrap_or(false),
        left_out: json["left_out"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(LeftOut::from_json)
            .collect(),
    })
}

/// The Horizon commit the mods that pin one (`pins`: those naming no hook points' version) give a
/// build, and which of them pin it. With `on_base`, the player chose the commit of the version the
/// build starts from (`base`) whatever the pins say; with no pin, it is `base`'s too. Mods
/// pinning different commits give the newest of them ([`newest`]).
fn pinned_revision(
    pins: &[Bundle],
    on_base: bool,
    base: impl FnOnce() -> Result<String, String>,
    date: impl Fn(&str) -> Result<String, String>,
) -> Result<(String, Vec<String>), String> {
    if on_base {
        return Ok((base()?, Vec::new()));
    }
    let revision = newest(pins, "Horizon", |b| b.horizon.as_ref(), base, None, date)?;
    let by = pins
        .iter()
        .filter(|b| b.horizon.as_ref() == Some(&revision))
        .map(|b| b.id.clone())
        .collect();
    Ok((revision, by))
}

/// The newest installed release or Nightly in `installed` (newest first).
pub(crate) fn newest_installed(installed: &[String]) -> Option<&String> {
    installed
        .iter()
        .find(|t| backend::is_game_tag(t) || t.starts_with("nightly-"))
}

/// The installed version a build for an instance that plays `current` starts from: `current`
/// itself, so a rebuild stays on the Horizon it plays (a native build's own revision, [`about`]).
/// With `update`, or when it plays nothing installed, the newest installed release or Nightly.
/// An imported build is no commit to build on: its game files' version is.
pub fn base_for(current: Option<&str>, installed: &[String], update: bool) -> Option<String> {
    let imported = current.and_then(crate::build_import::about).map(|a| a.base);
    match imported.as_deref().or(current) {
        Some(v) if !update && installed.iter().any(|t| t == v) => Some(v.to_owned()),
        _ => newest_installed(installed).cloned(),
    }
}

/// The newer Horizon a build with `update` would move native build `version` to: the newest
/// installed release or Nightly, when it is newer than the one the build started from.
pub fn update_for(version: &str, installed: &[String]) -> Option<String> {
    newer_than(&about(version)?.base, installed)
}

pub fn newer_than(base: &str, installed: &[String]) -> Option<String> {
    newest_installed(installed)
        .filter(|n| *n != base && backend::compare_versions(n, base).is_gt())
        .cloned()
}

/// Every file under `dir`, by its path relative to `dir` (with `/`), sorted. A symlink is refused:
/// horizon-mixins refuses them too.
fn files(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    files_with_api(dir, None)
}

/// Bundle files omit only the API package's Cargo build output.
fn bundle_files(bundle: &Bundle) -> Result<Vec<(String, PathBuf)>, String> {
    files_with_api(&bundle.dir, bundle.api.as_deref())
}

/// The API package's target folder in a canonical bundle root.
fn api_target(root: &Path, api: Option<&str>) -> Result<Option<PathBuf>, String> {
    api.map(|path| root.join(path).canonicalize().map(|p| p.join("target")))
        .transpose()
        .map_err(|e| e.to_string())
}

fn files_with_api(dir: &Path, api: Option<&str>) -> Result<Vec<(String, PathBuf)>, String> {
    fn walk(
        root: &Path,
        dir: &Path,
        skip: Option<&Path>,
        out: &mut Vec<(String, PathBuf)>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            let (path, kind) = (entry.path(), entry.file_type().map_err(|e| e.to_string())?);
            if kind.is_symlink() {
                return Err(format!(
                    "{} is a link: copy the file instead.",
                    path.display()
                ));
            } else if kind.is_dir() {
                if Some(path.as_path()) != skip {
                    walk(root, &path, skip, out)?;
                }
            } else {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                out.push((relative.to_string_lossy().replace('\\', "/"), path));
            }
        }
        Ok(())
    }
    let root = dir
        .canonicalize()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let skip = api_target(&root, api)?;
    let mut out = Vec::new();
    walk(&root, &root, skip.as_deref(), &mut out)?;
    out.sort();
    Ok(out)
}

/// The installed version `bundles` built on Horizon `revision` with horizon-mixins `mixins`
/// become: `native-<7 digits of the revision>-<12 hex digits>`. The two targets' keys never mix:
/// a server build salts its hash, so it is another version than the client build of the same
/// mods, and the client key is unchanged.
pub fn version_for(
    revision: &str,
    mixins: &str,
    profile: Profile,
    target: Target,
    platform: Option<&str>,
    bundles: &[Bundle],
) -> Result<String, String> {
    let mut hash = Sha256::new();
    let (cargo_profile, configs) = profile.cargo();
    for part in [mixins, cargo_profile]
        .into_iter()
        .chain(configs.iter().copied())
        .chain([target.features(), rust_toolchain::VERSION, revision])
        .chain(platform)
    {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    if target == Target::Server {
        hash.update(target.name().as_bytes());
        hash.update([0]);
    }
    // bundles() reads folders in path order, which with their manifests decides trait dispatch
    // order: hashed in that order, a different order is another build.
    for b in bundles {
        hash.update(b.id.as_bytes());
        hash.update([0]);
        for (name, path) in bundle_files(b)? {
            let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            hash.update(name.as_bytes());
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(&bytes);
        }
    }
    let key: String = hash.finalize()[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(format!("{PREFIX}{}-{key}", &revision[..7]))
}

/// A digest of a mod's files (name, length, bytes, sorted by name), kept in `native-mods.json` so
/// the Mods page sees a mod edited or reinstalled without a version bump.
pub fn digest<'a>(files: impl Iterator<Item = (&'a str, &'a [u8])>) -> String {
    let mut hash = Sha256::new();
    for (name, bytes) in files {
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The digest of a bundle's files as the Mods page reads it (`native_status::enabled`): an
/// unpacked `.hnmod`'s whole folder, its `horizon-native.json` included, else the bundle's own
/// folder. A build records it, so a build of the mods turned on reads as up to date.
fn source_digest(bundle: &Bundle) -> Option<String> {
    let root = bundle
        .dir
        .parent()
        .and_then(Path::parent)
        .filter(|root| {
            root.join(NATIVE_MANIFEST).is_file()
                && bundle.dir == root.join("mixins").join(&bundle.id)
        })
        .unwrap_or(&bundle.dir);
    // An installed .hnmod keeps its archive digest for the Mods page.
    let paths = if root == bundle.dir {
        bundle_files(bundle)
    } else {
        files(root)
    };
    let read: Vec<_> = paths
        .ok()?
        .into_iter()
        .map(|(name, path)| Some((name, fs::read(path).ok()?)))
        .collect::<Option<_>>()?;
    Some(digest(read.iter().map(|(n, b)| (n.as_str(), b.as_slice()))))
}

/// Where a build is.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Rust(f32),
    Tool,
    Source(String),
    Mixins,
    /// A previously patched source snapshot was reused.
    CachedMixins,
    /// Which case the build is, before anything runs.
    Plan(Plan, Profile),
    /// Crates done, the total once cargo says it, and the one just started.
    Compile {
        done: usize,
        total: Option<usize>,
        name: String,
        profile: Profile,
    },
    /// The mods' own crates checked against Horizon's `Cargo.lock`.
    Crates,
    /// A mod's post-build step being built.
    PostBuildCompile(String),
    /// A mod's post-build step running in the new build's folder.
    PostBuild(String),
    Install,
    /// This mod was left out of the build.
    LeftOut(LeftOut),
    /// These mods didn't build on the Horizon the instance plays: the build moves to the commit
    /// they were made for.
    OnTheirHorizon(Vec<String>),
    /// The build ended: seconds it took.
    Done {
        secs: u64,
        profile: Profile,
        plan: Plan,
    },
}

impl Step {
    pub fn label(&self) -> String {
        match self {
            Self::Rust(done) => trf(
                "Downloading Rust {version} (once): {done}%",
                &[
                    ("version", rust_toolchain::VERSION),
                    ("done", &format!("{:.0}", done * 100.0)),
                ],
            ),
            Self::Tool => tr("Building horizon-mixins (once)").into(),
            Self::Source(revision) => trf(
                "Downloading Horizon's source at {revision}",
                &[("revision", &revision[..7])],
            ),
            Self::Mixins => tr("Applying the mixins").into(),
            Self::CachedMixins => tr("Reusing patched game source").into(),
            Self::Crates => tr("Checking the mods' crates against Horizon's Cargo.lock").into(),
            Self::Plan(plan, profile) => format!("{} ({})", plan.label(), profile.name()),
            Self::Compile {
                done,
                total,
                name,
                profile,
            } => match total {
                Some(total) => trf(
                    "Compiling the game ({profile}): {done} of {total} crates, now {name}",
                    &[
                        ("profile", profile.name()),
                        ("done", &done.to_string()),
                        ("total", &total.to_string()),
                        ("name", name),
                    ],
                ),
                None => trf(
                    "Compiling the game ({profile}): {done} crates, now {name}",
                    &[
                        ("profile", profile.name()),
                        ("done", &done.to_string()),
                        ("name", name),
                    ],
                ),
            },
            Self::PostBuildCompile(id) => trf("Building {name}'s post-build step", &[("name", id)]),
            Self::PostBuild(id) => trf("Running {name}'s post-build step", &[("name", id)]),
            Self::Install => tr("Installing the build").into(),
            Self::LeftOut(left) => left.label(),
            Self::OnTheirHorizon(mods) => trf(
                "{mods} didn't build on the Horizon you play: building on the one they were made for",
                &[("mods", &mods.join(", "))],
            ),
            Self::Done {
                secs,
                profile,
                plan,
            } => trf(
                "Built in {duration} ({profile}, {plan})",
                &[
                    ("duration", &duration_label(*secs)),
                    ("profile", profile.name()),
                    ("plan", tr(plan.kind())),
                ],
            ),
        }
    }
}

/// Runs `cmd`, its output appended to the build log; `line` sees each line of its standard error.
/// An error ends with its first `error` lines and the last lines: a failed build script's output
/// alone could push the line naming its crate out of the last ones.
fn run(cmd: Command, what: &str, line: impl FnMut(&str)) -> Result<(), String> {
    run_checked(cmd, what, line, None).map_err(|e| e.error)
}

/// A [`run_checked`] that failed: its error, the last lines of its standard error, and the mods
/// the compiler's errors were traced to (`id version`).
struct Ran {
    error: String,
    tail: String,
    owners: Vec<String>,
}

fn run_checked(
    mut cmd: Command,
    what: &str,
    mut line: impl FnMut(&str),
    compiler: Option<diagnostics::Compiler<'_>>,
) -> Result<(), Ran> {
    let ran = |error: String| Ran {
        error,
        tail: String::new(),
        owners: Vec::new(),
    };
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
        .map_err(|e| ran(e.to_string()))?;
    let _ = writeln!(log, "$ {cmd:?}");
    let stdout = if compiler.is_some() {
        cmd.stdout(Stdio::piped());
        Some(log.try_clone().map_err(|e| ran(e.to_string()))?)
    } else {
        cmd.stdout(log.try_clone().map_err(|e| ran(e.to_string()))?);
        None
    };
    let mut child = cmd
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ran(app_control(format!("Could not start {what}: {e}"))))?;
    std::thread::scope(|scope| {
        let messages = compiler.zip(stdout).map(|(compiler, log)| {
            let input = BufReader::new(child.stdout.take().expect("piped"));
            scope.spawn(move || diagnostics::capture(&compiler, input, log))
        });
        let mut tail = std::collections::VecDeque::new();
        let mut errors = Vec::new();
        let mut blocked = false;
        for text in BufReader::new(child.stderr.take().expect("piped"))
            .lines()
            .map_while(Result::ok)
        {
            if let Some(kept) = log_text(&text) {
                let _ = writeln!(log, "{kept}");
            }
            line(&text);
            blocked |= rust_toolchain::blocked_by_app_control(&text);
            if text.starts_with("error") && errors.len() < 4 {
                errors.push(text.clone());
            }
            if tail.len() == 12 {
                tail.pop_front();
            }
            tail.push_back(text);
        }
        let status = child.wait().map_err(|e| ran(e.to_string()))?;
        let messages = messages
            .map(|m| m.join().expect("compiler output reader"))
            .unwrap_or_default();
        if status.success() {
            return Ok(());
        }
        let tail: Vec<String> = tail.into();
        let owners = messages.owners();
        let error = messages.explain(failure(what, &status.to_string(), &errors, &tail));
        Err(Ran {
            error: match blocked {
                true => with_app_control(error),
                false => app_control(error),
            },
            tail: tail.join("\n"),
            owners,
        })
    })
}

/// `error` led by what to do when Windows' Smart App Control refused one of the build's
/// programs (#5911): cargo's own lines only say a build script was "never executed".
fn app_control(error: String) -> String {
    match rust_toolchain::blocked_by_app_control(&error) {
        true => with_app_control(error),
        false => error,
    }
}

fn with_app_control(error: String) -> String {
    format!("{}\n\n{error}", rust_toolchain::smart_app_control())
}

/// [`run`]'s error: the `errors` lines not already among the last ones (`tail`), then those.
fn failure(what: &str, status: &str, errors: &[String], tail: &[String]) -> String {
    let end = diagnostic(&tail.join("\n")).unwrap_or_else(|| tail.join("\n"));
    let first: Vec<&str> = errors
        .iter()
        .filter(|e| !tail.contains(e))
        .map(String::as_str)
        .collect();
    let (shown, first) = if first.is_empty() {
        ("The end of its output", String::new())
    } else {
        (
            "Its first errors and the end of its output",
            format!("{}\n…\n", first.join("\n")),
        )
    };
    format!(
        "{what} failed ({status}). {shown}, the whole of it in {}:\n{first}{end}",
        log_path().display()
    )
}

/// Extracts the mod id from a mixin manifest or mod file path (e.g. `.../mixins.incoming/<mod_id>/...`
/// or `.../mods.incoming/<mod_id>/...`).
fn mod_id_from_file(file: &str) -> Option<&str> {
    for prefix in ["mixins.incoming", "mods.incoming"] {
        if let Some(pos) = file.find(prefix) {
            let rest = &file[pos + prefix.len()..];
            let sub = rest.trim_start_matches(['/', '\\']);
            if let Some(end) = sub.find(['/', '\\']) {
                let id = &sub[..end];
                if !id.is_empty() {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// A horizon-mixins diagnostic (`{"code", "message", "file"}`) in one line, led by the mod it
/// names (`mod_id`, its `injection_id`, or extracted from its `file` path for `E_MODULE_CONFLICT`),
/// so a player with several mods sees which one doesn't apply to the build's Horizon: their files'
/// paths are deep in the launcher's folder.
fn diagnostic(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let value: serde_json::Value = serde_json::from_str(&text[start..]).ok()?;
    let mod_id = value["mod_id"]
        .as_str()
        .or_else(|| value["file"].as_str().and_then(mod_id_from_file));
    let culprit = match (mod_id, value["injection_id"].as_str()) {
        (Some(m), Some(i)) => format!("{m} ({i}): "),
        (Some(m), None) => format!("{m}: "),
        _ => String::new(),
    };
    let mut line = format!(
        "{culprit}{}: {}",
        value["code"].as_str()?,
        value["message"].as_str()?
    );
    if let Some(file) = value["file"].as_str() {
        line += &format!(" ({file})");
    }
    Some(line)
}

/// horizon-mixins at commit `rev`, built once with the launcher's cargo (`cargo install --locked`);
/// `progress` hears [`Step::Tool`] only when it is built.
fn tool(rust: &Toolchain, rev: &str, progress: &impl Fn(Step)) -> Result<PathBuf, String> {
    let dir = root().join("tools").join(&rev[..12]);
    let bin = dir
        .join("bin")
        .join(format!("horizon-mixins{}", std::env::consts::EXE_SUFFIX));
    if !bin.is_file() {
        progress(Step::Tool);
        let mut cmd = rust.command(&rust.cargo());
        rust_toolchain::low_priority(&mut cmd);
        rust.configure_cargo(&mut cmd);
        cmd.args([
            "install", "--locked", "--git", MIXINS_GIT, "--rev", rev, "--root",
        ])
        .arg(&dir)
        .arg("horizon-mixins-cli");
        run(cmd, "Building horizon-mixins", |_| {})?;
    }
    Ok(bin)
}

/// `horizon-mixins apply`: the mods in `mods` applied to `source`, into `out`. It runs
/// `cargo metadata`, so it gets the launcher's toolchain like a build does: on a computer with no
/// Rust of its own, a plain command found no cargo (E_METADATA).
fn apply(rust: &Toolchain, tool: &Path, source: &Path, mods: &Path, out: &Path) -> Command {
    let mut cmd = rust.command(tool);
    cmd.arg("apply").arg("--workspace").arg(source);
    cmd.arg("--mods").arg(mods).arg("--out").arg(out);
    cmd
}

/// The build of the game in `stage` with `profile` (game-build.yml's, for `Release`) and
/// `target`'s features, into `target`. It starts outside any Cargo configuration of the
/// player's ([`rust_toolchain::outside_configs`]), so only the workspace's own
/// (`stage/.cargo/config.toml`, when it has one) applies. A server build compiles headless:
/// `--no-default-features` (the server build, docs/server.md), keeping the `mods` feature when
/// the source has one.
fn compile(
    rust: &Toolchain,
    stage: &Path,
    target: Target,
    target_dir: &Path,
    profile: Profile,
    package: &str,
) -> Command {
    let mut cmd = cargo(rust, stage, "build");
    // Cargo's progress bar (units done of the total) is read from its output. Incremental
    // compilation, the `fast` profile's, is not turned off by the player's environment: the
    // toolchain's command drops `CARGO_INCREMENTAL` and `CARGO_BUILD_*`.
    cmd.env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_TERM_PROGRESS_WHEN", "always")
        .env("CARGO_TERM_PROGRESS_WIDTH", "100");
    let (cargo_profile, configs) = profile.cargo();
    cmd.args([
        "--locked",
        "--message-format=json",
        "--profile",
        cargo_profile,
    ]);
    cmd.args(configs.iter().flat_map(|config| ["--config", config]));
    if target == Target::Server {
        cmd.arg("--no-default-features");
    }
    if has_mods_feature(stage) {
        cmd.args(["--features", FEATURES]);
    }
    cmd.args(["-p", package, "--bin", api::executable(package)]);
    cmd
}

/// The launcher's cargo running `subcommand` on the workspace `dir`, at low priority, with only
/// the workspace's own Cargo configuration ([`compile`]).
fn cargo(rust: &Toolchain, dir: &Path, subcommand: &str) -> Command {
    let mut cmd = rust.command(&rust.cargo());
    rust_toolchain::low_priority(&mut cmd);
    rust.configure_cargo(&mut cmd);
    cmd.arg(subcommand);
    let config = dir.join(".cargo").join("config.toml");
    if config.is_file() {
        cmd.arg("--config").arg(config);
    }
    cmd.arg("--manifest-path").arg(dir.join("Cargo.toml"));
    // Only this stage's declarations apply, never a path inherited from the launcher's shell.
    let injections = dir.join(inject::INPUT);
    if injections.is_file() {
        cmd.env(inject::ENV, injections);
    } else {
        cmd.env_remove(inject::ENV);
    }
    cmd
}

/// Checks the mods' crates (`native-crates/`) against `source`'s `Cargo.lock`, Horizon's own.
/// horizon-mixins already wrote them into `stage`'s lock (and refused a crate the lock lacks,
/// `E_CRATE_DEPENDENCY`), leaving every other package as it was: this checks that again, a crate
/// they name must be in Horizon's lock and every package downloaded exactly as there. Then the
/// crates are fetched, so a feature turning on a dependency the lock lacks fails here, readably.
fn lock_crates(
    rust: &Toolchain,
    source: &Path,
    stage: &Path,
    mods: &[Bundle],
) -> Result<(), String> {
    let read = |dir: &Path| {
        let path = dir.join("Cargo.lock");
        fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let horizon = crates::downloaded(&read(source)?)?;
    let staged = crates::downloaded(&read(stage)?)?;
    // `wanted` reads native-crates, which API-only stages do not have.
    let wanted = if crates::any(stage) {
        crates::wanted(stage)?
    } else {
        Vec::new()
    };
    let api_ids: Vec<&str> = mods
        .iter()
        .filter(|m| m.api.is_some())
        .map(|m| m.id.as_str())
        .collect();
    if let Some(why) = crates::refused(&horizon, Some(&staged), &wanted, &api_ids) {
        return Err(why);
    }
    // Only this computer's crates: the build needs no others.
    let mut fetch = cargo(rust, stage, "fetch");
    fetch.arg("--locked");
    if let Some(host) = rust_toolchain::host() {
        fetch.args(["--target", host.target]);
    }
    run(fetch, "Downloading the crates", |_| {}).map_err(crates::locked)
}

/// Builds the mods' post-build programs with the game's profile in `target`; each step and its
/// program, in mod order. A bin's file name doesn't include its package, so two mods' bins of the
/// same name (or one named like the game) are the same file in `target`: each program is copied
/// to `programs/<step>/` as soon as it is built. horizon-mixins refuses such names too
/// (`E_POST_BUILD`, horizon-mixins#8); this keeps a bundle built by an older one safe.
fn post_build_programs(
    rust: &Toolchain,
    stage: &Path,
    target: &Path,
    programs: &Path,
    profile: Profile,
    progress: &impl Fn(Step),
) -> Result<Vec<(crates::PostBuild, PathBuf)>, String> {
    let steps = crates::post_builds(stage)?;
    // The game's settings, so the crates it built are reused.
    let (profile, configs) = profile.cargo();
    let _ = fs::remove_dir_all(programs);
    let mut out = Vec::new();
    for (i, step) in steps.into_iter().enumerate() {
        progress(Step::PostBuildCompile(step.mod_id.clone()));
        let mut cmd = cargo(rust, stage, "build");
        cmd.env("CARGO_TARGET_DIR", target);
        cmd.args([
            "--locked",
            "--profile",
            profile,
            "-p",
            &step.package,
            "--bin",
            &step.bin,
        ]);
        cmd.args(configs.iter().flat_map(|config| ["--config", config]));
        run(
            cmd,
            &format!("Building {}'s post-build step", step.mod_id),
            |_| {},
        )?;
        let file = format!("{}{}", step.bin, std::env::consts::EXE_SUFFIX);
        let program = programs.join(i.to_string()).join(&file);
        copy_out(&target.join(profile).join(&file), &program)?;
        out.push((step, program));
    }
    Ok(out)
}

/// Horizon's source at commit `revision`: GitHub's archive of it, unpacked once, with the commit in
/// `.horizon-revision` (bundles pin it). A Nightly's commit stays downloadable after the Nightly
/// is gone: its `nightly-source/…` tag keeps it (docs/release-flow.md). Links are left out:
/// horizon-mixins refuses them, and none is needed to build.
fn source(revision: &str) -> Result<PathBuf, String> {
    let dir = root().join("source").join(revision);
    if dir.join(".horizon-revision").is_file() {
        return Ok(dir);
    }
    let archive = format!(
        "https://codeload.github.com/{}/tar.gz/{revision}",
        backend::REPO
    );
    let partial = root().join("source").join(format!(".{revision}.partial"));
    let top = unpack_archive(&archive, &partial, u64::MAX)?;
    fs::write(top.join(".horizon-revision"), revision).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&top, &dir).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&partial);
    Ok(dir)
}

/// GitHub's archive (`.tar.gz`) at `url`, unpacked into the fresh folder `partial`: the archive's
/// one top folder, which is returned. Links are left out; at most `limit` bytes are unpacked.
pub(crate) fn unpack_archive(url: &str, partial: &Path, limit: u64) -> Result<PathBuf, String> {
    use std::io::Read;
    let _ = fs::remove_dir_all(partial);
    fs::create_dir_all(partial).map_err(|e| e.to_string())?;
    let reader = flate2::read::GzDecoder::new(backend::get(url)?.into_reader()).take(limit);
    let mut tar = tar::Archive::new(reader);
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| format!("Download interrupted: {e}"))?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            continue;
        }
        entry
            .unpack_in(partial)
            .map_err(|e| format!("Could not unpack the source: {e}"))?;
    }
    // The archive's one top folder (<repo>-<commit>/) is the source's root.
    fs::read_dir(partial)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .ok_or_else(|| "The source archive is empty.".into())
}

/// Makes `to` hold `from`'s files, writing only those that changed, so Cargo, which goes by file
/// times, recompiles only what the mods changed. A file written gets the current time, not
/// `from`'s: a copy keeps the original's time on Windows and macOS, and an unpacked source has
/// its commit's, which can be older than the last build. Cargo would take that file for one it
/// already compiled and link the older crate (#10421).
fn sync(from: &Path, to: &Path) -> Result<(), String> {
    let new = files(from)?;
    let old = if to.is_dir() { files(to)? } else { Vec::new() };
    for (name, path) in &new {
        let target = to.join(name);
        if fs::read(&target).ok() != fs::read(path).ok() {
            fs::create_dir_all(target.parent().unwrap_or(to)).map_err(|e| e.to_string())?;
            fs::copy(path, &target)
                .and_then(|_| fs::File::options().write(true).open(&target))
                .and_then(|file| file.set_modified(std::time::SystemTime::now()))
                .map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    for (name, path) in old {
        if new.binary_search_by(|(n, _)| n.cmp(&name)).is_err() {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

/// Marks a `stage/` whose files all got the time they were written ([`sync`]).
const STAGE_STAMPED: &str = "stage.stamped";

/// The stage the compiler reads, in `root`. One written before #10421 can hold files with their
/// commit's time over crates compiled from another commit: it is written again, once, so each of
/// its files is newer than what `target/` holds.
fn stage_dir(root: &Path) -> Result<PathBuf, String> {
    let stage = root.join("stage");
    let stamped = root.join(STAGE_STAMPED);
    if !stamped.exists() {
        if stage.exists() {
            fs::remove_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
        }
        fs::write(&stamped, "").map_err(|e| format!("{}: {e}", stamped.display()))?;
    }
    Ok(stage)
}

/// Copy a bundle without its API package's Cargo build output.
fn copy_bundle(bundle: &Bundle, to: &Path) -> Result<(), String> {
    let root = bundle.dir.canonicalize().map_err(|e| e.to_string())?;
    let skip = api_target(&root, bundle.api.as_deref())?;
    backend::copy_dir_except(&root, to, skip.as_deref()).map_err(|e| e.to_string())
}

/// Stage only manifests horizon-mixins can apply, after trait injections are prepared.
fn stage_mixins(mods: &[Bundle], to: &Path) -> Result<(), String> {
    if to.exists() {
        fs::remove_dir_all(to).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for bundle in mods.iter().filter(|b| b.mixins_content) {
        copy_bundle(bundle, &to.join(&bundle.id))?;
    }
    Ok(())
}

/// The `data/` of the bundles horizon-mixins doesn't apply, staged in `stage` where it stages the
/// others' (`native-data/<mod id>/`), for `native_data!` and the mods' build steps.
fn stage_trait_data(mods: &[Bundle], stage: &Path) -> Result<(), String> {
    for bundle in mods.iter().filter(|b| !b.mixins_content) {
        let data = bundle.dir.join("data");
        if data.is_dir() {
            let to = stage.join("native-data").join(&bundle.id);
            backend::copy_dir(&data, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    Ok(())
}

/// What a build of `folder`'s mods came out as: the version it installed and what it did.
#[derive(Clone, Debug)]
struct Built {
    version: String,
    /// Which case the build was, said before anything ran.
    plan: Plan,
    /// The profile it compiled with: the chosen one, unless the source has no `[profile.fast]`.
    profile: Profile,
    /// How long the whole build took.
    secs: u64,
    /// The mods turned on that it was built without.
    left_out: Vec<LeftOut>,
}

/// Why [`pipeline`] stopped.
enum Failed {
    /// These mods stopped it (`error` says how), while `compiling` or before: it can run again
    /// without them.
    Mods {
        left: Vec<LeftOut>,
        error: String,
        compiling: bool,
    },
    /// Every mod was left out.
    Nothing,
    Other(String),
}

impl From<String> for Failed {
    fn from(error: String) -> Self {
        Self::Other(error)
    }
}

impl From<Failed> for String {
    fn from(failed: Failed) -> Self {
        match failed {
            Failed::Mods { error, .. } | Failed::Other(error) => error,
            Failed::Nothing => String::new(),
        }
    }
}

/// The pipeline both targets build with ([`build`], [`build_server`]): `folder`'s mods on the
/// Horizon revision they pin (else `base`'s, the installed version whose game files the build
/// reuses: a native build's own revision when it is one, [`base_for`]), compiled as `target`
/// (the game, or the headless engine) and installed as its own version,
/// `native-<revision>-<key>` (the key hashes the revision, the mods' files, the tool's commit,
/// the profile and the target). A build already made is used again. Mods pinning different
/// revisions build on the newest one, with the newest horizon-mixins they name, only when the
/// player agreed to try (`anyway`): else the error starts with [`MISMATCH`] and says which.
/// With `on_base`, every mod builds on `base`'s own commit, pinned or not (the player's choice).
/// The mods in `skip` and those needing them are left out; with `isolate`, a mod that alone
/// stops horizon-mixins or the compiler ends it with [`Failed::Mods`], so it can be left out too.
/// Nothing of the player's outside the launcher's data is touched: no instance is edited and
/// no version selected here.
#[allow(clippy::too_many_arguments)]
fn pipeline(
    base: &str,
    folder: &Path,
    anyway: bool,
    on_base: bool,
    profile: Profile,
    target: Target,
    progress: &impl Fn(Step),
    skip: &[LeftOut],
    isolate: bool,
) -> Result<Built, Failed> {
    let started = std::time::Instant::now();
    backend::need_prepared(base)?;
    let platform = platform_for_game(base)?;
    fs::create_dir_all(root()).map_err(|e| e.to_string())?;
    // The mods are copied first: the build and its key use the copy, whatever happens to the
    // folder meanwhile.
    let incoming = root().join("mods.incoming");
    let _ = fs::remove_dir_all(&incoming);
    let found = bundles(folder)?;
    // #6490: said before anything runs, so a build that fails later still
    // shows them.
    for warning in warn_conflicts(&found) {
        log_warning(&warning);
    }
    // A native build starts from the release or Nightly it was made from, on its own revision.
    let built_on = about(base);
    let origin = built_on.as_ref().map_or(base, |a| a.base.as_str());
    let mut ids = std::collections::HashSet::new();
    for b in &found {
        if !ids.insert(&b.id) {
            return Err(format!("Duplicate native mod id {:?}.", b.id).into());
        }
        copy_bundle(b, &incoming.join(&b.id))?;
    }
    // Read before `repin` rewrites a copy's manifest.
    let digests: std::collections::HashMap<String, Option<String>> = found
        .iter()
        .map(|b| (b.id.clone(), source_digest(b)))
        .collect();
    let mods: Vec<Bundle> = found
        .into_iter()
        .map(|b| Bundle {
            dir: incoming.join(&b.id),
            ..b
        })
        .collect();
    let declared: Vec<Vec<dependencies::Dependency>> = mods
        .iter()
        .map(|bundle| {
            let path = bundle.dir.join(MANIFEST);
            let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let table: toml::Table = text
                .parse()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            dependencies::parse(&bundle.id, table.get("dependencies")).map_err(|e| e.to_string())
        })
        .collect::<Result<_, String>>()?;
    let pairs: Vec<(&Bundle, &[dependencies::Dependency])> = mods
        .iter()
        .zip(&declared)
        .map(|(b, d)| (b, d.as_slice()))
        .collect();
    let left_out = isolate::with_dependents(skip, &pairs);
    let (mods, declared): (Vec<Bundle>, Vec<Vec<dependencies::Dependency>>) = mods
        .into_iter()
        .zip(declared)
        .filter(|(b, _)| !left_out.iter().any(|l| l.id == b.id))
        .unzip();
    if mods.is_empty() {
        return Err(Failed::Nothing);
    }
    let inputs: Vec<_> = mods
        .iter()
        .zip(&declared)
        .map(|(bundle, deps)| dependencies::Mod {
            id: &bundle.id,
            version: &bundle.version,
            has_api: bundle.api.is_some(),
            dependencies: deps,
        })
        .collect();
    let resolution = dependencies::resolve(&inputs).map_err(|e| e.to_string())?;
    let ordered: Vec<Bundle> = resolution.order.iter().map(|&i| mods[i].clone()).collect();
    // A mod naming a hook points' version doesn't pin: it follows the others, else the game.
    let pins: Vec<Bundle> = mods.iter().filter(|b| b.hooks.is_none()).cloned().collect();
    let free = pins.iter().all(|b| b.horizon.is_none());
    let on_base = on_base || built_on.as_ref().is_some_and(|a| a.on_base);
    // The commit of the release or Nightly the build starts from. A native build's own is that
    // of the version it started from when it recorded it; else, outside `on_base`, the commit it
    // compiled, so a rebuild stays on the Horizon it plays.
    let base_revision = || match &built_on {
        Some(about) => match &about.base_horizon {
            Some(r) => Ok(r.clone()),
            None if !on_base && !about.off_its_nightly() => Ok(about.horizon.clone()),
            None => revision_of(origin),
        },
        None => revision_of(base),
    };
    let date = |r: &str| committed(backend::REPO, r);
    let (pinned, mut pinned_by) = pinned_revision(&pins, on_base, base_revision, date)?;
    // Known without asking GitHub again unless a pin chose the commit.
    let base_horizon = match pinned_by.is_empty() {
        true => Some(pinned.clone()),
        false => base_revision().ok(),
    };
    let game = || revision_of(origin).ok();
    let revision = with_hooks(&mods, pinned.clone(), free, game, hooks_at, catalog_at)?;
    // Nothing to say when the pin is the base's own commit, or the hook points moved the build.
    if base_horizon.as_ref() == Some(&revision) || revision != pinned {
        pinned_by.clear();
    }
    let other = mismatch(&mods, &revision);
    if let (Some(list), false) = (&other, anyway || on_base) {
        return Err(format!("{MISMATCH}{list}.").into());
    }
    if let Some(list) = &other {
        log_warning(&format!(
            "Some mods were made for other Horizon versions ({list}). Building anyway on {}.",
            &revision[..revision.len().min(7)]
        ));
    }
    for b in mods
        .iter()
        .filter(|b| b.hooks.is_some() || b.horizon.as_ref().is_some_and(|r| *r != revision))
    {
        repin(b, &revision)?;
    }
    // Said with a failure: a mod made for another revision may not apply or compile on this one.
    let blame = |e: String| match &other {
        Some(list) => {
            let by = match pinned_by.as_slice() {
                [] => String::new(),
                [single] => format!(" (pinned by {single})"),
                many => format!(" (pinned by {})", many.join(", ")),
            };
            format!(
                "{e}\n\nSome mods were made for other Horizon versions ({list}{by}), which may be why. Install versions made for the same Nightly."
            )
        }
        None => e,
    };
    let mixins = newest(
        &mods,
        "horizon-mixins",
        |b| b.mixins.as_ref(),
        || Ok(MIXINS_REV.into()),
        Some(MIXINS_REV),
        |r| committed("horizon-rust/horizon-mixins", r),
    )?;
    let version = version_for(
        &revision,
        &mixins,
        profile,
        target,
        platform.cache_key(),
        &mods,
    )?;
    let now = Record {
        horizon: revision.clone(),
        mixins: mixins.clone(),
        mods: mods
            .iter()
            .map(|m| Ok((m.id.clone(), mod_hash(m)?)))
            .collect::<Result<_, String>>()?,
    };
    let exists = target.directory(&version).join(".installed").is_file();
    let plan = classify(
        exists,
        last_record(target, profile, platform.cache_key()).as_ref(),
        &now,
    );
    progress(Step::Plan(plan.clone(), profile));
    let mut built = Built {
        version: version.clone(),
        plan: plan.clone(),
        profile,
        secs: 0,
        left_out: left_out.clone(),
    };
    if !exists {
        match target {
            // The headless build links none of the sound or controller libraries: it needs
            // only a C compiler and the C library's headers ([`rust_toolchain::linker_server`]).
            Target::Client => rust_toolchain::linker()?,
            Target::Server => rust_toolchain::linker_server()?,
        }
        let rust = rust_toolchain::install(|f| progress(Step::Rust(f)))?;
        let needs_mixins = mods.iter().any(|m| m.mixins_content);
        progress(Step::Source(revision.clone()));
        let source = source(&revision)?;
        // `root()` is already the form horizon-mixins and MinGW take: resolved (no link, which
        // it refuses) and, on Windows, ASCII (which they can't read past), both by build_dir.
        let real = root();
        let fresh = real.join("stage.new");
        let _ = fs::remove_dir_all(&fresh);
        if needs_mixins {
            let key = inject_cache::key(&revision, &mixins, profile, &ordered)?;
            let snapshot = inject_cache::snapshot(&real, &key);
            if snapshot.is_dir() {
                progress(Step::CachedMixins);
                sync(&snapshot, &fresh)?;
            } else {
                let injections = inject::prepare(&ordered, &source)?;
                let tool = tool(&rust, &mixins, &progress)?;
                let mixin_input = real.join("mixins.incoming");
                stage_mixins(&ordered, &mixin_input)?;
                progress(Step::Mixins);
                let result = run_checked(
                    apply(&rust, &tool, &source, &mixin_input, &fresh),
                    "horizon-mixins",
                    |_| {},
                    None,
                );
                let _ = fs::remove_dir_all(&mixin_input);
                if let Err(ran) = result {
                    let error = blame(ran.error);
                    return Err(match isolate::from_mixins(&ran.tail, &ordered) {
                        Some(culprit) if isolate => Failed::Mods {
                            left: vec![culprit],
                            error,
                            compiling: false,
                        },
                        _ => Failed::Other(error),
                    });
                }
                inject::stage(&fresh, &injections)?;
                let parent = snapshot.parent().ok_or("invalid cache path".to_owned())?;
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                let partial = parent.join(format!(".{key}.partial"));
                let _ = fs::remove_dir_all(&partial);
                sync(&fresh, &partial)?;
                fs::rename(&partial, &snapshot).map_err(|e| e.to_string())?;
            }
            inject_cache::touch_and_prune(&real, &key)?;
        } else {
            sync(&source, &fresh)?;
        }
        stage_trait_data(&mods, &fresh)?;
        if mods.iter().any(|m| m.api.is_some()) {
            // Offline metadata resolves all targets, including packages not fetched for this host.
            let mut fetch = cargo(&rust, &source, "fetch");
            fetch.arg("--locked");
            run(fetch, "Downloading Horizon's crates", |_| {})?;
        }
        let has_api = api::stage(&fresh, &mods, &declared, &resolution)?;
        if has_api {
            // Add path packages to Cargo.lock before the locked dependency check.
            let mut metadata = cargo(&rust, &fresh, "metadata");
            metadata.args(["--offline", "--format-version", "1"]);
            run(metadata, "Resolving trait mod crates", |_| {})?;
        }
        // Absolute: the compiler runs from elsewhere (`compile`).
        let stage = stage_dir(&real)?;
        sync(&fresh, &stage)?;
        let _ = fs::remove_dir_all(&fresh);
        if crates::any(&stage) || has_api {
            progress(Step::Crates);
            lock_crates(&rust, &source, &stage, &mods)?;
        }
        let mut target_dir = real.join("target");
        if let Some(cache_dir) = platform.cache_dir {
            target_dir.push(cache_dir);
        }
        // A revision without `[profile.fast]` builds with `release`: the build says so.
        let compiled = profile_of(&stage, profile);
        built.profile = compiled;
        let (mut crates, mut total, mut name) = (0, None, String::new());
        let identities: Vec<_> = mods
            .iter()
            .map(|m| (m.id.clone(), m.version.clone()))
            .collect();
        run_checked(
            compile(
                &rust,
                &stage,
                target,
                &target_dir,
                compiled,
                api::game_package(&stage, target, has_api),
            ),
            "Compiling the game",
            |line| {
                for event in parse_cargo(line) {
                    match event {
                        Cargo::Compiling(n) => {
                            crates += 1;
                            name = n;
                        }
                        Cargo::Progress(done, all) => {
                            crates = crates.max(done);
                            total = Some(all);
                        }
                    }
                    if !name.is_empty() {
                        progress(Step::Compile {
                            done: crates,
                            total,
                            name: name.clone(),
                            profile: compiled,
                        });
                    }
                }
            },
            Some(diagnostics::Compiler {
                stage: &stage,
                source: &source,
                mods: &identities,
            }),
        )
        .map_err(|ran| {
            let error = blame(crates::locked(ran.error));
            match isolate::from_compiler(&ran.owners, &mods) {
                left if isolate && !left.is_empty() => Failed::Mods {
                    left,
                    error,
                    compiling: true,
                },
                _ => Failed::Other(error),
            }
        })?;
        api::as_client(&stage, target, has_api, &target_dir.join(compiled.cargo().0))?;
        if let Some(expected) = platform.machine {
            let executable = target_dir.join(compiled.cargo().0).join(backend::CLIENT);
            let actual = pe_machine(&executable).map_err(|error| {
                trf(
                    "Could not read {name}: {error}",
                    &[
                        ("name", &executable.display().to_string()),
                        ("error", &error),
                    ],
                )
            })?;
            if actual != expected {
                return Err(architecture_mismatch(
                    &format!("PE machine 0x{expected:04x}"),
                    &format!("PE machine 0x{actual:04x}"),
                )
                .into());
            }
        }
        save_record(target, profile, platform.cache_key(), &now);
        // Out of `target_dir` before any post-build bin is built there (`post_build_programs`).
        let binary = real.join("programs").join(target.binary());
        copy_out(
            &target_dir.join(compiled.cargo().0).join(backend::CLIENT),
            &binary,
        )?;
        let post = post_build_programs(
            &rust,
            &stage,
            &target_dir,
            &real.join("programs").join("post-build"),
            compiled,
            progress,
        )?;
        progress(Step::Install);
        let about = serde_json::json!({
            "base": origin,
            "horizon": revision,
            "base_horizon": base_horizon,
            "pinned_by": pinned_by,
            "on_base": on_base,
            "horizon_mixins": mixins,
            "hooks": mods.iter().find_map(|m| m.hooks),
            "rust": rust_toolchain::VERSION,
            "profile": compiled.name(),
            "target": target.name(),
            "rust_target": platform.target,
            "mods": mods.iter().map(|m| serde_json::json!({
                "id": m.id,
                "version": m.version,
                "sha256": digests.get(&m.id).cloned().flatten(),
            })).collect::<Vec<_>>(),
            "left_out": left_out.iter().map(LeftOut::to_json).collect::<Vec<_>>(),
        });
        install(base, &version, target, &binary, &about, |dir| {
            for (step, program) in &post {
                progress(Step::PostBuild(step.mod_id.clone()));
                let cmd = crates::post_build(program, step, dir, &real.join("cache"), &stage)?;
                run(cmd, &format!("{}'s post-build step", step.mod_id), |_| {})?;
            }
            Ok(())
        })?;
        let _ = fs::remove_dir_all(real.join("programs"));
    } else {
        // Used again: it counts as the newest of the builds kept (`prune`).
        let _ = fs::write(target.directory(&version).join(".installed"), "");
    }
    let _ = fs::remove_dir_all(&incoming);
    built.secs = started.elapsed().as_secs();
    Ok(built)
}

/// How many times a build compiles again without the mods the compiler's errors were traced to:
/// each costs a compile, where a mod that doesn't apply costs only horizon-mixins' run.
const COMPILE_RETRIES: usize = 3;

/// Runs `pipeline` until it builds, each time without the mods that stopped it before (they
/// are said with `progress` as they are left out, and those needing them once it built). The
/// error, when it can't, starts with the mods already left out; they come with it.
fn isolated(
    mut pipeline: impl FnMut(&[LeftOut]) -> Result<Built, Failed>,
    progress: &impl Fn(Step),
) -> Result<Built, (String, Vec<LeftOut>)> {
    let mut skip: Vec<LeftOut> = Vec::new();
    let mut compiles = 0;
    let mut last = String::new();
    let said = |skip: &[LeftOut], error: String| {
        let labels: Vec<String> = skip.iter().map(LeftOut::label).collect();
        match labels.is_empty() {
            true => error,
            false => format!("{}\n\n{error}", labels.join("\n")),
        }
    };
    loop {
        match pipeline(&skip) {
            Ok(built) => {
                for left in built.left_out.iter().filter(|l| !skip.contains(l)) {
                    log_warning(&left.label());
                    progress(Step::LeftOut(left.clone()));
                }
                return Ok(built);
            }
            Err(Failed::Mods {
                left,
                error,
                compiling,
            }) => {
                compiles += usize::from(compiling);
                let new: Vec<LeftOut> = left
                    .into_iter()
                    .filter(|l| !skip.iter().any(|s| s.id == l.id))
                    .collect();
                if new.is_empty() || compiles > COMPILE_RETRIES {
                    return Err((said(&skip, error), skip));
                }
                for left in new {
                    log_warning(&left.label());
                    progress(Step::LeftOut(left.clone()));
                    skip.push(left);
                }
                last = error;
            }
            Err(Failed::Nothing) => return Err((said(&skip, last), skip)),
            Err(Failed::Other(error)) => return Err((said(&skip, error), skip)),
        }
    }
}

/// Builds `folder`'s mods on `base`'s commit first, pinned or not (`base` is the installed version
/// whose game files the build reuses: a native build's own revision when it is one, [`base_for`]),
/// and points `instance` at the build, the default one included. A mod made for another commit
/// that doesn't build there moves the build to the revision the mods pin, once (else the first
/// build stays). Returns the build's version; a build already made is used again. Mods pinning
/// different revisions build on the newest one, with the newest horizon-mixins they name, only
/// when the player agreed to try (`anyway`): else the error starts with [`MISMATCH`] and says
/// which. With `on_base`, the player's Rebuild on Horizon, nothing moves off `base`'s commit.
pub fn build(
    base: &str,
    folder: &Path,
    instance: &Instance,
    anyway: bool,
    on_base: bool,
    profile: Profile,
    progress: impl Fn(Step),
) -> Result<String, String> {
    if let Some(error) = backend::native_mod_architecture_error() {
        return Err(error);
    }
    logged(|| {
        let run = |base: &str, on_base: bool| {
            isolated(
                |skip| {
                    pipeline(
                        base,
                        folder,
                        anyway,
                        on_base,
                        profile,
                        Target::Client,
                        &progress,
                        skip,
                        true,
                    )
                },
                &progress,
            )
        };
        let point = |version: &str| {
            instances::edit(&instance.id, |meta| meta.version = Some(version.to_owned()))
        };
        // The first build may fail outright when the mods made elsewhere are all it had (every
        // mod left out) or the ones left could not build without them: those mods still move it.
        let (first, left) = match run(base, true) {
            Ok(built) => {
                point(&built.version)?;
                let left = built.left_out.clone();
                (Ok(built), left)
            }
            Err((error, left)) => (Err(error), left),
        };
        let theirs = match on_base {
            true => Vec::new(),
            false => made_elsewhere(&bundles(folder)?, &left),
        };
        let mut built = match first {
            Err(error) if theirs.is_empty() => return Err(error),
            first => first,
        };
        if !theirs.is_empty() {
            progress(Step::OnTheirHorizon(theirs));
            let origin = about(base).map_or_else(|| base.to_owned(), |a| a.base);
            match (run(&origin, false), &built) {
                (Ok(moved), _) => {
                    point(&moved.version)?;
                    built = Ok(moved);
                }
                (Err((e, _)), _) if e.starts_with(MISMATCH) => return Err(e),
                (Err((e, _)), Ok(_)) => log_warning(&e),
                (Err((e, _)), Err(_)) => return Err(e),
            }
        }
        let built = built?;
        prune(&built.version);
        if built.plan != Plan::UpToDate {
            progress(Step::Done {
                secs: built.secs,
                profile: built.profile,
                plan: built.plan,
            });
        }
        Ok(built.version)
    })
}

/// The mods in `bundles` a build left out (`left_out`) for not building on its commit though they
/// pin another one: on theirs, they may.
fn made_elsewhere(bundles: &[Bundle], left_out: &[LeftOut]) -> Vec<String> {
    bundles
        .iter()
        .filter(|b| b.hooks.is_none() && b.horizon.is_some())
        .filter(|b| (left_out.iter()).any(|l| l.id == b.id && l.why == isolate::Why::OtherVersion))
        .map(|b| b.id.clone())
        .collect()
}

/// Builds `folder`'s mods as the headless engine a dedicated server runs (`horizon-engine`,
/// docs/server.md): the same pipeline as [`build`] on the same revision, compiled with
/// `--no-default-features`, installed as its own version (its key distinct from the game's, so
/// the two never stand in for each other) and exported into `output` as a standalone runtime
/// folder: the built engine, the base version's resources, the mods' post-build data and the
/// build's record. The folder lives outside the launcher's managed versions, where nothing of
/// the launcher's can remove it; nothing of it is overwritten, and a folder already at `output`
/// is refused before anything is downloaded or compiled. No instance is edited and no version
/// selected: a server build changes nothing the launcher's window plays. Returns the absolute
/// path of the exported engine binary.
pub fn build_server(
    base: &str,
    folder: &Path,
    output: &Path,
    anyway: bool,
    on_base: bool,
    profile: Profile,
    progress: impl Fn(Step),
) -> Result<PathBuf, String> {
    logged(|| {
        if output.symlink_metadata().is_ok() {
            return Err(trf(
                "{path} already exists. Move it away, or build into another folder.",
                &[("path", &output.display().to_string())],
            ));
        }
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let output = parent.canonicalize().map_err(|e| e.to_string())?.join(
            output
                .file_name()
                .ok_or_else(|| tr("Choose a new folder for the server export."))?,
        );
        for managed in [backend::version_dir(""), root()] {
            if managed
                .canonicalize()
                .is_ok_and(|dir| output.starts_with(dir))
            {
                return Err(tr("Export the server outside the launcher's managed folders.").into());
            }
        }
        // A server plays exactly the mods it was given: none is left out.
        let built = pipeline(
            base,
            folder,
            anyway,
            on_base,
            profile,
            Target::Server,
            &progress,
            &[],
            false,
        )
        .map_err(String::from)?;
        export(&Target::Server.directory(&built.version), &output)?;
        // Exports are independent; keep only the newest cached server builds.
        let cached = root().join("server-builds");
        let builds = fs::read_dir(&cached)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join(".installed").is_file())
            .map(|e| {
                let at = fs::metadata(e.path().join(".installed"))
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                (e.file_name().to_string_lossy().into_owned(), at)
            })
            .collect();
        for old in stale(builds, &built.version, &[]) {
            let _ = fs::remove_dir_all(cached.join(old));
        }
        if built.plan != Plan::UpToDate {
            progress(Step::Done {
                secs: built.secs,
                profile: built.profile,
                plan: built.plan,
            });
        }
        let engine = output.join(Target::Server.binary());
        Ok(std::path::absolute(&engine).unwrap_or(engine))
    })
}

/// Copies the built version folder `from` into `output` (outside the managed versions) as one
/// piece: staged in a hidden folder beside `output`, then renamed into place, so an interrupted
/// export leaves nothing half-written at `output` and overwrites nothing already there.
/// Executable files keep their permissions ([`backend::copy_dir`], a clone where the disk can).
fn export(from: &Path, output: &Path) -> Result<(), String> {
    let name = output.file_name().ok_or_else(|| {
        trf(
            "{path} is not a folder to build into.",
            &[("path", &output.display().to_string())],
        )
    })?;
    let parent = output.parent().unwrap_or(output);
    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let staging = parent.join(format!(
        ".{}.{}.exporting",
        name.to_string_lossy(),
        std::process::id()
    ));
    fs::create_dir(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    if let Err(e) = copy_runtime(from, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(trf(
            "Could not copy the build: {error}",
            &[("error", &e.to_string())],
        ));
    }
    if output.symlink_metadata().is_ok() {
        let _ = fs::remove_dir_all(&staging);
        return Err(trf(
            "{path} already exists. Move it away, or build into another folder.",
            &[("path", &output.display().to_string())],
        ));
    }
    fs::rename(&staging, output).map_err(|e| {
        let _ = fs::remove_dir_all(&staging);
        trf(
            "Could not move the build to {path}: {error}",
            &[
                ("path", &output.display().to_string()),
                ("error", &e.to_string()),
            ],
        )
    })
}

/// Reflinks are independent files but may not preserve executable permissions.
fn copy_runtime(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let destination = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_runtime(&entry.path(), &destination)?;
        } else {
            horizon_prepare::clone_or_copy(&entry.path(), &destination)?;
            fs::set_permissions(&destination, entry.metadata()?.permissions())?;
        }
    }
    Ok(())
}

/// Copies the program `from` (in the shared target folder) to `to`, keeping it executable.
fn copy_out(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", from.display()))
}

/// Installs `binary` as version `version`: a copy of installed version `base`'s folder (its
/// resources included; cloned where the disk can) with the built binary, as `target` names it,
/// in place of `base`'s. `finish` (the mods' post-build steps) runs in its folder before it
/// counts as installed; when it fails, the folder is removed.
fn install(
    base: &str,
    version: &str,
    target: Target,
    binary: &Path,
    about: &serde_json::Value,
    finish: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let from = backend::version_dir(base);
    let partial = target.directory(&format!(".{version}.partial"));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    copy_base(&from, &partial, target)?;
    let installed = partial.join(target.binary());
    fs::copy(binary, &installed).map_err(|e| format!("The built game: {e}"))?;
    fs::write(partial.join("native-mods.json"), about.to_string()).map_err(|e| e.to_string())?;
    let dir = target.directory(version);
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&partial, &dir).map_err(|e| e.to_string())?;
    if let Err(e) = finish(&dir) {
        let _ = fs::remove_dir_all(&dir);
        return Err(e);
    }
    fs::write(dir.join(".installed"), "").map_err(|e| e.to_string())
}

/// Copies version folder `from` into `to` but for what a new build of `target` makes its own
/// ([`carried_over`]), cloned where the disk can. Also an imported build's start (`build_import`).
pub(crate) fn copy_base(from: &Path, to: &Path, target: Target) -> Result<(), String> {
    for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name();
        if !carried_over(&name, target) {
            continue;
        }
        let to = to.join(&name);
        let copied = if entry.path().is_dir() {
            backend::copy_dir(&entry.path(), &to)
        } else {
            // A clone is a new file: horizon-ctl and horizon-server keep their permissions.
            horizon_prepare::clone_or_copy(&entry.path(), &to)
                .and_then(|()| fs::set_permissions(&to, entry.metadata()?.permissions()))
        };
        copied.map_err(|e| format!("Could not copy {}: {e}", entry.path().display()))?;
    }
    Ok(())
}

/// Whether a file of the version a build starts from goes into the new build. The game (the
/// target's own binary), its record, and the mods' post-build data (`native-data/`, which the
/// new build's steps write again) are the new build's own: a mod turned off leaves none of its
/// data behind. A server build is the headless engine: it keeps neither the game's own binary
/// nor any `horizon-engine` the base shipped, which would run unmodded beside its own.
fn carried_over(name: &std::ffi::OsStr, target: Target) -> bool {
    let mut own: Vec<String> = ["Horizon.app", "native-mods.json", "native-data"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    own.push(backend::CLIENT.to_string());
    own.push(".installed".to_string());
    if target == Target::Server {
        own.push("horizon-engine".to_string());
    }
    let named = |n: &str| name == std::ffi::OsStr::new(n);
    !own.iter()
        .any(|n| named(n) || named(&format!("{n}{}", std::env::consts::EXE_SUFFIX)))
}

/// How many native builds no instance plays stay installed, the newest first: switching a mod
/// off and on again plays the kept build again, without compiling.
const SPARE_BUILDS: usize = 3;

/// Removes the native builds no instance plays, but `keep` and the [`SPARE_BUILDS`] newest. The
/// compiled crates (`native/target`) stay, so the next build only compiles what changed.
pub fn prune(keep: &str) {
    let played: Vec<String> = instances::list()
        .into_iter()
        .filter_map(|i| i.version)
        .collect();
    let builds = backend::installed()
        .into_iter()
        .filter(|v| is_native(v))
        .map(|v| {
            let made = fs::metadata(backend::version_dir(&v).join(".installed"))
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (v, made)
        })
        .collect();
    for old in stale(builds, keep, &played) {
        if backend::in_use(&old) {
            continue;
        }
        let _ = backend::remove(&old);
    }
}

/// The native `builds` (with when each was made or last used) to remove: those no instance plays
/// (`played`), other than `keep`, past the [`SPARE_BUILDS`] newest.
fn stale(
    mut builds: Vec<(String, std::time::SystemTime)>,
    keep: &str,
    played: &[String],
) -> Vec<String> {
    builds.retain(|(v, _)| v != keep && !played.contains(v));
    builds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    builds
        .into_iter()
        .skip(SPARE_BUILDS)
        .map(|(v, _)| v)
        .collect()
}

#[cfg(test)]
#[path = "native_mods_tests.rs"]
mod tests;
