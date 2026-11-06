//! `events`: a jar's NeoForge listeners and capabilities, read from class
//! files, against the hook catalog's `neoforge` labels.
use horizon_port_pack::{
    classes,
    events::{self, Catalog, Status},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Write},
};

/// A NeoForge listener class: `onTick` takes `ServerTickEvent$Post` under
/// `@SubscribeEvent(priority = HIGH)`, `plain` takes `LevelEvent$Load` with
/// no annotation, a method type takes `ChunkEvent$Load` (a method
/// reference's), and a field reference reads `Capabilities$ItemHandler.BLOCK`.
fn neoforge_listener(name: &str) -> Vec<u8> {
    fn utf(pool: &mut Vec<Vec<u8>>, s: &str) -> u16 {
        let mut e = vec![1];
        e.extend((s.len() as u16).to_be_bytes());
        e.extend(s.as_bytes());
        pool.push(e);
        pool.len() as u16
    }
    fn entry(pool: &mut Vec<Vec<u8>>, tag: u8, refs: &[u16]) -> u16 {
        let mut e = vec![tag];
        for r in refs {
            e.extend(r.to_be_bytes());
        }
        pool.push(e);
        pool.len() as u16
    }
    let mut pool = Vec::new();
    let this = utf(&mut pool, name);
    let this = entry(&mut pool, 7, &[this]);
    let object = utf(&mut pool, "java/lang/Object");
    let object = entry(&mut pool, 7, &[object]);
    let subscribe = utf(&mut pool, "Lnet/neoforged/bus/api/SubscribeEvent;");
    let annotations = utf(&mut pool, "RuntimeVisibleAnnotations");
    let priority = utf(&mut pool, "priority");
    let priority_type = utf(&mut pool, "Lnet/neoforged/bus/api/EventPriority;");
    let high = utf(&mut pool, "HIGH");
    let on_tick = utf(&mut pool, "onTick");
    let event = "net/neoforged/neoforge/event";
    let tick = utf(
        &mut pool,
        &format!("(L{event}/tick/ServerTickEvent$Post;)V"),
    );
    let plain = utf(&mut pool, "plain");
    let level = utf(&mut pool, &format!("(L{event}/level/LevelEvent$Load;)V"));
    let chunk = utf(&mut pool, &format!("(L{event}/level/ChunkEvent$Load;)V"));
    entry(&mut pool, 16, &[chunk]);
    let caps = "net/neoforged/neoforge/capabilities";
    let item = utf(&mut pool, &format!("{caps}/Capabilities$ItemHandler"));
    let item = entry(&mut pool, 7, &[item]);
    let block = utf(&mut pool, "BLOCK");
    let block_type = utf(&mut pool, &format!("L{caps}/BlockCapability;"));
    let block = entry(&mut pool, 12, &[block, block_type]);
    entry(&mut pool, 9, &[item, block]);
    let mut out = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 65];
    out.extend(((pool.len() + 1) as u16).to_be_bytes());
    for e in &pool {
        out.extend(e);
    }
    out.extend([0, 0x21]);
    out.extend(this.to_be_bytes());
    out.extend(object.to_be_bytes());
    out.extend([0, 0, 0, 0]); // interfaces, fields
    out.extend([0, 2]);
    let mut body = vec![0, 1];
    body.extend(subscribe.to_be_bytes());
    body.extend([0, 1]);
    body.extend(priority.to_be_bytes());
    body.push(b'e');
    body.extend(priority_type.to_be_bytes());
    body.extend(high.to_be_bytes());
    out.extend([0, 1]);
    out.extend(on_tick.to_be_bytes());
    out.extend(tick.to_be_bytes());
    out.extend([0, 1]);
    out.extend(annotations.to_be_bytes());
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    out.extend([0, 1]);
    out.extend(plain.to_be_bytes());
    out.extend(level.to_be_bytes());
    out.extend([0, 0]); // no attributes
    out.extend([0, 0]); // class attributes
    out
}

const LABELS: &str = r#"{"hooks": [
    {"name": "server_tick", "neoforge": "ServerTickEvent.Pre / ServerTickEvent.Post"},
    {"name": "chunk_load", "neoforge": "ChunkEvent.Load / .Unload"},
    {"name": "arrow_nock", "neoforge": "net.neoforged.neoforge.event.entity.player.ArrowNockEvent (NeoForge.EVENT_BUS, ICancellableEvent)"},
    {"name": "level_tick_post", "neoforge": "LevelTickEvent.Post"},
    {"name": "config_loading", "neoforge": "ModConfigEvent.Loading"},
    {"name": "container_opened", "neoforge": "none (NeoForge's PlayerContainerEvent.Open fires after the menu is open)"},
    {"name": "open_menu", "neoforge": "IPlayerExtension.openMenu(MenuProvider)"},
    {"name": "block_entity_container", "neoforge": "Capabilities.Item.BLOCK"},
    {"name": "recipes_received", "neoforge": "RecipesReceivedEvent"},
    {"name": "standalone_models", "neoforge": "ModelEvent.RegisterStandalone"},
    {"name": "hud"}
]}"#;

