//! One connection to a server of an older release: which state each direction is in, followed from the
//! packets themselves, and every packet renumbered and translated on its way through.
//!
//! The states change where vanilla's `Connection` switches its codecs: the handshake's intent picks status
//! or login for both directions, `login_finished` and `login_acknowledged` move each direction to
//! configuration, `finish_configuration` to play, and `start_configuration` / `configuration_acknowledged`
//! back to configuration.
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use super::{ServerVersion, translate};
use crate::{
    Result, State,
    buf::{Reader, Writer},
    ids,
    packets::handshake,
};

fn to_u8(state: State) -> u8 {
    match state {
        State::Handshake => 0,
        State::Status => 1,
        State::Login => 2,
        State::Configuration => 3,
        State::Play => 4,
    }
}
fn from_u8(v: u8) -> State {
    match v {
        1 => State::Status,
        2 => State::Login,
        3 => State::Configuration,
        4 => State::Play,
        _ => State::Handshake,
    }
}

/// Shared by a connection's reader and writer.
pub struct Session {
    pub version: ServerVersion,
    /// Dimension of the last play login or respawn, for older packets with only a local block position.
    pub(super) dimension: Mutex<String>,
    /// Local entity id and last shift input, for pre-1.21.6 crouch commands.
    pub(super) local_player: Mutex<Option<(i32, bool)>>,
    pub(super) chat_index: std::sync::atomic::AtomicI32,
    pub(super) minecart_display: Mutex<HashMap<i32, (i32, bool)>>,
    pub(super) trim_patterns: Mutex<HashMap<i32, i32>>,
    pub(super) legacy_recipes: Mutex<translate::v1_21_1::recipes::Recipes>,
    pub(super) legacy_boats: Mutex<HashMap<i32, translate::v1_21_1::boats::Boat>>,
    pub(super) legacy_registries: Mutex<Arc<super::ids::LegacyRegistries>>,
    pub(super) legacy_metadata: Mutex<translate::v1_20_3::Metadata>,
    pub(super) legacy_items: Arc<Mutex<crate::item::v1_20_3::Cache>>,
    pub(super) legacy_resource_packs: Mutex<translate::v1_20_2::ResourcePacks>,
    login_requires_authentication: AtomicBool,
    legacy_secure_chat_enforcement: Mutex<Option<bool>>,
    reading: AtomicU8,
    writing: AtomicU8,
    /// Entity UUIDs by network id, for a server that is told which entity a spectator picked by its UUID.
    pub(super) uuids: Mutex<HashMap<i32, u128>>,
    /// Entity types (pinned ids) by network id, for entity data laid out by type.
    pub(super) kinds: Mutex<HashMap<i32, i32>>,
    /// Packets the translation made, for Horizon to read before the next one from the server
    /// (pinned id, payload).
    pub(super) incoming: Mutex<VecDeque<(i32, Vec<u8>)>>,
    /// Packet bodies to send right after the one being sent.
    pub(super) outgoing: Mutex<Vec<Vec<u8>>>,
}

impl Session {
    pub fn new(version: ServerVersion) -> Self {
        Self {
            version,
            dimension: Mutex::new("minecraft:overworld".into()),
            local_player: Mutex::new(None),
            chat_index: std::sync::atomic::AtomicI32::new(0),
            minecart_display: Mutex::new(HashMap::new()),
            trim_patterns: Mutex::new(HashMap::new()),
            legacy_recipes: Mutex::new(translate::v1_21_1::recipes::Recipes::default()),
            legacy_boats: Mutex::new(HashMap::new()),
            legacy_registries: Mutex::new(Arc::new(super::legacy::registries(version))),
            legacy_metadata: Mutex::new(Default::default()),
            legacy_items: Arc::new(Mutex::new(Default::default())),
            legacy_resource_packs: Mutex::new(Default::default()),
            login_requires_authentication: AtomicBool::new(false),
            legacy_secure_chat_enforcement: Mutex::new(None),
            reading: AtomicU8::new(0),
            writing: AtomicU8::new(0),
            uuids: Mutex::new(HashMap::new()),
            kinds: Mutex::new(HashMap::new()),
            incoming: Mutex::new(VecDeque::new()),
            outgoing: Mutex::new(Vec::new()),
        }
    }
    pub fn reading(&self) -> State {
        from_u8(self.reading.load(Ordering::Acquire))
    }
    pub fn writing(&self) -> State {
        from_u8(self.writing.load(Ordering::Acquire))
    }
    fn set_reading(&self, state: State) {
        self.reading.store(to_u8(state), Ordering::Release);
    }
    fn set_writing(&self, state: State) {
        self.writing.store(to_u8(state), Ordering::Release);
    }

