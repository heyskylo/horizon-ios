//! Add another Microsoft account: the device code page (microsoft.com/link) goes ahead with the
//! account the browser is signed in to, and the device code flow has no way to ask for a picker.
//! A private window has no Microsoft session, so the player signs in with the account they choose.
//! This finds the player's default browser and opens a URL in its private window. No Azure app
//! change is involved (#3786).
//!
//! Windows reads the `https` ProgId, macOS LaunchServices' handler for `https`, Linux
//! `xdg-settings`. Safari has no private-window flag, so it counts as unsupported, like any
//! browser not listed in [`Browser`]: the caller shows the link to copy instead.
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Signs the player out of Microsoft in their normal browser, for "Use a different Microsoft account".
pub const LOGOUT_URL: &str = "https://login.live.com/logout.srf";

/// A browser that has a private-window flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Chromium,
    Edge,
    Brave,
    Vivaldi,
    Opera,
    Firefox,
}

impl Browser {
    /// Which browser a system's name for it is: a Windows ProgId (`MSEdgeHTM`), a macOS bundle
    /// id (`com.google.chrome`) or a Linux desktop file (`firefox.desktop`). `None` for Safari and
    /// anything unknown.
    pub fn from_id(id: &str) -> Option<Browser> {
        let id = id.to_ascii_lowercase();
        // Order matters: Edge's and Opera's ids are not Chrome's, but Chromium's contains it.
        [
            ("edge", Browser::Edge),
            ("brave", Browser::Brave),
            ("vivaldi", Browser::Vivaldi),
            ("opera", Browser::Opera),
            ("firefox", Browser::Firefox),
            ("chromium", Browser::Chromium),
            ("chrome", Browser::Chrome),
        ]
        .into_iter()
        .find(|(name, _)| id.contains(name))
        .map(|(_, browser)| browser)
    }

    /// The flag that opens the page in a private window.
    pub fn private_flag(self) -> &'static str {
        match self {
            Browser::Edge => "--inprivate",
            Browser::Firefox => "--private-window",
            Browser::Opera => "--private",
            Browser::Chrome | Browser::Chromium | Browser::Brave | Browser::Vivaldi => {
                "--incognito"
            }
        }
    }

    /// The programs a Linux install puts on the PATH, most common first.
    #[cfg(all(unix, not(target_os = "macos")))]
    fn linux_programs(self) -> &'static [&'static str] {
        match self {
            Browser::Chrome => &["google-chrome-stable", "google-chrome"],
            Browser::Chromium => &["chromium", "chromium-browser"],
            Browser::Edge => &["microsoft-edge-stable", "microsoft-edge"],
            Browser::Brave => &["brave-browser", "brave", "brave-browser-stable"],
            Browser::Vivaldi => &["vivaldi-stable", "vivaldi"],
            Browser::Opera => &["opera"],
            Browser::Firefox => &["firefox", "firefox-esr"],
        }
    }
}

/// How to start the browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// An executable (a path, or a name on the PATH).
    Program(String),
    /// A macOS application, by bundle id.
    Bundle(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detected {
    pub browser: Browser,
    pub target: Target,
}

/// A program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
}

/// The command that opens `url` in `browser`'s private window.
pub fn launch(detected: &Detected, url: &str) -> Launch {
    let flag = detected.browser.private_flag().to_owned();
    match &detected.target {
        Target::Program(program) => Launch {
            program: program.clone(),
            args: vec![flag, url.to_owned()],
        },
        // `open --args` only reaches a browser that starts: Chromium-based ones need a new
        // instance (-n) to take the flag while running; Firefox forwards it to the running one.
        Target::Bundle(id) => {
            let mut args = vec!["-b".to_owned(), id.clone()];
            if detected.browser != Browser::Firefox {
                args.insert(0, "-n".to_owned());
            }
            args.extend(["--args".to_owned(), flag, url.to_owned()]);
            Launch {
                program: "open".to_owned(),
                args,
            }
        }
    }
}

/// The program in a Windows `shell\open\command` value:
/// `"C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1"` → the path.
pub fn windows_program(command: &str) -> Option<String> {
    let command = command.trim();
    let program = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => {
            let end = command.to_ascii_lowercase().find(".exe")? + ".exe".len();
            &command[..end]
        }
    };
    (!program.is_empty()).then(|| program.to_owned())
}

