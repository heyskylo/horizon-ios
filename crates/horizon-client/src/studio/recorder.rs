//! Studio Live Session Packet Recorder
//!
//! Captures clientbound network packets in real-time onto a background writer
//! thread so gameplay frame rates and latency are unaffected.

use super::mcpr::{McprWriter, Metadata, TimestampedPacket};
pub use crate::net::RecorderStatus;
use crate::net::PacketRecorder;
use horizon_protocol::{buf::Reader, crypto::generate_secret, packets::play};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static RECORDING_NAME_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Commands sent to the background recording thread.
enum RecordCommand {
    Packet(TimestampedPacket),
    Stop,
}

const RECORD_QUEUE_PACKETS: usize = 2048;

/// Asynchronous packet recorder that writes `.mcpr` sessions on a worker thread.
pub struct StudioRecorder {
    active: Arc<AtomicBool>,
    packet_count: Arc<AtomicU64>,
    bytes_written: Arc<AtomicU64>,
    sender: SyncSender<RecordCommand>,
    failure: Arc<std::sync::Mutex<Option<String>>>,
    worker: Option<JoinHandle<io::Result<PathBuf>>>,
    start_time: Instant,
    dest_path: PathBuf,
}

impl StudioRecorder {
    /// Start a new recording session saving to `dest_path`.
    pub fn start(dest_path: PathBuf, mut metadata: Metadata) -> io::Result<Self> {
        let active = Arc::new(AtomicBool::new(true));
        let packet_count = Arc::new(AtomicU64::new(0));
        let bytes_written = Arc::new(AtomicU64::new(0));

        let start_time = Instant::now();
        let start_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        metadata.date = start_unix_ms;

        let (sender, receiver): (SyncSender<RecordCommand>, Receiver<RecordCommand>) =
            sync_channel(RECORD_QUEUE_PACKETS);
        let mut writer = McprWriter::new(dest_path.clone(), metadata)?;

        let worker_active = active.clone();
        let worker_count = packet_count.clone();
        let worker_bytes = bytes_written.clone();
        let failure = Arc::new(std::sync::Mutex::new(None));
        let worker_failure = failure.clone();
        let thread_dest = dest_path.clone();

        let worker = thread::Builder::new()
            .name("Studio Recorder".into())
            .spawn(move || -> io::Result<PathBuf> {
                let mut duration_ms = 0;
                while let Ok(cmd) = receiver.recv() {
                    match cmd {
                        RecordCommand::Packet(pkt) => {
                            duration_ms = duration_ms.max(pkt.timestamp_ms);
                            if let Err(error) =
                                writer.write_packet(pkt.timestamp_ms, pkt.id, &pkt.payload)
                            {
                                *worker_failure.lock().unwrap() = Some(error.to_string());
                                worker_active.store(false, Ordering::SeqCst);
                                break;
                            }
                            worker_count.fetch_add(1, Ordering::Relaxed);
                            worker_bytes.store(writer.bytes_written(), Ordering::Relaxed);
                        }
                        RecordCommand::Stop => break,
                    }
                }
                // Drain any lingering packets in queue
                while let Ok(cmd) = receiver.try_recv() {
                    if let RecordCommand::Packet(pkt) = cmd {
                        duration_ms = duration_ms.max(pkt.timestamp_ms);
                        if let Err(error) =
                            writer.write_packet(pkt.timestamp_ms, pkt.id, &pkt.payload)
                        {
                            *worker_failure.lock().unwrap() = Some(error.to_string());
                            break;
                        }
                        worker_count.fetch_add(1, Ordering::Relaxed);
                        worker_bytes.store(writer.bytes_written(), Ordering::Relaxed);
                    }
                }
                worker_active.store(false, Ordering::SeqCst);
                writer.finish(duration_ms)?;
                if let Some(error) = worker_failure.lock().unwrap().take() {
                    return Err(io::Error::other(format!(
                        "recording stopped with an error: {error}"
                    )));
                }
                Ok(thread_dest)
            })?;

        Ok(Self {
            active,
            packet_count,
            bytes_written,
            sender,
            failure,
            worker: Some(worker),
            start_time,
            dest_path,
        })
    }

