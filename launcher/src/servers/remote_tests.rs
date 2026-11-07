use super::*;

#[test]
fn the_command_runs_horizon_server_in_the_folder_there() {
    let args = ssh_args(
        "mc@vps-1",
        "/srv/minecraft",
        &["config", "set", "world.motd=Hi there"],
    )
    .unwrap();
    let (line, rest) = args.split_last().unwrap();
    assert_eq!(&rest[rest.len() - 3..], ["-T", "--", "mc@vps-1"]);
    assert!(rest.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
    assert_eq!(
        line,
        "PATH=\"$HOME/.local/bin:$PATH\" exec horizon-server --dir '/srv/minecraft' 'config' 'set' \
         'world.motd=Hi there'"
    );
    // A folder in the home folder: the command starts there.
    let args = ssh_args("box", "~/servers/smp", &["start"]).unwrap();
    assert!(args.last().unwrap().contains("--dir 'servers/smp' 'start'"));
}

#[test]
fn what_the_remote_shell_gets_stays_one_argument_each() {
    assert_eq!(quote("plain"), "'plain'");
    assert_eq!(quote("it's"), r"'it'\''s'");
    assert_eq!(quote("$(rm -rf ~); `x`"), "'$(rm -rf ~); `x`'");
    let args = ssh_args("box", "/srv/a b", &["cmd", "say it's $HOME"]).unwrap();
    assert!(args
        .last()
        .unwrap()
        .ends_with(r"--dir '/srv/a b' 'cmd' 'say it'\''s $HOME'"));
}

#[test]
fn only_a_computer_name_is_taken_as_the_host() {
    for host in ["mc@203.0.113.24", "vps-fra-1", "me@[2001:db8::1]"] {
        assert!(check_host(host).is_ok(), "{host}");
    }
    for host in ["", "-oProxyCommand=evil", "a b", "x\ny"] {
        assert!(check_host(host).is_err(), "{host:?}");
        assert!(ssh_args(host, "/srv", &["status"]).is_err());
    }
    assert!(ssh_args("box", "", &["status"]).is_err());
    assert!(ssh_args("box", "~", &["status"]).is_err());
}

#[test]
fn problems_say_what_to_do() {
    let says = |code, err: &str| explain("mc@vps", "/srv/mc", Some(code), err);
    assert!(says(255, "mc@vps: Permission denied (publickey).").contains("ssh-copy-id mc@vps"));
    assert!(says(255, "Host key verification failed.").contains("accept it"));
    assert!(says(
        255,
        "ssh: Could not resolve hostname vps: Name or service not known"
    )
    .contains("no computer called"));
    assert!(says(
        255,
        "ssh: connect to host vps port 22: Connection timed out"
    )
    .contains("Can't reach"));
    assert!(says(127, "sh: 1: exec: horizon-server: not found").contains("has no horizon-server"));
    assert!(says(3, NO_SERVER).contains("no server in /srv/mc"));
    assert_eq!(
        says(
            255,
            "kex_exchange_identification: read: Connection reset by peer"
        ),
        "Couldn't connect to mc@vps: kex_exchange_identification: read: Connection reset by peer"
    );
    assert_eq!(
        says(
            1,
            "horizon-server: the game is starting or stopping: try again in a moment"
        ),
        "The game is starting or stopping: try again in a moment"
    );
}

/// The check, with a stand-in for ssh that runs the remote line in a local shell.
#[cfg(unix)]
#[test]
fn the_check_reads_the_status_there() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("horizon-remote-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bin = root.join("bin");
    let server = root.join("srv");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&server).unwrap();
    // ssh: the last argument is the remote line.
    let ssh = root.join("ssh");
    std::fs::write(
        &ssh,
        format!(
            "#!/bin/sh\nfor a; do line=$a; done\nPATH={}:/usr/bin:/bin; export PATH\nexec sh -c \"$line\"\n",
            bin.display()
        ),
    )
    .unwrap();
    let program = bin.join("horizon-server");
    std::fs::write(
        &program,
        "#!/bin/sh\necho '{\"phase\":\"running\",\"sample\":{\"game\":{\"players\":[{\"name\":\"Alex\"}],\"max_players\":20}}}'\n",
    )
    .unwrap();
    for f in [&ssh, &program] {
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let dir = server.to_string_lossy();
    let err = check_with(&ssh, "box", &dir).unwrap_err();
    assert!(err.contains("There's no server in"), "{err}");
    std::fs::write(server.join("server.toml"), "").unwrap();
    let status = check_with(&ssh, "box", &dir).unwrap();
    assert_eq!(status.phase, "running");
    assert_eq!(status.players[0].name, "Alex");
    assert_eq!(status.max_players, Some(20));
    std::fs::remove_file(&program).unwrap();
    let err = check_with(&ssh, "box", &dir).unwrap_err();
    assert!(err.contains("has no horizon-server"), "{err}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn lists_written_before_remote_servers_still_read() {
    let old: super::super::Entry = serde_json::from_str(r#"{"name":"A","dir":"/x"}"#).unwrap();
    assert_eq!(old.host, None);
    assert!(!serde_json::to_string(&old).unwrap().contains("host"));
}
