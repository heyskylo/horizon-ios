//! Signed downloads (docs/security.md): a game or launcher archive is installed only when its
//! SHA-256 matches the release's SHA256SUMS and that list is signed by one of the keys built in
//! from crates/horizon-release-sign/release-keys.txt. A release from before signing installs only
//! with the SHA-256 pinned for it in legacy-releases.txt (a rollback). Anything else is refused
//! with a message saying why.

use crate::backend::REPO;
use horizon_release_sign::{PublicKey, SIGNATURE, SIGNATURE_V2, SUMS};

/// Where a release's attached file downloads from (GitHub's fixed address for it).
pub fn asset_url(tag: &str, name: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{tag}/{name}")
}

/// The signed SHA-256 of `asset` in release `tag`, fetching its SHA256SUMS and SHA256SUMS.sig
/// with `fetch` (None: the release has no such file), or why the download must be refused. A
/// release from before signing gets its pinned SHA-256, whatever files it carries now.
pub fn digest(
    tag: &str,
    asset: &str,
    keys: &[PublicKey],
    fetch: impl Fn(&str) -> Result<Option<Vec<u8>>, String>,
) -> Result<String, String> {
    if let Some(hex) = horizon_release_sign::legacy_digest(tag, asset) {
        return Ok(hex.to_owned());
    }
    let sums = fetch(&asset_url(tag, SUMS))?;
    let signature = fetch(&asset_url(tag, SIGNATURE))?
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    horizon_release_sign::signed_digest(tag, asset, sums.as_deref(), signature.as_deref(), keys)
}

