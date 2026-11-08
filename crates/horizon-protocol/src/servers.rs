//! The multiplayer server list, stored like vanilla's ServerList in an
//! uncompressed `servers.dat`: `{servers: [{name, ip, icon?, acceptTextures?,
//! acceptedCodeOfConduct?, hidden}]}`. Hidden entries (servers joined by
//! Direct Connection or quick play) are not shown but keep their settings.
use horizon_world::nbt::{self, JavaString, NamedTag, Tag};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct ServerEntry {
    pub name: String,
    pub ip: String,
    /// Base64 PNG favicon from the last status ping.
    pub icon: Option<String>,
    /// ServerPackStatus: None = prompt, Some(true) enabled, Some(false) disabled.
    pub accept_textures: Option<bool>,
    /// `String.hashCode` of the Code of Conduct accepted for good (ServerData's
    /// `acceptedCodeOfConduct`); 0 for none.
    pub accepted_code_of_conduct: i32,
}
impl ServerEntry {
    pub fn new(name: &str, ip: &str) -> Self {
        Self {
            name: name.to_owned(),
            ip: ip.to_owned(),
            icon: None,
            accept_textures: None,
            accepted_code_of_conduct: 0,
        }
    }
    fn to_tag(&self) -> Tag {
        let mut entries = vec![
            ("name", Tag::String(JavaString::from(self.name.as_str()))),
            ("ip", Tag::String(JavaString::from(self.ip.as_str()))),
        ];
        if let Some(icon) = &self.icon {
            entries.push(("icon", Tag::String(JavaString::from(icon.as_str()))));
        }
        if let Some(accept) = self.accept_textures {
            entries.push(("acceptTextures", Tag::Byte(i8::from(accept))));
        }
        if self.accepted_code_of_conduct != 0 {
            entries.push((
                "acceptedCodeOfConduct",
                Tag::Int(self.accepted_code_of_conduct),
            ));
        }
        Tag::compound(entries)
    }
    fn from_tag(tag: &Tag) -> Option<Self> {
        let string = |k: &str| match tag.get(k) {
            Some(Tag::String(s)) => Some(s.to_string_lossy()),
            _ => None,
        };
        Some(Self {
            name: string("name").unwrap_or_default(),
            ip: string("ip")?,
            icon: string("icon"),
            accept_textures: match tag.get("acceptTextures") {
                Some(Tag::Byte(b)) => Some(*b != 0),
                _ => None,
            },
            accepted_code_of_conduct: match tag.get("acceptedCodeOfConduct") {
                Some(Tag::Int(hash)) => *hash,
                _ => 0,
            },
        })
    }
}

