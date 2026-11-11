//! Per-user setup on Windows, no administrator needed.
//!
//! `horizon-launcher-setup.exe` (the launcher's own binary under another name, made by the
//! release workflow) copies itself to `%LOCALAPPDATA%\Programs\Horizon Launcher\horizon-launcher.exe`,
//! makes the Start menu shortcut (and the desktop one if asked) and the Uninstall entry under HKCU,
//! and starts the installed copy. `horizon-launcher.exe uninstall` takes it all away again: the
//! launcher's data folder only when asked, the game's folder never.
//!
//! Nothing here deletes a folder it didn't make: the install folder is always called
//! "Horizon Launcher" ([`folder_for`]) and is removed with `remove_dir`, which only takes an empty one.
//!
//! Command lines (handled in `main` before the CLI parser, so MCP never sees them):
//! `horizon-launcher-setup.exe` (window), `... setup --quiet [--dir <folder>] [--desktop-shortcut]`,
//! `horizon-launcher.exe uninstall` (window), `... uninstall --quiet [--remove-data]`.
use crate::backend;
use crate::data_location::{self, Roots};
use crate::shortcut;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

pub const APP_NAME: &str = "Horizon Launcher";
const BINARY: &str = "horizon-launcher.exe";
const PUBLISHER: &str = "Horizon";
const ABOUT_URL: &str = "https://playhorizon.dev";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\HorizonLauncher";
const OWN_KEY: &str = r"Software\Horizon\Launcher";
const ASIDE_PREFIX: &str = "horizon-launcher-uninstall-";

pub use crate::setup_args::Mode;
use crate::setup_args::{classify, Invocation};

/// The parts of the work, for the progress line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Uninstall: asking a running launcher to quit.
    Closing,
    Files,
    Shortcuts,
    Registry,
    Removing,
    Data,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Where the launcher goes (always a folder called "Horizon Launcher").
    pub dir: PathBuf,
    pub desktop: bool,
    /// Uninstall only: also delete the launcher's data folder.
    pub remove_data: bool,
}

impl Options {
    /// The folder of an earlier install if there is one, else the default.
    pub fn new() -> Self {
        Self {
            dir: installed().map(|i| i.dir).unwrap_or_else(default_dir),
            desktop: true,
            remove_data: false,
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Installed {
    pub dir: PathBuf,
    pub version: String,
}

fn args() -> Vec<String> {
    std::env::args().skip(1).collect()
}

/// Whether this binary is the setup: its file name starts with `horizon-launcher-setup` (the
/// release workflow copies the launcher under that name). `HORIZON_LAUNCHER_SETUP=1` forces it,
/// for screenshots and tests.
pub fn is_setup_exe() -> bool {
    if std::env::var_os("HORIZON_LAUNCHER_SETUP").is_some_and(|v| v.to_string_lossy() == "1") {
        return true;
    }
    backend::exe_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.to_ascii_lowercase().starts_with("horizon-launcher-setup"))
}

/// The window to open instead of the launcher: the wizard for the setup exe started without
/// arguments, the uninstaller for `uninstall`. None: this is the launcher.
pub fn window_mode() -> Option<Mode> {
    match invocation() {
        Invocation::Window(mode) => Some(mode),
        _ => None,
    }
}

fn invocation() -> Invocation {
    classify(&args(), is_setup_exe())
}

/// `%LOCALAPPDATA%\Programs\Horizon Launcher`; `HORIZON_LAUNCHER_SETUP_DIR` replaces it (tests).
pub fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("HORIZON_LAUNCHER_SETUP_DIR").filter(|d| !d.is_empty()) {
        return folder_for(Path::new(&dir));
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Programs")
        .join(APP_NAME)
}

/// The install folder for a folder the player picked: always one called "Horizon Launcher", so
/// uninstalling never has to guess what is ours.
pub fn folder_for(chosen: &Path) -> PathBuf {
    if chosen
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case(APP_NAME))
    {
        chosen.to_path_buf()
    } else {
        chosen.join(APP_NAME)
    }
}

/// The install this account has, if its launcher is still there.
pub fn installed() -> Option<Installed> {
    let installed = registered()?;
    installed.dir.join(BINARY).is_file().then_some(installed)
}

/// The install this account has in the registry, even if its launcher is gone (a removal still
/// clears the keys and the shortcuts then).
fn registered() -> Option<Installed> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(OWN_KEY, KEY_READ)
        .ok()?;
    let dir: String = key.get_value("InstallDir").ok()?;
    let version: String = key.get_value("Version").unwrap_or_default();
    Some(Installed {
        dir: PathBuf::from(dir),
        version,
    })
}

