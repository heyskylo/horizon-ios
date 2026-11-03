//! Client item discovery uses the complete resource path below `items/`, including subfolders.
use horizon_assets::item_model::ClientItem;
use std::{collections::BTreeMap, fs, path::Path};

pub(super) fn load(root: &Path) -> BTreeMap<String, ClientItem> {
    let mut out = BTreeMap::new();
    let Ok(namespaces) = fs::read_dir(root.join("assets")) else {
        return out;
    };
    for ns in namespaces.flatten() {
        let namespace = ns.file_name().to_string_lossy().into_owned();
        let Ok(folder) = horizon_world::json_folder::read(&ns.path().join("items")) else {
            continue;
        };
        for file in &folder.files {
            let name = &file.name;
            match file.bytes() {
                Ok(bytes) => {
                    let item = std::str::from_utf8(bytes)
                        .map_err(|e| e.to_string())
                        .and_then(|text| ClientItem::from_json(text).map_err(|e| e.to_string()));
                    match item {
                        Ok(item) => {
                            out.insert(format!("{namespace}:{name}"), item);
                        }
                        Err(error) => log::warn!(
                            "Horizon: invalid item definition {namespace}:{name}: {error}"
                        ),
                    }
                }
                Err(error) => log::warn!(
                    "Horizon: could not read item definition {namespace}:{name}: {error}"
                ),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_assets::item_model::{
        Context, DisplayContext, ItemModels, LayerContent, NoEnv, StackView,
    };

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(std::env::temp_dir().join(format!("horizon-item-discovery-{unique}")))
        }
        fn write(&self, file: &str, text: &str) {
            let path = self.0.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn prepared_item_definitions_keep_nested_ids_and_use_packed_bytes() {
        let fixture = Fixture::new();
        let packed = r#"{"model":{"type":"minecraft:model","model":"server:item/packed"}}"#;
        fixture.write("horizon-resources.json", "{}");
        fixture.write("assets/server/items/weapons/orb.json", packed);
        horizon_core::prepared_files::write(&fixture.0).unwrap();
        // A packed prepared root must read the prepared bytes, like the other
        // tables, rather than silently reverting to the loose-file loader.
        fixture.write("assets/server/items/weapons/orb.json", "{");
        let definitions = load(&fixture.0);
        let horizon_assets::item_model::ItemModelNode::Model(model) =
            &definitions["server:weapons/orb"].model
        else {
            panic!("expected packed model");
        };
        assert_eq!(model.model, "server:item/packed");
        assert!(!definitions.contains_key("server:orb"));
    }

    #[test]
    fn nested_server_definitions_resolve_by_full_path_and_reload() {
        let fixture = Fixture::new();
        let first = r#"{"model":{"type":"minecraft:model","model":"server:item/stones/orb"}}"#;
        let second = r#"{"model":{"type":"minecraft:model","model":"server:item/weapons/orb"}}"#;
        fixture.write("assets/server/items/item/stones/orb.json", first);
        fixture.write("assets/server/items/item/weapons/orb.json", second);
        fixture.write("assets/minecraft/items/paper.json", first);
        fixture.write("assets/other/items/item/stones/orb.json", second);
        fixture.write("assets/server/items/item/invalid.json", "{");
        fixture.write("assets/server/items/item/ignored.txt", first);
        let definitions = load(&fixture.0);
        assert_eq!(definitions.len(), 4);
        assert!(definitions.contains_key("minecraft:paper"));
        assert!(definitions.contains_key("other:item/stones/orb"));
        assert!(!definitions.contains_key("server:orb"));
        let models = ItemModels {
            definitions,
            ..Default::default()
        };
        let context = Context {
            display: DisplayContext::Gui,
            owner: None,
            level: None,
            seed: 0,
        };
        for (id, model) in [
            ("item/stones/orb", "server:item/stones/orb"),
            ("item/weapons/orb", "server:item/weapons/orb"),
        ] {
            let mut stack = StackView::new("minecraft:paper", 1);
            stack.patch.insert(
                "minecraft:item_model".into(),
                Some(serde_json::json!(format!("server:{id}"))),
            );
            let resolved = models.resolve(&stack, &context, &NoEnv);
            assert_eq!(resolved.layers.len(), 1);
            assert_eq!(
                resolved.layers[0].content,
                LayerContent::Model {
                    model: model.into()
                }
            );
        }
        fixture.write("assets/server/items/item/stones/orb.json", second);
        let reloaded = load(&fixture.0);
        let horizon_assets::item_model::ItemModelNode::Model(leaf) =
            &reloaded["server:item/stones/orb"].model
        else {
            panic!("expected model leaf")
        };
        assert_eq!(leaf.model, "server:item/weapons/orb");
    }
}
