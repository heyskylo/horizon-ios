//! Advice for a launcher that cannot replace its own executable.
use std::path::Path;

/// Package-owned files must be replaced by their package manager, not a downloaded archive.
pub fn package_managed(target: &Path) -> bool {
    ["/usr", "/nix/store", "/app"]
        .iter()
        .any(|root| target.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_roots_are_matched_as_path_components() {
        for path in [
            "/usr/bin/horizon-launcher",
            "/usr/local/bin/horizon-launcher",
            "/nix/store/hash-horizon/bin/horizon-launcher",
            "/app/bin/horizon-launcher",
        ] {
            assert!(package_managed(Path::new(path)), "{path}");
        }
        for path in [
            "/home/player/Downloads/Horizon.AppImage",
            "/usr-backup/horizon-launcher",
            "/nix/storehouse/horizon-launcher",
            "/application/horizon-launcher",
            "/tmp/.mount_Horizon/usr/bin/horizon-launcher",
        ] {
            assert!(!package_managed(Path::new(path)), "{path}");
        }
    }
}