/// ServerList.load: the shown entries; missing or unreadable files give an empty list.
pub fn load(path: &Path) -> Vec<ServerEntry> {
    load_all(path).0
}
/// ServerList.load: the shown entries (`serverList`) and the hidden ones
/// (`hiddenServerList`, `hidden` set), each in the file's order.
pub fn load_all(path: &Path) -> (Vec<ServerEntry>, Vec<ServerEntry>) {
    let (mut shown, mut hidden) = (Vec::new(), Vec::new());
    let Ok(bytes) = std::fs::read(path) else {
        return (shown, hidden);
    };
    let Ok((root, _)) = nbt::read_named(&bytes, nbt::Limits::default()) else {
        return (shown, hidden);
    };
    if let Some(Tag::List(list)) = root.tag.get("servers") {
        for t in list {
            let Some(entry) = ServerEntry::from_tag(t) else {
                continue;
            };
            // CompoundTag.getBooleanOr("hidden", false): any number but zero.
            let is_hidden = match t.get("hidden") {
                Some(Tag::Byte(b)) => *b != 0,
                Some(Tag::Short(n)) => *n as i8 != 0,
                Some(Tag::Int(n)) => *n as i8 != 0,
                Some(Tag::Long(n)) => *n as i8 != 0,
                _ => false,
            };
            if is_hidden {
                hidden.push(entry);
            } else {
                shown.push(entry);
            }
        }
    }
    (shown, hidden)
}
/// ServerList.save for the shown entries, keeping the hidden ones the file has.
pub fn save(path: &Path, servers: &[ServerEntry]) -> std::io::Result<()> {
    let (_, hidden) = load_all(path);
    save_all(path, servers, &hidden)
}
/// ServerList.save: the shown entries with `hidden: false`, then the hidden
/// ones with `hidden: true`, written to a temporary file, then replaced.
pub fn save_all(
    path: &Path,
    servers: &[ServerEntry],
    hidden: &[ServerEntry],
) -> std::io::Result<()> {
    let flagged = |entry: &ServerEntry, flag: bool| {
        let mut tag = entry.to_tag();
        if let Tag::Compound(c) = &mut tag {
            c.insert(JavaString::from("hidden"), Tag::Byte(i8::from(flag)));
        }
        tag
    };
    let list = servers
        .iter()
        .map(|e| flagged(e, false))
        .chain(hidden.iter().map(|e| flagged(e, true)))
        .collect();
    let root = NamedTag {
        name: JavaString::from(""),
        tag: Tag::compound([("servers", Tag::List(list))]),
    };
    write(path, &root)
}
/// ServerList.add(server, true): a hidden entry goes first, and only the 16
/// newest hidden entries are kept (`MAX_HIDDEN_SERVERS`); then saved.
pub fn add_hidden(path: &Path, entry: ServerEntry) -> std::io::Result<()> {
    let (shown, mut hidden) = load_all(path);
    hidden.insert(0, entry);
    hidden.truncate(16);
    save_all(path, &shown, &hidden)
}
/// Puts `entry` first, keeping every other entry and field as the file has them (the hidden
/// ones too). A file that is there but can't be read is left as it is.
pub fn prepend(path: &Path, entry: &ServerEntry) -> std::io::Result<()> {
    insert(path, entry, |_| None)
}
/// Puts `entry` just after the first entry whose address `after` accepts, or last when none
/// does, keeping every other entry and field as the file has them (the hidden ones too).
pub fn insert_after(
    path: &Path,
    entry: &ServerEntry,
    after: impl Fn(&str) -> bool,
) -> std::io::Result<()> {
    insert(path, entry, |list| {
        let ip = |t: &Tag| match t.get("ip") {
            Some(Tag::String(s)) => s.to_string_lossy(),
            _ => String::new(),
        };
        Some(
            list.iter()
                .position(|t| after(&ip(t)))
                .map_or(list.len(), |i| i + 1),
        )
    })
}
/// Inserts `entry` at the index `at` picks in the file's list (`None`: first).
fn insert(
    path: &Path,
    entry: &ServerEntry,
    at: impl Fn(&[Tag]) -> Option<usize>,
) -> std::io::Result<()> {
    let mut root = match std::fs::read(path) {
        Ok(bytes) => {
            nbt::read_named(&bytes, nbt::Limits::default())
                .map_err(std::io::Error::other)?
                .0
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => NamedTag {
            name: JavaString::from(""),
            tag: Tag::Compound(Default::default()),
        },
        Err(e) => return Err(e),
    };
    let Tag::Compound(top) = &mut root.tag else {
        return Err(std::io::Error::other("servers.dat is not a compound"));
    };
    match top
        .entry(JavaString::from("servers"))
        .or_insert_with(|| Tag::List(Vec::new()))
    {
        Tag::List(list) => {
            let index = at(list).unwrap_or(0);
            list.insert(index, entry.to_tag());
        }
        _ => return Err(std::io::Error::other("servers.dat has no server list")),
    }
    write(path, &root)
}
fn write(path: &Path, root: &NamedTag) -> std::io::Result<()> {
    let bytes = nbt::write_named(root, nbt::Limits::default()).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("dat_tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

/// ServerAddress.parseString: `host`, `host:port` or `[v6]:port`; default 25565.
pub fn parse_address(text: &str) -> Option<(String, u16)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(rest) = text.strip_prefix('[') {
        let (host, tail) = rest.rsplit_once(']')?;
        // HostAndPort requires a colon inside a bracketed host; it does not
        // accept a bracketed DNS name as an IPv6-style address.
        if !host.contains(':') {
            return None;
        }
        let port = match tail.strip_prefix(':') {
            Some(p) if p.bytes().all(|byte| byte.is_ascii_digit()) => parse_port(p)?,
            None if tail.is_empty() => 25565,
            _ => return None,
        };
        return Some((host.to_owned(), port));
    }
    match text.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            if host.is_empty() {
                return None;
            }
            Some((host.to_owned(), parse_port(port)?))
        }
        Some(_) => Some((text.to_owned(), 25565)),
        None => Some((text.to_owned(), 25565)),
    }
}