fn hooks(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn listeners_are_subscribed_methods_and_method_references() {
    let class = classes::parse(&neoforge_listener("a/Listener")).unwrap();
    let listens: Vec<&str> = class.listens.iter().map(String::as_str).collect();
    assert_eq!(
        listens,
        [
            "net/neoforged/neoforge/event/level/ChunkEvent$Load",
            "net/neoforged/neoforge/event/tick/ServerTickEvent$Post",
        ]
    );
    let capabilities: Vec<&str> = class.capabilities.iter().map(String::as_str).collect();
    assert_eq!(capabilities, ["Capabilities.ItemHandler.BLOCK"]);
    assert_eq!(class.methods, ["onTick", "plain"]);
}

#[test]
fn labels_give_counterparts_phases_notes_and_new_names() {
    let c = Catalog::parse(LABELS).unwrap();
    let hooked = |h: &[&str]| (Status::Hooked(hooks(h)), None);
    assert_eq!(c.status("ServerTickEvent.Post"), hooked(&["server_tick"]));
    assert_eq!(c.status("ChunkEvent.Unload"), hooked(&["chunk_load"]));
    assert_eq!(c.status("ArrowNockEvent"), hooked(&["arrow_nock"]));
    let phase = |h: &[&str]| (Status::Phase(hooks(h)), None);
    assert_eq!(c.status("LevelTickEvent.Pre"), phase(&["level_tick_post"]));
    assert_eq!(c.status("ModConfigEvent"), phase(&["config_loading"]));
    assert_eq!(
        c.status("PlayerContainerEvent.Open"),
        (Status::Noted(hooks(&["container_opened"])), None)
    );
    assert_eq!(
        c.status("Capabilities.ItemHandler.BLOCK"),
        (
            Status::Hooked(hooks(&["block_entity_container"])),
            Some("Capabilities.Item.BLOCK".to_owned())
        )
    );
    assert_eq!(
        c.status("RecipesUpdatedEvent"),
        (
            Status::Hooked(hooks(&["recipes_received"])),
            Some("RecipesReceivedEvent".to_owned())
        )
    );
    assert_eq!(
        c.status("ModelEvent.RegisterAdditional"),
        (
            Status::Hooked(hooks(&["standalone_models"])),
            Some("ModelEvent.RegisterStandalone".to_owned())
        )
    );
    for missing in [
        "ModConfigEvent.Unloading",
        "RegisterPayloadHandlersEvent",
        "IPlayerExtension.openMenu",
    ] {
        assert_eq!(c.status(missing), (Status::Missing, None), "{missing}");
    }
}

#[test]
fn a_parent_is_hooked_when_each_of_its_events_is() {
    let all = r#"{"hooks": [
        {"name": "config_loading", "neoforge": "ModConfigEvent.Loading"},
        {"name": "config_reloading", "neoforge": "ModConfigEvent.Reloading"},
        {"name": "config_unloading", "neoforge": "ModConfigEvent.Unloading"}
    ]}"#;
    let c = Catalog::parse(all).unwrap();
    assert_eq!(
        c.status("ModConfigEvent"),
        (
            Status::Hooked(hooks(&[
                "config_loading",
                "config_reloading",
                "config_unloading"
            ])),
            None
        )
    );
}

#[test]
fn the_built_in_catalog_parses() {
    let c = Catalog::parse(events::CATALOG).unwrap();
    let (Status::Hooked(hooks), None) = c.status("LivingDeathEvent") else {
        panic!("LivingDeathEvent has no hook");
    };
    assert!(hooks.contains(&"living_death".to_owned()), "{hooks:?}");
}

#[test]
fn a_jar_and_a_pack_are_ranked_by_how_many_mods_use_each_name() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("a/Listener.class", options).unwrap();
    zip.write_all(&neoforge_listener("a/Listener")).unwrap();
    zip.start_file("a/notes.txt", options).unwrap();
    zip.write_all(b"not a class").unwrap();
    let jar = zip.finish().unwrap();
    let names = events::scan(Cursor::new(jar.into_inner())).unwrap();
    let listed: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(
        listed,
        [
            "Capabilities.ItemHandler.BLOCK",
            "ChunkEvent.Load",
            "ServerTickEvent.Post"
        ]
    );
    let mut mods = BTreeMap::new();
    mods.insert("alpha".to_owned(), names);
    let one: BTreeSet<String> = ["ServerTickEvent.Post".to_owned()].into();
    mods.insert("beta".to_owned(), one);
    mods.insert("gamma".to_owned(), BTreeSet::new());
    let report = events::table(&mods, &Catalog::parse(LABELS).unwrap());
    assert!(report.starts_with("3 mods; 2 listen"), "{report}");
    assert!(
        report.contains("3 names: 3 with a hook, 0 with another phase's"),
        "{report}"
    );
    let rows: Vec<&str> = report.lines().filter(|l| l.starts_with("| ")).collect();
    assert_eq!(
        rows[1..],
        [
            "| 2 | `ServerTickEvent.Post` | `server_tick` | alpha, beta |",
            "| 1 | `Capabilities.ItemHandler.BLOCK` | `block_entity_container` (as `Capabilities.Item.BLOCK`) | alpha |",
            "| 1 | `ChunkEvent.Load` | `chunk_load` | alpha |",
        ]
    );
}
