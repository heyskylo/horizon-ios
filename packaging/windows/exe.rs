//! Windows executables' resources, shared by the build scripts of the game's executables
//! (`crates/horizon-client/build.rs`, `crates/horizon-official/build.rs`, which include it with
//! `#[path]`). Embeds Horizon's icon (`packaging/icons/horizon.ico`) as icon resource 1: Explorer
//! and the task bar show it, and the window takes it as its icon. The optional Segment Heap
//! manifest (`crates/horizon-client/windows-segment-heap.manifest`, the package's
//! `windows-segment-heap` feature) is embedded in the same resource compilation.

/// The resources and the stack size of the executable `bin` of the package being built; nothing
/// for other targets than Windows.
pub fn resources(bin: &str) {
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
        println!("cargo:rerun-if-env-changed=WINDRES");
        println!("cargo:rerun-if-env-changed=AR");
        println!("cargo:rerun-if-env-changed=RC_PATH");
        let target_env = std::env::var("CARGO_CFG_TARGET_ENV")
            .expect("Cargo must provide the target environment");
        let arg = match target_env.as_str() {
            "msvc" => "/STACK:8388608",
            _ => "-Wl,--stack,8388608",
        };
        println!("cargo:rustc-link-arg-bin={bin}={arg}");
        let (resource_manifest_dir, resource_icon) = match resource_path(&manifest_dir, &target_env)
            .and_then(|dir| Ok((dir, resource_path(&ico, &target_env)?)))
        {
            Ok(paths) => paths,
            // #10398: a local GNU build in a folder whose path has a space or a non-ASCII letter,
            // on a volume without 8.3 names, still builds (the launcher's native mod builds use
            // that toolchain), without the icon. CI never does.
            Err(e) if std::env::var_os("CI").is_none() => {
                println!("cargo:warning=building without Horizon's icon: {e}");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon(&resource_icon);
        if std::env::var_os("CARGO_FEATURE_WINDOWS_SEGMENT_HEAP").is_some() {
            let manifest = manifest_dir.join("../horizon-client/windows-segment-heap.manifest");
            println!("cargo:rerun-if-changed={}", manifest.display());
            let resource_manifest =
                resource_path(&manifest, &target_env).unwrap_or_else(|e| panic!("{e}"));
            resource.set_manifest_file(&resource_manifest);
        }

        let original_manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
        if cfg!(windows) && target_env == "gnu" {
            // GNU windres can split paths with spaces or fail on non-ASCII paths while
            // preprocessing the .rc file. Pass an 8.3 alias for the include directory; the icon
            // and optional manifest use the same alias.
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

/// `path` as the resource compiler takes it: an error only when GNU windres would need an 8.3 alias
/// that the volume doesn't have.
fn resource_path(path: &std::path::Path, target_env: &str) -> Result<String, String> {
    let path = path.canonicalize().unwrap_or_else(|error| {
        panic!(
            "resolving Windows resource path {}: {error}",
            path.display()
        )
    });
    #[cfg(windows)]
    let path = {
        let path_string = path.to_string_lossy();
        if target_env == "gnu"
            && (!path_string.is_ascii() || path_string.chars().any(char::is_whitespace))
        {
            short_windows_path(&path).map_err(|error| {
                format!(
                    "GNU windres needs an ASCII Windows resource path for {}: {error}",
                    path.display()
                )
            })?
        } else {
            path
        }
    };
    #[cfg(not(windows))]
    let _ = target_env;
    Ok(path.to_string_lossy().replace('\\', "/"))
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
