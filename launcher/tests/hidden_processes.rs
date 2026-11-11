//! Windows background processes must keep logs, input and exit status without a console.
#![cfg(windows)]

mod support;
use horizon_launcher::{
    backend::{self, Job, Msg},
    process,
};
use std::{fs, path::Path, sync::mpsc, time::Duration};
use support::console_probe as probe;

fn logged_child(fail: bool) -> (Vec<String>, Result<(), String>) {
    let (sender, receiver) = mpsc::channel();
    let mut command = std::process::Command::new(probe());
    if fail {
        command.arg("--fail");
    }
    let mut child = backend::spawn(Job::Prepare, command, sender).unwrap();
    child.send("input reaches the child");
    let mut lines = Vec::new();
    loop {
        match receiver.recv_timeout(Duration::from_secs(10)).unwrap() {
            Msg::Line(Job::Prepare, line) => lines.push(line),
            Msg::Exit(Job::Prepare, result) => return (lines, result),
            _ => panic!("unexpected job message"),
        }
    }
}

#[test]
fn hidden_children_keep_stdout_stderr_stdin_and_success() {
    let (lines, result) = logged_child(false);
    assert!(result.is_ok(), "{result:?}");
    for expected in [
        "console_window_visible=false",
        "stdout is captured",
        "stderr is captured",
        "stdin=input reaches the child",
    ] {
        assert!(
            lines.iter().any(|line| line == expected),
            "missing {expected}: {lines:?}"
        );
    }
}

#[test]
fn hidden_children_report_failure_and_keep_their_logs() {
    let (lines, result) = logged_child(true);
    assert!(result.unwrap_err().contains('7'));
    assert!(lines.iter().any(|line| line == "stderr is captured"));
}

#[test]
fn exporters_write_to_their_log_file_without_a_console() {
    let root = probe().parent().unwrap();
    let path = root.join("exporter.log");
    let log = fs::File::create(&path).unwrap();
    let jdk = horizon_platform::prepare::java::Jdk {
        java: probe().into(),
        javac: probe().into(),
    };
    jdk.java(&[], Some(root), &log).unwrap();
    let output = fs::read_to_string(path).unwrap();
    for expected in [
        "console_window_visible=false",
        "stdout is captured",
        "stderr is captured",
    ] {
        assert!(output.contains(expected), "missing {expected}: {output}");
    }
}

#[test]
fn cli_keeps_redirected_stdout_and_stderr() {
    let executable = env!("CARGO_BIN_EXE_horizon-launcher");
    let version = process::command(executable)
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("horizon-launcher "));
    let invalid = process::command(executable)
        .arg("--unknown-option")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("--unknown-option"));
}

#[test]
fn mcp_keeps_its_stdio_transport() {
    let executable = Path::new(env!("CARGO_BIN_EXE_horizon-launcher"));
    assert!(horizon_launcher::agents::self_test(executable).unwrap() > 0);
}

#[test]
fn desktop_executable_uses_the_windows_gui_subsystem() {
    const PE_HEADER_POINTER: usize = 0x3c;
    const PE_OPTIONAL_HEADER_OFFSET: usize = 24;
    const SUBSYSTEM_OFFSET: usize = 68;
    const WINDOWS_GUI_SUBSYSTEM: u16 = 2;

    let executable = fs::read(env!("CARGO_BIN_EXE_horizon-launcher")).unwrap();
    let header = u32::from_le_bytes(
        executable[PE_HEADER_POINTER..PE_HEADER_POINTER + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    assert_eq!(&executable[header..header + 4], b"PE\0\0");
    let subsystem = header + PE_OPTIONAL_HEADER_OFFSET + SUBSYSTEM_OFFSET;
    assert_eq!(
        u16::from_le_bytes(executable[subsystem..subsystem + 2].try_into().unwrap()),
        WINDOWS_GUI_SUBSYSTEM
    );
}
