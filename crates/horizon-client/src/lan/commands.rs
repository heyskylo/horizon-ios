//! The command tree players receive (Commands.sendCommands and
//! ClientboundCommandsPacket): vanilla's tree from the data generator's
//! commands report, cut down to the nodes the player's permission level may
//! use and the commands of the server kind (dedicated servers have `op`,
//! `ban`, `stop` ...; an integrated server `publish`), numbered breadth first
//! as vanilla does, with the argument parsers' network properties and the
//! custom suggestion providers (`command_suggestions`). The report sorts each
//! node's children; the tree keeps vanilla's registration order
//! (`command_order`).
use super::{command_order::ORDER, command_suggestions::SUGGESTIONS};
use horizon_protocol::{
    buf::Writer,
    packets::play::{CommandNode, Commands},
    registry::Builtin,
};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::OnceLock,
};

/// Commands.CommandSelection: registered only on dedicated servers.
const DEDICATED_ONLY: &[&str] = &[
    "ban",
    "ban-ip",
    "banlist",
    "deop",
    "op",
    "pardon",
    "pardon-ip",
    "perf",
    "save-all",
    "save-off",
    "save-on",
    "setidletimeout",
    "stop",
    "transfer",
    "whitelist",
];
/// Registered only on integrated servers.
const INTEGRATED_ONLY: &[&str] = &["publish", "unpublish"];
/// Commands that check permissions only on dedicated servers
/// (`checkPermissions` in SeedCommand and VersionCommand).
const OPEN_WHEN_INTEGRATED: &[&str] = &["seed", "version"];
/// Game rules of experimental features (minecart improvements), which the
/// report lists but a vanilla-features world does not register.
const EXPERIMENTAL_RULES: &[&str] = &["max_minecart_speed", "minecraft:max_minecart_speed"];

pub struct CommandTree {
    root: Value,
}

/// A node of the filtered tree.
struct Node {
    name: String,
    kind: u8,
    executable: bool,
    restricted: bool,
    children: Vec<usize>,
    redirect: Option<Vec<String>>,
    parser: Option<(String, Value)>,
    suggestions: Option<&'static str>,
}

/// PermissionLevel names in the report's requirements.
fn required_level(node: &Value) -> u8 {
    match node["permissions"]["permission"]["level"].as_str() {
        Some("moderators") => 1,
        Some("gamemasters") => 2,
        Some("admins") => 3,
        Some("owners") => 4,
        _ => 0,
    }
}

impl CommandTree {
    /// commands.json next to registries.json (the data generator's reports).
    pub fn load(resources: &Path) -> Option<Self> {
        let dir = Builtin::find(resources)?.parent()?.to_path_buf();
        let text = std::fs::read_to_string(dir.join("commands.json")).ok()?;
        Some(Self {
            root: serde_json::from_str(&text).ok()?,
        })
    }

    /// Whether vanilla has a command of this name.
    pub fn has(&self, name: &str) -> bool {
        self.root["children"].get(name).is_some()
    }

    /// The ClientboundCommandsPacket for a player with `level` permissions.
    pub fn packet(&self, level: u8, dedicated: bool, builtin: &Builtin) -> Commands {
        self.packet_with(level, dedicated, builtin, &Value::Null)
    }

    /// [`CommandTree::packet`] with more commands after vanilla's: `extra`
    /// is a root in the report's format (server mods' commands).
    pub fn packet_with(
        &self,
        level: u8,
        dedicated: bool,
        builtin: &Builtin,
        extra: &Value,
    ) -> Commands {
        self.packet_with_features(level, dedicated, builtin, extra, &[])
    }

    pub fn packet_with_features(
        &self,
        level: u8,
        dedicated: bool,
        builtin: &Builtin,
        extra: &Value,
        features: &[String],
    ) -> Commands {
        self.packet_granting(level, dedicated, builtin, extra, features, &[])
    }

