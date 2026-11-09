//! Horizon's own entries in the multiplayer list (docs/layers.md: Horizon layer): the official
//! server, added once at the top, and the withdrawn partner server, taken back out once. Behind
//! `horizon-extras` (docs/forking.md), so a plain build adds no server and holds no Horizon
//! address. It reads and writes servers.dat only through `horizon_protocol::servers`, ready to
//! leave the engine crates for a Horizon crate.
use horizon_protocol::servers::{self, ServerEntry};
use std::path::Path;

/// At startup of a player's game: the official server into the list in `game_dir` and the
/// withdrawn partner server out of it, a failure only logged.
pub fn update(game_dir: &Path) {
    if let Err(e) = add_official_server(game_dir) {
        log::warn!("could not add the official server to servers.dat: {e}");
    }
    if let Err(e) = withdraw_partner_servers(game_dir) {
        log::warn!("could not take the withdrawn partner server out of servers.dat: {e}");
    }
}

/// Horizon's official survival server: a name and an address like any entry. The address is
/// `horizon_telemetry::server_status::OFFICIAL_ADDRESS`, the one place it is kept.
pub const OFFICIAL_SERVER: (&str, &str) = (
    "Horizon Official",
    horizon_telemetry::server_status::OFFICIAL_ADDRESS,
);
/// Beside servers.dat once the official server was offered, so a player who
/// deletes it never gets it back: the address offered, so a new one moves the
/// list once. The launcher writes the same file.
const OFFICIAL_MARKER: &str = "horizon_official_server.txt";

/// An address as the game connects to it: any case, 25565 when no port is given.
fn address_key(ip: &str) -> Option<(String, u16)> {
    servers::parse_address(ip).map(|(h, p)| (h.to_ascii_lowercase(), p))
}

/// Puts the official server at the top of the list in `game_dir`, once: not
/// again after the player deletes it, and not when the list has its address.
/// When it moved (the marker names another address), the entries for its
/// earlier addresses move to the new one, once (`move_official`).
pub fn add_official_server(game_dir: &Path) -> std::io::Result<()> {
    let (name, address) = OFFICIAL_SERVER;
    let marker = game_dir.join(OFFICIAL_MARKER);
    // A marker that is there but can't be read: offered already, at an address not known.
    let offered = match std::fs::read_to_string(&marker) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some(String::new()),
    };
    if offered
        .as_deref()
        .is_some_and(|a| address_key(a) == address_key(address))
    {
        return Ok(());
    }
    let file = game_dir.join("servers.dat");
    let _guard = servers::lock();
    let mut list = servers::load(&file);
    let previous = horizon_telemetry::server_status::OFFICIAL_PREVIOUS;
    if move_official(&mut list, address, previous) {
        servers::save(&file, &list)?;
    }
    if offered.is_none()
        && !list
            .iter()
            .any(|s| address_key(&s.ip) == address_key(address))
    {
        servers::prepend(&file, &ServerEntry::new(name, address))?;
    }
    std::fs::write(marker, format!("{address}\n"))
}

/// Afri-Network, which the game added once below the official server (#9299) until the
/// partnership ended: the name and address the game gave its entry.
const WITHDRAWN_PARTNER: (&str, &str) = ("Afri-Network (South Africa)", "afri-network.co.za");
/// Beside servers.dat: the partner addresses the game offered, one per line (#9299).
const PARTNER_MARKER: &str = "horizon_partner_servers.txt";

/// Takes the withdrawn partner server back out of the list in `game_dir`, once, where the game
/// added it: the marker says it was offered and the entry still has the name the game gave it
/// (an entry the player made or renamed keeps its place). Then the marker goes.
pub fn withdraw_partner_servers(game_dir: &Path) -> std::io::Result<()> {
    let marker = game_dir.join(PARTNER_MARKER);
    let offered = match std::fs::read_to_string(&marker) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let (name, address) = WITHDRAWN_PARTNER;
    if offered
        .lines()
        .any(|l| address_key(l) == address_key(address))
    {
        let _guard = servers::lock();
        servers::remove_where(&game_dir.join("servers.dat"), |n, ip| {
            n == name && address_key(ip) == address_key(address)
        })?;
    }
    std::fs::remove_file(marker)
}

