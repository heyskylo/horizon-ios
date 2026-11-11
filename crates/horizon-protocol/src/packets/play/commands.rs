//! The command tree (`ClientboundCommandsPacket`): its nodes, the 26.3 argument types
//! and their parser properties.
use crate::{
    Result,
    buf::{Codec, Reader, Writer},
};

/// One entry of ClientboundCommandsPacket: a literal or an argument node.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandNode {
    /// 0 root, 1 literal, 2 argument.
    pub kind: u8,
    pub executable: bool,
    pub restricted: bool,
    pub children: Vec<i32>,
    pub redirect: Option<i32>,
    pub name: String,
    /// Argument parser registry id and its raw property bytes.
    pub parser: Option<(i32, Vec<u8>)>,
    /// Custom suggestion provider (`minecraft:ask_server`).
    pub suggestions: Option<String>,
}
/// The `minecraft:command_argument_type` registry of 26.3 in id order
/// (ArgumentTypeInfos.bootstrap).
pub const ARGUMENT_TYPES: &[&str] = &[
    "brigadier:bool",
    "brigadier:float",
    "brigadier:double",
    "brigadier:integer",
    "brigadier:long",
    "brigadier:string",
    "minecraft:entity",
    "minecraft:game_profile",
    "minecraft:block_pos",
    "minecraft:column_pos",
    "minecraft:vec3",
    "minecraft:vec2",
    "minecraft:block_state",
    "minecraft:block_predicate",
    "minecraft:item_stack",
    "minecraft:item_predicate",
    "minecraft:team_color",
    "minecraft:hex_color",
    "minecraft:component",
    "minecraft:style",
    "minecraft:message",
    "minecraft:nbt_compound_tag",
    "minecraft:nbt_tag",
    "minecraft:nbt_path",
    "minecraft:objective",
    "minecraft:objective_criteria",
    "minecraft:operation",
    "minecraft:particle",
    "minecraft:angle",
    "minecraft:rotation",
    "minecraft:scoreboard_slot",
    "minecraft:score_holder",
    "minecraft:swizzle",
    "minecraft:team",
    "minecraft:item_slot",
    "minecraft:item_slots",
    "minecraft:resource_location",
    "minecraft:function",
    "minecraft:entity_anchor",
    "minecraft:int_range",
    "minecraft:float_range",
    "minecraft:dimension",
    "minecraft:gamemode",
    "minecraft:time",
    "minecraft:resource_or_tag",
    "minecraft:resource_or_tag_key",
    "minecraft:resource",
    "minecraft:resource_key",
    "minecraft:resource_selector",
    "minecraft:template_mirror",
    "minecraft:template_rotation",
    "minecraft:heightmap",
    "minecraft:loot_table",
    "minecraft:loot_predicate",
    "minecraft:loot_modifier",
    "minecraft:context_float_provider",
    "minecraft:context_int_provider",
    "minecraft:slot_source",
    "minecraft:dialog",
    "minecraft:feature",
    "minecraft:swing_animation",
    "minecraft:uuid",
];

/// The properties of an argument parser (`ArgumentTypeInfo.Template`) as
/// one of the 26.3 parsers decoded from the network.
#[derive(Clone, Debug, PartialEq)]
pub enum ArgumentProperties {
    None,
    Integer {
        min: Option<i32>,
        max: Option<i32>,
    },
    Long {
        min: Option<i64>,
        max: Option<i64>,
    },
    Float {
        min: Option<f32>,
        max: Option<f32>,
    },
    Double {
        min: Option<f64>,
        max: Option<f64>,
    },
    /// StringArgumentType.StringType: 0 word, 1 phrase, 2 greedy.
    String(i32),
    Entity {
        single: bool,
        players_only: bool,
    },
    ScoreHolder {
        multiple: bool,
    },
    Time {
        min: i32,
    },
    /// The resource arguments' registry.
    Registry(String),
}

impl ArgumentProperties {
    /// ArgumentTypeInfo.deserializeFromNetwork for parser `id`.
    pub fn read(r: &mut Reader, id: i32) -> Result<Self> {
        let name = usize::try_from(id)
            .ok()
            .and_then(|i| ARGUMENT_TYPES.get(i))
            .ok_or_else(|| crate::Error::new(format!("unknown argument type {id}")))?;
        let flagged = |r: &mut Reader| -> Result<(bool, bool)> {
            let f = r.u8()?;
            Ok((f & 1 != 0, f & 2 != 0))
        };
        Ok(match *name {
            "brigadier:integer" => {
                let (min, max) = flagged(r)?;
                let min = if min { Some(r.i32()?) } else { None };
                let max = if max { Some(r.i32()?) } else { None };
                Self::Integer { min, max }
            }
            "brigadier:long" => {
                let (min, max) = flagged(r)?;
                let min = if min { Some(r.i64()?) } else { None };
                let max = if max { Some(r.i64()?) } else { None };
                Self::Long { min, max }
            }
            "brigadier:float" => {
                let (min, max) = flagged(r)?;
                let min = if min { Some(r.f32()?) } else { None };
                let max = if max { Some(r.f32()?) } else { None };
                Self::Float { min, max }
            }
            "brigadier:double" => {
                let (min, max) = flagged(r)?;
                let min = if min { Some(r.f64()?) } else { None };
                let max = if max { Some(r.f64()?) } else { None };
                Self::Double { min, max }
            }
            "brigadier:string" => Self::String(r.varint()?),
            "minecraft:entity" => {
                let f = r.u8()?;
                Self::Entity {
                    single: f & 1 != 0,
                    players_only: f & 2 != 0,
                }
            }
            "minecraft:score_holder" => Self::ScoreHolder {
                multiple: r.u8()? & 1 != 0,
            },
            "minecraft:time" => Self::Time { min: r.i32()? },
            "minecraft:resource"
            | "minecraft:resource_key"
            | "minecraft:resource_or_tag"
            | "minecraft:resource_or_tag_key"
            | "minecraft:resource_selector" => Self::Registry(r.string(32767)?),
            _ => Self::None,
        })
    }
    /// Decodes a node's raw `parser` bytes.
    pub fn decode(id: i32, bytes: &[u8]) -> Result<Self> {
        Self::read(&mut Reader::new(bytes), id)
    }
}

