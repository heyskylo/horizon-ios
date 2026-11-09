//! A prebuilt native build signed as horizon-mods' workflow signs it (tools/ci/sign_native_build.sh,
//! OpenSSL), then checked as the launcher checks it (`native_build::verify`).
use horizon_release_sign::native_build::{Mod, Request, archive, signature, verify};
use horizon_release_sign::{SigningKey, parse_keys, sha256_hex};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// An OpenSSL 3 (the runner's), or None on a machine without one (macOS ships LibreSSL).
fn openssl() -> Option<String> {
    [
        "openssl",
        "/opt/homebrew/opt/openssl@3/bin/openssl",
        "/usr/local/opt/openssl@3/bin/openssl",
    ]
    .into_iter()
    .find(|o| {
        Command::new(o)
            .arg("version")
            .output()
            .is_ok_and(|v| v.stdout.starts_with(b"OpenSSL 3"))
    })
    .map(String::from)
}

fn request() -> Request {
    Request::new(
        "x86_64-unknown-linux-gnu",
        &"6".repeat(40),
        vec![Mod {
            id: "minimap".into(),
            version: "1.4.0".into(),
            sha256: "a".repeat(64),
        }],
    )
    .unwrap()
}

/// A folder with the build's archive, as the build job leaves it.
fn folder(name: &str, request: &Request) -> (PathBuf, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("horizon-native-sign-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(archive(&request.key(), &request.target));
    std::fs::write(&file, b"the game with the minimap").unwrap();
    (dir, file)
}

fn sign(key: &str, target: &str, file: &Path, env: &[(&str, &str)], keys: &Path) -> Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/ci/sign_native_build.sh");
    let mut cmd = Command::new("bash");
    cmd.arg(script)
        .args([key, target])
        .arg(file)
        .env_remove("NATIVE_BUILD_SIGNING_KEY")
        .env_remove("NATIVE_BUILD_SIGNING_KEY_NEXT")
        .env("OPENSSL", openssl().unwrap())
        .env("NATIVE_BUILD_KEYS", keys);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn read_signature(dir: &Path, r: &Request) -> String {
    std::fs::read_to_string(dir.join(signature(&r.key(), &r.target))).unwrap()
}

#[test]
fn the_workflows_signature_is_what_the_launcher_checks() {
    // The script runs on the Linux runner of horizon-mods' sign job.
    if cfg!(windows) || openssl().is_none() {
        eprintln!("skipped: tools/ci/sign_native_build.sh is tested on Linux (bash, OpenSSL 3)");
        return;
    }
    let r = request();
    let (dir, file) = folder("secret", &r);
    let sha = sha256_hex(b"the game with the minimap");
    let name = archive(&r.key(), &r.target);
    let (old, old_secret) = SigningKey::generate().unwrap();
    let (new, new_secret) = SigningKey::generate().unwrap();
    let keys = dir.join("keys.txt");
    std::fs::write(
        &keys,
        format!(
            "# listed\n{}\n{}\n",
            old.public_base64(),
            new.public_base64()
        ),
    )
    .unwrap();
    let out = sign(
        &r.key(),
        &r.target,
        &file,
        &[
            ("NATIVE_BUILD_SIGNING_KEY", &old_secret),
            ("NATIVE_BUILD_SIGNING_KEY_NEXT", &new_secret),
        ],
        &keys,
    );
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{printed}");
    assert!(!printed.contains(&old_secret) && !printed.contains(&new_secret));
    let sig = read_signature(&dir, &r);
    assert_eq!(sig.lines().count(), 2);
    let listed = parse_keys(&std::fs::read_to_string(&keys).unwrap()).unwrap();
    for key in [&listed[..1], &listed[1..]] {
        assert_eq!(verify(&r.key(), &r.target, &sha, &name, &sig, key), Ok(()));
    }
    // The same signature for another set, target or file is refused.
    let other = "0".repeat(64);
    assert!(verify(&other, &r.target, &sha, &name, &sig, &listed).is_err());
    assert!(verify(&r.key(), "aarch64-apple-darwin", &sha, &name, &sig, &listed).is_err());
    assert!(verify(&r.key(), &r.target, &other, &name, &sig, &listed).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_key_launchers_would_refuse_or_a_misnamed_archive_signs_nothing() {
    if cfg!(windows) || openssl().is_none() {
        eprintln!("skipped: tools/ci/sign_native_build.sh is tested on Linux (bash, OpenSSL 3)");
        return;
    }
    let r = request();
    let (dir, file) = folder("refused", &r);
    let (_, secret) = SigningKey::generate().unwrap();
    let keys = dir.join("keys.txt");
    std::fs::write(&keys, "# none yet\n").unwrap();
    let env = [("NATIVE_BUILD_SIGNING_KEY", secret.as_str())];
    // Not listed in the keys file.
    assert!(
        !sign(&r.key(), &r.target, &file, &env, &keys)
            .status
            .success()
    );
    // No key at all.
    assert!(
        !sign(&r.key(), &r.target, &file, &[], &keys)
            .status
            .success()
    );
    // The archive named for another key than the one signed.
    let ephemeral = [("EPHEMERAL", "yes")];
    assert!(
        !sign(&"0".repeat(64), &r.target, &file, &ephemeral, &keys)
            .status
            .success()
    );
    // A dry run signs with a throwaway key, whose public half it leaves for the check.
    let out = sign(&r.key(), &r.target, &file, &ephemeral, &keys);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let public = parse_keys(&std::fs::read_to_string(dir.join("ephemeral.pub")).unwrap()).unwrap();
    let sha = sha256_hex(b"the game with the minimap");
    let name = archive(&r.key(), &r.target);
    let sig = read_signature(&dir, &r);
    assert_eq!(
        verify(&r.key(), &r.target, &sha, &name, &sig, &public),
        Ok(())
    );
    std::fs::remove_dir_all(dir).unwrap();
}
