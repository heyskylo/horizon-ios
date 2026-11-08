//! A `.hshader`'s limits hold while it is read, before anything is kept: the registry reads every
//! submitted package, so a small zip must not make it hold more than the pack's 8 MiB.
use horizon_shader_api::{Pack, package::MAX_TOTAL};
use std::io::Write;

/// A zip of `count` files of `size` zeros each (deflate makes it tiny).
fn zeros(path: &std::path::Path, count: usize, size: usize) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let block = vec![0u8; size];
    for i in 0..count {
        zip.start_file(format!("f{i}.png"), options).unwrap();
        zip.write_all(&block).unwrap();
    }
    zip.finish().unwrap();
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hshader-limits-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_package_over_the_total_is_refused_while_it_is_read() {
    let dir = scratch("total");
    let path = dir.join("big.hshader");
    // 64 files of 1 MiB: 64 MiB inflated, a few hundred KiB zipped; reading stops past 8 MiB.
    zeros(&path, 64, 1 << 20);
    assert!(std::fs::metadata(&path).unwrap().len() < 1 << 20);
    let refused = Pack::read(&path).unwrap_err();
    assert_eq!(refused, format!("the pack is over {} MiB", MAX_TOTAL >> 20));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn one_file_over_its_cap_is_named() {
    let dir = scratch("file");
    let path = dir.join("one.hshader");
    zeros(&path, 1, (1 << 20) + 1);
    assert_eq!(Pack::read(&path).unwrap_err(), "f0.png is over 1024 KiB");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_folder_over_the_total_is_refused_too() {
    let dir = scratch("folder");
    for i in 0..9 {
        std::fs::write(dir.join(format!("f{i}.png")), vec![0u8; 1 << 20]).unwrap();
    }
    assert_eq!(
        Pack::read(&dir).unwrap_err(),
        format!("the pack is over {} MiB", MAX_TOTAL >> 20)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