    /// [`CommandTree::packet_with_features`] with root commands the level
    /// would leave out sent anyway, each as for a player of the level it
    /// requires: a server mod granted them (`native_player_commands_sent_v2`).
    pub fn packet_granting(
        &self,
        level: u8,
        dedicated: bool,
        builtin: &Builtin,
        extra: &Value,
        features: &[String],
        granted: &[String],
    ) -> Commands {
        let minecart_improvements = features
            .iter()
            .any(|f| f == crate::gamerules::MINECART_IMPROVEMENTS);
        let mut nodes = vec![Node {
            name: String::new(),
            kind: 0,
            executable: false,
            restricted: false,
            children: Vec::new(),
            redirect: None,
            parser: None,
            suggestions: None,
        }];
        let suggestions: HashMap<&str, &'static str> = SUGGESTIONS.iter().copied().collect();
        let mut paths: HashMap<Vec<String>, usize> = HashMap::new();
        paths.insert(Vec::new(), 0);
        let horizon = if crate::command::vanilla::game_dispatcher()
            .find("pregen")
            .is_some()
        {
            crate::pregen::command_tree()
        } else {
            Value::Null
        };
        // A server mod's root of vanilla's name replaces vanilla's (`replace_vanilla`).
        let mut vanilla = self.root.clone();
        if let (Some(children), Some(mods)) = (
            vanilla.get_mut("children").and_then(Value::as_object_mut),
            extra.get("children").and_then(Value::as_object),
        ) {
            children.retain(|name, _| !mods.contains_key(name));
        }
        for root in [&vanilla, extra, &horizon] {
            fill(
                root,
                0,
                &mut Vec::new(),
                level,
                granted,
                dedicated,
                minecart_improvements,
                &suggestions,
                &mut nodes,
                &mut paths,
            );
        }
        // enumerateNodes: breadth first from the root, each node's children
        // (in registration order) then its redirect target.
        let mut ids = vec![None; nodes.len()];
        let mut order = Vec::new();
        let mut queue = VecDeque::from([0usize]);
        while let Some(n) = queue.pop_front() {
            if ids[n].is_some() {
                continue;
            }
            ids[n] = Some(order.len() as i32);
            order.push(n);
            queue.extend(nodes[n].children.iter().copied());
            if let Some(target) = nodes[n].redirect.as_ref().and_then(|p| paths.get(p)) {
                queue.push_back(*target);
            }
        }
        let argument_types = builtin.get("minecraft:command_argument_type");
        let out = order
            .iter()
            .map(|&n| {
                let node = &nodes[n];
                CommandNode {
                    kind: node.kind,
                    executable: node.executable,
                    restricted: node.restricted,
                    children: node.children.iter().filter_map(|c| ids[*c]).collect(),
                    redirect: node
                        .redirect
                        .as_ref()
                        .and_then(|p| paths.get(p))
                        .and_then(|t| ids[*t]),
                    name: node.name.clone(),
                    parser: node.parser.as_ref().map(|(parser, props)| {
                        let id = argument_types.and_then(|t| t.id(parser)).unwrap_or(0);
                        (id, properties(parser, props))
                    }),
                    suggestions: node.suggestions.map(str::to_owned),
                }
            })
            .collect();
        Commands {
            nodes: out,
            root: 0,
        }
    }
}