    pub(super) fn login_requires_authentication(&self) -> bool {
        self.login_requires_authentication.load(Ordering::Acquire)
    }

    /// Before 1.20.5 this flag arrives in ServerData instead of the play login.
    pub fn legacy_secure_chat_enforcement(&self) -> Option<bool> {
        if !self.version.at_most(ServerVersion::V1_20_3) {
            return None;
        }
        *self.legacy_secure_chat_enforcement.lock().unwrap()
    }

    pub(super) fn set_legacy_secure_chat_enforcement(&self, value: Option<bool>) {
        *self.legacy_secure_chat_enforcement.lock().unwrap() = value;
    }

    /// A packet the server sent, as the pinned version numbers and lays it out. `None` drops it: Horizon
    /// knows no such packet.
    pub fn clientbound(&self, id: i32, payload: Vec<u8>) -> Result<Option<(i32, Vec<u8>)>> {
        let state = self.reading();
        let _legacy = self
            .version
            .at_most(ServerVersion::V1_20_3)
            .then(|| super::ids::legacy_registries(self.legacy_registries.lock().unwrap().clone()));
        let _items = self
            .version
            .at_most(ServerVersion::V1_20_3)
            .then(|| super::ids::legacy_items(self.legacy_items.clone()));
        let recipe_book = if self.version.at_most(ServerVersion::V1_20_3) {
            63
        } else {
            65
        };
        if self.version.at_most(ServerVersion::V1_21_1) && state == State::Play && id == recipe_book
        {
            return translate::v1_21_1::recipes::recipe_book(self, &payload).map(Some);
        }
        // Vanilla 1.21.4's separate add_experience_orb (id 2) became AddEntity plus metadata.
        if self.version.at_most(ServerVersion::V1_21_4) && state == State::Play && id == 2 {
            return translate::old_experience_orb(self, &payload).map(Some);
        }
        let Some(pinned) = self.version.clientbound_id(state, id) else {
            return Ok(None);
        };
        match (state, pinned) {
            (State::Login, ids::login::clientbound::LOGIN_FINISHED) => {
                self.set_reading(State::Configuration)
            }
            (State::Configuration, ids::configuration::clientbound::FINISH_CONFIGURATION) => {
                self.set_reading(State::Play)
            }
            (State::Play, ids::play::clientbound::START_CONFIGURATION) => {
                self.set_legacy_secure_chat_enforcement(None);
                self.set_reading(State::Configuration)
            }
            (State::Play, ids::play::clientbound::LOGIN) => {
                self.set_legacy_secure_chat_enforcement(None);
            }
            _ => {}
        }
        // A packet the translation can't read goes on as it came, as an unknown packet would: Horizon's
        // own reader then skips it or reports it, rather than the connection dropping here.
        let translated = match translate::clientbound(self, state, pinned, &payload) {
            Ok(Some((id, Some(translated)))) => Some((id, translated)),
            Ok(Some((id, None))) => Some((id, payload)),
            Ok(None) => None,
            Err(e) => {
                log::warn!(
                    "Horizon: could not translate {} packet {pinned} ({state:?}): {e}",
                    self.version.name()
                );
                Some((pinned, payload))
            }
        };
        if state == State::Login
            && let Some((id, payload)) = &translated
            && let Ok(crate::packets::login::Clientbound::Hello(hello)) =
                crate::packets::login::Clientbound::decode(*id, payload)
        {
            self.login_requires_authentication
                .store(hello.should_authenticate, Ordering::Release);
        }
        if self.version.at_most(ServerVersion::V1_21_1)
            && state == State::Play
            && let Some((id, body)) = translated
        {
            return translate::v1_21_1::boats::finish(self, id, body).map(Some);
        }
        Ok(translated)
    }

    /// A packet body (VarInt id + payload) Horizon sends, as this server numbers and lays it out. `None`:
    /// the server has no such packet, so nothing is sent.
    pub fn serverbound(&self, body: &[u8]) -> Result<Option<Vec<u8>>> {
        let state = self.writing();
        let _legacy = self
            .version
            .at_most(ServerVersion::V1_20_3)
            .then(|| super::ids::legacy_registries(self.legacy_registries.lock().unwrap().clone()));
        let _items = self
            .version
            .at_most(ServerVersion::V1_20_3)
            .then(|| super::ids::legacy_items(self.legacy_items.clone()));
        let mut r = Reader::new(body);
        let id = r.varint()?;
        let payload = r.rest();
        match (state, id) {
            (State::Handshake, ids::handshake::serverbound::INTENTION) => {
                return self.intention(payload).map(Some);
            }
            (State::Login, ids::login::serverbound::LOGIN_ACKNOWLEDGED) => {
                self.set_writing(State::Configuration)
            }
            (State::Configuration, ids::configuration::serverbound::FINISH_CONFIGURATION) => {
                self.set_writing(State::Play)
            }
            (State::Play, ids::play::serverbound::CONFIGURATION_ACKNOWLEDGED) => {
                self.set_writing(State::Configuration)
            }
            _ => {}
        }
        let Some((mapped, payload)) = translate::serverbound(self, state, id, payload)? else {
            return Ok(None);
        };
        let mut w = Writer::new();
        w.varint(mapped);
        w.bytes(&payload);
        Ok(Some(w.data))
    }

