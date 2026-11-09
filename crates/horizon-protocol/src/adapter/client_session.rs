//! One connection from a client of an older release to a Horizon server: [`super::Session`] run the other
//! way (roadmap `older-clients`). What the client sends is renumbered to the pinned ids before the server
//! reads it; what the server sends is renumbered to the client's.
//!
//! This layer maps packet ids only: the registry ids inside packets, and the packets whose layout changed
//! between the client's release and the pinned one, are later steps.
//!
//! The states change where vanilla's server-side `Connection` switches its codecs, after the handshake the
//! server read itself: `login_acknowledged` and `login_finished` move each direction to configuration,
//! `finish_configuration` to play, and `configuration_acknowledged` / `start_configuration` back.
use std::sync::atomic::{AtomicU8, Ordering};

use super::ServerVersion;
use crate::{
    Result, State,
    buf::{Reader, Writer},
    ids as packet_ids,
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
pub struct ClientSession {
    pub version: ServerVersion,
    /// What the client sends.
    reading: AtomicU8,
    /// What the server sends.
    writing: AtomicU8,
}

impl ClientSession {
    /// A client whose login handshake stated `version`'s protocol: both directions are in login.
    pub fn new(version: ServerVersion) -> Self {
        Self {
            version,
            reading: AtomicU8::new(to_u8(State::Login)),
            writing: AtomicU8::new(to_u8(State::Login)),
        }
    }
    pub fn reading(&self) -> State {
        from_u8(self.reading.load(Ordering::Acquire))
    }
    pub fn writing(&self) -> State {
        from_u8(self.writing.load(Ordering::Acquire))
    }

    /// A packet the client sent, with the id the pinned version knows it by. `None` drops it: the pinned
    /// version has no such packet (26.2's `swing`).
    pub fn serverbound(&self, id: i32, payload: Vec<u8>) -> Result<Option<(i32, Vec<u8>)>> {
        let state = self.reading();
        let Some(pinned) = self.version.serverbound_id_up(state, id) else {
            return Ok(None);
        };
        use packet_ids::{configuration::serverbound as cfg, login::serverbound as login};
        let next = match (state, pinned) {
            (State::Login, login::LOGIN_ACKNOWLEDGED) => State::Configuration,
            (State::Configuration, cfg::FINISH_CONFIGURATION) => State::Play,
            (State::Play, packet_ids::play::serverbound::CONFIGURATION_ACKNOWLEDGED) => {
                State::Configuration
            }
            _ => state,
        };
        self.reading.store(to_u8(next), Ordering::Release);
        Ok(Some((pinned, payload)))
    }

    /// A packet body (VarInt id + payload) the server sends, as the client numbers it. `None`: the client
    /// has no such packet, so nothing is sent.
    pub fn clientbound(&self, body: &[u8]) -> Result<Option<Vec<u8>>> {
        let state = self.writing();
        let mut r = Reader::new(body);
        let id = r.varint()?;
        let payload = r.rest();
        use packet_ids::{configuration::clientbound as cfg, login::clientbound as login};
        let next = match (state, id) {
            (State::Login, login::LOGIN_FINISHED) => State::Configuration,
            (State::Configuration, cfg::FINISH_CONFIGURATION) => State::Play,
            (State::Play, packet_ids::play::clientbound::START_CONFIGURATION) => {
                State::Configuration
            }
            _ => state,
        };
        self.writing.store(to_u8(next), Ordering::Release);
        let Some(mapped) = self.version.clientbound_id_down(state, id) else {
            return Ok(None);
        };
        let mut w = Writer::new();
        w.varint(mapped);
        w.bytes(payload);
        Ok(Some(w.data))
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

    fn in_play() -> ClientSession {
        let s = ClientSession::new(ServerVersion::V26_2);
        s.reading.store(to_u8(State::Play), Ordering::Release);
        s.writing.store(to_u8(State::Play), Ordering::Release);
        s
    }

    // Expected ids are read off the 26.2 and 26.3 packets reports by hand, not from the generated tables.

    #[test]
    fn each_direction_follows_its_own_switches() {
        let s = ClientSession::new(ServerVersion::V26_2);
        assert_eq!((s.reading(), s.writing()), (State::Login, State::Login));
        // The server finishes the login: it speaks configuration, the client still login.
        s.clientbound(&body(packet_ids::login::clientbound::LOGIN_FINISHED, &[]))
            .unwrap();
        assert_eq!(
            (s.reading(), s.writing()),
            (State::Login, State::Configuration)
        );
        s.serverbound(
            packet_ids::login::serverbound::LOGIN_ACKNOWLEDGED,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(s.reading(), State::Configuration);
        // finish_configuration is 3 both ways in 26.2 as in 26.3.
        s.clientbound(&body(3, &[])).unwrap();
        s.serverbound(3, Vec::new()).unwrap();
        assert_eq!((s.reading(), s.writing()), (State::Play, State::Play));
    }

    #[test]
    fn packets_are_renumbered_both_ways_and_unknown_ones_dropped() {
        let s = in_play();
        // keep_alive: 26.3's clientbound 45 is 26.2's 44; serverbound 28 in both.
        let sent = s
            .clientbound(&body(packet_ids::play::clientbound::KEEP_ALIVE, &[0; 8]))
            .unwrap()
            .unwrap();
        assert_eq!(sent[0], 44);
        assert_eq!(
            s.serverbound(28, vec![0; 8]).unwrap(),
            Some((packet_ids::play::serverbound::KEEP_ALIVE, vec![0; 8]))
        );
        // add_transient_block (37) is new in 26.3: not sent.
        let transient = packet_ids::play::clientbound::ADD_TRANSIENT_BLOCK;
        assert_eq!(s.clientbound(&body(transient, &[0; 9])).unwrap(), None);
        // 26.2's resource_pack (49) is 26.3's 50; its swing (63) has no pinned packet.
        assert_eq!(
            s.serverbound(49, vec![7]).unwrap(),
            Some((packet_ids::play::serverbound::RESOURCE_PACK, vec![7]))
        );
        assert_eq!(s.serverbound(63, vec![0]).unwrap(), None);
    }
}
