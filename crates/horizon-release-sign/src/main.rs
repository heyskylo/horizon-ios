//! horizon-release-sign: checks a release's signed checksums, and makes new keys
//! (docs/release-flow.md, "Signed checksums"). The signing itself is tools/ci/sign_release.sh: the
//! job that holds the private key runs only the runner's sha256sum and OpenSSL, no code from the
//! repository.
//!
//!     horizon-release-sign verify --tag v2026.10.19 [--key <base64>] [--signature <file>] [--publishing] <folder>
//!     horizon-release-sign keygen --public-out key.pub | gh secret set RELEASE_SIGNING_KEY --env release
//!
//! `verify` checks SHA256SUMS.sig (or `--signature`, e.g. a Nightly's SHA256SUMS.v2.sig with
//! `--tag nightly-<date>-<commit>`) and every file SHA256SUMS lists with the keys built in from
//! release-keys.txt (or `--key`), as the launcher does. With `--publishing` (release.yml) a release
//! may go out unsigned, or signed with a key nobody can check yet, only while release-keys.txt
//! lists no key; once it lists one, anything launchers would refuse fails the release.

use horizon_release_sign::{SIGNATURE, SUMS, Sha256, SigningKey, parse_keys, trusted_keys, verify};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("verify") => check_folder(&args[1..]),
        Some("keygen") => keygen(&args[1..]),
        _ => Err(
            "usage: horizon-release-sign verify --tag <tag> [--key <base64>]... [--signature <file>] \
             [--publishing] <folder> | keygen --public-out <file>"
                .into(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("horizon-release-sign: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The values of every `--name value` in `args`, and the other arguments.
fn options(args: &[String], name: &str) -> (Vec<String>, Vec<String>) {
    let mut values = Vec::new();
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == name {
            values.extend(it.next().cloned());
        } else {
            rest.push(arg.clone());
        }
    }
    (values, rest)
}

fn tag_of(args: &[String]) -> Result<(String, Vec<String>), String> {
    let (tags, rest) = options(args, "--tag");
    let tag = tags.into_iter().next().ok_or("--tag <tag> is needed")?;
    Ok((tag, rest))
}

fn file_hash(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hash = Sha256::default();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            return Ok(hash.hex());
        }
        hash.update(&buffer[..n]);
    }
}

/// Checks a folder holding a release's assets, as the launcher does.
fn check_folder(args: &[String]) -> Result<(), String> {
    let (tag, rest) = tag_of(args)?;
    let (keys, rest) = options(&rest, "--key");
    let (signature_file, rest) = options(&rest, "--signature");
    let signature_file = signature_file
        .into_iter()
        .next()
        .unwrap_or_else(|| SIGNATURE.to_owned());
    if signature_file.contains(['/', '\\']) {
        return Err(format!(
            "--signature names a file of the folder, not {signature_file:?}"
        ));
    }
    let publishing = rest.iter().any(|a| a == "--publishing");
    let folder = PathBuf::from(
        rest.iter()
            .find(|a| *a != "--publishing")
            .ok_or("verify needs the release's folder")?,
    );
    let keys = if keys.is_empty() {
        trusted_keys()
    } else {
        parse_keys(&keys.join("\n"))?
    };
    let sums = std::fs::read(folder.join(SUMS)).map_err(|e| format!("{SUMS}: {e}"))?;
    let listed = match std::fs::read_to_string(folder.join(&signature_file)) {
        Ok(signature) if !keys.is_empty() => verify(&tag, &sums, &signature, &keys)?,
        Ok(_) if publishing => {
            println!(
                "::warning::{tag}: signed, but release-keys.txt lists no key yet, so launchers can't check it"
            );
            horizon_release_sign::parse_sums(&String::from_utf8_lossy(&sums))?
        }
        Err(_) if publishing && keys.is_empty() => {
            println!(
                "::warning::{tag}: published with {SUMS} but unsigned: release-keys.txt lists no key yet"
            );
            horizon_release_sign::parse_sums(&String::from_utf8_lossy(&sums))?
        }
        Ok(_) => return Err("no key to check the signature with".into()),
        Err(_) => {
            return Err(format!(
                "{tag} has no {signature_file}: launchers would refuse it (is the release environment's \
                 RELEASE_SIGNING_KEY set?)"
            ));
        }
    };
    for (name, hex) in &listed {
        horizon_release_sign::check(name, hex, &file_hash(&folder.join(name))?)?;
    }
    println!("{tag}: {} files match {SUMS}", listed.len());
    Ok(())
}

/// Prints a new private key to stdout, which must be a pipe (into `gh secret set`), never a
/// terminal, and writes its public key to `--public-out`.
fn keygen(args: &[String]) -> Result<(), String> {
    let (public_out, _) = options(args, "--public-out");
    let public_out = public_out
        .into_iter()
        .next()
        .ok_or("keygen needs --public-out <file>")?;
    if std::io::stdout().is_terminal() {
        return Err(
            "keygen writes the private key to its output: pipe it into `gh secret set`".into(),
        );
    }
    let (key, secret) = SigningKey::generate()?;
    std::fs::write(&public_out, key.public_base64() + "\n").map_err(|e| e.to_string())?;
    let mut out = std::io::stdout().lock();
    out.write_all(secret.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| e.to_string())
}