/// The bundle id LaunchServices (the `com.apple.launchservices.secure` preferences, as JSON) has
/// for the `https` scheme. `None` when the player never chose one: that is Safari.
pub fn macos_https_handler(preferences: &serde_json::Value) -> Option<String> {
    let handlers = preferences.get("LSHandlers")?.as_array()?;
    ["https", "http"].into_iter().find_map(|scheme| {
        handlers.iter().find_map(|handler| {
            let said = handler.get("LSHandlerURLScheme")?.as_str()?;
            if !said.eq_ignore_ascii_case(scheme) {
                return None;
            }
            ["LSHandlerRoleAll", "LSHandlerRoleViewer"]
                .into_iter()
                .find_map(|role| handler.get(role)?.as_str())
                .map(str::to_owned)
        })
    })
}

#[cfg(windows)]
fn detect() -> Option<Detected> {
    use winreg::enums::HKEY_CLASSES_ROOT;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let choice = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(
            r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
        )
        .ok()?;
    let prog_id: String = choice.get_value("ProgId").ok()?;
    let browser = Browser::from_id(&prog_id)?;
    let command: String = RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(format!(r"{prog_id}\shell\open\command"))
        .ok()?
        .get_value("")
        .ok()?;
    Some(Detected {
        browser,
        target: Target::Program(windows_program(&command)?),
    })
}

