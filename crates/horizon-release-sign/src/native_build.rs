//! Prebuilt native-mod builds (#11859): the game compiled for one exact set of registry native
//! mods, by horizon-mods' CI, signed there and checked by the launcher before it installs one.
//!
//! A build is named by its [`Request`]: the target, the Horizon commit it builds on (the Nightly or
//! release the instance plays) and each mod's id, version and the SHA-256 of its registry file. The
//! launcher computes the request's [`Request::key`] from what it has installed; CI computes it from
//! the workflow's own inputs, never from what a build wrote. The file for a target is
//! [`archive`] in the release [`release_tag`] of horizon-mods, and its signature [`signature`]
//! beside it: one base64 Ed25519 signature per line over [`message`], so a signature can't pass for
//! another set's file, another target's, a release's (`horizon release <tag>`) or a Nightly's.
//!
//! The keys are not the release keys: `native-build-keys.txt` lists them, and their private halves
//! are the `NATIVE_BUILD_SIGNING_KEY` secret(s) of horizon-mods' `native-builds` environment. The
//! registry compiles other people's code there, so a key it holds must never sign a game release.

use crate::{PublicKey, Sha256, parse_keys};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::signature::{ED25519, UnparsedPublicKey};

const KEYS: &str = include_str!("../native-build-keys.txt");

/// The first line of a request's text: a new layout gets a new number, and so new keys.
const REQUEST_HEADER: &str = "horizon native build request 1";

/// The public keys prebuilt native builds are signed with: those in `native-build-keys.txt`. A
/// file that doesn't parse gives none, so every prebuilt build is refused (a test keeps it valid).
pub fn trusted_keys() -> Vec<PublicKey> {
    parse_keys(KEYS).unwrap_or_default()
}

/// One mod of a build: its registry id and version, and the SHA-256 of its registry file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mod {
    pub id: String,
    pub version: String,
    pub sha256: String,
}

/// What a prebuilt build is made of. Two requests with the same mods in another order are the same
/// request: the mods are sorted by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The Rust target triple the game is compiled for.
    pub target: String,
    /// The commit of the Nightly or release the build starts from (40 hex digits).
    pub base: String,
    pub mods: Vec<Mod>,
}

impl Request {
    /// A checked request, its mods sorted by id.
    pub fn new(target: &str, base: &str, mut mods: Vec<Mod>) -> Result<Self, String> {
        if !is_target(target) {
            return Err(format!("{target:?} is not a target triple"));
        }
        if !is_hex(base, 40) {
            return Err(format!("{base:?} is not a commit (40 hex digits)"));
        }
        if mods.is_empty() {
            return Err("a native build needs at least one mod".into());
        }
        for m in &mods {
            if !is_id(&m.id) {
                return Err(format!("{:?} is not a mod id", m.id));
            }
            if !is_version(&m.version) {
                return Err(format!("{}: {:?} is not a version", m.id, m.version));
            }
            if !is_hex(&m.sha256, 64) {
                return Err(format!("{}: {:?} is not a SHA-256", m.id, m.sha256));
            }
        }
        mods.sort();
        if let Some(pair) = mods.windows(2).find(|w| w[0].id == w[1].id) {
            return Err(format!("{} is listed twice", pair[0].id));
        }
        Ok(Self {
            target: target.into(),
            base: base.to_ascii_lowercase(),
            mods: mods
                .into_iter()
                .map(|m| Mod {
                    sha256: m.sha256.to_ascii_lowercase(),
                    ..m
                })
                .collect(),
        })
    }

    /// The text the key hashes: a header, the target, the base, then one line per mod.
    pub fn text(&self) -> String {
        let mut text = format!(
            "{REQUEST_HEADER}\ntarget {}\nbase {}\n",
            self.target, self.base
        );
        for m in &self.mods {
            text.push_str(&format!("mod {} {} {}\n", m.id, m.version, m.sha256));
        }
        text
    }

    /// The request's SHA-256, in hex: what names the build.
    pub fn key(&self) -> String {
        let mut hash = Sha256::default();
        hash.update(self.text().as_bytes());
        hash.hex()
    }
}

/// The horizon-mods release holding the builds made on commit `base`.
pub fn release_tag(base: &str) -> String {
    format!("native-builds-{}", &base[..base.len().min(12)])
}

