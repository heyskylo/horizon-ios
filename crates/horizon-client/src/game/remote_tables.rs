//! The game's tables with what a server's synced data puts over them: its
//! data packs' enchantments, and its block and item tags.
use super::super::Game;
use horizon_client::{enchant::Enchantments, items::Items};
use horizon_protocol::{packets::configuration::UpdateTags, registry::Dynamic};
use std::{collections::HashMap, sync::Arc};

/// `None` where the server sent nothing over the game's own table.
#[derive(Default)]
pub(crate) struct ServerTables {
    /// The game's enchantments and the server's that it lacks (a data
    /// pack's), from their synced data.
    pub(crate) enchantments: Option<Arc<Enchantments>>,
    /// The game's items with the block and item tags the server sent
    /// (`ClientboundUpdateTagsPacket`) in place of its own.
    pub(crate) items: Option<Arc<Items>>,
}

impl Game<'_> {
    /// The tables `registries` puts over the game's.
    pub(super) fn server_tables(&self, registries: &Dynamic) -> ServerTables {
        let entries: Vec<(&str, serde_json::Value)> = registries
            .registries
            .get("minecraft:enchantment")
            .into_iter()
            .flatten()
            .filter_map(|(name, data)| {
                Some((
                    name.as_str(),
                    horizon_client::scoreboard::tag_to_json(data.as_ref()?),
                ))
            })
            .collect();
        let enchantments = self
            .stations()
            .enchantments
            .with_entries(entries.iter().map(|(n, v)| (*n, v)))
            .map(Arc::new);
        ServerTables {
            enchantments,
            items: self.server_items(registries),
        }
    }
    /// The game's items with the server's block and item tags, by name. The
    /// window's own server thread plays with the world's tables instead.
    fn server_items(&self, registries: &Dynamic) -> Option<Arc<Items>> {
        let r = self.remote.as_ref().filter(|_| self.integrated.is_none())?;
        let names = |registry: &str| -> Option<HashMap<String, Vec<String>>> {
            let ids = r.builtin.get(registry)?;
            let tags = registries
                .tags
                .get(registry)?
                .iter()
                .map(|(name, members)| {
                    let members = members.iter().filter_map(|&id| ids.name(id));
                    (name.clone(), members.map(str::to_owned).collect())
                });
            Some(tags.collect())
        };
        let (blocks, items) = (names("minecraft:block"), names("minecraft:item"));
        if blocks.is_none() && items.is_none() {
            return None;
        }
        Some(Arc::new(self.res.items().with_tags(blocks, items)))
    }
    /// `ClientPacketListener.handleUpdateTags` (after a `/reload`): the tags
    /// it names replace the ones the configuration sent.
    pub(super) fn remote_update_tags(&mut self, packet: &UpdateTags) {
        let Some(r) = self.remote.as_ref() else {
            return;
        };
        let mut registries = Dynamic::clone(&r.registries);
        registries.add_tags(packet);
        let tables = self.server_tables(&registries);
        if let Some(r) = self.remote.as_mut() {
            r.registries = Arc::new(registries);
            r.tables = tables;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        Audio, Game, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
    };
    use horizon_protocol::{
        connection::Pipe,
        packets::configuration::{RegistryTags, TagEntry, UpdateTags},
    };
    use std::collections::HashSet;

    fn client() -> Game<'static> {
        let root = std::path::PathBuf::from(
            std::env::var_os("HORIZON_TEST_RESOURCES")
                .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
        );
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!("no menu handoff"),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = ExecutionRole::Client;
        let res: &'static _ = Box::leak(Box::new(loaded.resources));
        let mut game = Game::new(
            res,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let (client, _server) = Pipe::pair();
        game.connect_local(client, "TagTest").unwrap();
        game
    }

    /// A server whose data pack makes stone climbable and sticks planks: the
    /// client reads its tags, not the game's own (#7843).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_servers_block_and_item_tags_replace_the_games() {
        let mut game = client();
        assert!(!game.items().block_tags["minecraft:climbable"].contains("minecraft:stone"));
        let builtin = game.remote.as_ref().unwrap().builtin.clone();
        let id = |registry: &str, name: &str| builtin.get(registry).unwrap().id(name).unwrap();
        let tag = |registry: &str, name: &str, members: &[&str]| RegistryTags {
            registry: registry.into(),
            tags: vec![TagEntry {
                name: name.into(),
                ids: members.iter().map(|m| id(registry, m)).collect(),
            }],
        };
        game.remote_update_tags(&UpdateTags {
            registries: vec![
                tag(
                    "minecraft:block",
                    "minecraft:climbable",
                    &["minecraft:ladder", "minecraft:stone"],
                ),
                tag(
                    "minecraft:item",
                    "minecraft:planks",
                    &["minecraft:stick", "minecraft:oak_planks"],
                ),
            ],
        });

        let items = game.items();
        let set = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<HashSet<_>>();
        assert_eq!(
            items.block_tags["minecraft:climbable"],
            set(&["minecraft:ladder", "minecraft:stone"])
        );
        // The server's tags replace the game's: a tag it didn't send is gone.
        assert!(!items.block_tags.contains_key("minecraft:fences"));
        assert_eq!(
            items.item_tag_order["minecraft:planks"],
            ["minecraft:stick", "minecraft:oak_planks"]
        );
        assert!(items.item_tags["minecraft:planks"].contains("minecraft:stick"));
    }
}
