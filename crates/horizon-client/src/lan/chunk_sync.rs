//! Who has a chunk, and the chunk sent again: what a server mod that wrote
//! sections itself needs to get the result to the players.
use super::hub::Hub;
use crate::world::World;

impl Hub {
    /// The players who are sent chunk `chunk` of the level in place and have
    /// it (`ChunkMap.getPlayers(pos, false)`): their UUIDs, in join order.
    pub fn chunk_trackers(&self, chunk: (i32, i32)) -> Vec<u128> {
        self.guests
            .iter()
            .filter(|g| self.watches(g) && g.sent_chunks.contains(&chunk))
            .map(|g| g.uuid)
            .collect()
    }
    /// The chunk `chunk` sent whole again, with its light, block entities and
    /// biomes as it is now, to each player who has it, or to `only` when that
    /// player has it (a player not sent the chunk is not sent one). How many
    /// players were sent it.
    pub fn resend_chunk(&mut self, world: &World, chunk: (i32, i32), only: Option<u128>) -> usize {
        let Some(column) = world.chunks.get(&chunk) else {
            return 0;
        };
        let viewers: Vec<usize> = (0..self.guests.len())
            .filter(|&i| {
                let g = &self.guests[i];
                self.watches(g)
                    && g.sent_chunks.contains(&chunk)
                    && only.is_none_or(|uuid| g.uuid == uuid)
            })
            .collect();
        if viewers.is_empty() {
            return 0;
        }
        if let Some(far) = self.far.as_mut() {
            far.changed(chunk.0, chunk.1);
        }
        let body = self.encoder.body_in(column, world);
        for &i in &viewers {
            self.guests[i].sender.send(body.clone());
        }
        viewers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chunk::Chunk,
        lan::{Lan, guest::Guest, hub::HubConfig},
    };
    use horizon_protocol::packets::play::Clientbound as C;
    use std::sync::{Arc, mpsc};

    fn hub_with_a_guest() -> (Hub, mpsc::Receiver<crate::lan::Out>) {
        let config = crate::lan::NetConfig {
            announce: false,
            ..crate::lan::NetConfig::loopback(0, "ChunkSyncTest".into(), 2)
        };
        let sync = Arc::new(crate::lan::data::SyncData::empty());
        let lan = Lan::start_with(sync.clone(), config).unwrap();
        let encoder = crate::lan::chunks::ChunkEncoder::new(
            [] as [(Arc<str>, bool); 0],
            &horizon_protocol::registry::Builtin::default(),
            &[],
            &[],
        );
        let hub_config = HubConfig {
            default_game_mode: 0,
            guest_commands: false,
            force_game_mode: false,
            hardcore: false,
            view_distance: 10,
            simulation_distance: 10,
            dimension: "minecraft:overworld".into(),
            allow_flight: false,
            seed: 0,
            max_players: 20,
            integrated: false,
            levels: Default::default(),
        };
        let mut hub = Hub::new(lan, sync, encoder, hub_config);
        let rx = add_guest(&mut hub, 1, [0.5, 64.0, 0.5], &[]);
        (hub, rx)
    }

    /// A placed guest at `at` that has been sent the chunks `chunks`.
    fn add_guest(
        hub: &mut Hub,
        uuid: u128,
        at: [f64; 3],
        chunks: &[(i32, i32)],
    ) -> std::sync::mpsc::Receiver<crate::lan::Out> {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut guest = Guest::new(
            crate::lan::Joined {
                conn: uuid as u32,
                uuid,
                name: format!("P{uuid}"),
                properties: Vec::new(),
                info: horizon_protocol::packets::configuration::ClientInformation::new(8),
                brand: "vanilla".into(),
                sender: crate::lan::PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(std::sync::atomic::AtomicI32::new(0)),
                address: "127.0.0.1:25566".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            uuid as i32,
            at,
            8,
        );
        guest.placed = true;
        guest.dimension = "minecraft:overworld".into();
        guest.sent_chunks.extend(chunks.iter().copied());
        hub.guests.push(guest);
        rx
    }

    #[test]
    fn chunk_trackers_are_the_players_that_have_the_chunk_and_resends_reach_only_them() {
        let (mut hub, near) = hub_with_a_guest();
        hub.guests[0].sent_chunks.extend([(0, 0), (1, 0)]);
        let far = add_guest(&mut hub, 2, [900.5, 64.0, 900.5], &[(56, 56)]);
        let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
        world.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        assert_eq!(hub.chunk_trackers((0, 0)), [1]);
        assert_eq!(hub.chunk_trackers((56, 56)), [2]);
        assert!(hub.chunk_trackers((9, 9)).is_empty());
        let chunks = |rx: &std::sync::mpsc::Receiver<crate::lan::Out>| {
            let mut seen = Vec::new();
            while let Ok(crate::lan::Out::Packet(body)) = rx.try_recv() {
                let mut r = horizon_protocol::buf::Reader::new(&body);
                let id = r.varint().unwrap();
                if let Ok(C::LevelChunkWithLight(l)) = C::decode(id, r.rest()) {
                    seen.push((l.x, l.z));
                }
            }
            seen
        };
        // Everyone who has the chunk is sent it; the far player does not.
        assert_eq!(hub.resend_chunk(&world, (0, 0), None), 1);
        assert_eq!(chunks(&near), [(0, 0)]);
        assert!(chunks(&far).is_empty());
        // One player asked for: sent it only if they have it.
        assert_eq!(hub.resend_chunk(&world, (0, 0), Some(2)), 0);
        assert_eq!(hub.resend_chunk(&world, (0, 0), Some(1)), 1);
        assert_eq!(chunks(&near), [(0, 0)]);
        assert!(chunks(&far).is_empty());
        // A chunk not loaded is not sent.
        assert_eq!(hub.resend_chunk(&world, (1, 0), None), 0);
        assert!(chunks(&near).is_empty());
    }
}
