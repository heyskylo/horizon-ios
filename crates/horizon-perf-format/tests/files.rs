use std::fs;
use std::path::{Path, PathBuf};

use horizon_perf_format::files::{self, KEEP_FILES};
use horizon_perf_format::v1::{Side, Stop};
use horizon_perf_format::{CaptureWriter, Field};

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("hzperf-files-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn touch(dir: &Path, name: &str, bytes: usize) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, vec![0u8; bytes]).unwrap();
    path
}

/// Expected strings come from `date -u -d @N +%Y%m%d-%H%M%S`.
#[test]
fn names_are_the_utc_time() {
    assert_eq!(
        files::file_name(1_762_526_324),
        "profile-20251107-143844Z.hzperf"
    );
    assert_eq!(
        files::file_name(951_782_400),
        "profile-20000229-000000Z.hzperf"
    );
    assert_eq!(
        files::file_name(4_107_542_399),
        "profile-21000228-235959Z.hzperf"
    );
}

#[test]
fn only_names_the_game_makes_match() {
    assert!(files::is_capture_name("profile-20251107-143844Z.hzperf"));
    for renamed in [
        "profile-20251107-143844Z-lag.hzperf",
        "profile-20251107-143844Z.hzperf.bak",
        "my-profile-20251107-143844Z.hzperf",
        "profile-20251107-14384Z.hzperf",
        "profile-2025110x-143844Z.hzperf",
        "perf-2025-11-07_14-38-44Z.json",
    ] {
        assert!(!files::is_capture_name(renamed), "{renamed}");
    }
}

#[test]
fn retention_deletes_the_oldest_of_31_and_never_a_renamed_file() {
    let scratch = Scratch::new("retention");
    let dir = &scratch.0;
    let names: Vec<String> = (0..31)
        .map(|i| files::file_name(1_762_526_000 + i * 60))
        .collect();
    for name in &names {
        touch(dir, name, 10);
    }
    let renamed = touch(dir, "profile-20251107-140000Z-keep-me.hzperf", 10);
    let old_report = touch(dir, "perf-2025-11-07_14-00-00Z.json", 10);
    let keep = dir.join(&names[30]);
    let deleted = files::retain(dir, &keep, KEEP_FILES, u64::MAX).unwrap();
    assert_eq!(deleted, vec![dir.join(&names[0])]);
    assert!(!dir.join(&names[0]).exists());
    for name in &names[1..] {
        assert!(dir.join(name).exists(), "{name}");
    }
    assert!(renamed.exists() && old_report.exists());
}

#[test]
fn retention_by_size_stops_once_under_the_limit_and_spares_the_new_file() {
    let scratch = Scratch::new("bytes");
    let dir = &scratch.0;
    let a = touch(dir, "profile-20251107-100000Z.hzperf", 400);
    let b = touch(dir, "profile-20251107-110000Z.hzperf", 400);
    let c = touch(dir, "profile-20251107-120000Z.hzperf", 400);
    let deleted = files::retain(dir, &c, 30, 1000).unwrap();
    assert_eq!(deleted, vec![a.clone()]);
    assert!(!a.exists() && b.exists() && c.exists());
    let deleted = files::retain(dir, &c, 30, 100).unwrap();
    assert_eq!(
        deleted,
        vec![b],
        "the file just written stays even when alone it is over"
    );
    assert!(c.exists());
}

#[test]
fn a_second_capture_in_the_same_second_gets_the_next_name() {
    let scratch = Scratch::new("create");
    let (_, first) = files::create(&scratch.0, 1_762_526_324).unwrap();
    let (_, second) = files::create(&scratch.0, 1_762_526_324).unwrap();
    assert_eq!(
        first.file_name().unwrap().to_str().unwrap(),
        "profile-20251107-143844Z.hzperf"
    );
    assert_eq!(
        second.file_name().unwrap().to_str().unwrap(),
        "profile-20251107-143845Z.hzperf"
    );
}

#[test]
fn a_capture_at_a_path_of_its_own_makes_the_folders_and_replaces_what_was_there() {
    let scratch = Scratch::new("create-at");
    let path = scratch.0.join("a").join("b").join("trace.hzperf");
    let mut first = files::create_at(&path).unwrap();
    std::io::Write::write_all(&mut first, b"old contents").unwrap();
    drop(first);
    assert_eq!(fs::metadata(&path).unwrap().len(), 12);
    drop(files::create_at(&path).unwrap());
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
}

#[test]
fn the_saved_line_names_the_file_and_no_address() {
    let line = files::saved_line(Path::new("reports/profile-20251107-143844Z.hzperf"));
    assert!(
        line.contains("reports/profile-20251107-143844Z.hzperf"),
        "{line}"
    );
    assert!(!line.contains("://"), "{line}");
}

#[test]
fn a_capture_is_complete_only_after_its_stream_ends() {
    let scratch = Scratch::new("complete");
    let path = scratch.0.join("c.hzperf");
    let mut writer = CaptureWriter::new(fs::File::create(&path).unwrap()).unwrap();
    writer
        .write(Field::Stop(Stop {
            side: Side::Server as i32,
            ..Stop::default()
        }))
        .unwrap();
    assert!(!files::is_complete(&path).unwrap(), "still being written");
    writer.finish().unwrap();
    assert!(files::is_complete(&path).unwrap());
}

#[test]
fn a_file_just_created_is_not_complete() {
    let scratch = Scratch::new("empty");
    let path = scratch.0.join("c.hzperf");
    fs::File::create(&path).unwrap();
    assert!(!files::is_complete(&path).unwrap());
}
