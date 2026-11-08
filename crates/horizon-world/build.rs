//! `HORIZON_WORLD_SOURCES`: a hash of the code that resolves the behaviour and
//! block-registry snapshots (`src/level/data/cache.rs`). This crate's sources,
//! `horizon-core`'s, both manifests, the workspace's `Cargo.lock` and the build
//! profile and target. A rebuild that changes none of them (an edit in the
//! renderer or in `game.rs`) keeps the snapshots; the executable's own
//! modification time, the key before, threw them away on every build.

use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let core = manifest.join("../horizon-core");
    let lock = manifest.join("../../Cargo.lock");

    let mut files = Vec::new();
    for dir in [manifest.join("src"), core.join("src")] {
        walk(&dir, &mut files);
        println!("cargo:rerun-if-changed={}", dir.display());
    }
    for file in [manifest.join("Cargo.toml"), core.join("Cargo.toml"), lock] {
        if file.is_file() {
            println!("cargo:rerun-if-changed={}", file.display());
            files.push(file);
        }
    }
    files.sort();

    // Two differently seeded hashes: 128 bits, so two builds never share a key by chance.
    let mut hashers = [DefaultHasher::new(), DefaultHasher::new()];
    1u8.hash(&mut hashers[1]);
    for hasher in &mut hashers {
        for name in ["PROFILE", "TARGET", "OPT_LEVEL", "CARGO_ENCODED_RUSTFLAGS"] {
            std::env::var(name).unwrap_or_default().hash(hasher);
        }
    }
    for file in &files {
        let relative = file.strip_prefix(&manifest).unwrap_or(file);
        let bytes = fs::read(file).unwrap_or_default();
        for hasher in &mut hashers {
            relative.to_string_lossy().replace('\\', "/").hash(hasher);
            bytes.hash(hasher);
        }
    }
    println!(
        "cargo:rustc-env=HORIZON_WORLD_SOURCES={:016x}{:016x}",
        hashers[0].finish(),
        hashers[1].finish()
    );
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}