    /// Record a clientbound packet if recording is active.
    pub fn record_packet(&self, id: i32, payload: &[u8]) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        let timestamp_ms = self.start_time.elapsed().as_millis() as u32;
        let pkt = TimestampedPacket::new(timestamp_ms, id, payload.to_vec());
        if let Err(error) = self.sender.try_send(RecordCommand::Packet(pkt)) {
            let message = match error {
                TrySendError::Full(_) => "recorder queue is full; capture stopped",
                TrySendError::Disconnected(_) => "recorder worker stopped unexpectedly",
            };
            *self.failure.lock().unwrap() = Some(message.into());
            self.active.store(false, Ordering::SeqCst);
        }
    }

    /// Record a clientbound packet at an explicit timestamp (e.g. 0ms for initial world baseline).
    pub fn record_packet_at(&self, timestamp_ms: u32, id: i32, payload: &[u8]) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        let pkt = TimestampedPacket::new(timestamp_ms, id, payload.to_vec());
        if let Err(error) = self.sender.try_send(RecordCommand::Packet(pkt)) {
            let message = match error {
                TrySendError::Full(_) => "recorder queue is full; capture stopped",
                TrySendError::Disconnected(_) => "recorder worker stopped unexpectedly",
            };
            *self.failure.lock().unwrap() = Some(message.into());
            self.active.store(false, Ordering::SeqCst);
        }
    }

    /// Check if currently recording.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Query current recording status.
    pub fn status(&self) -> RecorderStatus {
        RecorderStatus {
            active: self.is_active(),
            path: self.dest_path.clone(),
            elapsed: self.start_time.elapsed(),
            packet_count: self.packet_count.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            error: self.failure.lock().unwrap().clone(),
        }
    }

    /// Stop recording and finalize the `.mcpr` archive.
    pub fn stop(mut self) -> io::Result<PathBuf> {
        self.active.store(false, Ordering::SeqCst);
        let _ = self.sender.send(RecordCommand::Stop);
        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(res) => res,
                Err(_) => Err(io::Error::other("studio recorder worker thread panicked")),
            }
        } else {
            Ok(self.dest_path.clone())
        }
    }
}

impl PacketRecorder for StudioRecorder {
    fn is_active(&self) -> bool {
        StudioRecorder::is_active(self)
    }
    /// Keeps what [`recordable`] lets through: the filter copies what it keeps, and re-encodes
    /// AddEntity with a fresh UUID.
    fn received(&self, packet: &play::Clientbound, id: i32, payload: &[u8]) {
        if let Some((id, payload)) = recordable(packet, id, payload) {
            StudioRecorder::record_packet(self, id, &payload);
        }
    }
    fn record_packet(&self, id: i32, payload: &[u8]) {
        StudioRecorder::record_packet(self, id, payload);
    }
    fn status(&self) -> RecorderStatus {
        StudioRecorder::status(self)
    }
    fn stop(self: Box<Self>) -> io::Result<PathBuf> {
        StudioRecorder::stop(*self)
    }
}

/// Return the packet bytes safe to persist in a Studio recording.
///
/// Unknown packets fail closed so a newly added protocol packet cannot bypass
/// the privacy policy. `AddEntity` retains its entity id and state, but gets a
/// fresh UUID so the recording cannot link that entity to an account UUID.
fn recordable<'a>(
    packet: &play::Clientbound,
    packet_id: i32,
    payload: &'a [u8],
) -> Option<(i32, std::borrow::Cow<'a, [u8]>)> {
    use play::Clientbound as P;
    use std::borrow::Cow;
    match packet {
        P::AddEntity(entity) => {
            let mut anonymous = entity.clone();
            anonymous.uuid = u128::from_be_bytes(generate_secret().ok()?);
            let encoded = P::AddEntity(anonymous).encode();
            let mut reader = Reader::new(&encoded);
            let id = reader.varint().ok()?;
            Some((id, Cow::Owned(reader.rest().to_vec())))
        }
        P::CookieRequest(_)
        | P::CustomChatCompletions(_)
        | P::CustomPayload(_)
        | P::DeleteChat(_)
        | P::Disconnect(_)
        | P::DisguisedChat(_)
        | P::PlayerChat(_)
        | P::PlayerCombatKill(_)
        | P::PlayerInfoRemove(_)
        | P::PlayerInfoUpdate(_)
        | P::ResetScore(_)
        | P::ResourcePackPush(_)
        | P::ServerLinks(_)
        | P::SetPlayerTeam(_)
        | P::SetScore(_)
        | P::StoreCookie(_)
        | P::SystemChat(_)
        | P::TabList(_)
        | P::TrackedWaypoint(_)
        | P::Transfer(_)
        | P::Unknown(..) => None,
        _ => Some((packet_id, Cow::Borrowed(payload))),
    }
}