pub fn installed_exe() -> Option<PathBuf> {
    installed().map(|i| i.dir.join(BINARY))
}

fn start_menu_link() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| {
        PathBuf::from(a)
            .join(r"Microsoft\Windows\Start Menu\Programs")
            .join(format!("{APP_NAME}.lnk"))
    })
}

fn desktop_link() -> Option<PathBuf> {
    shortcut::desktop_dir().map(|d| d.join(format!("{APP_NAME}.lnk")))
}

fn write_link(path: &Path, exe: &Path) -> io::Result<()> {
    shortcut::write_windows_link(path, exe, "", APP_NAME).map_err(io::Error::other)
}

/// Whether the shortcut file names `exe` (its path, as the Shell Link keeps it, in UTF-16).
fn points_to(link: &Path, exe: &Path) -> bool {
    let want: Vec<u8> = exe
        .to_string_lossy()
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    fs::read(link)
        .is_ok_and(|b| !want.is_empty() && b.windows(want.len()).any(|w| w == want.as_slice()))
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .trim_end_matches('\\')
        .eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches('\\'))
}

/// Puts `src` at `dest`, as a self-update does (`backend::replace_binary`): a running `dest` can't
/// be written or deleted but can be renamed, so it steps aside and `remove_replaced_binaries`
/// clears it at the next start.
fn place(src: &Path, dest: &Path) -> io::Result<()> {
    let dir = dest
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no folder"))?;
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    let pid = std::process::id();
    let temp = dir.join(format!("{name}.update-{pid}"));
    // A leftover from an earlier run (the same pid, a crash) must not stop the rename.
    let mut old = dir.join(format!("{name}.old-{pid}-0"));
    for n in 1..100 {
        if !old.exists() {
            break;
        }
        old = dir.join(format!("{name}.old-{pid}-{n}"));
    }
    if let Err(e) = fs::copy(src, &temp) {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    match fs::rename(dest, &old) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        _ => {}
    }
    if let Err(e) = fs::rename(&temp, dest) {
        let _ = fs::rename(&old, dest);
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    let _ = fs::remove_file(&old);
    Ok(())
}

fn size_kb(exe: &Path) -> u32 {
    fs::metadata(exe).map_or(0, |m| (m.len() / 1024) as u32)
}

fn register(dir: &Path, exe: &Path, version: &str) -> io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(UNINSTALL_KEY)?;
    let exe_s = exe.to_string_lossy().to_string();
    let text = |name: &str, value: &str| key.set_value(name, &value.to_string());
    text("DisplayName", APP_NAME)?;
    text("DisplayVersion", version)?;
    text("Publisher", PUBLISHER)?;
    text("InstallLocation", &dir.to_string_lossy())?;
    text("DisplayIcon", &format!("{exe_s},0"))?;
    text("UninstallString", &format!("\"{exe_s}\" uninstall"))?;
    text(
        "QuietUninstallString",
        &format!("\"{exe_s}\" uninstall --quiet"),
    )?;
    text("URLInfoAbout", ABOUT_URL)?;
    text(
        "InstallDate",
        &chrono::Local::now().format("%Y%m%d").to_string(),
    )?;
    key.set_value("EstimatedSize", &size_kb(exe))?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    let (own, _) = hkcu.create_subkey(OWN_KEY)?;
    own.set_value("InstallDir", &dir.to_string_lossy().to_string())?;
    own.set_value("Version", &version.to_string())?;
    Ok(())
}

