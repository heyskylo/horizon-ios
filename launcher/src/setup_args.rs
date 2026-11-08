//! What the Windows setup and uninstaller make of the command line, and which running launchers
//! stand in the uninstaller's way. Plain text in, plain values out, so every OS tests it.
//!
//! Windows starts the Uninstall entry as `"<exe>" uninstall` from Control Panel, with no console.
//! If that line reached the CLI parser it would be refused as an unknown subcommand, with the
//! message going nowhere: the player would see nothing at all. [`classify`] therefore decides
//! first, and only [`Invocation::Launcher`] may go on to the CLI parser.

/// What the setup window shows: installing, or removing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Install,
    Uninstall,
}

/// What a command line asks of the launcher's binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invocation {
    /// A window instead of the launcher's: the wizard, or the uninstaller.
    Window(Mode),
    /// `setup --quiet ...` (the setup exe only): installs without a window.
    QuietSetup,
    /// `uninstall --quiet [--remove-data]`: removes without a window.
    QuietUninstall,
    /// The launcher: its window, or a CLI command.
    Launcher,
}

/// Reads the arguments after the program name. `is_setup_exe`: this binary is the setup copy.
pub fn classify(args: &[String], is_setup_exe: bool) -> Invocation {
    let quiet = args.iter().any(|a| a == "--quiet");
    match args.first().map(String::as_str) {
        None if is_setup_exe => Invocation::Window(Mode::Install),
        Some("uninstall") if quiet => Invocation::QuietUninstall,
        // Any other `uninstall`, with or without extra words, is the player asking: never a CLI error.
        Some("uninstall") => Invocation::Window(Mode::Uninstall),
        Some("setup") if quiet && is_setup_exe => Invocation::QuietSetup,
        _ => Invocation::Launcher,
    }
}

/// The ids in `tasklist /FO CSV /NH` output (`"horizon-launcher.exe","1234","Console",...`),
/// other than `own`: the launchers still running while this process removes their files.
/// `tasklist` prints a sentence instead of rows when nothing matches; that gives no ids.
pub fn other_launcher_pids(tasklist_csv: &str, own: u32) -> Vec<u32> {
    let mut pids: Vec<u32> = tasklist_csv
        .lines()
        .filter_map(|line| {
            let mut fields = line.trim().trim_matches('"').split("\",\"");
            let image = fields.next()?;
            if !image.eq_ignore_ascii_case("horizon-launcher.exe") {
                return None;
            }
            fields.next()?.trim().parse::<u32>().ok()
        })
        .filter(|&pid| pid != own)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn the_uninstall_entry_opens_the_window_and_never_the_cli_parser() {
        // The exact line Control Panel runs (`UninstallString`), from the installed launcher.
        assert_eq!(
            classify(&args(&["uninstall"]), false),
            Invocation::Window(Mode::Uninstall)
        );
        // Extra words (a shell adding one, a hand-typed flag) still open the window.
        assert_eq!(
            classify(&args(&["uninstall", "--remove-data"]), false),
            Invocation::Window(Mode::Uninstall)
        );
    }

    #[test]
    fn the_quiet_uninstall_entry_stays_windowless() {
        // `QuietUninstallString`, and what the CI smoke test runs.
        assert_eq!(
            classify(&args(&["uninstall", "--quiet"]), false),
            Invocation::QuietUninstall
        );
        assert_eq!(
            classify(&args(&["uninstall", "--quiet", "--remove-data"]), false),
            Invocation::QuietUninstall
        );
    }

    #[test]
    fn the_setup_exe_without_arguments_opens_the_wizard_only_as_the_setup() {
        assert_eq!(classify(&[], true), Invocation::Window(Mode::Install));
        // The installed launcher with no arguments is the launcher.
        assert_eq!(classify(&[], false), Invocation::Launcher);
    }

    #[test]
    fn quiet_setup_needs_the_setup_exe() {
        let line = args(&["setup", "--quiet", "--dir", r"C:\Games"]);
        assert_eq!(classify(&line, true), Invocation::QuietSetup);
        assert_eq!(classify(&line, false), Invocation::Launcher);
    }

    #[test]
    fn everything_else_goes_to_the_cli() {
        for line in [
            &["settings", "--json"][..],
            &["mcp"],
            &["--help"],
            // Only the first word is the verb.
            &["play", "uninstall"],
            &["--quiet"],
            &["setup"],
        ] {
            assert_eq!(
                classify(&args(line), false),
                Invocation::Launcher,
                "{line:?}"
            );
        }
    }

    const TASKLIST: &str = "\"horizon-launcher.exe\",\"4120\",\"Console\",\"1\",\"212,044 K\"\r\n\
        \"Horizon-Launcher.exe\",\"9988\",\"Console\",\"1\",\"80,000 K\"\r\n\
        \"horizon-launcher.exe\",\"4120\",\"Console\",\"1\",\"212,044 K\"\r\n\
        \"horizon-launcher-setup.exe\",\"77\",\"Console\",\"1\",\"9,000 K\"\r\n";

    #[test]
    fn a_running_launcher_is_found_but_not_this_process() {
        // The uninstaller is itself `horizon-launcher.exe`: it must not ask itself to quit.
        assert_eq!(other_launcher_pids(TASKLIST, 9988), vec![4120]);
        assert_eq!(other_launcher_pids(TASKLIST, 1), vec![4120, 9988]);
        assert_eq!(other_launcher_pids(TASKLIST, 4120), vec![9988]);
    }

    #[test]
    fn tasklist_with_no_match_gives_no_launchers() {
        let none = "INFO: No tasks are running which match the specified criteria.\r\n";
        assert!(other_launcher_pids(none, 1).is_empty());
        assert!(other_launcher_pids("", 1).is_empty());
        // The setup exe and other programs are not launchers.
        let other = "\"horizon-launcher-setup.exe\",\"77\",\"Console\",\"1\",\"9 K\"\r\n\
            \"horizon-client.exe\",\"78\",\"Console\",\"1\",\"9 K\"\r\n";
        assert!(other_launcher_pids(other, 1).is_empty());
    }
}
