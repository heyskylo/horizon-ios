//! Embeds Horizon's icon (`packaging/icons/horizon.ico`) in the Windows executables, as icon
//! resource 1: Explorer and the task bar show it, and the window takes it as its icon.
//! The optional Segment Heap manifest is embedded in the same resource compilation.
fn main() {
    let root =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    horizon_mod_api_gen::generate(&root, &out_dir, horizon_mod_api_gen::Target::Client)
        .expect("generating native mod API");
    let manifest_dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let ico = manifest_dir
        .join("../..")
        .join("packaging/icons/horizon.ico");
    let target_os =
        std::env::var("CARGO_CFG_TARGET_OS").expect("Cargo must provide the build target OS");
    println!("cargo:rerun-if-changed={}", ico.display());
    // The game's main thread gets 8 MB of stack on Windows as on macOS and Linux (Windows gives 1
    // MB): splitscreen builds a joining player's `Game`, ~67 KB, deep in the event loop.
    if target_os == "windows" {
        let target_env = std::env::var("CARGO_CFG_TARGET_ENV")
            .expect("Cargo must provide the target environment");
        let arg = match target_env.as_str() {
            "msvc" => "/STACK:8388608",
            _ => "-Wl,--stack,8388608",
        };
        println!("cargo:rustc-link-arg-bin=horizon-client={arg}");
        let mut resource = winresource::WindowsResource::new();
        let resource_manifest_dir = resource_path(&manifest_dir, &target_env);
        let resource_icon = resource_path(&ico, &target_env);
        resource.set_icon(&resource_icon);
        if std::env::var_os("CARGO_FEATURE_WINDOWS_SEGMENT_HEAP").is_some() {
            let manifest = manifest_dir.join("windows-segment-heap.manifest");
            println!("cargo:rerun-if-changed={}", manifest.display());
            let resource_manifest = resource_path(&manifest, &target_env);
            resource.set_manifest_file(&resource_manifest);
        }

        let original_manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
        if cfg!(windows) && target_env == "gnu" {
            // GNU windres can split paths with spaces or fail on non-ASCII paths while
            // preprocessing the .rc file. Pass an 8.3 alias for the include directory; the icon
            // and optional manifest use the same alias. If short paths are unavailable, fail the
            // build rather than emit an iconless executable.
            // This build script has no other Rust threads, and winresource reads this variable
            // only while compiling the resource; restore the original value immediately after.
            unsafe { std::env::set_var("CARGO_MANIFEST_DIR", &resource_manifest_dir) };
        }
        let result = resource.compile();
        if cfg!(windows) && target_env == "gnu" {
            unsafe { std::env::set_var("CARGO_MANIFEST_DIR", original_manifest_dir) };
        }
        result.expect("embedding Horizon's icon in Windows resources");
    }
}

fn resource_path(path: &std::path::Path, target_env: &str) -> String {
    let mut path = path.canonicalize().unwrap_or_else(|error| {
        panic!(
            "resolving Windows resource path {}: {error}",
            path.display()
        )
    });
    #[cfg(windows)]
    {
        let path_string = path.to_string_lossy();
        if target_env == "gnu"
            && (!path_string.is_ascii() || path_string.chars().any(char::is_whitespace))
        {
            path = short_windows_path(&path).unwrap_or_else(|error| {
                panic!(
                    "GNU windres needs an ASCII Windows resource path for {}: {error}",
                    path.display()
                )
            });
        }
    }
    let _ = target_env;
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(windows)]
fn short_windows_path(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetShortPathNameW(input: *const u16, output: *mut u16, capacity: u32) -> u32;
    }

    let input: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut output = vec![0u16; 32_768];
    let length =
        unsafe { GetShortPathNameW(input.as_ptr(), output.as_mut_ptr(), output.len() as u32) };
    if length == 0 || length as usize >= output.len() {
        return Err(std::io::Error::last_os_error());
    }
    let short_path = std::ffi::OsString::from_wide(&output[..length as usize]);
    let short_path_string = short_path.to_string_lossy();
    if !short_path_string.is_ascii() || short_path_string.chars().any(char::is_whitespace) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the volume has no ASCII 8.3 path for this file",
        ));
    }
    Ok(short_path.into())
}