/// The archive of build `key` for `target` (its first 16 hex digits name it).
pub fn archive(key: &str, target: &str) -> String {
    format!("{}-{target}.tar.gz", &key[..key.len().min(16)])
}

/// The signature file beside [`archive`].
pub fn signature(key: &str, target: &str) -> String {
    format!("{}-{target}.sig", &key[..key.len().min(16)])
}

/// What a build's signature covers: the whole key, the target, then the archive's SHA-256 and
/// name as `sha256sum` writes them. CI signs this text with OpenSSL.
pub fn message(key: &str, target: &str, sha256: &str, asset: &str) -> Vec<u8> {
    format!("horizon native build {key} {target}\n{sha256}  {asset}\n").into_bytes()
}

/// Checks that one of `keys` signed archive `asset` with SHA-256 `sha256` as build `key` for
/// `target`. `signatures` is the signature file's text.
pub fn verify(
    key: &str,
    target: &str,
    sha256: &str,
    asset: &str,
    signatures: &str,
    keys: &[PublicKey],
) -> Result<(), String> {
    if keys.is_empty() {
        return Err("This launcher has no key for prebuilt native builds built in.".into());
    }
    let message = message(key, target, &sha256.to_ascii_lowercase(), asset);
    let signed = signatures
        .lines()
        .filter_map(|line| STANDARD.decode(line.trim()).ok())
        .any(|signature| {
            keys.iter().any(|k| {
                UnparsedPublicKey::new(&ED25519, k)
                    .verify(&message, &signature)
                    .is_ok()
            })
        });
    if signed {
        Ok(())
    } else {
        Err(format!(
            "{asset}'s signature doesn't match Horizon's key for prebuilt native builds, so it was \
             not installed."
        ))
    }
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A registry id: what `horizon_mods::index::valid_id` takes, and no space or line break.
fn is_id(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn is_version(text: &str) -> bool {
    (1..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_graphic())
}

/// `x86_64-unknown-linux-gnu`: lowercase words joined by hyphens, at least two of them.
fn is_target(text: &str) -> bool {
    text.len() <= 64
        && text.split('-').count() >= 2
        && text.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SigningKey;

    const BASE: &str = "656064105c0e1fbd0a8f1d6e5f5d3a8b3c2e1f00";

    fn mod_(id: &str, version: &str, byte: char) -> Mod {
        Mod {
            id: id.into(),
            version: version.into(),
            sha256: byte.to_string().repeat(64),
        }
    }

    fn request() -> Request {
        Request::new(
            "x86_64-unknown-linux-gnu",
            BASE,
            vec![mod_("zoom", "2.0.0", 'b'), mod_("minimap", "1.4.0", 'a')],
        )
        .unwrap()
    }

    /// A key computed by hand, so the registry's CI (Python) and the launcher name a build alike.
    #[test]
    fn the_key_is_the_sha256_of_the_documented_text() {
        let r = request();
        assert_eq!(
            r.text(),
            format!(
                "horizon native build request 1\ntarget x86_64-unknown-linux-gnu\nbase {BASE}\n\
                 mod minimap 1.4.0 {}\nmod zoom 2.0.0 {}\n",
                "a".repeat(64),
                "b".repeat(64)
            )
        );
        // python3 -c 'import hashlib,sys; print(hashlib.sha256(sys.stdin.buffer.read()).hexdigest())'
        assert_eq!(
            r.key(),
            "c07fffc296cf14c5c496a016a930f84b0d0ac028a63c667c2bdf47657788445d"
        );
    }

    #[test]
    fn order_and_case_do_not_change_the_key() {
        let shuffled = Request::new(
            "x86_64-unknown-linux-gnu",
            &BASE.to_ascii_uppercase(),
            vec![mod_("minimap", "1.4.0", 'A'), mod_("zoom", "2.0.0", 'B')],
        )
        .unwrap();
        assert_eq!(shuffled.key(), request().key());
    }

    #[test]
    fn any_input_changes_the_key() {
        let key = request().key();
        let other = |target: &str, base: &str, mods: Vec<Mod>| {
            Request::new(target, base, mods).unwrap().key()
        };
        let mods = || vec![mod_("zoom", "2.0.0", 'b'), mod_("minimap", "1.4.0", 'a')];
        assert_ne!(other("x86_64-pc-windows-gnu", BASE, mods()), key);
        assert_ne!(
            other("x86_64-unknown-linux-gnu", &"1".repeat(40), mods()),
            key
        );
        assert_ne!(
            other(
                "x86_64-unknown-linux-gnu",
                BASE,
                vec![mod_("zoom", "2.0.1", 'b'), mod_("minimap", "1.4.0", 'a')]
            ),
            key
        );
        assert_ne!(
            other(
                "x86_64-unknown-linux-gnu",
                BASE,
                vec![mod_("zoom", "2.0.0", 'c'), mod_("minimap", "1.4.0", 'a')]
            ),
            key
        );
        assert_ne!(
            other(
                "x86_64-unknown-linux-gnu",
                BASE,
                vec![mod_("zoom", "2.0.0", 'b')]
            ),
            key
        );
    }

    #[test]
    fn a_request_that_could_forge_another_line_is_refused() {
        let base = "x86_64-unknown-linux-gnu";
        assert!(Request::new(base, BASE, vec![]).is_err());
        assert!(Request::new("linux", BASE, vec![mod_("a", "1", 'a')]).is_err());
        assert!(Request::new("x86 64-linux", BASE, vec![mod_("a", "1", 'a')]).is_err());
        assert!(Request::new(base, "abc", vec![mod_("a", "1", 'a')]).is_err());
        assert!(Request::new(base, BASE, vec![mod_("a b", "1", 'a')]).is_err());
        assert!(Request::new(base, BASE, vec![mod_("a", "1\nmod x", 'a')]).is_err());
        assert!(Request::new(base, BASE, vec![mod_("a", "1", 'g')]).is_err());
        assert!(Request::new(base, BASE, vec![mod_("a", "1", 'a'), mod_("a", "2", 'b')]).is_err());
    }

    #[test]
    fn a_signed_build_verifies_and_nothing_else_does() {
        let (signer, _) = SigningKey::generate().unwrap();
        let (stranger, _) = SigningKey::generate().unwrap();
        let r = request();
        let key = r.key();
        let target = &r.target;
        let asset = archive(&key, target);
        let sha = "c".repeat(64);
        let sign = |k: &SigningKey, msg: Vec<u8>| k.sign_bytes(&msg) + "\n";
        let good = sign(&signer, message(&key, target, &sha, &asset));
        let keys = [signer.public()];
        assert_eq!(verify(&key, target, &sha, &asset, &good, &keys), Ok(()));
        // Another key, another archive, another set or target, no key at all.
        let theirs = sign(&stranger, message(&key, target, &sha, &asset));
        assert!(verify(&key, target, &sha, &asset, &theirs, &keys).is_err());
        assert!(verify(&key, target, &"d".repeat(64), &asset, &good, &keys).is_err());
        let other = "e".repeat(64);
        assert!(verify(&other, target, &sha, &asset, &good, &keys).is_err());
        assert!(verify(&key, "aarch64-apple-darwin", &sha, &asset, &good, &keys).is_err());
        assert!(verify(&key, target, &sha, &asset, &good, &[]).is_err());
        // A release's signature over the same bytes is no build's.
        let release = signer.sign(&key, format!("{sha}  {asset}\n").as_bytes()) + "\n";
        assert!(verify(&key, target, &sha, &asset, &release, &keys).is_err());
        // One line of several is enough (a key being replaced signs beside the old one).
        let both = theirs + &good;
        assert_eq!(verify(&key, target, &sha, &asset, &both, &keys), Ok(()));
    }

    #[test]
    fn the_built_in_keys_file_parses() {
        assert!(parse_keys(KEYS).is_ok());
    }

    #[test]
    fn assets_are_named_by_the_keys_first_16_digits() {
        let key = request().key();
        assert_eq!(release_tag(BASE), "native-builds-656064105c0e");
        assert_eq!(
            archive(&key, "aarch64-apple-darwin"),
            format!("{}-aarch64-apple-darwin.tar.gz", &key[..16])
        );
        assert_eq!(
            signature(&key, "aarch64-apple-darwin"),
            format!("{}-aarch64-apple-darwin.sig", &key[..16])
        );
    }
}