/// Uninstall copies that stepped aside in earlier runs and are no longer in use: in %TEMP%, and
/// beside `install` (the install folder) when %TEMP% was on another drive.
fn clean_aside(install: &Path) {
    let own = format!("{ASIDE_PREFIX}{}.exe", std::process::id());
    let temp = std::env::temp_dir();
    let places =
        crate::uninstall_aside::aside_dirs(&install.to_string_lossy(), &temp.to_string_lossy());
    for place in std::iter::once(temp).chain(places.into_iter().map(PathBuf::from)) {
        for entry in fs::read_dir(&place).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(ASIDE_PREFIX) && name.ends_with(".exe") && name != own {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Moves the running launcher out of `dir` so the folder can go: into %TEMP% when it is on the
/// same drive, else beside `dir` as a hidden file (a rename never crosses volumes: os error 17).
/// Windows deletes it at the next restart when the account may ask that (administrators), and
/// the next setup or uninstall clears it anyway ([`clean_aside`]).
fn step_aside(own: &Path, dir: &Path) -> io::Result<()> {
    let name = format!("{ASIDE_PREFIX}{}.exe", std::process::id());
    let temp = std::env::temp_dir();
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no folder to step aside to");
    for place in crate::uninstall_aside::aside_dirs(&dir.to_string_lossy(), &temp.to_string_lossy())
    {
        let aside = Path::new(&place).join(&name);
        match fs::rename(own, &aside) {
            Ok(()) => {
                if !same_path(Path::new(&place), &temp) {
                    hide(&aside);
                }
                delete_at_restart(&aside);
                return Ok(());
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain([0]).collect()
}

fn hide(path: &Path) {
    use windows_sys::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};
    let path = wide(path);
    // SAFETY: `path` is a NUL-terminated UTF-16 string that outlives the call.
    unsafe { SetFileAttributesW(path.as_ptr(), FILE_ATTRIBUTE_HIDDEN) };
}

/// Best effort: without administrator rights Windows refuses, and [`clean_aside`] does it later.
fn delete_at_restart(path: &Path) {
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
    let path = wide(path);
    // SAFETY: as in `hide`; a null new name means "delete".
    unsafe { MoveFileExW(path.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
}

/// Installs the launcher this process is into `options.dir`. Returns the installed `.exe`.
pub fn install(options: &Options, progress: impl Fn(Step, f32)) -> Result<PathBuf, String> {
    let source = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = options.dir.clone();
    clean_aside(&dir);
    if dir.file_name().is_none() {
        return Err(format!("{} is not a folder to install in.", dir.display()));
    }
    let exe = dir.join(BINARY);
    progress(Step::Files, 0.05);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // Running the installed copy as the setup (`HORIZON_LAUNCHER_SETUP=1`) rewrites the shortcuts
    // and the registry, but does not copy the file onto itself.
    if !same_path(&source, &exe) {
        place(&source, &exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    }
    progress(Step::Shortcuts, 0.6);
    if let Some(link) = start_menu_link() {
        write_link(&link, &exe).map_err(|e| format!("{}: {e}", link.display()))?;
    }
    if let Some(link) = desktop_link().filter(|_| options.desktop) {
        write_link(&link, &exe).map_err(|e| format!("{}: {e}", link.display()))?;
    }
    progress(Step::Registry, 0.85);
    register(&dir, &exe, backend::VERSION).map_err(|e| format!("The registry: {e}"))?;
    progress(Step::Registry, 1.0);
    Ok(exe)
}

fn remove_file(path: &Path, left: &mut Vec<String>) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => left.push(format!("{}: {e}", path.display())),
    }
}

/// Removes a folder this launcher made; what stays behind is listed rather than ignored.
fn remove_all(path: &Path, left: &mut Vec<String>) {
    if !path.exists() {
        return;
    }
    if let Err(e) = fs::remove_dir_all(path) {
        left.push(format!("{}: {e}", path.display()));
    }
}

/// The ids of the other launchers running now (this process is a launcher too).
fn running_launchers() -> Vec<u32> {
    let out = crate::process::command("tasklist")
        .args([
            "/FO",
            "CSV",
            "/NH",
            "/FI",
            "IMAGENAME eq horizon-launcher.exe",
        ])
        .output();
    out.map(|o| {
        crate::setup_args::other_launcher_pids(
            &String::from_utf8_lossy(&o.stdout),
            std::process::id(),
        )
    })
    .unwrap_or_default()
}

/// Asks a launcher that is still open to quit (a window closing is how it shuts down), waits a
/// few seconds, and ends what stays: the player pressed Uninstall, and its files are going.
/// The game a launcher started is another process and keeps running.
fn close_running_launchers(progress: &impl Fn(Step, f32)) {
    let taskkill = |pid: u32, force: bool| {
        let mut cmd = crate::process::command("taskkill");
        cmd.args(["/PID", &pid.to_string()]);
        if force {
            cmd.arg("/F");
        }
        let _ = cmd.output();
    };
    let mut pids = running_launchers();
    if pids.is_empty() {
        return;
    }
    progress(Step::Closing, 0.05);
    for &pid in &pids {
        taskkill(pid, false);
    }
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        pids = running_launchers();
        if pids.is_empty() {
            return;
        }
    }
    for pid in pids {
        taskkill(pid, true);
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
}

/// The data location the player moved the launcher's data to (Settings > Data folder), when
/// [`backend::data_dir`] follows it: not under `HORIZON_LAUNCHER_HOME`, which comes first.
fn moved_location() -> Option<Roots> {
    if std::env::var_os("HORIZON_LAUNCHER_HOME").is_some_and(|p| !p.is_empty()) {
        return None;
    }
    data_location::configured_roots().ok().flatten()
}

fn named(path: &Path, name: &str) -> bool {
    let Some(last) = path.file_name() else {
        return false;
    };
    last.eq_ignore_ascii_case(name)
}

/// Whether `data`, the folder [`backend::data_dir`] names, is one the launcher made, so that
/// `--remove-data` may delete it: the default `horizon-launcher` folder, or the `launcher` folder
/// of a moved data location, which the move lays out as `<folder>\launcher`, `\game` and `\build`.
fn made_by_launcher(data: &Path, location: Option<&Roots>) -> bool {
    let laid_out = location.is_some_and(|r| r.launcher == data && named(&r.launcher, "launcher"));
    named(data, "horizon-launcher") || laid_out
}

/// Whether `builds`, [`backend::build_dir`], goes with a data folder the launcher made. In a moved
/// location it is `<folder>\build` or inside the launcher folder; elsewhere it is always ours.
fn builds_are_launchers(builds: &Path, data: &Path, location: Option<&Roots>) -> bool {
    location.is_none() || builds.starts_with(data) || named(builds, "build")
}

/// Takes the install away. Returns what could not be removed (a file in use, say).
pub fn uninstall(remove_data: bool, progress: impl Fn(Step, f32)) -> Result<Vec<String>, String> {
    let dir = registered()
        .ok_or("Horizon Launcher is not installed for this account.")?
        .dir;
    close_running_launchers(&progress);
    clean_aside(&dir);
    let exe = dir.join(BINARY);
    let own = backend::exe_path().to_path_buf();
    let mut left = Vec::new();

    progress(Step::Shortcuts, 0.15);
    if let Some(link) = start_menu_link().filter(|l| points_to(l, &exe)) {
        remove_file(&link, &mut left);
    }
    // The desktop's shortcuts to this launcher: its own and the ones that play one instance.
    if let Some(desktop) = shortcut::desktop_dir() {
        for entry in fs::read_dir(desktop).into_iter().flatten().flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
                && points_to(&path, &exe)
            {
                remove_file(&path, &mut left);
            }
        }
    }

    progress(Step::Registry, 0.3);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for key in [UNINSTALL_KEY, OWN_KEY] {
        match hkcu.delete_subkey(key) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => left.push(format!(r"HKCU\{key}: {e}")),
        }
    }
    // `Software\Horizon` goes only when nothing else (the game) keeps anything in it.
    if let Ok(parent) = hkcu.open_subkey_with_flags(r"Software\Horizon", KEY_READ) {
        let empty = parent
            .query_info()
            .is_ok_and(|info| info.sub_keys == 0 && info.values == 0);
        drop(parent);
        if empty {
            let _ = hkcu.delete_subkey(r"Software\Horizon");
        }
    }

    progress(Step::Removing, 0.55);
    // The running file can be renamed but not deleted: it steps out of the folder ([`step_aside`]).
    if own.starts_with(&dir) {
        if let Err(e) = step_aside(&own, &dir) {
            left.push(format!("{}: {e}", own.display()));
        }
    }
    // Only what setup and self-updates put there: the launcher and what they moved aside.
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name == BINARY || name.starts_with("horizon-launcher.exe.") {
            remove_file(&entry.path(), &mut left);
        }
    }
    if let Err(e) = fs::remove_dir(&dir) {
        // Something else lives there now (a file the launcher didn't make): say so.
        if !dir.exists() || !left.is_empty() {
            // Gone already, or what stayed is listed above.
        } else {
            left.push(format!("{}: {e}", dir.display()));
        }
    }

    if remove_data {
        progress(Step::Data, 0.8);
        let data = backend::data_dir();
        let location = moved_location();
        // Only a folder the launcher made: `HORIZON_LAUNCHER_HOME` may point anywhere.
        if made_by_launcher(&data, location.as_ref()) {
            remove_all(&data, &mut left);
            // A profile whose name isn't ASCII keeps the build folder in %ProgramData%
            // (`backend::build_dir`); it is the launcher's too. Already gone with `data` otherwise.
            let builds = backend::build_dir();
            if builds_are_launchers(&builds, &data, location.as_ref()) {
                remove_all(&builds, &mut left);
            } else {
                left.push(format!(
                    "{} (not the launcher's own folder)",
                    builds.display()
                ));
            }
        } else {
            left.push(format!(
                "{} (not the launcher's own folder)",
                data.display()
            ));
        }
    }
    progress(Step::Data, 1.0);
    Ok(left)
}

/// After a self-update: the Uninstall entry names the new version and size. Only for the install
/// this launcher runs from; a launcher unpacked from the ZIP touches nothing.
pub fn after_update(version: &str) {
    let Some(registered) = registered() else {
        return;
    };
    let exe = backend::exe_path();
    if !exe
        .parent()
        .is_some_and(|dir| same_path(dir, &registered.dir))
    {
        return;
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(UNINSTALL_KEY, KEY_WRITE) {
        let _ = key.set_value("DisplayVersion", &version.to_string());
        let _ = key.set_value("EstimatedSize", &size_kb(exe));
    }
    if let Ok(key) = hkcu.open_subkey_with_flags(OWN_KEY, KEY_WRITE) {
        let _ = key.set_value("Version", &version.to_string());
    }
}

/// `setup --quiet [--dir <folder>] [--desktop-shortcut]` (the setup exe only) and
/// `uninstall --quiet [--remove-data]`: no window, for scripts. None: not a quiet command.
pub fn run_quiet() -> Option<i32> {
    let args = args();
    let flag = |name: &str| args.iter().any(|a| a == name);
    match invocation() {
        Invocation::QuietUninstall => Some(match uninstall(flag("--remove-data"), |_, _| {}) {
            Ok(left) => {
                for item in left {
                    eprintln!("Not removed: {item}");
                }
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }),
        Invocation::QuietSetup => {
            let mut options = Options::new();
            if let Some(dir) = args
                .iter()
                .position(|a| a == "--dir")
                .and_then(|i| args.get(i + 1))
            {
                options.dir = folder_for(Path::new(dir));
            }
            options.desktop = flag("--desktop-shortcut");
            Some(match install(&options, |_, _| {}) {
                Ok(exe) => {
                    println!("{}", exe.display());
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_install_folder_is_always_ours() {
        assert_eq!(
            folder_for(Path::new(r"C:\Games")),
            PathBuf::from(r"C:\Games\Horizon Launcher")
        );
        assert_eq!(
            folder_for(Path::new(r"C:\Games\horizon launcher")),
            PathBuf::from(r"C:\Games\horizon launcher")
        );
    }

    fn moved_to(folder: &str) -> Roots {
        let folder = PathBuf::from(folder);
        Roots {
            launcher: folder.join("launcher"),
            game: folder.join("game"),
            build: folder.join("build"),
        }
    }

    #[test]
    fn uninstall_deletes_only_folders_the_launcher_made() {
        // The default folder, and one `HORIZON_LAUNCHER_HOME` names the same.
        let default = Path::new(r"C:\Users\p\AppData\Roaming\horizon-launcher");
        assert!(made_by_launcher(default, None));
        assert!(builds_are_launchers(default, default, None));
        assert!(!made_by_launcher(Path::new(r"D:\Documents"), None));
        assert!(!made_by_launcher(Path::new(r"D:\Games\launcher"), None));

        // Data moved to D:\Horizon (`data_location::layout`): its launcher and build folders go,
        // the game folder never.
        let moved = moved_to(r"D:\Horizon");
        assert!(made_by_launcher(&moved.launcher, Some(&moved)));
        assert!(builds_are_launchers(
            &moved.build,
            &moved.launcher,
            Some(&moved)
        ));
        assert!(!made_by_launcher(&moved.game, Some(&moved)));
        // A build folder inside the launcher folder goes with it.
        let inside = Path::new(r"D:\Horizon\launcher\build");
        assert!(builds_are_launchers(inside, &moved.launcher, Some(&moved)));

        // Not what a move lays out (a location file edited by hand): left alone.
        let odd = Roots {
            launcher: PathBuf::from(r"D:\Documents"),
            ..moved.clone()
        };
        assert!(!made_by_launcher(&odd.launcher, Some(&odd)));
        let odd = Roots {
            build: PathBuf::from(r"D:\Documents"),
            ..moved.clone()
        };
        assert!(!builds_are_launchers(&odd.build, &odd.launcher, Some(&odd)));
        // Another folder called `launcher` than the configured one.
        assert!(!made_by_launcher(
            Path::new(r"D:\Elsewhere\launcher"),
            Some(&moved)
        ));
    }

    /// What the shell itself reads as the shortcut's target: the field Explorer starts from.
    fn target_of(link: &Path) -> String {
        use windows::core::{Interface, HSTRING};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED, STGM_READ,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        // SAFETY: COM calls on interfaces made and dropped here, on this test's own thread.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let shell: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            let file: IPersistFile = shell.cast().unwrap();
            file.Load(&HSTRING::from(link.as_os_str()), STGM_READ)
                .unwrap();
            let mut buf = [0u16; 520];
            shell.GetPath(&mut buf, std::ptr::null_mut(), 0).unwrap();
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..end])
        }
    }

    #[test]
    fn the_shell_reads_the_target_of_the_shortcut_it_gets() {
        // GitHub's Windows runners give TEMP as an 8.3 short path (C:\Users\RUNNER~1\...), which the
        // shell hands back expanded: start from the long path, as an install folder is.
        let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
        let temp = temp
            .to_str()
            .and_then(|t| t.strip_prefix(r"\\?\"))
            .map_or(temp.clone(), PathBuf::from);
        let dir = temp.join(format!("horizon-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let exe = dir.join("Horizon Launcher").join("horizon-launcher.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"x").unwrap();
        let link = dir.join("Horizon Launcher.lnk");
        write_link(&link, &exe).unwrap();
        // The bug this guards: a link whose target the shell read as empty, so a double click did nothing.
        assert_eq!(
            target_of(&link).to_lowercase(),
            exe.to_string_lossy().to_lowercase()
        );
        // The uninstaller finds its shortcuts by the program's path inside them.
        assert!(points_to(&link, &exe));
        assert!(!points_to(
            &link,
            Path::new(r"C:\Other\horizon-launcher.exe")
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
