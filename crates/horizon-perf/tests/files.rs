mod common;

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use common::{CLIENT, SERVER, TempDir, health, start, stop, write_capture};
use horizon_perf::cli::files::{captures_in, latest, list_line, peek};
use horizon_perf_format::v1::Kind;

fn put(dir: &Path, name: &str, bytes: &[u8], age_s: u64) {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    let when = SystemTime::now() - Duration::from_secs(age_s);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

fn finished(seconds: f64) -> Vec<u8> {
    write_capture(&[
        start(SERVER, Kind::Dedicated),
        health(SERVER, 1.0, 20.0),
        stop(SERVER, seconds),
    ])
    .bytes
}

fn being_written() -> Vec<u8> {
    write_capture(&[start(SERVER, Kind::Dedicated), health(SERVER, 1.0, 20.0)]).bytes
}

#[test]
fn latest_picks_the_newest_finished_file_and_skips_one_still_being_written() {
    let dir = TempDir::new("latest");
    put(
        dir.path(),
        "profile-20261107-100000Z.hzperf",
        &finished(10.0),
        300,
    );
    put(
        dir.path(),
        "profile-20261107-110000Z.hzperf",
        &finished(11.0),
        200,
    );
    put(
        dir.path(),
        "profile-20261107-120000Z.hzperf",
        &being_written(),
        100,
    );
    put(dir.path(), "notes.txt", b"not a capture", 1);
    put(
        dir.path(),
        "profile-20261107-130000Z.hzperf",
        b"not gzip at all",
        50,
    );

    let path = latest(dir.path(), true).unwrap();
    assert_eq!(path.file_name().unwrap(), "profile-20261107-110000Z.hzperf");
}

#[test]
fn latest_for_follow_takes_the_newest_file_even_while_it_is_written() {
    let dir = TempDir::new("latest-any");
    put(
        dir.path(),
        "profile-20261107-100000Z.hzperf",
        &finished(10.0),
        300,
    );
    put(
        dir.path(),
        "profile-20261107-120000Z.hzperf",
        &being_written(),
        100,
    );
    let path = latest(dir.path(), false).unwrap();
    assert_eq!(path.file_name().unwrap(), "profile-20261107-120000Z.hzperf");
}

#[test]
fn latest_with_nothing_finished_says_so() {
    let dir = TempDir::new("latest-none");
    put(dir.path(), "a.hzperf", &being_written(), 10);
    let error = latest(dir.path(), true).unwrap_err();
    assert!(error.starts_with("no finished captures in "), "{error}");
}

#[test]
fn a_renamed_capture_still_counts_and_files_are_listed_newest_first() {
    let dir = TempDir::new("order");
    put(dir.path(), "before-the-fix.hzperf", &finished(1.0), 500);
    put(dir.path(), "after-the-fix.hzperf", &finished(2.0), 5);
    let names: Vec<_> = captures_in(dir.path())
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["after-the-fix.hzperf", "before-the-fix.hzperf"]);
}

#[test]
fn list_lines_say_what_each_file_holds_and_whether_it_is_finished() {
    let dir = TempDir::new("list");
    let both = write_capture(&[
        start(SERVER, Kind::Integrated),
        start(CLIENT, Kind::Client),
        stop(CLIENT, 7.5),
        stop(SERVER, 8.0),
    ])
    .bytes;
    put(dir.path(), "both.hzperf", &both, 30);
    put(dir.path(), "live.hzperf", &being_written(), 20);
    put(dir.path(), "junk.hzperf", b"plain text, not gzip", 10);

    let line = |name: &str| {
        let path = dir.path().join(name);
        list_line(&path, &peek(&path).unwrap())
    };
    let size = |name: &str| fs::metadata(dir.path().join(name)).unwrap().len();
    assert_eq!(
        line("both.hzperf"),
        format!(
            "both.hzperf  {} B  finished  integrated 8.0 s + client 7.5 s",
            size("both.hzperf")
        )
    );
    assert_eq!(
        line("live.hzperf"),
        format!("live.hzperf  {} B  writing  dedicated", size("live.hzperf"))
    );
    assert_eq!(
        line("junk.hzperf"),
        format!(
            "junk.hzperf  {} B  unreadable: this file is not a .hzperf capture: it is not gzip data",
            size("junk.hzperf")
        )
    );
}