impl Drop for StudioRecorder {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        let _ = self.sender.send(RecordCommand::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Default storage directory for video studio recordings (`recordings/`).
pub fn default_recordings_dir() -> PathBuf {
    Path::new("recordings").to_path_buf()
}

/// Generate a timestamped file name for a recording.
pub fn generate_recording_filename(name: Option<&str>) -> String {
    let now = chrono::Local::now();
    let sequence = RECORDING_NAME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stamp = format!("{}_{}", now.format("%Y-%m-%d_%H-%M-%S_%3f"), sequence);
    match name {
        Some(custom) if !custom.trim().is_empty() => {
            let stem: String = custom
                .trim()
                .chars()
                .filter_map(|c| match c {
                    'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => Some(c),
                    _ if c.is_whitespace() => Some('_'),
                    _ => None,
                })
                .take(80)
                .collect();
            let stem = stem.trim_matches(['-', '_']);
            if stem.is_empty() {
                format!("recording_{stamp}.mcpr")
            } else {
                format!("{stem}_{stamp}.mcpr")
            }
        }
        _ => format!("recording_{stamp}.mcpr"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol::packets::configuration as cfg;
    use horizon_world::nbt::Tag;
    use std::fs;

    #[test]
    fn studio_recording_drops_packets_with_direct_player_identity_or_private_text() {
        use horizon_protocol::packets::configuration::{
            ResourcePackPush, ServerLink, ServerLinkKind, ServerLinks, StoreCookie, Transfer,
        };
        use play::{
            ChatTypeBound, Clientbound as P, CustomChatCompletions, PlayerInfo, PlayerInfoRemove,
            PlayerInfoUpdate, ResetScore, SetPlayerTeam, SetScore, TrackedWaypoint, WaypointId,
            WaypointTarget,
        };

        let packets = [
            P::CookieRequest(cfg::CookieRequest {
                key: "private_cookie".into(),
            }),
            P::CustomChatCompletions(CustomChatCompletions {
                action: 2,
                entries: vec!["/msg PlayerName private text".into()],
            }),
            P::CustomPayload(cfg::CustomPayload {
                channel: "private:channel".into(),
                data: b"private payload".to_vec(),
            }),
            P::DeleteChat(play::DeleteChat {
                signature: (3, Some(vec![1, 2, 3])),
            }),
            P::DisguisedChat(play::DisguisedChat {
                message: Tag::String("private message".into()),
                chat_type: ChatTypeBound {
                    chat_type: 0,
                    name: Tag::String("PlayerName".into()),
                    target: None,
                },
            }),
            P::PlayerCombatKill(play::PlayerCombatKill {
                id: 7,
                message: Tag::String("PlayerName was slain".into()),
            }),
            P::PlayerChat(play::PlayerChat {
                global_index: 1,
                sender: 7,
                index: 2,
                signature: None,
                content: "private message".into(),
                timestamp: 3,
                salt: 4,
                last_seen: vec![],
                unsigned_content: None,
                filter: (0, vec![]),
                chat_type: ChatTypeBound {
                    chat_type: 0,
                    name: Tag::String("PlayerName".into()),
                    target: None,
                },
            }),
            P::ResetScore(ResetScore {
                owner: "PlayerName".into(),
                objective: Some("deaths".into()),
            }),
            P::SetScore(SetScore {
                owner: "PlayerName".into(),
                objective: "deaths".into(),
                score: 1,
                display: None,
                number_format: None,
            }),
            P::PlayerInfoRemove(PlayerInfoRemove { uuids: vec![7] }),
            P::PlayerInfoUpdate(PlayerInfoUpdate {
                actions: 1,
                entries: vec![PlayerInfo {
                    uuid: 7,
                    name: Some("PlayerName".into()),
                    ..PlayerInfo::default()
                }],
            }),
            P::ResourcePackPush(ResourcePackPush {
                id: 8,
                url: "https://private.example/pack.zip".into(),
                hash: "hash".into(),
                required: false,
                prompt: None,
            }),
            P::ServerLinks(ServerLinks {
                links: vec![ServerLink {
                    kind: ServerLinkKind::Known(0),
                    link: "https://private.example".into(),
                }],
            }),
            P::SetPlayerTeam(SetPlayerTeam {
                name: "red".into(),
                method: 3,
                parameters: None,
                players: vec!["PlayerName".into()],
            }),
            P::StoreCookie(StoreCookie {
                key: "private_cookie".into(),
                payload: b"secret".to_vec(),
            }),
            P::SystemChat(play::SystemChat {
                content: Tag::String("private message".into()),
                overlay: false,
            }),
            P::TabList(play::TabList {
                header: Tag::String("PlayerName".into()),
                footer: Tag::End,
            }),
            P::TrackedWaypoint(TrackedWaypoint {
                operation: play::waypoint_operation::TRACK,
                id: WaypointId::Uuid(7),
                style: "minecraft:default".into(),
                color: None,
                target: WaypointTarget::Empty,
            }),
            P::Transfer(Transfer {
                host: "private.example".into(),
                port: 25565,
            }),
            P::Disconnect(cfg::Disconnect {
                reason: Tag::String("PlayerName was disconnected".into()),
            }),
            P::Unknown(999, b"unclassified private payload".to_vec()),
        ];

        for packet in packets {
            assert!(
                recordable(&packet, packet.id(), b"raw packet payload").is_none(),
                "{} should be excluded from recordings",
                packet.name()
            );
        }
    }

    #[test]
    fn studio_recording_replaces_entity_uuids_but_keeps_entity_state() {
        let original = play::AddEntity {
            id: 18,
            uuid: 0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10,
            kind: 42,
            position: [1.0, 64.0, -2.0],
            velocity: [0.0, 0.0, 0.0],
            pitch: 1,
            yaw: 2,
            head_yaw: 3,
            data: 4,
        };
        let packet = play::Clientbound::AddEntity(original.clone());
        let (id, payload) = recordable(&packet, packet.id(), b"unused").unwrap();
        let play::Clientbound::AddEntity(recorded) =
            play::Clientbound::decode(id, &payload).unwrap()
        else {
            panic!("sanitized packet changed type")
        };

        assert_ne!(recorded.uuid, original.uuid);
        assert_eq!(recorded.id, original.id);
        assert_eq!(recorded.kind, original.kind);
        assert_eq!(recorded.position, original.position);
        assert_eq!(recorded.velocity, original.velocity);
        assert_eq!(recorded.pitch, original.pitch);
        assert_eq!(recorded.yaw, original.yaw);
        assert_eq!(recorded.head_yaw, original.head_yaw);
        assert_eq!(recorded.data, original.data);
    }


    #[test]
    fn test_studio_recorder_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!(
            "studio_rec_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::create_dir_all(&temp_dir);
        let mcpr_path = temp_dir.join("live_test.mcpr");

        let meta = Metadata::default();
        let recorder = StudioRecorder::start(mcpr_path.clone(), meta).unwrap();
        assert!(recorder.is_active());

        // Send a few packets
        recorder.record_packet(0x01, &[1, 2, 3]);
        recorder.record_packet(0x02, &[4, 5]);

        let st = recorder.status();
        assert_eq!(st.path, mcpr_path);

        let finished = recorder.stop().unwrap();
        assert_eq!(finished, mcpr_path);
        assert!(mcpr_path.exists());

        // Verify with reader
        let mut reader = super::super::mcpr::McprReader::open(&mcpr_path).unwrap();
        assert_eq!(reader.total_packets(), 2);
        let p0 = reader.next_packet().unwrap().unwrap();
        assert_eq!(p0.id, 0x01);
        let p1 = reader.next_packet().unwrap().unwrap();
        assert_eq!(p1.id, 0x02);

        let _ = fs::remove_file(mcpr_path);
        let _ = fs::remove_dir(temp_dir);
    }
}