/// HostAndPort uses the default for an empty port, refuses a leading plus,
/// and accepts signed zero on its non-bracketed path before checking range.
fn parse_port(text: &str) -> Option<u16> {
    if text.is_empty() {
        Some(25565)
    } else if text.starts_with('+') {
        None
    } else {
        u16::try_from(text.parse::<i32>().ok()?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_dat_round_trips() {
        let dir = std::env::temp_dir().join(format!("horizon-servers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("servers.dat");
        let list = vec![
            ServerEntry::new("Minecraft Server", "localhost:25565"),
            ServerEntry {
                accept_textures: Some(false),
                ..ServerEntry::new("Other", "example.org")
            },
        ];
        save(&path, &list).unwrap();
        assert_eq!(load(&path), list);
        // Vanilla layout: an unnamed root compound with a "servers" list.
        let (root, _) =
            nbt::read_named(&std::fs::read(&path).unwrap(), nbt::Limits::default()).unwrap();
        assert_eq!(root.name, JavaString::from(""));
        assert!(matches!(root.tag.get("servers"), Some(Tag::List(l)) if l.len() == 2));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prepend_keeps_hidden_entries_and_unknown_fields() {
        let dir = std::env::temp_dir().join(format!("horizon-prepend-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("servers.dat");
        prepend(&path, &ServerEntry::new("First", "a.example")).unwrap();
        assert_eq!(load(&path), [ServerEntry::new("First", "a.example")]);
        let hidden = Tag::compound([
            ("name", Tag::String(JavaString::from("Direct"))),
            ("ip", Tag::String(JavaString::from("b.example"))),
            ("hidden", Tag::Byte(1)),
            ("acceptedCodeOfConduct", Tag::String(JavaString::from("x"))),
        ]);
        let root = NamedTag {
            name: JavaString::from(""),
            tag: Tag::compound([("servers", Tag::List(vec![hidden.clone()]))]),
        };
        write(&path, &root).unwrap();
        prepend(&path, &ServerEntry::new("Top", "c.example")).unwrap();
        let (root, _) =
            nbt::read_named(&std::fs::read(&path).unwrap(), nbt::Limits::default()).unwrap();
        let Some(Tag::List(list)) = root.tag.get("servers") else {
            panic!("a server list");
        };
        assert_eq!(
            list,
            &[ServerEntry::new("Top", "c.example").to_tag(), hidden]
        );
        // A file that can't be read stays as it is.
        std::fs::write(&path, b"not nbt").unwrap();
        assert!(prepend(&path, &ServerEntry::new("Top", "c.example")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not nbt");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parses_addresses_like_server_address() {
        assert_eq!(
            parse_address("localhost"),
            Some(("localhost".into(), 25565))
        );
        assert_eq!(
            parse_address("127.0.0.1:25570"),
            Some(("127.0.0.1".into(), 25570))
        );
        assert_eq!(parse_address("[::1]:1234"), Some(("::1".into(), 1234)));
        assert_eq!(parse_address("host:notaport"), None);
        assert_eq!(parse_address(""), None);
    }

    #[test]
    fn empty_explicit_ports_use_the_vanilla_default() {
        for (input, host) in [
            ("example.com:", "example.com"),
            ("127.0.0.1:", "127.0.0.1"),
            ("[::1]:", "::1"),
            ("[2001:db8::1]:", "2001:db8::1"),
            (" \t[::1]:\r\n", "::1"),
        ] {
            assert_eq!(parse_address(input), Some((host.into(), 25565)), "{input}");
        }
    }

    #[test]
    fn ports_and_brackets_keep_vanilla_validation() {
        for input in [
            ":",
            ":25565",
            "[]",
            "[]:",
            "[example.com]",
            "[example.com]:",
            "[::1]junk",
            "example.com:+25565",
            "[::1]:+25565",
            "[::1]:-0",
            "example.com:-1",
            "example.com:65536",
            "[::1]:65536",
            "host:2147483648",
            "host:1_000",
            "host:\u{0661}\u{0662}",
        ] {
            assert_eq!(parse_address(input), None, "{input}");
        }
        assert_eq!(parse_address("host:-0"), Some(("host".into(), 0)));
        assert_eq!(parse_address("host:0"), Some(("host".into(), 0)));
        assert_eq!(parse_address("[::1]:65535"), Some(("::1".into(), 65535)));
    }
}
