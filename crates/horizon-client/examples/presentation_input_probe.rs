//! Run on the main thread with isolated homes and prepared HORIZON_TEST_RESOURCES.
fn main() {
    let mut args = std::env::args_os().skip(1);
    assert_eq!(args.next().as_deref(), Some(std::ffi::OsStr::new("--game-dir")));
    let game_dir = args.next().expect("isolated --game-dir required");
    assert!(args.next().is_none(), "unexpected probe arguments");
    std::fs::create_dir_all(&game_dir).expect("create isolated game directory");
    std::env::set_current_dir(&game_dir).expect("use isolated game directory");
    let home = std::path::PathBuf::from(
        std::env::var_os("HORIZON_HOME").expect("isolated HORIZON_HOME required"),
    );
    horizon_client::logging::init(&home, "Presentation probe");
    horizon_client::game::presentation_input_probe().expect("presentation input probe failed");
}