/// Reads past an argument node's parser properties.
fn read_argument_properties(r: &mut Reader, id: i32) -> Result<()> {
    ArgumentProperties::read(r, id).map(|_| ())
}

/// Whether following `edges` from any node never comes back to it. An index
/// outside the nodes leads nowhere (the dispatcher drops it).
fn acyclic(nodes: &[CommandNode], edges: impl Fn(&CommandNode) -> &[i32]) -> bool {
    const NEW: u8 = 0;
    const ON_PATH: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![NEW; nodes.len()];
    // Depth first without recursion: a node and how many of its edges are followed.
    let mut path: Vec<(usize, usize)> = Vec::new();
    for start in 0..nodes.len() {
        if state[start] != NEW {
            continue;
        }
        state[start] = ON_PATH;
        path.push((start, 0));
        while let Some(&(node, followed)) = path.last() {
            let Some(&edge) = edges(&nodes[node]).get(followed) else {
                state[node] = DONE;
                path.pop();
                continue;
            };
            if let Some(top) = path.last_mut() {
                top.1 += 1;
            }
            let Some(next) = usize::try_from(edge).ok().filter(|&e| e < nodes.len()) else {
                continue;
            };
            match state[next] {
                ON_PATH => return false,
                NEW => {
                    state[next] = ON_PATH;
                    path.push((next, 0));
                }
                _ => {}
            }
        }
    }
    true
}

/// The command tree: nodes and the index of the root.
#[derive(Clone, Debug, PartialEq)]
pub struct Commands {
    pub nodes: Vec<CommandNode>,
    pub root: i32,
}
impl Codec<Commands> for Commands {
    fn read(r: &mut Reader) -> Result<Self> {
        let n = r.count(r.remaining())?;
        let mut nodes = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            let flags = r.u8()?;
            let kind = flags & 3;
            if kind > 2 {
                return Err(crate::Error::new("unknown command node kind"));
            }
            let count = r.count(r.remaining())?;
            let children = (0..count).map(|_| r.varint()).collect::<Result<_>>()?;
            let redirect = if flags & 8 != 0 {
                Some(r.varint()?)
            } else {
                None
            };
            let (mut name, mut parser, mut suggestions) = (String::new(), None, None);
            if kind >= 1 {
                name = r.string(32767)?;
            }
            if kind == 2 {
                let id = crate::adapter::ids::command_argument_type(r.varint()?);
                let start = r.position();
                read_argument_properties(r, id)?;
                parser = Some((id, r.since(start).to_vec()));
                if flags & 16 != 0 {
                    suggestions = Some(r.string(32767)?);
                }
            }
            nodes.push(CommandNode {
                kind,
                executable: flags & 4 != 0,
                restricted: flags & 32 != 0,
                children,
                redirect,
                name,
                parser,
                suggestions,
            });
        }
        let root = r.varint()?;
        let valid_index = |index: i32| usize::try_from(index).ok().is_some_and(|i| i < nodes.len());
        if !valid_index(root) || nodes[root as usize].kind != 0 {
            return Err(crate::Error::new("invalid command tree root"));
        }
        if nodes.iter().any(|node| {
            node.children.iter().any(|&child| !valid_index(child))
                || node.redirect.is_some_and(|redirect| !valid_index(redirect))
        }) {
            return Err(crate::Error::new("command tree references a missing node"));
        }
        // ClientboundCommandsPacket.validateEntries: every node's redirect builds
        // first (canBuild), then its children (canResolve). A cycle in either never
        // does, and walking it would recurse without end.
        if !acyclic(&nodes, |n| n.redirect.as_slice()) || !acyclic(&nodes, |n| &n.children) {
            return Err(crate::Error::new("Server sent an impossible command tree"));
        }
        Ok(Self { nodes, root })
    }
    fn write(v: &Self, w: &mut Writer) {
        w.varint(v.nodes.len() as i32);
        for n in &v.nodes {
            let mut flags = n.kind & 3;
            if n.executable {
                flags |= 4;
            }
            if n.redirect.is_some() {
                flags |= 8;
            }
            if n.suggestions.is_some() {
                flags |= 16;
            }
            if n.restricted {
                flags |= 32;
            }
            w.u8(flags);
            w.varint(n.children.len() as i32);
            n.children.iter().for_each(|c| w.varint(*c));
            if let Some(r) = n.redirect {
                w.varint(r);
            }
            if n.kind >= 1 {
                w.string(&n.name);
            }
            if n.kind == 2 {
                let (id, props) = n.parser.clone().unwrap_or((5, Vec::new()));
                w.varint(id);
                w.bytes(&props);
                if let Some(s) = &n.suggestions {
                    w.string(s);
                }
            }
        }
        w.varint(v.root);
    }
}
