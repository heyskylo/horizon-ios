//! Native mods' profile helpers (docs/native-mods.md, "Server hooks for plugin ports: login and
//! profiles"): a player's profile properties changed while they are on, and the player shown again
//! with them (#10461).
use super::{Game, lan::LanHost};
use horizon_client::items::ItemStack;
use horizon_protocol::packets::login::Property;

/// What the profile helpers left for the hub: edits made where it was out of reach, and the players
/// to show again at the end of their level's pass.
#[derive(Default)]
pub(super) struct PendingProfiles {
    /// Player, property name, and the new value and signature (None removes the property).
    edits: Vec<(u128, String, Option<(String, Option<String>)>)>,
    refreshes: Vec<u128>,
}

/// `name` set to `value` (or removed) in a profile's properties.
fn edit(properties: &mut Vec<Property>, name: String, value: Option<(String, Option<String>)>) {
    let at = properties.iter().position(|p| p.name == name);
    match (value, at) {
        (Some((value, signature)), Some(i)) => {
            properties[i] = Property {
                name,
                value,
                signature,
            }
        }
        (Some((value, signature)), None) => properties.push(Property {
            name,
            value,
            signature,
        }),
        (None, Some(i)) => {
            properties.remove(i);
        }
        (None, None) => {}
    }
}