/// Commands.fillUsableCommands over the report: the children the source may use.
#[allow(clippy::too_many_arguments)]
fn fill(
    source: &Value,
    target: usize,
    path: &mut Vec<String>,
    level: u8,
    granted: &[String],
    dedicated: bool,
    minecart_improvements: bool,
    suggestions: &HashMap<&str, &'static str>,
    nodes: &mut Vec<Node>,
    paths: &mut HashMap<Vec<String>, usize>,
) {
    let Some(children) = source["children"].as_object() else {
        return;
    };
    let mut names: Vec<&String> = children.keys().collect();
    names.sort();
    // Vanilla's registration order; names it doesn't list (the game rules,
    // registered in an identity-hashed order) keep the report's.
    static ORDERS: OnceLock<HashMap<&str, Vec<&str>>> = OnceLock::new();
    let orders = ORDERS.get_or_init(|| {
        ORDER
            .iter()
            .map(|(path, names)| (*path, names.split(' ').collect()))
            .collect()
    });
    if let Some(order) = orders.get(path.join(" ").as_str()) {
        names.sort_by_key(|n| order.iter().position(|o| o == n).unwrap_or(order.len()));
    }
    for name in names {
        let child = &children[name];
        let required =
            if path.is_empty() && !dedicated && OPEN_WHEN_INTEGRATED.contains(&name.as_str()) {
                0
            } else {
                required_level(child)
            };
        // A granted root, and what it leads to, as for its own level.
        let level = if path.is_empty() && granted.contains(name) {
            level.max(required)
        } else {
            level
        };
        if required > level
            || path.len() == 1
                && path[0] == "gamerule"
                && !minecart_improvements
                && EXPERIMENTAL_RULES.contains(&name.as_str())
        {
            continue;
        }
        if path.is_empty()
            && (dedicated && INTEGRATED_ONLY.contains(&name.as_str())
                || !dedicated && DEDICATED_ONLY.contains(&name.as_str()))
        {
            continue;
        }
        path.push(name.clone());
        let argument = child["type"] == "argument";
        let index = nodes.len();
        nodes.push(Node {
            name: name.clone(),
            kind: if argument { 2 } else { 1 },
            executable: child["executable"].as_bool().unwrap_or(false),
            restricted: required > 0,
            children: Vec::new(),
            redirect: redirect(child),
            parser: argument.then(|| {
                (
                    child["parser"].as_str().unwrap_or_default().to_owned(),
                    child["properties"].clone(),
                )
            }),
            // A server mod's node with custom suggestions (the report has none)
            // asks the server.
            suggestions: if argument {
                suggestions
                    .get(path.join(" ").as_str())
                    .copied()
                    .or(child["suggestions"]
                        .is_string()
                        .then_some("minecraft:ask_server"))
            } else {
                None
            },
        });
        nodes[target].children.push(index);
        paths.insert(path.clone(), index);
        fill(
            child,
            index,
            path,
            level,
            granted,
            dedicated,
            minecart_improvements,
            suggestions,
            nodes,
            paths,
        );
        path.pop();
    }
}

/// The redirect target's path. The report leaves out redirects to the root
/// (`execute ... run`, `return run`): a node that neither runs nor leads
/// anywhere forks to the root.
fn redirect(node: &Value) -> Option<Vec<String>> {
    if let Some(path) = node["redirect"].as_array() {
        return Some(
            path.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect(),
        );
    }
    let dead_end = node["children"].as_object().is_none_or(|c| c.is_empty())
        && !node["executable"].as_bool().unwrap_or(false);
    dead_end.then(Vec::new)
}

