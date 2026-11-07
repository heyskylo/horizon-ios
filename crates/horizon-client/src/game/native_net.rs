//! Mod messages for native mods (docs/native-mods.md, "Mod messages", API
//! 23): a mod's server half and its client half send each other bytes
//! (`horizon_protocol::mod_net`). The hub is out of the game while a guest's
//! packet or turn runs, so both directions wait here: what a guest sent
//! reaches `native_net_from_client` once the hub is back, and what a mod sends
//! a player leaves at the tick's end, from whichever hook sent it.
use super::{Game, lan::LanHost};
use horizon_client::mod_api::ServerboundPlayPacket;
use horizon_protocol::{
    mod_net::{self, Message},
    packets::{
        configuration::CustomPayload,
        play::{Clientbound as C, Serverbound as S},
    },
    quiet,
};
use std::collections::HashMap;

/// Messages a tick at most: from one player, to one player, and from the
/// window to its server.
pub(super) const PER_TICK: u32 = 32;

/// The messages waiting, and what each player used of this tick's allowance.
#[derive(Default)]
pub(super) struct NativeNet {
    /// What the profile helpers left for the hub (`native_profiles`).
    pub(super) profiles: super::native_profiles::PendingProfiles,
    from_players: Vec<(u128, Message)>,
    from_counts: HashMap<u128, u32>,
    to_players: Vec<(u128, Vec<u8>)>,
    to_counts: HashMap<u128, u32>,
    pub(super) to_server: u32,
    /// Plugin channel messages from players on channels that aren't Horizon's own, for
    /// `native_custom_payload_from_client`: player, channel, data.
    payloads_from: Vec<(u128, String, Vec<u8>)>,
    /// What `native_send_custom_payload_to_player` sent this tick: player, channel, data.
    payloads_to: Vec<(u128, String, Vec<u8>)>,
}

/// `ClientboundCustomPayloadPacket.MAX_PAYLOAD_SIZE`.
const MAX_CLIENTBOUND_PAYLOAD: usize = 1_048_576;

/// A plugin channel `namespace:path` as `Identifier` takes it, not in Horizon's own namespace.
fn plugin_channel_ok(channel: &str) -> bool {
    let Some((namespace, path)) = channel.split_once(':') else {
        return false;
    };
    let allowed = |b: u8, slash: bool| {
        b.is_ascii_lowercase()
            || b.is_ascii_digit()
            || matches!(b, b'_' | b'.' | b'-')
            || (slash && b == b'/')
    };
    !namespace.is_empty()
        && !path.is_empty()
        && namespace != "horizon"
        && namespace.bytes().all(|b| allowed(b, false))
        && path.bytes().all(|b| allowed(b, true))
}

/// A message's payload as `native_net_send_*` check it.
fn payload(channel: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    if !mod_net::channel_ok(channel) {
        return Err(format!("the channel {channel:?} is not mod_id:name"));
    }
    mod_net::encode(&Message {
        channel: channel.to_owned(),
        data: data.to_vec(),
    })
    .map_err(|e| e.to_string())
}

