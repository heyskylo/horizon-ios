//! Packets, plugin channels, resource packs and kicks for server mods (docs/native-mods.md,
//! "Server hooks for plugin ports: packets"): what plugin ports did with raw packets and
//! injections before. The packet hooks themselves run where the hub sends
//! (`horizon_client::lan::conn`), with no `Game`; the helpers here switch them on.
use super::{Game, lan::LanHost};
use horizon_client::{lan::data::component_nbt, text::Text};
use horizon_protocol::packets::{
    configuration::{ResourcePackPop, ResourcePackPush},
    play::Clientbound as C,
};
use std::sync::atomic::Ordering;

/// `ClientboundResourcePackPushPacket.MAX_HASH_LENGTH`.
const MAX_PACK_HASH: usize = 40;
/// The longest URL the push's `STRING_UTF8` takes, in characters.
const MAX_PACK_URL: usize = 32767;

impl Game<'_> {
    /// A `minecraft:register` (`register`) or `minecraft:unregister` message from the guest at
    /// `gi` in play: their channels change, and the mods hear the names that did, with the
    /// guest's state swapped in, as their other packets.
    pub(super) fn guest_channels(
        &mut self,
        host: &mut LanHost,
        gi: usize,
        data: &[u8],
        register: bool,
    ) {
        let g = &mut host.hub.guests[gi];
        let changed = horizon_client::lan::apply_channels(&mut g.channels, data, register);
        if changed.is_empty() {
            return;
        }
        let (uuid, name, conn) = (g.uuid, g.name.clone(), g.conn);
        self.as_guest(host, conn, |game| {
            game.native_player_register_channels(uuid, &name, &changed, register);
        });
    }

    /// What a guest who just logged in said during their configuration, for the mods: the
    /// channels they registered. The hub is in place.
    pub(super) fn configuration_heard(&mut self, uuid: u128, name: &str) {
        let Some(channels) = self.lan.as_ref().and_then(|host| {
            host.hub
                .guests
                .iter()
                .find(|g| g.uuid == uuid)
                .map(|g| g.channels.clone())
        }) else {
            return;
        };
        if !channels.is_empty() {
            self.native_player_register_channels(uuid, name, &channels, true);
        }
    }
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// From now on every play packet the server sends a player reaches
    /// `native_server_packet`, decoded, until the server stops; there is no
    /// switching it off, so one mod can't silence another's. Until a mod calls
    /// this, nothing is decoded or copied. Any thread, any time, a client
    /// included (where nothing is sent). @API:server_packet@.
    pub(super) fn native_watch_server_packets(&self) {
        horizon_client::lan::conn::WATCHED
            .fetch_or(horizon_client::lan::conn::WATCH_ALL, Ordering::Relaxed);
    }
    /// From now on the entity data the server sends each player
    /// (`ClientboundSetEntityDataPacket`) reaches `native_entity_data_to`,
    /// until the server stops; only that packet is decoded for it. As
    /// `native_watch_server_packets`, it cannot be switched off and works
    /// from any thread. @API:entity_data_to@.
    pub(super) fn native_watch_entity_data(&self) {
        horizon_client::lan::conn::WATCHED.fetch_or(
            horizon_client::lan::conn::WATCH_ENTITY_DATA,
            Ordering::Relaxed,
        );
    }
    /// The plugin channels `player`'s client registered (`minecraft:register`, NUL-separated
    /// names, in the configuration and in play, less those it unregistered), in the order it
    /// registered them: what Bukkit's `Player.getListeningPluginChannels` gives. Horizon keeps
    /// at most 128 names of at most 256 bytes, printable ASCII with a `:`. Empty for a player
    /// who isn't on, on a client, and inside a player's own hook, the hub being out then
    /// (`native_player_register_channels` names what changed). @API:player_channels@.
    pub(super) fn native_player_channels(&self, player: u128) -> Vec<String> {
        let Some(host) = &self.lan else {
            return Vec::new();
        };
        host.hub
            .guests
            .iter()
            .find(|g| g.uuid == player)
            .map(|g| g.channels.clone())
            .unwrap_or_default()
    }
    /// Offers `player`'s client the resource pack at `url` (Bukkit's
    /// `Player.addResourcePack`): `ClientboundResourcePackPushPacket` with the
    /// pack's own `id` (a UUID the mod picks, so it can pop that pack again and
    /// match the answers `native_resource_pack_status_v2` gets), `sha1` its
    /// lowercase hex hash (empty: the client doesn't check it), the prompt's
    /// extra line and whether the client calls it required. A pack with an id
    /// the client already has replaces it. Vanilla kicks a player who declines
    /// only when the server's own pack (`require-resource-pack`) is required,
    /// whichever pack they declined; anything else is the mod's to decide from
    /// the answer. False when the player isn't on, on a client, for an empty
    /// URL, one past 32767 characters or a hash past 40. Works from
    /// `native_server_tick` and the player's own hooks, as `native_send`.
    /// @API:resource_packs@.
    pub(super) fn native_send_resource_pack(
        &mut self,
        player: u128,
        id: u128,
        url: &str,
        sha1: &str,
        prompt: Option<&Text>,
        required: bool,
    ) -> bool {
        if url.is_empty() || url.chars().count() > MAX_PACK_URL || sha1.len() > MAX_PACK_HASH {
            return false;
        }
        let push = ResourcePackPush {
            id,
            url: url.to_owned(),
            hash: sha1.to_owned(),
            required,
            prompt: prompt.map(|text| component_nbt(&text.component())),
        };
        self.native_send(player, C::ResourcePackPush(push))
    }
    /// Takes the pack `id` away from `player`'s client, or every server pack it
    /// has for None (`ClientboundResourcePackPopPacket`, Bukkit's
    /// `Player.removeResourcePack` and `removeResourcePacks`); the client
    /// reloads without it. False when the player isn't on or on a client, as
    /// `native_send`. @API:resource_packs@.
    pub(super) fn native_pop_resource_pack(&mut self, player: u128, id: Option<u128>) -> bool {
        self.native_send(player, C::ResourcePackPop(ResourcePackPop { id }))
    }
}