/// Points the entries for an earlier official address at `address`, each
/// keeping the player's name and fields. One entry for the server at most: an
/// old one goes when the list already has `address`. Whether anything changed.
fn move_official(list: &mut Vec<ServerEntry>, address: &str, previous: &[&str]) -> bool {
    let new = address_key(address);
    let old = |ip: &str| {
        let ip = address_key(ip);
        ip.is_some() && ip != new && previous.iter().any(|p| address_key(p) == ip)
    };
    let mut present = list.iter().any(|s| address_key(&s.ip) == new);
    let mut changed = false;
    list.retain_mut(|s| {
        if !old(&s.ip) {
            return true;
        }
        changed = true;
        if present {
            return false;
        }
        present = true;
        s.ip = address.to_owned();
        true
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_official_server_is_added_once_at_the_top() {
        let dir = std::env::temp_dir().join(format!("horizon-mp-official-{}", std::process::id()));
        let (name, address) = OFFICIAL_SERVER;
        let file = dir.join("servers.dat");
        let ips = || -> Vec<String> { servers::load(&file).into_iter().map(|s| s.ip).collect() };
        let fresh = |list: &[ServerEntry]| {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            if !list.is_empty() {
                servers::save(&file, list).unwrap();
            }
        };
        // A fresh list: just the official server, named like it.
        fresh(&[]);
        add_official_server(&dir).unwrap();
        assert_eq!(servers::load(&file), [ServerEntry::new(name, address)]);
        // An existing list: on top, the player's servers kept in their order.
        let mine = [
            ServerEntry::new("Mine", "127.0.0.1:2"),
            ServerEntry::new("Other", "example.org"),
        ];
        fresh(&mine);
        add_official_server(&dir).unwrap();
        assert_eq!(ips(), [address, "127.0.0.1:2", "example.org"]);
        // Once only: a second start adds nothing.
        add_official_server(&dir).unwrap();
        assert_eq!(ips(), [address, "127.0.0.1:2", "example.org"]);
        // Already there under the player's own name and spelling: not duplicated.
        fresh(&[
            ServerEntry::new("SMP", "Play.PlayHorizon.dev:25565"),
            mine[0].clone(),
        ]);
        add_official_server(&dir).unwrap();
        assert_eq!(ips(), ["Play.PlayHorizon.dev:25565", "127.0.0.1:2"]);
        // Deleted by the player: never back.
        fresh(&mine);
        add_official_server(&dir).unwrap();
        servers::save(&file, &mine).unwrap();
        add_official_server(&dir).unwrap();
        assert_eq!(ips(), ["127.0.0.1:2", "example.org"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_withdrawn_partner_server_leaves_lists_the_game_put_it_in_once() {
        let dir = std::env::temp_dir().join(format!("horizon-mp-partner-{}", std::process::id()));
        let official = OFFICIAL_SERVER.1;
        let file = dir.join("servers.dat");
        let marker = dir.join(PARTNER_MARKER);
        let ips = || -> Vec<String> { servers::load(&file).into_iter().map(|s| s.ip).collect() };
        let fresh = |list: &[ServerEntry], offered: bool| {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            servers::save(&file, list).unwrap();
            if offered {
                std::fs::write(&marker, "afri-network.co.za\n").unwrap();
            }
        };
        let added = ServerEntry::new("Afri-Network (South Africa)", "afri-network.co.za");
        // The game added it: it goes, the rest stays in order, and only once.
        fresh(
            &[
                ServerEntry::new("Horizon Official", official),
                added.clone(),
                ServerEntry::new("Mine", "127.0.0.1:2"),
            ],
            true,
        );
        withdraw_partner_servers(&dir).unwrap();
        assert_eq!(ips(), [official, "127.0.0.1:2"]);
        assert!(!marker.exists());
        servers::save(&file, std::slice::from_ref(&added)).unwrap();
        withdraw_partner_servers(&dir).unwrap();
        assert_eq!(ips(), ["afri-network.co.za"]);
        // The player's own entry for that address (their name) stays.
        fresh(&[ServerEntry::new("ZA", "Afri-Network.co.za:25565")], true);
        withdraw_partner_servers(&dir).unwrap();
        assert_eq!(ips(), ["Afri-Network.co.za:25565"]);
        // Never offered (no marker): nothing changes.
        fresh(&[added], false);
        withdraw_partner_servers(&dir).unwrap();
        assert_eq!(ips(), ["afri-network.co.za"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_moved_official_server_moves_the_old_entry_once() {
        let dir = std::env::temp_dir().join(format!("horizon-mp-moved-{}", std::process::id()));
        let address = OFFICIAL_SERVER.1;
        let file = dir.join("servers.dat");
        let marker = dir.join(OFFICIAL_MARKER);
        let ips = || -> Vec<String> { servers::load(&file).into_iter().map(|s| s.ip).collect() };
        let offered_before = |list: &[ServerEntry]| {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            servers::save(&file, list).unwrap();
            std::fs::write(&marker, "horizon.bunchy7s.dev\n").unwrap();
        };
        // The old address under the player's own name and spelling: moved, name and place kept.
        let mut old = ServerEntry::new("Our SMP", "Horizon.Bunchy7s.dev:25565");
        old.icon = Some("AAAA".into());
        offered_before(&[ServerEntry::new("Mine", "127.0.0.1:2"), old]);
        add_official_server(&dir).unwrap();
        let list = servers::load(&file);
        assert_eq!(ips(), ["127.0.0.1:2", address]);
        assert_eq!(
            (list[1].name.as_str(), list[1].icon.as_deref()),
            ("Our SMP", Some("AAAA"))
        );
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            format!("{address}\n")
        );
        // Once: the old address added back afterwards is the player's.
        servers::prepend(&file, &ServerEntry::new("Old", "horizon.bunchy7s.dev")).unwrap();
        add_official_server(&dir).unwrap();
        assert_eq!(ips(), ["horizon.bunchy7s.dev", "127.0.0.1:2", address]);
        // The new address already listed: no second entry.
        offered_before(&[
            ServerEntry::new("Old", "horizon.bunchy7s.dev"),
            ServerEntry::new("New", address),
        ]);
        add_official_server(&dir).unwrap();
        assert_eq!(ips(), [address]);
        // Deleted by the player before the move: not offered again; another port is left alone.
        offered_before(&[ServerEntry::new("Test", "horizon.bunchy7s.dev:25570")]);
        add_official_server(&dir).unwrap();
        assert_eq!(ips(), ["horizon.bunchy7s.dev:25570"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
