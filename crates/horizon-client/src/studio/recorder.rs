//! Studio Live Session Packet Recorder
//!
//! Captures clientbound network packets in real-time onto a background writer
//! thread so gameplay frame rates and latency are unaffected.

use super::mcpr::{McprWriter, Metadata, TimestampedPacket};
use crate::net::PacketRecorder;
pub use crate::net::RecorderStatus;
use horizon_protocol::{buf::Reader, crypto::generate_secret, packets::play};
use std::time::Duration;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static RECORDING_NAME_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Commands sent to the background recording thread.
enum RecordCommand {
    Packet(QueuedPacket),
    Stop,
}

const RECORD_QUEUE_PACKETS: usize = 2048;
// Packet-count bounds alone can retain many GiB when packets contain chunks.
// Include the packet being written, whose payload is still alive on the worker.
const RECORD_QUEUE_BYTES: usize = 16 << 20;

struct QueuedPacket {
    packet: TimestampedPacket,
    queued_bytes: Arc<AtomicUsize>,
}
impl Drop for QueuedPacket {
    fn drop(&mut self) {
        self.queued_bytes
            .fetch_sub(self.packet.payload.len(), Ordering::AcqRel);
    }
}

/// The owner retains this handle until archive finalization finishes. Polling
/// never joins a running worker, so stopping a recording cannot block UIKit.
#[derive(Clone)]
pub struct WorkHandle(Arc<RecordingWork>);
struct RecordingWork {
    active: Arc<AtomicBool>,
    state: std::sync::Mutex<WorkState>,
}
struct WorkState {
    thread: Option<JoinHandle<io::Result<PathBuf>>>,
    result: Option<Result<PathBuf, String>>,
}
impl WorkHandle {
    /// Request a stop and return whether every recording write has finished.
    pub fn shutdown(&self) -> bool {
        self.0.active.store(false, Ordering::Release);
        self.poll_finish().is_some()
    }
    fn poll_finish(&self) -> Option<io::Result<PathBuf>> {
        let mut state = self.0.state.lock().unwrap();
        if state.thread.as_ref().is_some_and(JoinHandle::is_finished) {
            let worker = state.thread.take().unwrap();
            self.0.active.store(false, Ordering::Release);
            state.result = Some(match worker.join() {
                Ok(result) => result.map_err(|error| error.to_string()),
                Err(_) => Err("studio recorder worker thread panicked".into()),
            });
        }
        state.result.as_ref().map(|result| match result {
            Ok(path) => Ok(path.clone()),
            Err(error) => Err(io::Error::other(error.clone())),
        })
    }
}

/// Asynchronous packet recorder that writes `.mcpr` sessions on a worker thread.
pub struct StudioRecorder {
    active: Arc<AtomicBool>,
    packet_count: Arc<AtomicU64>,
    bytes_written: Arc<AtomicU64>,
    queued_bytes: Arc<AtomicUsize>,
    sender: SyncSender<RecordCommand>,
    failure: Arc<std::sync::Mutex<Option<String>>>,
    worker: WorkHandle,
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

        let worker_active = active.clone();
        let worker_count = packet_count.clone();
        let worker_bytes = bytes_written.clone();
        let failure = Arc::new(std::sync::Mutex::new(None));
        let worker_failure = failure.clone();
        let thread_dest = dest_path.clone();