#[cfg(target_os = "macos")]
fn detect() -> Option<Detected> {
    let plist = dirs::home_dir()?
        .join("Library/Preferences/com.apple.LaunchServices/com.apple.launchservices.secure.plist");
    let json = Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(plist)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let id = macos_https_handler(&serde_json::from_slice(&json.stdout).ok()?)?;
    Some(Detected {
        browser: Browser::from_id(&id)?,
        target: Target::Bundle(id),
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect() -> Option<Detected> {
    let said = Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let browser = Browser::from_id(String::from_utf8_lossy(&said.stdout).trim())?;
    let program = browser
        .linux_programs()
        .iter()
        .find(|program| crate::launch::on_path(program))?;
    Some(Detected {
        browser,
        target: Target::Program((*program).to_owned()),
    })
}

#[cfg(not(any(windows, unix)))]
fn detect() -> Option<Detected> {
    None
}

/// The default browser, if it has a private window; asked once.
pub fn detected() -> Option<&'static Detected> {
    static DETECTED: OnceLock<Option<Detected>> = OnceLock::new();
    DETECTED.get_or_init(detect).as_ref()
}

/// Opens `url` (https only) in the default browser's private window.
pub fn open(url: &str) -> io::Result<()> {
    if !url.starts_with("https://") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an https link",
        ));
    }
    let detected =
        detected().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no browser"))?;
    let launch = launch(detected, url);
    let mut child = Command::new(PathBuf::from(&launch.program))
        .args(&launch.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Some browsers stay as this process for as long as they run: reap it without waiting here.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn systems_name_the_same_browsers_differently() {
        for (id, browser) in [
            ("ChromeHTML", Browser::Chrome),
            ("com.google.chrome", Browser::Chrome),
            ("google-chrome.desktop", Browser::Chrome),
            ("com.google.Chrome.desktop", Browser::Chrome),
            ("chromium.desktop", Browser::Chromium),
            ("chromium-browser.desktop", Browser::Chromium),
            ("org.chromium.Chromium", Browser::Chromium),
            ("MSEdgeHTM", Browser::Edge),
            ("com.microsoft.edgemac", Browser::Edge),
            ("microsoft-edge.desktop", Browser::Edge),
            ("BraveHTML", Browser::Brave),
            ("com.brave.Browser", Browser::Brave),
            ("brave-browser.desktop", Browser::Brave),
            ("VivaldiHTM.Q1W2E3", Browser::Vivaldi),
            ("vivaldi-stable.desktop", Browser::Vivaldi),
            ("OperaStable", Browser::Opera),
            ("com.operasoftware.Opera", Browser::Opera),
            ("FirefoxURL-308046B0AF4A39CB", Browser::Firefox),
            ("org.mozilla.firefox", Browser::Firefox),
            ("firefox-esr.desktop", Browser::Firefox),
        ] {
            assert_eq!(Browser::from_id(id), Some(browser), "{id}");
        }
        for id in [
            "",
            "AppX4hxtad77fbk3jkkeerkrm0ze94wjf3s9",
            "com.apple.safari",
            "epiphany.desktop",
        ] {
            assert_eq!(Browser::from_id(id), None, "{id}");
        }
    }

    #[test]
    fn each_family_gets_its_own_private_flag() {
        assert_eq!(Browser::Edge.private_flag(), "--inprivate");
        assert_eq!(Browser::Firefox.private_flag(), "--private-window");
        for browser in [
            Browser::Chrome,
            Browser::Chromium,
            Browser::Brave,
            Browser::Vivaldi,
        ] {
            assert_eq!(browser.private_flag(), "--incognito");
        }
    }

    fn found(browser: Browser, target: Target) -> Detected {
        Detected { browser, target }
    }

    #[test]
    fn a_program_gets_the_flag_then_the_page() {
        let url = "https://www.microsoft.com/link";
        let edge = found(Browser::Edge, Target::Program("msedge.exe".into()));
        assert_eq!(
            launch(&edge, url),
            Launch {
                program: "msedge.exe".into(),
                args: vec!["--inprivate".into(), url.into()],
            }
        );
    }

    #[test]
    fn a_mac_app_starts_through_open_by_bundle_id() {
        let url = "https://www.microsoft.com/link";
        let chrome = found(Browser::Chrome, Target::Bundle("com.google.chrome".into()));
        let words = |l: Launch| l.args.join(" ");
        let l = launch(&chrome, url);
        assert_eq!(l.program, "open");
        // A new instance, so a running Chrome takes the flag.
        assert_eq!(
            words(l),
            "-n -b com.google.chrome --args --incognito https://www.microsoft.com/link"
        );
        let firefox = found(
            Browser::Firefox,
            Target::Bundle("org.mozilla.firefox".into()),
        );
        assert_eq!(
            words(launch(&firefox, url)),
            "-b org.mozilla.firefox --args --private-window https://www.microsoft.com/link"
        );
    }

    #[test]
    fn windows_commands_give_their_program() {
        assert_eq!(
            windows_program(r#""C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1""#),
            Some(r"C:\Program Files\Mozilla Firefox\firefox.exe".into())
        );
        assert_eq!(
            windows_program(
                r#"  "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --single-argument %1"#
            ),
            Some(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".into())
        );
        assert_eq!(
            windows_program(r"C:\Chrome\chrome.EXE --single-argument %1"),
            Some(r"C:\Chrome\chrome.EXE".into())
        );
        assert_eq!(windows_program(""), None);
        assert_eq!(windows_program(r#""""#), None);
        assert_eq!(windows_program("rundll32 something"), None);
    }

    #[test]
    fn launchservices_names_the_https_handler() {
        let prefs = json!({ "LSHandlers": [
            { "LSHandlerContentType": "public.html", "LSHandlerRoleAll": "com.apple.safari" },
            { "LSHandlerURLScheme": "mailto", "LSHandlerRoleAll": "com.apple.mail" },
            { "LSHandlerURLScheme": "http", "LSHandlerRoleAll": "com.brave.browser" },
            { "LSHandlerURLScheme": "https", "LSHandlerRoleAll": "com.google.chrome" },
        ]});
        assert_eq!(
            macos_https_handler(&prefs).as_deref(),
            Some("com.google.chrome")
        );
        // http alone is enough when https was never set.
        let http_only = json!({ "LSHandlers": [
            { "LSHandlerURLScheme": "HTTP", "LSHandlerRoleViewer": "org.mozilla.firefox" },
        ]});
        assert_eq!(
            macos_https_handler(&http_only).as_deref(),
            Some("org.mozilla.firefox")
        );
        // Never chosen: the system default.
        assert_eq!(macos_https_handler(&json!({ "LSHandlers": [] })), None);
        assert_eq!(macos_https_handler(&json!({})), None);
    }

    #[test]
    fn only_https_pages_are_opened() {
        for url in [
            "http://example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "",
        ] {
            let err = open(url).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{url}");
        }
    }
}
