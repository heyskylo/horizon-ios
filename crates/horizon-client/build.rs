//! Generates the native mod API, and gives the Windows executable Horizon's icon and its stack
//! (`packaging/windows/exe.rs`, shared with the official executable's build script).
#[path = "../../packaging/windows/exe.rs"]
mod windows_exe;

fn main() {
    let root =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    horizon_mod_api_gen::generate(&root, &out_dir, horizon_mod_api_gen::Target::Client)
        .expect("generating native mod API");
    // A workspace horizon-mixins changed holds native mods, whichever tool ran it (the launcher,
    // or a mod's own script with no `native-mods.json` beside its game): `HORIZON_MIXIN_MODS`
    // names them for `game::mods::built_with_native_mods`. Watched only once it exists: a
    // bundle applied in place also edits `game/`, which `generate` watches.
    let stage = root.join(horizon_mod_api_gen::MIXINS_STAGE);
    if stage.is_file() {
        println!("cargo:rerun-if-changed={}", stage.display());
    }
    let staged = horizon_mod_api_gen::staged_mods(&root);
    if !staged.is_empty() {
        println!("cargo:rustc-env=HORIZON_MIXIN_MODS={}", staged.join(","));
    }
    windows_exe::resources("horizon-client");
}