        let worker = thread::Builder::new()
            .name("Studio Recorder".into())
            .spawn(move || -> io::Result<PathBuf> {
                // Recovery can compress a large leftover stream. Keep both it
                // and initial disk IO off the frame thread too.
                let mut writer = match McprWriter::new(thread_dest.clone(), metadata) {
                    Ok(writer) => writer,
                    Err(error) => {
                        *worker_failure.lock().unwrap() = Some(error.to_string());
                        worker_active.store(false, Ordering::Release);
                        return Err(error);
                    }
                };
                let mut duration_ms = 0;
                loop {
                    let cmd = match receiver.recv_timeout(Duration::from_millis(25)) {
                        Ok(cmd) => cmd,
                        Err(RecvTimeoutError::Timeout) if worker_active.load(Ordering::Acquire) => {
                            continue;
                        }
                        Err(_) => break,
                    };
                    match cmd {
                        RecordCommand::Packet(queued) => {
                            let pkt = &queued.packet;
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
                    if let RecordCommand::Packet(queued) = cmd {
                        let pkt = &queued.packet;
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
                if let Err(error) = writer.finish(duration_ms) {
                    *worker_failure.lock().unwrap() = Some(error.to_string());
                    return Err(error);
                }
                if let Some(error) = worker_failure.lock().unwrap().as_ref() {
                    return Err(io::Error::other(format!(
                        "recording stopped with an error: {error}"
                    )));
                }
                Ok(thread_dest)
            })?;

        let worker = WorkHandle(Arc::new(RecordingWork {
            active: active.clone(),
            state: std::sync::Mutex::new(WorkState {
                thread: Some(worker),
                result: None,
            }),
        }));
        Ok(Self {
            active,
            packet_count,
            bytes_written,
            queued_bytes: Arc::new(AtomicUsize::new(0)),
            sender,
            failure,
            worker,
            start_time,
            dest_path,
        })
    }

    /// Record a clientbound packet if recording is active.
    pub fn record_packet(&self, id: i32, payload: &[u8]) {
        let timestamp_ms = self.start_time.elapsed().as_millis().min(u32::MAX as u128) as u32;
        self.record_packet_at(timestamp_ms, id, payload);
    }

    /// Record a clientbound packet at an explicit timestamp (e.g. 0ms for initial world baseline).
    pub fn record_packet_at(&self, timestamp_ms: u32, id: i32, payload: &[u8]) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        // Reserve before copying: refusing an over-budget packet must not itself
        // allocate another large payload on the network thread.
        // compare_exchange_weak keeps this compatible with the workspace MSRV;
        // fetch_update is deprecated on newer toolchains, while its replacement
        // was stabilized after the oldest supported Rust version.
        let reserved = {
            let mut current = self.queued_bytes.load(Ordering::Acquire);
            loop {
                let Some(next) = current
                    .checked_add(payload.len())
                    .filter(|&n| n <= RECORD_QUEUE_BYTES)
                else {
                    break false;
                };
                match self.queued_bytes.compare_exchange_weak(
                    current,
                    next,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break true,
                    Err(observed) => current = observed,
                }
            }
        };
        if !reserved {
            *self.failure.lock().unwrap() =
                Some("recorder byte queue is full; capture stopped".into());
            self.active.store(false, Ordering::SeqCst);
            return;
        }
        let queued = QueuedPacket {
            packet: TimestampedPacket::new(timestamp_ms, id, payload.to_vec()),
            queued_bytes: self.queued_bytes.clone(),
        };
        if let Err(error) = self.sender.try_send(RecordCommand::Packet(queued)) {
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
        let completed = self.poll_finish();
        let active = self.is_active();
        RecorderStatus {
            active,
            finishing: !active && completed.is_none(),
            path: self.dest_path.clone(),
            elapsed: self.start_time.elapsed(),
            packet_count: self.packet_count.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            error: self.failure.lock().unwrap().clone().or_else(|| {
                completed.and_then(|result| result.err().map(|error| error.to_string()))
            }),
        }
    }

    /// Where the completed archive will be written.
    pub fn path(&self) -> &Path {
        &self.dest_path
    }

    /// Keep this handle in the game owner across a Session/guest teardown.
    pub fn work_handle(&self) -> WorkHandle {
        self.worker.clone()
    }

    /// Stop accepting packets without waiting for disk IO or compression.
    pub fn request_stop(&self) {
        self.active.store(false, Ordering::Release);
        // Wakes an idle worker. A full queue is drained before it observes the
        // stop flag, and must never make a frame thread wait for a queue slot.
        let _ = self.sender.try_send(RecordCommand::Stop);
    }

    /// The final path/error, only once archive finalization has completed.
    pub fn poll_finish(&self) -> Option<io::Result<PathBuf>> {
        self.worker.poll_finish()
    }

    /// Synchronous compatibility helper for tools and tests. Game UI uses
    /// request_stop/poll_finish so it can stay responsive while saving.
    pub fn stop(self) -> io::Result<PathBuf> {
        self.request_stop();
        loop {
            if let Some(result) = self.poll_finish() {
                return result;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl PacketRecorder for StudioRecorder {
    fn request_stop(&self) {
        StudioRecorder::request_stop(self);
    }
    fn poll_finish(&self) -> Option<io::Result<PathBuf>> {
        StudioRecorder::poll_finish(self)
    }
    fn work_handle(&self) -> Option<WorkHandle> {
        Some(StudioRecorder::work_handle(self))
    }

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
pub(crate) fn recordable<'a>(
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
        self.request_stop();
        // The Session/Game owner retains work_handle() until this completes.
        // Even an exceptional drop cannot join a worker still writing to disk.
        let _ = self.poll_finish();
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

    fn recorder_with_worker(
        capacity: usize,
        active: Arc<AtomicBool>,
        worker: Option<JoinHandle<io::Result<PathBuf>>>,
    ) -> (StudioRecorder, Receiver<RecordCommand>) {
        let (sender, receiver) = sync_channel(capacity);
        let recorder = StudioRecorder {
            active: active.clone(),
            packet_count: Arc::new(AtomicU64::new(0)),
            bytes_written: Arc::new(AtomicU64::new(0)),
            queued_bytes: Arc::new(AtomicUsize::new(0)),
            sender,
            failure: Arc::new(std::sync::Mutex::new(None)),
            worker: WorkHandle(Arc::new(RecordingWork {
                active,
                state: std::sync::Mutex::new(WorkState {
                    thread: worker,
                    result: None,
                }),
            })),
            start_time: Instant::now(),
            dest_path: PathBuf::from("unused.mcpr"),
        };
        (recorder, receiver)
    }

    #[test]
    fn queued_packet_bytes_include_inflight_writes_and_reject_before_another_copy() {
        let (recorder, receiver) = recorder_with_worker(4, Arc::new(AtomicBool::new(true)), None);
        let payload = vec![7; 8 << 20];
        recorder.record_packet_at(0, 1, &payload);
        recorder.record_packet_at(1, 2, &payload);
        assert_eq!(
            recorder.queued_bytes.load(Ordering::Acquire),
            RECORD_QUEUE_BYTES
        );
        let first = receiver.try_recv().unwrap();
        // Receiving it is not enough to release the budget: a blocked disk
        // write is still holding this payload in memory.
        assert_eq!(
            recorder.queued_bytes.load(Ordering::Acquire),
            RECORD_QUEUE_BYTES
        );
        recorder.record_packet_at(2, 3, &[9]);
        assert!(!recorder.is_active());
        assert!(recorder.status().error.unwrap().contains("byte queue"));
        assert_eq!(
            recorder.queued_bytes.load(Ordering::Acquire),
            RECORD_QUEUE_BYTES
        );
        match &first {
            RecordCommand::Packet(queued) => assert_eq!(queued.packet.payload, payload),
            RecordCommand::Stop => panic!("the first accepted packet must remain queued"),
        }
        drop(first);
        assert_eq!(recorder.queued_bytes.load(Ordering::Acquire), 8 << 20);
        drop(receiver.try_recv().unwrap());
        assert_eq!(recorder.queued_bytes.load(Ordering::Acquire), 0);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn count_queue_rejection_releases_its_byte_reservation() {
        let (recorder, receiver) = recorder_with_worker(1, Arc::new(AtomicBool::new(true)), None);
        recorder.record_packet_at(0, 1, &[1, 2]);
        recorder.record_packet_at(1, 2, &[3, 4, 5]);
        assert!(!recorder.is_active());
        assert_eq!(recorder.queued_bytes.load(Ordering::Acquire), 2);
        drop(receiver.try_recv().unwrap());
        assert_eq!(recorder.queued_bytes.load(Ordering::Acquire), 0);
    }

    #[test]
    fn stop_and_drop_do_not_join_a_busy_writer_or_wait_for_a_queue_slot() {
        let (release, gate) = std::sync::mpsc::channel();
        let (recorder, receiver) = recorder_with_worker(1, Arc::new(AtomicBool::new(true)), None);
        let worker = thread::spawn(move || {
            gate.recv().unwrap();
            // Also releases any sender blocked by a regressed blocking Stop.
            while receiver.try_recv().is_ok() {}
            Ok(PathBuf::from("finished.mcpr"))
        });
        recorder.worker.0.state.lock().unwrap().thread = Some(worker);
        recorder.record_packet_at(0, 1, &[1]); // Fill the bounded queue.
        let work = recorder.work_handle();
        let (done, returned) = std::sync::mpsc::channel();
        let dropping = thread::spawn(move || {
            recorder.request_stop();
            drop(recorder);
            done.send(()).unwrap();
        });
        let immediate = returned.recv_timeout(Duration::from_millis(500));
        let finished_before_release = work.shutdown();
        // Always release the worker before asserting, even if the old blocking
        // implementation is substituted to demonstrate the regression.
        release.send(()).unwrap();
        dropping.join().unwrap();
        assert!(
            immediate.is_ok(),
            "stopping/dropping waited for the busy writer"
        );
        assert!(!finished_before_release);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !work.shutdown() {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!(
            work.poll_finish().unwrap().unwrap(),
            PathBuf::from("finished.mcpr")
        );
    }

    #[test]
    fn archive_finalization_failure_remains_visible_after_worker_finishes() {
        let root = std::env::temp_dir().join(format!(
            "studio_finalize_error_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("not-a-file.mcpr");
        fs::create_dir_all(&path).unwrap();
        let recorder = StudioRecorder::start(path, Metadata::default()).unwrap();
        recorder.record_packet_at(0, 1, &[1]);
        recorder.request_stop();
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            if let Some(result) = recorder.poll_finish() {
                break result;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        };
        assert!(result.is_err());
        let status = recorder.status();
        assert!(!status.active);
        assert!(!status.finishing);
        assert!(status.error.is_some());
        drop(recorder);
        fs::remove_dir_all(root).unwrap();
    }

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