    /// A packet the translation made that Horizon reads next, already in the pinned version's numbering.
    pub fn next_incoming(&self) -> Option<(i32, Vec<u8>)> {
        self.incoming.lock().unwrap().pop_front()
    }
    /// Bodies the translation added after the packet just sent, in the server's numbering.
    pub fn take_outgoing(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.outgoing.lock().unwrap())
    }

    /// The handshake states this server's protocol, and its intent moves both directions on.
    fn intention(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let _pinned = r.varint()?;
        let host = r.string(255)?;
        let port = r.u16()?;
        let intent = r.varint()?;
        let next = if intent == handshake::STATUS {
            State::Status
        } else {
            State::Login
        };
        self.set_writing(next);
        self.set_reading(next);
        let mut w = Writer::new();
        w.varint(ids::handshake::serverbound::INTENTION);
        w.varint(self.version.protocol());
        w.string(&host);
        w.u16(port);
        w.varint(intent);
        Ok(w.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(id: i32, payload: &[u8]) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(id);
        w.bytes(payload);
        w.data
    }

    fn old_play_id(version: ServerVersion, pinned: i32) -> i32 {
        (0..256)
            .find(|&id| version.clientbound_id(State::Play, id) == Some(pinned))
            .unwrap()
    }

    fn old_login(version: ServerVersion) -> Vec<u8> {
        let mut w = Writer::new();
        w.i32(42);
        w.bool(false);
        w.varint(1);
        w.string("minecraft:overworld");
        for n in [20, 8, 6] {
            w.varint(n);
        }
        for flag in [false, true, false] {
            w.bool(flag);
        }
        if version.at_most(ServerVersion::V1_20_3) {
            w.string("minecraft:overworld");
        } else {
            w.varint(0);
        }
        w.string("minecraft:overworld");
        w.i64(0);
        w.i8(0);
        w.i8(-1);
        for _ in 0..3 {
            w.bool(false);
        }
        w.varint(0);
        if !version.at_most(ServerVersion::V1_20_3) {
            w.bool(true); // The newer login has its own enforcement flag.
        }
        w.data
    }

    #[test]
    fn legacy_logins_preserve_the_encryption_requests_authentication_signal() {
        use crate::packets::{login, play};
        for version in [ServerVersion::V1_20_3, ServerVersion::V1_20_6] {
            for authenticated in [false, true] {
                let s = Session::new(version);
                s.set_reading(State::Login);
                // 765 had no opt-out bit: offline login omitted the encryption request.
                if authenticated || version == ServerVersion::V1_20_6 {
                    let mut hello = Writer::new();
                    hello.string("");
                    hello.byte_array(&[1, 2]);
                    hello.byte_array(&[3, 4]);
                    if version == ServerVersion::V1_20_6 {
                        hello.bool(authenticated);
                    }
                    let (id, payload) = s.clientbound(1, hello.data).unwrap().unwrap();
                    let login::Clientbound::Hello(hello) =
                        login::Clientbound::decode(id, &payload).unwrap()
                    else {
                        panic!("hello")
                    };
                    assert_eq!(hello.should_authenticate, authenticated);
                }
                assert_eq!(s.login_requires_authentication(), authenticated);
                if version == ServerVersion::V1_20_3 {
                    Arc::make_mut(&mut s.legacy_registries.lock().unwrap()).insert(
                        "minecraft:dimension_type".into(),
                        crate::registry::Ids::from_names(vec!["minecraft:overworld".into()]),
                    );
                }
                s.set_reading(State::Play);
                let (id, payload) = s
                    .clientbound(
                        old_play_id(version, ids::play::clientbound::LOGIN),
                        old_login(version),
                    )
                    .unwrap()
                    .unwrap();
                let play::Clientbound::Login(login) =
                    play::Clientbound::decode(id, &payload).unwrap()
                else {
                    panic!("login")
                };
                assert_eq!(login.online_mode, authenticated, "{}", version.name());
                assert_eq!(
                    login.enforces_secure_chat,
                    version == ServerVersion::V1_20_6
                );
            }
        }
    }

    #[test]
    fn legacy_server_data_updates_and_resets_secure_chat_enforcement() {
        use crate::buf::Codec;
        use crate::packets::play;
        use horizon_world::nbt::Tag;
        let s = Session::new(ServerVersion::V1_20_3);
        s.set_reading(State::Play);
        let data = play::ServerData {
            motd: Tag::String("Wire server".into()),
            icon: None,
        };
        for enforced in [true, false] {
            let mut w = Writer::new();
            play::ServerData::write(&data, &mut w);
            let expected = w.data.clone();
            w.bool(enforced);
            let (id, payload) = s
                .clientbound(
                    old_play_id(s.version, ids::play::clientbound::SERVER_DATA),
                    w.data,
                )
                .unwrap()
                .unwrap();
            assert_eq!(payload, expected);
            assert!(matches!(
                play::Clientbound::decode(id, &payload).unwrap(),
                play::Clientbound::ServerData(_)
            ));
            assert_eq!(s.legacy_secure_chat_enforcement(), Some(enforced));
        }
        s.set_legacy_secure_chat_enforcement(Some(true));
        s.clientbound(
            old_play_id(s.version, ids::play::clientbound::START_CONFIGURATION),
            vec![],
        )
        .unwrap();
        assert_eq!(s.legacy_secure_chat_enforcement(), None);
        s.set_reading(State::Play);
        s.set_legacy_secure_chat_enforcement(Some(true));
        // Reset belongs to the login boundary even if a malformed body cannot be translated.
        s.clientbound(
            old_play_id(s.version, ids::play::clientbound::LOGIN),
            vec![],
        )
        .unwrap();
        assert_eq!(s.legacy_secure_chat_enforcement(), None);
        assert_eq!(
            Session::new(ServerVersion::Current).legacy_secure_chat_enforcement(),
            None
        );
    }

    #[test]
    fn the_handshake_states_the_older_protocol_and_moves_both_directions() {
        for (version, protocol) in [
            (ServerVersion::V26_2, 776),
            (ServerVersion::V1_21_8, 772),
            (ServerVersion::V1_21_6, 771),
            (ServerVersion::V1_21_5, 770),
            (ServerVersion::V1_21_4, 769),
            (ServerVersion::V1_21_3, 768),
            (ServerVersion::V1_21_1, 767),
        ] {
            let s = Session::new(version);
            let mut payload = Writer::new();
            payload.varint(crate::PROTOCOL_VERSION);
            payload.string("example.org");
            payload.u16(25565);
            payload.varint(handshake::LOGIN);
            let sent = s.serverbound(&body(0, &payload.data)).unwrap().unwrap();
            let mut r = Reader::new(&sent);
            assert_eq!(r.varint().unwrap(), 0);
            assert_eq!(r.varint().unwrap(), protocol);
            assert_eq!(r.string(255).unwrap(), "example.org");
            assert_eq!((s.reading(), s.writing()), (State::Login, State::Login));
        }
    }

    #[test]
    fn each_direction_follows_its_own_switches() {
        let s = Session::new(ServerVersion::V26_2);
        s.set_reading(State::Login);
        s.set_writing(State::Login);
        // login_finished arrives: the server already speaks configuration, Horizon still login.
        s.clientbound(ids::login::clientbound::LOGIN_FINISHED, Vec::new())
            .unwrap();
        assert_eq!(
            (s.reading(), s.writing()),
            (State::Configuration, State::Login)
        );
        s.serverbound(&body(ids::login::serverbound::LOGIN_ACKNOWLEDGED, &[]))
            .unwrap();
        assert_eq!(s.writing(), State::Configuration);
        // finish_configuration: 26.2 numbers it 3 as 26.3 does.
        s.clientbound(3, Vec::new()).unwrap();
        assert_eq!(s.reading(), State::Play);
        s.serverbound(&body(
            ids::configuration::serverbound::FINISH_CONFIGURATION,
            &[],
        ))
        .unwrap();
        assert_eq!(s.writing(), State::Play);
    }

    #[test]
    fn packets_are_renumbered_and_unknown_ones_dropped() {
        let s = Session::new(ServerVersion::V26_2);
        s.set_reading(State::Play);
        s.set_writing(State::Play);
        // 26.2's keep_alive is 44, 26.3's 45.
        let (id, payload) = s.clientbound(44, vec![0; 8]).unwrap().unwrap();
        assert_eq!((id, payload.len()), (ids::play::clientbound::KEEP_ALIVE, 8));
        assert_eq!(s.clientbound(141, Vec::new()).unwrap(), None);
        // resource_pack: Horizon's 50 is 26.2's 49.
        let sent = s
            .serverbound(&body(ids::play::serverbound::RESOURCE_PACK, &[7]))
            .unwrap()
            .unwrap();
        assert_eq!(sent, vec![49, 7]);
    }
}