/// The signed SHA-256 of `asset` in the Nightly published under `tag` (`nightly`, or a dated
/// `nightly-<date>`) that installs as `version` (`nightly-<date>-<commit>`, parsed strictly from its
/// title by `horizon_release_sign::nightly_title_version`): its SHA256SUMS.v2.sig must sign the
/// checksums for exactly that version, so an older Nightly can't be served as a newer one. The
/// channel's SHA256SUMS.sig is for launchers from before and is not read.
pub fn nightly_digest(
    tag: &str,
    version: &str,
    asset: &str,
    keys: &[PublicKey],
    fetch: impl Fn(&str) -> Result<Option<Vec<u8>>, String>,
) -> Result<String, String> {
    let sums = fetch(&asset_url(tag, SUMS))?;
    let signature = fetch(&asset_url(tag, SIGNATURE_V2))?
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    horizon_release_sign::nightly_digest(
        version,
        asset,
        sums.as_deref(),
        signature.as_deref(),
        keys,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_release_sign::{sha256_hex, sums_file, SigningKey};

    #[test]
    fn the_launcher_ships_with_a_release_key() {
        // With no key in release-keys.txt every release after the pinned ones would be refused:
        // this fails until the release environment's public key is listed (docs/security.md).
        assert!(
            !horizon_release_sign::trusted_keys().is_empty(),
            "crates/horizon-release-sign/release-keys.txt lists no key"
        );
    }

    #[test]
    fn releases_from_before_signing_install_only_with_their_pinned_files() {
        let tampered =
            |_: &str| -> Result<Option<Vec<u8>>, String> { Ok(Some(b"anything".to_vec())) };
        let pinned =
            horizon_release_sign::legacy_digest("v2026.10.12", "horizon-linux-x86_64.tar.gz")
                .expect("v2026.10.12 is from before signing");
        assert_eq!(
            digest("v2026.10.12", "horizon-linux-x86_64.tar.gz", &[], tampered),
            Ok(pinned.to_owned())
        );
    }

    #[test]
    fn only_archives_signed_by_a_built_in_key_install() {
        let (key, _) = SigningKey::generate().unwrap();
        let sums = sums_file(&[("horizon.zip".into(), sha256_hex(b"game"))]);
        let signature = key.sign("v2099.01.01", sums.as_bytes());
        // GitHub as it answers for a signed release (any tag: the files are what a replay would serve).
        let signed = |url: &str| -> Result<Option<Vec<u8>>, String> {
            assert!(url.starts_with("https://github.com/horizon-rust/horizon/releases/download/"));
            Ok(Some(
                if url.ends_with(".sig") {
                    signature.clone()
                } else {
                    sums.clone()
                }
                .into_bytes(),
            ))
        };
        let keys = [key.public()];
        assert_eq!(
            digest("v2099.01.01", "horizon.zip", &keys, signed),
            Ok(sha256_hex(b"game"))
        );
        // Signed for another tag, by another key, or not listing the archive: refused.
        assert!(digest("v2099.01.02", "horizon.zip", &keys, signed).is_err());
        let (other, _) = SigningKey::generate().unwrap();
        assert!(digest("v2099.01.01", "horizon.zip", &[other.public()], signed).is_err());
        assert!(digest("v2099.01.01", "other.zip", &keys, signed).is_err());
        // No checksums (a release from before signing): refused, saying so.
        let unsigned = |_: &str| -> Result<Option<Vec<u8>>, String> { Ok(None) };
        let refused = digest("v2099.01.01", "horizon.zip", &keys, unsigned).unwrap_err();
        assert!(refused.contains("not signed"), "{refused}");
        // Checksums that can't be fetched are an error too, never a pass.
        let offline = |_: &str| -> Result<Option<Vec<u8>>, String> { Err("No connection".into()) };
        assert!(digest("v2099.01.01", "horizon.zip", &keys, offline).is_err());
    }

    /// The release channel never installs what was signed for a Nightly, whatever tag it is served
    /// under; a Nightly installs only with its own version's signature (SHA256SUMS.v2.sig), never the
    /// channel's, another Nightly's or a release's.
    #[test]
    fn the_release_channel_refuses_nightly_signatures() {
        const VERSION: &str = "nightly-2026-10-28-0c02067";
        let (key, _) = SigningKey::generate().unwrap();
        let keys = [key.public()];
        let sums = sums_file(&[("horizon.zip".into(), sha256_hex(b"game"))]);
        // Serves SHA256SUMS, SHA256SUMS.sig signed for `channel` and SHA256SUMS.v2.sig for `v2`.
        let serving = |channel: &'static str, v2: &'static str| {
            let (one, two) = (
                key.sign(channel, sums.as_bytes()),
                key.sign(v2, sums.as_bytes()),
            );
            let sums = sums.clone();
            move |url: &str| -> Result<Option<Vec<u8>>, String> {
                Ok(Some(
                    if url.ends_with(SIGNATURE_V2) {
                        two.clone()
                    } else if url.ends_with(SIGNATURE) {
                        one.clone()
                    } else {
                        sums.clone()
                    }
                    .into_bytes(),
                ))
            }
        };
        let nightly = serving(horizon_release_sign::NIGHTLY, VERSION);
        assert_eq!(
            nightly_digest("nightly", VERSION, "horizon.zip", &keys, &nightly),
            Ok(sha256_hex(b"game"))
        );
        // Another version than the one signed (an older Nightly served as tonight's, or the reverse).
        assert!(nightly_digest(
            "nightly",
            "nightly-2026-10-29-1111111",
            "horizon.zip",
            &keys,
            &nightly
        )
        .is_err());
        for tag in ["v0.2.81", "nightly", "nightly-2026-10-27"] {
            assert!(
                digest(tag, "horizon.zip", &keys, &nightly).is_err(),
                "{tag}"
            );
        }
        // Only the channel's signature (a Nightly from before v2): refused by a current launcher.
        let old = |url: &str| -> Result<Option<Vec<u8>>, String> {
            if url.ends_with(SIGNATURE_V2) {
                Ok(None)
            } else {
                nightly(url)
            }
        };
        let refused = nightly_digest("nightly", VERSION, "horizon.zip", &keys, old).unwrap_err();
        assert!(refused.contains("SHA256SUMS.v2.sig"), "{refused}");
        let release = serving("v0.2.81", "v0.2.81");
        assert!(digest("v0.2.81", "horizon.zip", &keys, &release).is_ok());
        assert!(nightly_digest("nightly", VERSION, "horizon.zip", &keys, &release).is_err());
    }
}
