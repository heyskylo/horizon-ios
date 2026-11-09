//! Transport acknowledgements retained while a guest is held or handed over.
use super::*;

impl Hub {
    /// Keeps, in a frozen player's inbox, the packets that answer the server
    /// (teleport, chunk batch, loaded, keep-alive, ping, tick end, client
    /// information) and drops the rest. A chat message or signed command is
    /// dropped too, but the messages it says the client has seen are taken
    /// first (`unpackAndApplyLastSeen`), and a chat acknowledgement is handled
    /// here, in the order they came: the client counts them as acknowledged,
    /// and its next message after the freeze would not validate otherwise.
    pub(crate) fn keep_acknowledgements(&mut self, i: usize, events: &mut Vec<HostEvent>) {
        let inbox = std::mem::take(&mut self.guests[i].inbox);
        let mut bytes = 0;
        for (packet, size) in inbox {
            match &packet {
                S::Chat(c) => {
                    self.apply_last_seen(i, &c.last_seen, events);
                }
                S::ChatCommandSigned(c) => {
                    self.apply_last_seen(i, &c.last_seen, events);
                }
                S::ChatAck(c) => self.chat_ack(i, c.offset, events),
                S::AcceptTeleportation(_)
                | S::PlayerLoaded(_)
                | S::ChunkBatchReceived(_)
                | S::ClientInformation(_)
                | S::ClientTickEnd(_)
                | S::KeepAlive(_)
                | S::Pong(_)
                | S::PingRequest(_) => {
                    bytes += size;
                    self.guests[i].inbox.push_back((packet, size));
                }
                _ => {}
            }
        }
        self.guests[i].inbox_bytes = bytes;
    }
}
