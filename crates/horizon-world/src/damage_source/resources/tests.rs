use super::*;
use crate::DATA_VERSION;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "horizon-damage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let types: Vec<_> = ["fall", "stalagmite", "starve"].into_iter().map(|name| json!({
            "id": format!("minecraft:{name}"), "message_id": name,
            "exhaustion": "3dcccccd", "scaling": "never", "effects": "hurt", "death_message_type": "default"
        })).collect();
        fs::write(
            root.join("damage-catalog.json"),
            json!({"data_version": DATA_VERSION, "types": types}).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("damage-tags.json"),
            json!({"minecraft:bypasses_armor": "{\"values\":[\"minecraft:fall\"]}"}).to_string(),
        )
        .unwrap();
        Self(root)
    }
    fn write(&self, path: &str, value: Value) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn definition(message: &str) -> Value {
    json!({"message_id": message, "scaling": "never", "exhaustion": 0.125})
}
fn file(id: &str, value: Value) -> (String, Vec<Vec<u8>>) {
    (id.into(), vec![value.to_string().into_bytes()])
}

#[test]
fn root_namespaces_and_ordered_world_overrides_preserve_vanilla_indices() {
    let f = Fixture::new();
    let base = Catalog::load(&f.0, &PackOverlay::NONE).unwrap();
    let fall = base.id("minecraft:fall").unwrap();
    f.write("data/demo/damage_type/hot/air.json", definition("root"));
    let mut heat = file("damage_type/demo:hot/air", definition("low"));
    heat.1.push(definition("high").to_string().into_bytes());
    let files = vec![heat, file("damage_type/aaa:new", definition("new"))];
    let packed = base
        .with_packs(&f.0, &PackOverlay { files: &files })
        .unwrap();
    assert_eq!(packed.id("minecraft:fall"), Some(fall));
    assert_eq!(
        packed
            .definition(packed.id("demo:hot/air").unwrap())
            .unwrap()
            .message_id,
        "high"
    );
    assert_eq!(
        packed
            .definition(packed.id("aaa:new").unwrap())
            .unwrap()
            .effects,
        "hurt"
    );
    assert!(
        base.id("demo:hot/air").is_none(),
        "the immutable base is unchanged"
    );
}

#[test]
fn reload_rebinds_tags_but_ignores_changed_removed_and_added_definitions() {
    let f = Fixture::new();
    let files = vec![
        file("damage_type/demo:heat", definition("original")),
        file(
            "tags/minecraft:damage_type/is_fire",
            json!({"values":["demo:heat"]}),
        ),
    ];
    let open = Catalog::load(&f.0, &PackOverlay { files: &files }).unwrap();
    let heat = open.id("demo:heat").unwrap();
    assert!(open.in_tag(heat, "minecraft:is_fire"));
    let changed = vec![
        file("damage_type/demo:heat", json!({"invalid":true})),
        file("damage_type/demo:new", definition("added")),
        file(
            "tags/demo:damage_type/optional",
            json!({"values":[{"id":"demo:new","required":false}, "demo:heat"]}),
        ),
    ];
    let reloaded = open
        .reload_tags(&f.0, &PackOverlay { files: &changed })
        .unwrap();
    assert_eq!(reloaded.id("demo:heat"), Some(heat));
    assert_eq!(reloaded.definition(heat).unwrap().message_id, "original");
    assert!(reloaded.id("demo:new").is_none());
    assert!(!reloaded.in_tag(heat, "minecraft:is_fire"));
    assert_eq!(
        reloaded.tag_members("demo:optional").collect::<Vec<_>>(),
        ["demo:heat"]
    );
    let disabled = reloaded.reload_tags(&f.0, &PackOverlay::NONE).unwrap();
    assert_eq!(disabled.id("demo:heat"), Some(heat));
    assert_eq!(disabled.tag_members("demo:optional").count(), 0);
}

#[test]
fn replace_nested_optional_and_missing_required_tags_share_one_resolution() {
    let f = Fixture::new();
    f.write("data/demo/damage_type/heat.json", definition("heat"));
    f.write(
        "data/demo/tags/damage_type/nested.json",
        json!({"values":["demo:heat"]}),
    );
    let files = vec![
        file(
            "tags/minecraft:damage_type/bypasses_armor",
            json!({"replace":true,"values":["#demo:nested", {"id":"demo:absent","required":false}]}),
        ),
        file(
            "tags/demo:damage_type/broken",
            json!({"values":["demo:absent"]}),
        ),
        file(
            "tags/demo:damage_type/broken_child",
            json!({"values":["#demo:broken"]}),
        ),
    ];
    let catalog = Catalog::load(&f.0, &PackOverlay { files: &files }).unwrap();
    assert_eq!(
        catalog
            .tag_members("minecraft:bypasses_armor")
            .collect::<Vec<_>>(),
        ["demo:heat"]
    );
    assert!(!catalog.in_tag(
        catalog.id("minecraft:fall").unwrap(),
        "minecraft:bypasses_armor"
    ));
    assert!(
        !catalog
            .tags()
            .any(|id| id == "demo:broken" || id == "demo:broken_child")
    );
}

#[test]
fn independent_worlds_never_share_reload_state() {
    let f = Fixture::new();
    let a = vec![
        file("damage_type/demo:heat", definition("a")),
        file(
            "tags/minecraft:damage_type/is_fire",
            json!({"values":["demo:heat"]}),
        ),
    ];
    let b = vec![file("damage_type/demo:heat", definition("b"))];
    let a = Catalog::load(&f.0, &PackOverlay { files: &a }).unwrap();
    let b = Catalog::load(&f.0, &PackOverlay { files: &b }).unwrap();
    assert!(a.in_tag(a.id("demo:heat").unwrap(), "minecraft:is_fire"));
    assert!(!b.in_tag(b.id("demo:heat").unwrap(), "minecraft:is_fire"));
    let _ = a.reload_tags(&f.0, &PackOverlay::NONE).unwrap();
    assert_eq!(
        b.definition(b.id("demo:heat").unwrap()).unwrap().message_id,
        "b"
    );
}

#[test]
fn invalid_top_definition_refuses_the_snapshot_without_mutating_base() {
    let f = Fixture::new();
    let base = Catalog::load(&f.0, &PackOverlay::NONE).unwrap();
    let mut file = file("damage_type/demo:heat", definition("valid-low-priority"));
    file.1.push(
        json!({"message_id":"invalid", "scaling":"invalid", "exhaustion":0})
            .to_string()
            .into_bytes(),
    );
    assert!(
        base.with_packs(&f.0, &PackOverlay { files: &[file] })
            .is_err()
    );
    assert!(base.id("demo:heat").is_none());
}