/// ArgumentTypeInfo.serializeToNetwork for the report's properties.
fn properties(parser: &str, p: &Value) -> Vec<u8> {
    let mut w = Writer::new();
    let flags = |w: &mut Writer| {
        w.u8(u8::from(p.get("min").is_some()) | u8::from(p.get("max").is_some()) << 1);
    };
    match parser {
        "brigadier:integer" => {
            flags(&mut w);
            for key in ["min", "max"] {
                if let Some(v) = p[key].as_i64() {
                    w.i32(v as i32);
                }
            }
        }
        "brigadier:long" => {
            flags(&mut w);
            for key in ["min", "max"] {
                if let Some(v) = p[key].as_i64() {
                    w.i64(v);
                }
            }
        }
        "brigadier:float" => {
            flags(&mut w);
            for key in ["min", "max"] {
                if let Some(v) = p[key].as_f64() {
                    w.f32(v as f32);
                }
            }
        }
        "brigadier:double" => {
            flags(&mut w);
            for key in ["min", "max"] {
                if let Some(v) = p[key].as_f64() {
                    w.f64(v);
                }
            }
        }
        "brigadier:string" => w.varint(match p["type"].as_str() {
            Some("phrase") => 1,
            Some("greedy") => 2,
            _ => 0,
        }),
        "minecraft:entity" => {
            let single = u8::from(p["amount"] == "single");
            let players = u8::from(p["type"] == "players");
            w.u8(single | players << 1);
        }
        "minecraft:score_holder" => w.u8(u8::from(p["amount"] == "multiple")),
        "minecraft:time" => w.i32(p["min"].as_i64().unwrap_or(0) as i32),
        "minecraft:resource"
        | "minecraft:resource_key"
        | "minecraft:resource_or_tag"
        | "minecraft:resource_or_tag_key"
        | "minecraft:resource_selector" => w.string(p["registry"].as_str().unwrap_or_default()),
        _ => {}
    }
    w.data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pregen_completion_keeps_permissions_and_radius_bounds() {
        let tree = CommandTree {
            root: crate::pregen::command_tree(),
        };
        let builtin = Builtin::default();
        assert_eq!(tree.packet(0, false, &builtin).nodes.len(), 1);
        let packet = tree.packet(2, false, &builtin);
        let root = packet.nodes.iter().find(|n| n.name == "pregen").unwrap();
        assert!(root.restricted && !root.executable);
        let radius = packet.nodes.iter().find(|n| n.name == "radius").unwrap();
        assert!(radius.executable);
        assert_eq!(
            radius.parser.as_ref().unwrap().1,
            vec![3, 0, 0, 0, 0, 1, 201, 195, 128]
        );
        let shapes: Vec<_> = radius
            .children
            .iter()
            .map(|&i| packet.nodes[i as usize].name.as_str())
            .collect();
        assert_eq!(shapes, ["circle", "square"]);
        assert_eq!(
            packet
                .nodes
                .iter()
                .filter(|n| n.name == "center" && n.executable)
                .count(),
            2
        );
    }

    #[test]
    fn integer_bounds_and_entity_flags() {
        let p: Value = serde_json::from_str(r#"{"min": 1, "max": 1000}"#).unwrap();
        assert_eq!(
            properties("brigadier:integer", &p),
            vec![3, 0, 0, 0, 1, 0, 0, 3, 0xE8]
        );
        let p: Value = serde_json::from_str(r#"{"amount": "single", "type": "players"}"#).unwrap();
        assert_eq!(properties("minecraft:entity", &p), vec![3]);
        let p: Value = serde_json::from_str(r#"{"type": "greedy"}"#).unwrap();
        assert_eq!(properties("brigadier:string", &p), vec![2]);
    }

    /// A mod's root of a vanilla name (`NativeCommand::replace_vanilla`)
    /// takes vanilla's place in the merged tree: the client gets one `stop`,
    /// the mod's, and the vanilla roots the mods don't name stay.
    #[test]
    fn a_mods_root_replaces_vanillas_of_its_name() {
        let report: Value = serde_json::from_str(
            r#"{"type":"root","children":{
                "me":{"type":"literal","children":{"action":{"type":"argument","parser":"minecraft:message","executable":true}}},
                "stop":{"type":"literal","executable":true}}}"#,
        )
        .unwrap();
        let mods: Value = serde_json::from_str(
            r#"{"type":"root","children":{
                "stop":{"type":"literal","children":{"now":{"type":"literal","executable":true}}}}}"#,
        )
        .unwrap();
        let tree = CommandTree { root: report };
        let builtin = Builtin::default();
        let packet = tree.packet_with(4, true, &builtin, &mods);
        let stops: Vec<_> = packet.nodes.iter().filter(|n| n.name == "stop").collect();
        assert_eq!(stops.len(), 1, "one root of the name");
        assert!(
            !stops[0].executable,
            "the mod's root, not vanilla's executable one"
        );
        assert!(packet.nodes.iter().any(|n| n.name == "now"));
        assert!(
            packet.nodes.iter().any(|n| n.name == "me"),
            "other roots stay"
        );
        let alone = tree.packet_with(4, true, &builtin, &Value::Null);
        let stop = alone.nodes.iter().find(|n| n.name == "stop").unwrap();
        assert!(stop.executable, "without a mod's root vanilla's is sent");
    }

    #[test]
    fn levels_filter_and_redirects_resolve() {
        let report: Value = serde_json::from_str(
            r#"{"type":"root","children":{
                "say":{"type":"literal","children":{"message":{"type":"argument","parser":"minecraft:message","executable":true}},
                       "permissions":{"type":"minecraft:require","permission":{"type":"minecraft:command_level","level":"gamemasters"}}},
                "me":{"type":"literal","children":{"action":{"type":"argument","parser":"minecraft:message","executable":true}}},
                "stop":{"type":"literal","executable":true},
                "tell":{"type":"literal","redirect":["me"]}}}"#,
        )
        .unwrap();
        let tree = CommandTree { root: report };
        let builtin = Builtin::default();
        let guest = tree.packet(0, false, &builtin);
        let names: Vec<_> = guest.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["", "me", "tell", "action"]);
        // `tell` redirects to `me` (node 1).
        assert_eq!(guest.nodes[2].redirect, Some(1));
        let op = tree.packet(2, true, &builtin);
        let names: Vec<_> = op.nodes.iter().map(|n| n.name.as_str()).collect();
        // The root's children in vanilla's registration order.
        assert_eq!(
            names,
            ["", "me", "tell", "say", "stop", "action", "message"]
        );
        assert!(op.nodes[3].restricted);
    }

    #[test]
    fn a_granted_root_comes_with_its_subtree() {
        let report: Value = serde_json::from_str(
            r#"{"type":"root","children":{
                "say":{"type":"literal","children":{"message":{"type":"argument","parser":"minecraft:message","executable":true}},
                       "permissions":{"type":"minecraft:require","permission":{"type":"minecraft:command_level","level":"gamemasters"}}},
                "stop":{"type":"literal","executable":true,
                       "permissions":{"type":"minecraft:require","permission":{"type":"minecraft:command_level","level":"owners"}}},
                "me":{"type":"literal","children":{"action":{"type":"argument","parser":"minecraft:message","executable":true}}}}}"#,
        )
        .unwrap();
        let tree = CommandTree { root: report };
        let b = Builtin::default();
        let names = |p: &Commands| p.nodes.iter().map(|n| n.name.clone()).collect::<Vec<_>>();
        let plain = tree.packet_granting(0, true, &b, &Value::Null, &[], &[]);
        assert_eq!(names(&plain), ["", "me", "action"]);
        let granted = tree.packet_granting(0, true, &b, &Value::Null, &[], &["say".into()]);
        assert_eq!(names(&granted), ["", "me", "say", "action", "message"]);
        assert!(granted.nodes[2].restricted);
        // Nothing granted: the same tree as before.
        assert_eq!(
            names(&tree.packet_with_features(0, true, &b, &Value::Null, &[])),
            names(&plain)
        );
    }

    #[test]
    fn server_mods_commands_follow_vanillas() {
        let report: Value = serde_json::from_str(
            r#"{"type":"root","children":{"me":{"type":"literal","children":{"action":{"type":"argument","parser":"minecraft:message","executable":true}}}}}"#,
        )
        .unwrap();
        let mods: Value = serde_json::from_str(
            r#"{"type":"root","children":{"beacon":{"type":"literal","executable":true,"children":{
                "height":{"type":"argument","parser":"brigadier:integer","properties":{"min":1,"max":64},"executable":true},
                "who":{"type":"argument","parser":"brigadier:string","properties":{"type":"word"},"suggestions":"horizon:player_names","executable":true}}}}}"#,
        )
        .unwrap();
        let tree = CommandTree { root: report };
        let packet = tree.packet_with(0, true, &Builtin::default(), &mods);
        let names: Vec<_> = packet.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["", "me", "beacon", "action", "height", "who"]);
        // A mod's node with custom suggestions asks the server; vanilla's own keep theirs.
        assert_eq!(
            packet.nodes[5].suggestions.as_deref(),
            Some("minecraft:ask_server")
        );
        assert_eq!(packet.nodes[4].suggestions, None);
        assert_eq!(packet.nodes[3].suggestions, None);
        let beacon = &packet.nodes[2];
        assert!(beacon.executable && !beacon.restricted);
        assert_eq!(beacon.children, [4, 5]);
        let height = packet.nodes[4].parser.as_ref().unwrap();
        assert_eq!(height.1, vec![3, 0, 0, 0, 1, 0, 0, 0, 64]);
        // No mods: vanilla's tree as it is.
        assert_eq!(
            tree.packet_with(0, true, &Builtin::default(), &Value::Null)
                .nodes
                .len(),
            3
        );
    }
}