impl Game<'_> {
    /// A `horizon:mod_net` payload from `conn`: queued for
    /// `run_native_net`. Past the caps it is dropped in silence.
    pub(super) fn guest_native_net(&mut self, host: &LanHost, conn: u32, data: &[u8]) {
        let Some(player) = host.hub.guest(conn).map(|g| g.uuid) else {
            return;
        };
        let count = self.native_net.from_counts.entry(player).or_default();
        if *count >= PER_TICK {
            return;
        }
        *count += 1;
        if let Ok(message) = mod_net::decode(data) {
            self.native_net.from_players.push((player, message));
        }
    }

    /// A plugin channel message from `conn` on a channel that isn't Horizon's own: queued for
    /// `run_native_net`, which hands it to `native_custom_payload_from_client`.
    pub(super) fn guest_custom_payload(
        &mut self,
        host: &LanHost,
        conn: u32,
        channel: String,
        data: Vec<u8>,
    ) {
        if let Some(player) = host.hub.guest(conn).map(|g| g.uuid) {
            self.native_net.payloads_from.push((player, channel, data));
        }
    }

    /// Whether messages from players wait for `run_native_net`.
    pub(super) fn native_net_waiting(&self) -> bool {
        !self.native_net.from_players.is_empty() || !self.native_net.payloads_from.is_empty()
    }

    /// The waiting messages reach the mods, with the hub back in the game.
    pub(super) fn run_native_net(&mut self) {
        for (player, m) in std::mem::take(&mut self.native_net.from_players) {
            self.native_net_from_client(player, &m.channel, &m.data);
        }
        for (player, channel, data) in std::mem::take(&mut self.native_net.payloads_from) {
            self.native_custom_payload_from_client(player, &channel, &data);
        }
    }

    /// What the mods sent this tick leaves; the tick's allowances start over.
    pub(super) fn flush_native_net(&mut self, host: &mut LanHost) {
        self.flush_native_profiles(host);
        // The mods' kicks (native_packets.rs) leave with their messages.
        self.flush_native_kicks(host);
        for (player, data) in std::mem::take(&mut self.native_net.to_players) {
            if let Some(g) = host
                .hub
                .guests
                .iter()
                .find(|g| g.placed && g.uuid == player && g.brand == "horizon")
            {
                host.hub.send_to(
                    g.conn,
                    &C::CustomPayload(CustomPayload {
                        channel: mod_net::CHANNEL.into(),
                        data,
                    }),
                );
            }
        }
        for (player, channel, data) in std::mem::take(&mut self.native_net.payloads_to) {
            if let Some(g) = host
                .hub
                .guests
                .iter()
                .find(|g| g.placed && g.uuid == player)
            {
                host.hub
                    .send_to(g.conn, &C::CustomPayload(CustomPayload { channel, data }));
            }
        }
        // The allowance is a server tick's, not a level pass's: it starts over
        // after the last level.
        if self.level_pass.last {
            self.native_net.from_counts.clear();
            self.native_net.to_counts.clear();
        }
    }

    /// A `horizon:mod_net` payload the server sent the window.
    pub(super) fn remote_native_net(&mut self, data: &[u8]) {
        match mod_net::decode(data) {
            Ok(m) => self.native_net_from_server(&m.channel, &m.data),
            Err(e) => log::debug!("Horizon: a bad mod message: {e}"),
        }
    }
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// Sends `data` on `channel` (`mod_id:name`, at most 30 000 bytes, at
    /// most 32 a player a tick) to `player`'s client, at the tick's end, from
    /// any server hook. Ok means accepted: a player who left or isn't on a
    /// Horizon client doesn't get it. API 40.
    pub(super) fn native_net_send_to_player(
        &mut self,
        player: u128,
        channel: &str,
        data: &[u8],
    ) -> Result<(), String> {
        if self.remote.is_some() {
            return Err("a client has no players to send to".into());
        }
        let payload = payload(channel, data)?;
        let count = self.native_net.to_counts.entry(player).or_default();
        if *count >= PER_TICK {
            return Err(format!("past {PER_TICK} messages to this player this tick"));
        }
        *count += 1;
        self.native_net.to_players.push((player, payload));
        Ok(())
    }

    /// Sends `data` on the plugin channel `channel` (`namespace:path`, not
    /// Horizon's own `horizon:` ones, at most 1 MiB as
    /// `ClientboundCustomPayloadPacket` takes) to `player`'s client, any
    /// client, a vanilla one included, at the tick's end, from any server
    /// hook (Bukkit's `Player.sendPluginMessage`). Ok means queued: a player
    /// who isn't on by then gets nothing. Nothing counts these; the bytes
    /// are the mod's to get right. An error on a client, and for a bad
    /// channel or size. API @API:send_custom_payload_to_player@.
    pub(super) fn native_send_custom_payload_to_player(
        &mut self,
        player: u128,
        channel: &str,
        data: &[u8],
    ) -> Result<(), String> {
        if self.remote.is_some() {
            return Err("a client has no players to send to".into());
        }
        if !plugin_channel_ok(channel) {
            return Err(format!(
                "the channel {channel:?} is not namespace:path outside horizon:"
            ));
        }
        if data.len() > MAX_CLIENTBOUND_PAYLOAD {
            return Err(format!(
                "the payload is past {MAX_CLIENTBOUND_PAYLOAD} bytes"
            ));
        }
        self.native_net
            .payloads_to
            .push((player, channel.to_owned(), data.to_vec()));
        Ok(())
    }

    /// The client half: sends `data` on `channel` to the server (the same
    /// caps, 32 a tick). An error when the server doesn't carry mod messages
    /// (a vanilla server), and until the player has joined: a server says it
    /// carries them during configuration, before it can take one. API 40.
    pub(super) fn native_net_send_to_server(
        &mut self,
        channel: &str,
        data: &[u8],
    ) -> Result<(), String> {
        // Not joined first: the announcement may still be on its way, and a
        // mod that gives up on "carries no mod messages" never tries again.
        let Some(remote) = self.remote.as_ref() else {
            return Err("the server carries no mod messages".into());
        };
        // A server that reconfigures the player has them out of the play state.
        if !remote.joined || remote.reconfiguring.is_some() {
            return Err("the player has not joined yet".into());
        }
        if !remote.mod_net {
            return Err("the server carries no mod messages".into());
        }
        let payload = payload(channel, data)?;
        if self.native_net.to_server >= PER_TICK {
            return Err(format!("past {PER_TICK} messages this tick"));
        }
        self.native_net.to_server += 1;
        self.send(S::CustomPayload(CustomPayload {
            channel: mod_net::CHANNEL.into(),
            data: payload,
        }));
        Ok(())
    }

    /// A mod's chat line or command (`/` first), sent as `send_chat` sends
    /// it (ClientPacketListener.sendChat / sendCommand), to any server: no
    /// chat history, and no client chat event (vanilla's `ClientChatEvent`
    /// comes only from ChatScreen). An error when the player isn't in a
    /// server's play state or the Chat option blocks the message. API
    /// 404.
    pub(super) fn native_send_chat(&mut self, message: &str) -> Result<(), String> {
        let Some(remote) = self.remote.as_ref() else {
            return Err("not connected to a server".into());
        };
        // A server that reconfigures the player has them out of the play state.
        if !remote.joined || remote.reconfiguring.is_some() {
            return Err("the player has not joined yet".into());
        }
        let message = message.trim();
        if message.is_empty() {
            return Err("the message is empty".into());
        }
        if !self.chat_abilities().allows(message) {
            return Err("the Chat option blocks this message".into());
        }
        self.send_chat(message);
        Ok(())
    }

    /// Opens the chat screen with `text` typed in and the cursor at its end,
    /// as T and / open it (`ChatScreen` with its initial text): a command
    /// macro can fill in `/tp ` for the player to finish. Nothing is sent; the
    /// player edits and sends it, or presses Escape. The box keeps its limit
    /// (256 characters) and its character filter. An error with no player in a
    /// world, or while another screen is open. API @API:open_chat@.
    pub(super) fn native_open_chat(&mut self, text: &str) -> Result<(), String> {
        if self.player.is_none() {
            return Err("no player in a world".into());
        }
        if self.screen != super::Screen::Playing {
            return Err("another screen is open".into());
        }
        self.open_chat(text, false);
        Ok(())
    }

    /// Sends `packet` to the server as it is, to any server, a vanilla one
    /// included: a plugin channel's payload (`ServerboundCustomPayloadPacket`)
    /// or any other play packet. Nothing counts it, and the game doesn't apply
    /// it on its side: a packet that needs a sequence number, a signature or
    /// the client's own state is the mod's to get right. An error when the
    /// player isn't in a server's play state, and for a custom payload past
    /// `ServerboundCustomPayloadPacket.MAX_PAYLOAD_SIZE`, which a server
    /// disconnects for. API 409.
    pub(super) fn native_send_packet(
        &mut self,
        packet: ServerboundPlayPacket,
    ) -> Result<(), String> {
        let Some(remote) = self.remote.as_ref() else {
            return Err("not connected to a server".into());
        };
        // A server that reconfigures the player has them out of the play state.
        if !remote.joined || remote.reconfiguring.is_some() {
            return Err("the player has not joined yet".into());
        }
        if let S::CustomPayload(payload) = &packet
            && payload.data.len() > quiet::MAX_PAYLOAD
        {
            return Err(format!("the payload is past {} bytes", quiet::MAX_PAYLOAD));
        }
        self.send(packet);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        Audio, Game, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
    };
    use super::{CustomPayload, S, quiet};
    use horizon_protocol::{connection::Pipe, packets::configuration::Ping};

    /// A mod's packet goes out only from a joined player, and a custom
    /// payload only within vanilla's size limit.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mod_packets_need_a_joined_player_and_a_payload_within_the_limit() {
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
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let payload = |len| {
            S::CustomPayload(CustomPayload {
                channel: "voicechat:request_secret".into(),
                data: vec![0; len],
            })
        };

        assert!(game.native_send_packet(payload(4)).is_err());
        let (client, _server) = Pipe::pair();
        game.connect_local(client, "PacketTest").unwrap();
        assert!(game.native_send_packet(payload(4)).is_err());
        game.remote.as_mut().unwrap().joined = true;
        assert!(game.native_send_packet(payload(4)).is_ok());
        assert!(game.native_send_packet(payload(quiet::MAX_PAYLOAD)).is_ok());
        assert!(
            game.native_send_packet(payload(quiet::MAX_PAYLOAD + 1))
                .is_err()
        );
        assert!(game.native_send_packet(S::Pong(Ping { id: 7 })).is_ok());
    }
}