impl Game<'_> {
    /// Sets the property `name` of an online player's profile (`textures` for a skin) to `value`,
    /// its value and signature (None unsigned), or removes it with None: what every player list
    /// entry of theirs carries from now on, as Paper's `Player.setPlayerProfile`. Nothing is sent:
    /// `native_refresh_player` shows the change. Works from any server hook; where the hub is out of
    /// reach (inside a player's own hook) it applies at the tick's end. False on a client and for a
    /// name past 64 characters, a value past 32767 or a signature past 1024 (`GameProfile`'s
    /// limits); true does not mean the player was found. API @API:profile_refresh@.
    pub(super) fn native_set_profile_property(
        &mut self,
        player: u128,
        name: &str,
        value: Option<(&str, Option<&str>)>,
    ) -> bool {
        let long = |s: &str, max: usize| s.chars().count() > max;
        if self.remote.is_some()
            || name.is_empty()
            || long(name, 64)
            || value.is_some_and(|(v, sig)| long(v, 32767) || sig.is_some_and(|s| long(s, 1024)))
        {
            return false;
        }
        let value = value.map(|(v, sig)| (v.to_owned(), sig.map(str::to_owned)));
        match self
            .lan
            .as_mut()
            .map(|host| host.hub.guests.iter_mut().find(|g| g.uuid == player))
        {
            Some(Some(g)) => edit(&mut g.properties, name.to_owned(), value),
            Some(None) => {}
            None => self
                .native_net
                .profiles
                .edits
                .push((player, name.to_owned(), value)),
        }
        true
    }

    /// Shows an online player again with their current profile (Paper's `hidePlayer` then
    /// `showPlayer`, plus the player's own respawn): every player who sees them gets their list
    /// entry anew and their entity again, and the player is respawned in place into the level they
    /// are in (all their state kept), so their own client draws a new skin too. Queued for the end
    /// of their level's pass, so it works from any server hook; once a tick at most. A dead player
    /// is only shown again to the others (their respawn shows them the rest). False on a client;
    /// true does not mean the player was found. API @API:profile_refresh@.
    pub(super) fn native_refresh_player(&mut self, player: u128) -> bool {
        if self.remote.is_some() {
            return false;
        }
        let refreshes = &mut self.native_net.profiles.refreshes;
        if !refreshes.contains(&player) {
            refreshes.push(player);
        }
        true
    }

    /// At the end of a level's pass: the edits made where the hub was out of reach, then the
    /// refreshes of this level's players. The others wait for their level's pass; those who left
    /// are dropped.
    pub(super) fn flush_native_profiles(&mut self, host: &mut LanHost) {
        for (player, name, value) in std::mem::take(&mut self.native_net.profiles.edits) {
            if let Some(g) = host.hub.guests.iter_mut().find(|g| g.uuid == player) {
                edit(&mut g.properties, name, value);
            }
        }
        let refreshes = std::mem::take(&mut self.native_net.profiles.refreshes);
        for player in refreshes {
            let Some(i) = host
                .hub
                .guests
                .iter()
                .position(|g| g.placed && g.uuid == player)
            else {
                continue;
            };
            if host.hub.guests[i].dimension != host.hub.config.dimension {
                self.native_net.profiles.refreshes.push(player);
                continue;
            }
            host.hub.resend_profile(i);
            let g = &host.hub.guests[i];
            if g.dead {
                continue;
            }
            let (at, yaw, pitch, conn) = (g.position, g.yaw, g.pitch, g.conn);
            let cooldown = host.play.get(&conn).map_or(0, |p| p.portal.cooldown);
            let ctx = self.server_ctx();
            host.hub
                .change_level(i, at, yaw, Some(pitch), false, cooldown, &ctx);
        }
    }

    /// The name and profile properties of a player who is on, where the hub is in reach.
    fn online_profile(&self, uuid: u128) -> Option<(String, Vec<Property>)> {
        let host = self.lan.as_ref()?;
        let g = host.hub.guests.iter().find(|g| g.uuid == uuid)?;
        Some((g.name.clone(), g.properties.clone()))
    }

    /// The `textures` property of a player who is on (Paper's `Player.getPlayerProfile`): its
    /// value (base64 JSON naming the skin and cape) and signature (None unsigned), as their player
    /// list entry carries it, `native_set_profile_property`'s edits included. None for a player who
    /// isn't on or has none (offline mode without a skin mod), on a client, and where the hub is
    /// out of reach (inside a player's own hook). API @API:player_textures@.
    pub(super) fn native_player_textures(&self, uuid: u128) -> Option<(String, Option<String>)> {
        let (_, properties) = self.online_profile(uuid)?;
        properties
            .into_iter()
            .find(|p| p.name == "textures")
            .map(|p| (p.value, p.signature))
    }

    /// A player head of the player `uuid` (Bukkit's `SkullMeta.setOwningPlayer`): its
    /// `minecraft:profile` holds their id, and their name and properties (the skin) when they are
    /// on and the hub is in reach, so it shows their skin at once (else their name from the profile
    /// cache, if there); a profile without properties the client resolves itself. API @API:player_textures@.
    pub(super) fn native_player_head(&self, uuid: u128) -> ItemStack {
        let (name, properties) = self.online_profile(uuid).map_or(
            (self.native_profile_by_uuid(uuid), Vec::new()),
            |(name, p)| (Some(name), p),
        );
        let profile = horizon_protocol::item::Profile {
            name,
            id: Some(uuid),
            properties: properties
                .into_iter()
                .map(|p| (p.name, p.value, p.signature))
                .collect(),
            skin: Default::default(),
        };
        let mut head = ItemStack::new("minecraft:player_head", 1);
        head.data_mut().set_other(
            "minecraft:profile",
            Some(horizon_client::item_sync::profile_tag(&profile)),
        );
        head
    }

    /// A player the server knows by name, any case (Bukkit's `getOfflinePlayer` without its
    /// network lookup, vanilla's `GameProfileCache.get`): one who is on, else one in a dedicated
    /// server's profile cache (`usercache.json`: who joined in the last month, and names its
    /// commands looked up). Their id and their name as written. Asks no profile service and makes
    /// up no offline player; None when nobody is found. API @API:profile_cache@.
    pub(super) fn native_profile_by_name(&self, name: &str) -> Option<(u128, String)> {
        let online = self.lan.as_ref().and_then(|host| {
            host.hub
                .guests
                .iter()
                .find(|g| g.name.eq_ignore_ascii_case(name))
                .map(|g| (g.uuid, g.name.clone()))
        });
        online.or_else(|| {
            super::dedicated::cached_user_by_name(name).map(|user| (user.id, user.name))
        })
    }

    /// The name of the player `uuid`: one who is on, else one in a dedicated server's profile
    /// cache, as `native_profile_by_name`. API @API:profile_cache@.
    pub(super) fn native_profile_by_uuid(&self, uuid: u128) -> Option<String> {
        self.online_profile(uuid)
            .map(|(name, _)| name)
            .or_else(|| super::dedicated::cached_user_by_id(uuid).map(|user| user.name))
    }
}
