use super::*;
use horizon_assets::root_lease::RootLease;
use std::sync::{Arc, Barrier};
use std::time::{Duration, SystemTime};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "horizon-pack-generation-{}",
            finish(Sha256::new().chain_update(nonce()))
        ));
        fs::create_dir_all(path.join("vanilla/assets")).unwrap();
        Self(path)
    }
    fn cache(&self) -> PathBuf {
        self.0.join("cache")
    }
    fn vanilla(&self) -> PathBuf {
        self.0.join("vanilla")
    }
    fn zip(&self, value: &str, old_only: bool) -> Pack {
        let mut files = vec![
            (
                "pack.mcmeta",
                br#"{"pack":{"description":"original generation control","pack_format":97}}"#
                    .to_vec(),
            ),
            ("assets/test/sounds/late.ogg", value.as_bytes().to_vec()),
            (
                "assets/test/lang/en_us.json",
                format!(r#"{{"value":"{value}"}}"#).into_bytes(),
            ),
        ];
        if old_only {
            files.push(("assets/test/sounds/old.ogg", b"only A".to_vec()));
        }
        let path = self.0.join("same.zip");
        fs::write(&path, super::tests::archive(&files, false)).unwrap();
        crate::packs::server_pack(1, &path).unwrap()
    }
    fn load(&self, pack: &Pack) -> (PathBuf, RootLease) {
        crate::packs::resource_root_leased(
            &self.vanilla(),
            std::slice::from_ref(pack),
            &self.cache(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn sound(root: &Path) -> Vec<u8> {
    fs::read(root.join("assets/test/sounds/late.ogg")).unwrap()
}

#[test]
fn same_id_archive_replacement_keeps_the_old_generation_and_hits_reuse() {
    let f = Fixture::new();
    let a = f.zip("A", true);
    let (old, _old_lease) = f.load(&a);
    assert_eq!(sound(&old), b"A");
    fs::write(old.join("hit-sentinel"), b"owned cache control").unwrap();
    let (hit, _hit_lease) = f.load(&a);
    assert_eq!(old, hit);
    assert!(hit.join("hit-sentinel").exists());
    fs::File::options()
        .write(true)
        .open(f.0.join("same.zip"))
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        f.load(&a).0,
        old,
        "ZIP usage timestamps must not force a rebuild"
    );
    let b = f.zip("B", false);
    assert_eq!(a.id, b.id);
    let (new, _new_lease) = f.load(&b);
    assert_ne!(old, new);
    assert_eq!(sound(&old), b"A");
    assert_eq!(sound(&new), b"B");
    assert!(old.join("assets/test/sounds/old.ogg").exists());
    assert!(!new.join("assets/test/sounds/old.ogg").exists());
    assert!(
        fs::read_to_string(old.join("assets/test/lang/en_us.json"))
            .unwrap()
            .contains('A')
    );
}

#[cfg(unix)]
#[test]
fn native_ordinary_links_remain_live_but_materialized_json_gets_a_generation() {
    let f = Fixture::new();
    let folder = f.0.join("native");
    let live = folder.join("assets/test/sounds/late.ogg");
    let json = folder.join("assets/test/lang/en_us.json");
    fs::create_dir_all(live.parent().unwrap()).unwrap();
    fs::create_dir_all(json.parent().unwrap()).unwrap();
    fs::write(
        folder.join("pack.mcmeta"),
        br#"{"pack":{"description":"original native control","pack_format":97}}"#,
    )
    .unwrap();
    fs::write(&live, b"live A").unwrap();
    fs::write(&json, br#"{"value":"A"}"#).unwrap();
    let pack = crate::packs::pack(
        "file/native".into(),
        crate::text::Text::empty(),
        Source::Dir(folder),
    )
    .unwrap();
    let (old, _old_lease) = f.load(&pack);
    fs::write(old.join("hit-sentinel"), b"keep").unwrap();
    fs::write(&live, b"live B").unwrap();
    assert_eq!(f.load(&pack).0, old);
    assert_eq!(sound(&old), b"live B");
    assert!(old.join("hit-sentinel").exists());
    fs::write(&json, br#"{"value":"new materialized B"}"#).unwrap();
    let (new, _new_lease) = f.load(&pack);
    assert_ne!(old, new);
    assert!(
        fs::read_to_string(old.join("assets/test/lang/en_us.json"))
            .unwrap()
            .contains("\"A\"")
    );
    assert!(
        fs::read_to_string(new.join("assets/test/lang/en_us.json"))
            .unwrap()
            .contains("new materialized B")
    );
    assert_eq!(
        sound(&old),
        b"live B",
        "ordinary folder resources intentionally stay live"
    );
}

#[test]
fn failed_and_concurrent_publications_never_replace_a_published_root() {
    let f = Fixture::new();
    let a = f.zip("A", true);
    let (old, _lease) = f.load(&a);
    fs::write(f.0.join("same.zip"), b"not a ZIP").unwrap();
    assert!(crate::packs::resource_root_leased(&f.vanilla(), &[a], &f.cache()).is_err());
    assert_eq!(sound(&old), b"A");
    let b = f.zip("B", false);
    let barrier = Arc::new(Barrier::new(3));
    let jobs: Vec<_> = (0..2)
        .map(|_| {
            let (pack, base, cache, barrier) = (b.clone(), f.vanilla(), f.cache(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                crate::packs::resource_root_leased(&base, &[pack], &cache).unwrap()
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = jobs.into_iter().map(|job| job.join().unwrap()).collect();
    assert_eq!(results[0].0, results[1].0);
    assert_ne!(results[0].0, old);
    assert_eq!(sound(&old), b"A");
    assert_eq!(sound(&results[0].0), b"B");
}

#[test]
fn invalid_proof_never_authorizes_rebuilding_the_same_generation() {
    let f = Fixture::new();
    let a = f.zip("A", true);
    let (old, _lease) = f.load(&a);
    fs::remove_file(marker_path(&old)).unwrap();
    fs::write(old.join("held-reader-sentinel"), b"leave alone").unwrap();
    let (new, _new_lease) = f.load(&a);
    assert_ne!(new, old);
    assert!(old.join("held-reader-sentinel").exists());
    assert_eq!(sound(&old), b"A");
    assert_eq!(f.load(&a).0, new);
}

#[test]
fn changed_materialized_base_dependencies_publish_a_new_generation() {
    let f = Fixture::new();
    let language = f.vanilla().join("assets/test/lang/en_us.json");
    fs::create_dir_all(language.parent().unwrap()).unwrap();
    fs::write(&language, br#"{"base":"A"}"#).unwrap();
    let pack = f.zip("pack", false);
    let (old, _old_lease) = f.load(&pack);
    fs::write(&language, br#"{"base":"changed B"}"#).unwrap();
    let (new, _new_lease) = f.load(&pack);
    assert_ne!(
        old, new,
        "the pack signature alone is not the generation key"
    );
    assert!(
        fs::read_to_string(old.join("assets/test/lang/en_us.json"))
            .unwrap()
            .contains("\"A\"")
    );
    assert!(
        fs::read_to_string(new.join("assets/test/lang/en_us.json"))
            .unwrap()
            .contains("changed B")
    );
}

#[cfg(unix)]
#[test]
fn a_linked_mod_generation_keeps_its_old_pack_base_through_collection() {
    let f = Fixture::new();
    let a = f.zip("A", true);
    let (old, lease) = f.load(&a);
    let mod_root = f.0.join("mod-content/00000000000000ab");
    fs::create_dir_all(&mod_root).unwrap();
    std::os::unix::fs::symlink(old.join("assets"), mod_root.join("assets")).unwrap();
    let mod_lease = RootLease::acquire_with_parent(&mod_root, lease).unwrap();
    let b = f.zip("B", false);
    let (new, _new_lease) = f.load(&b);
    fs::File::options()
        .write(true)
        .open(marker_path(&old))
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(2 * 86400))
        .unwrap();
    prune_generations(&f.cache(), Duration::from_secs(86400));
    assert_eq!(sound(&mod_root), b"A");
    assert_eq!(sound(&new), b"B");
    drop(mod_lease);
    prune_generations(&f.cache(), Duration::from_secs(86400));
    assert!(!old.exists() && !marker_path(&old).exists());
    assert!(new.exists());
}
