//! `horizon:perf`: a client asks a Horizon server to capture its side (docs/perf-platform.md §4.2).
use crate::client_perf::request::{HZ_RANGE, StartOptions, Threads};
use crate::server_perf::MAX_WINDOW_S;
use horizon_perf_format::MAX_PART_BYTES;
use horizon_protocol::{
    Error, Result,
    buf::{Reader, Writer},
};
use std::time::{Duration, Instant};

pub const CHANNEL: &str = "horizon:perf";
/// The server brand of every Horizon server and LAN host.
pub const HORIZON_BRAND: &str = "horizon";
/// The operator level a server capture needs, as vanilla's `/perf` (`Commands.LEVEL_OWNERS`).
pub const OPERATOR_LEVEL: u8 = 4;
/// How long a request waits for the server before it counts as unanswered.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
/// Bytes of a serverbound payload at most (ServerboundCustomPayloadPacket.MAX_PAYLOAD_SIZE).
const MAX_PAYLOAD: usize = 32767;
const MAX_PATH: usize = 256;
/// Bytes of the server's file in one chunk.
pub const CHUNK: usize = 32 * 1024;
/// The most bytes per second the server sends of its file.
pub const RATE: usize = 256 * 1024;
/// Bytes already queued for the guest above which the server holds its next chunk back, so gameplay packets go first.
pub const BUSY_QUEUE: usize = 64 * 1024;
/// How long a client waits for the file to be finished, or for the next chunk, before it gives up on the server's part.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REMOVED: usize = 64;

/// What a capture on the server is asked to record; the switches a server has (no GPU, no file).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wanted {
    pub seconds: Option<u32>,
    pub hz: Option<u32>,
    pub over_ms: Option<f32>,
    pub threads: Threads,
    pub wall: bool,
    pub samples: bool,
    pub timeline: bool,
}

impl From<&StartOptions> for Wanted {
    fn from(o: &StartOptions) -> Self {
        Self {
            seconds: o.seconds,
            hz: o.hz,
            over_ms: o.over_ms,
            threads: o.threads,
            wall: o.wall,
            samples: o.samples,
            timeline: o.timeline,
        }
    }
}

impl Wanted {
    pub fn options(&self) -> StartOptions {
        StartOptions {
            seconds: self.seconds,
            hz: self.hz,
            over_ms: self.over_ms,
            threads: self.threads,
            wall: self.wall,
            samples: self.samples,
            timeline: self.timeline,
            ..StartOptions::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    Start(Wanted),
    Stop,
}

/// Why the server did not do what was asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The player is below operator level 4.
    Denied,
    /// A server capture already runs.
    AlreadyRunning,
    /// No capture of this player's runs.
    NotRunning,
    /// The capture could not be started or written; the server's log says why.
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Started {
        path: String,
        seconds: Option<u32>,
        samples: bool,
    },
    /// The capture ended and its file is whole on the server; `bytes` of it follow as [`Reply::Chunk`]s, none if 0.
    Finished {
        path: String,
        removed: Vec<String>,
        bytes: u64,
    },
    Refused(Refusal),
    /// The next bytes of the file, at most [`CHUNK`].
    Chunk(Vec<u8>),
}

const START: u8 = 0;
const STOP: u8 = 1;
const WALL: u8 = 1;
const SAMPLES: u8 = 2;
const TIMELINE: u8 = 4;
const THREADS_ALL: u8 = 1;

impl Request {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            Self::Stop => w.u8(STOP),
            Self::Start(s) => {
                w.u8(START);
                w.varint(s.seconds.unwrap_or(0) as i32);
                w.varint(s.hz.unwrap_or(0) as i32);
                w.f32(s.over_ms.unwrap_or(0.0));
                w.u8(match s.threads {
                    Threads::Main => 0,
                    Threads::All => THREADS_ALL,
                });
                let flags = [(s.wall, WALL), (s.samples, SAMPLES), (s.timeline, TIMELINE)]
                    .into_iter()
                    .filter(|(on, _)| *on)
                    .fold(0, |acc, (_, bit)| acc | bit);
                w.u8(flags);
            }
        }
        w.data
    }

    /// A payload from a player; every value is checked against the limits `profile start` has.
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() > MAX_PAYLOAD {
            return Err(Error::new("perf: payload too big"));
        }
        let mut r = Reader::new(data);
        let request = match r.u8()? {
            STOP => Self::Stop,
            START => {
                let seconds = r.varint()?;
                let hz = r.varint()?;
                let over = r.f32()?;
                let threads = match r.u8()? {
                    0 => Threads::Main,
                    THREADS_ALL => Threads::All,
                    other => return Err(Error::new(format!("perf: threads {other}"))),
                };
                let flags = r.u8()?;
                if flags & !(WALL | SAMPLES | TIMELINE) != 0 {
                    return Err(Error::new(format!("perf: flags {flags}")));
                }
                let seconds = match u32::try_from(seconds) {
                    Ok(0) => None,
                    Ok(s) if f64::from(s) <= MAX_WINDOW_S => Some(s),
                    _ => return Err(Error::new(format!("perf: {seconds} seconds"))),
                };
                let hz = match u32::try_from(hz) {
                    Ok(0) => None,
                    Ok(h) if HZ_RANGE.contains(&h) => Some(h),
                    _ => return Err(Error::new(format!("perf: {hz} Hz"))),
                };
                let over_ms = if over == 0.0 {
                    None
                } else if over.is_finite() && over > 0.0 {
                    Some(over)
                } else {
                    return Err(Error::new("perf: bad slow threshold"));
                };
                Self::Start(Wanted {
                    seconds,
                    hz,
                    over_ms,
                    threads,
                    wall: flags & WALL != 0,
                    samples: flags & SAMPLES != 0,
                    timeline: flags & TIMELINE != 0,
                })
            }
            other => return Err(Error::new(format!("perf: request {other}"))),
        };
        if r.remaining() > 0 {
            return Err(Error::new("perf: trailing bytes"));
        }
        Ok(request)
    }
}

const STARTED: u8 = 0;
const FINISHED: u8 = 1;
const REFUSED: u8 = 2;
const CHUNK_TAG: u8 = 3;

impl Reply {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            Self::Started {
                path,
                seconds,
                samples,
            } => {
                w.u8(STARTED);
                w.string(path);
                w.varint(seconds.unwrap_or(0) as i32);
                w.bool(*samples);
            }
            Self::Finished {
                path,
                removed,
                bytes,
            } => {
                w.u8(FINISHED);
                w.string(path);
                w.varlong(*bytes as i64);
                w.varint(removed.len().min(MAX_REMOVED) as i32);
                for name in removed.iter().take(MAX_REMOVED) {
                    w.string(name);
                }
            }
            Self::Chunk(data) => {
                w.u8(CHUNK_TAG);
                w.byte_array(data);
            }
            Self::Refused(why) => {
                w.u8(REFUSED);
                w.u8(match why {
                    Refusal::Denied => 0,
                    Refusal::AlreadyRunning => 1,
                    Refusal::NotRunning => 2,
                    Refusal::Failed => 3,
                });
            }
        }
        w.data
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let reply = match r.u8()? {
            STARTED => {
                let path = r.string(MAX_PATH)?;
                let seconds = r.varint()?;
                Self::Started {
                    path,
                    seconds: u32::try_from(seconds).ok().filter(|s| *s > 0),
                    samples: r.bool()?,
                }
            }
            FINISHED => {
                let path = r.string(MAX_PATH)?;
                let bytes = u64::try_from(r.varlong()?)
                    .map_err(|_| Error::new("perf: negative file size"))?;
                let removed = (0..r.count(MAX_REMOVED)?)
                    .map(|_| r.string(MAX_PATH))
                    .collect::<Result<_>>()?;
                Self::Finished {
                    path,
                    removed,
                    bytes,
                }
            }
            CHUNK_TAG => Self::Chunk(r.byte_array(CHUNK)?),
            REFUSED => Self::Refused(match r.u8()? {
                0 => Refusal::Denied,
                1 => Refusal::AlreadyRunning,
                2 => Refusal::NotRunning,
                3 => Refusal::Failed,
                other => return Err(Error::new(format!("perf: refusal {other}"))),
            }),
            other => return Err(Error::new(format!("perf: reply {other}"))),
        };
        if r.remaining() > 0 {
            return Err(Error::new("perf: trailing bytes"));
        }
        Ok(reply)
    }
}

/// Who the capture a guest asked of this server belongs to, as the asker sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    Nothing,
    /// The asker's own capture runs.
    Asker,
    /// Another guest's capture runs or waits to begin.
    Someone,
}

/// What the server does with a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not from a Horizon client: no answer.
    Ignore,
    Refuse(Refusal),
    Accept,
}

/// The server's rules for a guest's request: Horizon clients only, operator level 4, one capture at a time, and only its owner stops it.
pub fn judge(brand: &str, permission: u8, request: &Request, held: Held) -> Verdict {
    if !is_horizon(brand) {
        return Verdict::Ignore;
    }
    if permission < OPERATOR_LEVEL {
        return Verdict::Refuse(Refusal::Denied);
    }
    match (request, held) {
        (Request::Start(_), Held::Nothing) | (Request::Stop, Held::Asker) => Verdict::Accept,
        (Request::Start(_), _) => Verdict::Refuse(Refusal::AlreadyRunning),
        (Request::Stop, _) => Verdict::Refuse(Refusal::NotRunning),
    }
}

/// Whether a server calls itself Horizon.
pub fn is_horizon(brand: &str) -> bool {
    brand == HORIZON_BRAND
}

/// Why a client could not send a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsent {
    NotHorizon,
    /// A request is unanswered, or a server capture of ours runs.
    Busy,
    /// No capture of ours runs on the server.
    NothingToStop,
}

/// Where this client's request to the connected server has got.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Link {
    #[default]
    Idle,
    /// The start request waits for its answer.
    Asking { since: Instant },
    Running {
        path: String,
        since: Instant,
        seconds: Option<u32>,
    },
    /// The stop request waits for the file to be whole.
    Stopping { path: String, since: Instant },
    /// The server's file arrives: `got` of `total` bytes, the last at `since`.
    Receiving {
        path: String,
        since: Instant,
        got: u64,
        total: u64,
    },
}

/// What a chunk of the server's file means for the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Received {
    /// No file was expected.
    Stray,
    More,
    /// The file is whole.
    Last,
    /// More bytes arrived than the server announced.
    TooMuch,
}

/// The server's pacing of the file it sends: chunks of at most [`CHUNK`], never faster than [`RATE`], and none while the guest's connection has a backlog.
#[derive(Clone, Debug)]
pub struct Outgoing {
    remaining: u64,
    next_at: Instant,
}

impl Outgoing {
    pub fn new(total: u64, now: Instant) -> Self {
        Self {
            remaining: total,
            next_at: now,
        }
    }

    pub fn done(&self) -> bool {
        self.remaining == 0
    }

    /// The size of the chunk to send at `now`, if one is due; `queued` is the bytes already waiting to be sent to the guest.
    pub fn next(&mut self, now: Instant, queued: usize) -> Option<usize> {
        if self.remaining == 0 || now < self.next_at || queued > BUSY_QUEUE {
            return None;
        }
        let n = self.remaining.min(CHUNK as u64) as usize;
        self.remaining -= n as u64;
        self.next_at = now + Duration::from_secs_f64(n as f64 / RATE as f64);
        Some(n)
    }
}

/// What an answer from the server means for the client.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Started {
        path: String,
        seconds: Option<u32>,
        samples: bool,
    },
    Finished {
        path: String,
        removed: Vec<String>,
        /// Bytes of the server's file to come; 0 when the server sends none.
        bytes: u64,
    },
    Refused(Refusal),
    /// The server announced a file over [`MAX_PART_BYTES`]; none of it is taken.
    TooLarge {
        bytes: u64,
    },
    /// The reply fits no request of ours, for example a duplicate.
    Stray,
}

impl Link {
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Whether the server runs a capture for this client, or is about to.
    pub fn running(&self) -> bool {
        matches!(self, Self::Running { .. } | Self::Stopping { .. })
    }

    /// Whether a start is allowed against a server of `brand`; the link waits for the answer.
    pub fn ask(&mut self, brand: &str, now: Instant) -> std::result::Result<(), Unsent> {
        if !is_horizon(brand) {
            return Err(Unsent::NotHorizon);
        }
        if !self.is_idle() {
            return Err(Unsent::Busy);
        }
        *self = Self::Asking { since: now };
        Ok(())
    }

    /// Whether a stop can be sent; the link waits for the file.
    pub fn stop(&mut self, now: Instant) -> std::result::Result<(), Unsent> {
        match std::mem::take(self) {
            Self::Running { path, .. } => {
                *self = Self::Stopping { path, since: now };
                Ok(())
            }
            other => {
                *self = other;
                Err(Unsent::NothingToStop)
            }
        }
    }

    /// The server file's name and the seconds since the capture began, while one runs.
    pub fn status(&self, now: Instant) -> Option<(&str, f64)> {
        match self {
            Self::Running { path, since, .. } | Self::Stopping { path, since } => {
                Some((path, now.duration_since(*since).as_secs_f64()))
            }
            _ => None,
        }
    }

    pub fn answer(&mut self, reply: Reply, now: Instant) -> Outcome {
        match (std::mem::take(self), reply) {
            (
                Self::Asking { .. },
                Reply::Started {
                    path,
                    seconds,
                    samples,
                },
            ) => {
                *self = Self::Running {
                    path: path.clone(),
                    since: now,
                    seconds,
                };
                Outcome::Started {
                    path,
                    seconds,
                    samples,
                }
            }
            (
                Self::Running { .. } | Self::Stopping { .. },
                Reply::Finished {
                    path,
                    removed,
                    bytes,
                },
            ) => {
                if bytes > MAX_PART_BYTES {
                    return Outcome::TooLarge { bytes };
                }
                if bytes > 0 {
                    *self = Self::Receiving {
                        path: path.clone(),
                        since: now,
                        got: 0,
                        total: bytes,
                    };
                }
                Outcome::Finished {
                    path,
                    removed,
                    bytes,
                }
            }
            (Self::Receiving { .. }, Reply::Refused(why)) => Outcome::Refused(why),
            (Self::Asking { .. }, Reply::Refused(why)) => Outcome::Refused(why),
            (Self::Running { .. } | Self::Stopping { .. }, Reply::Refused(why)) => {
                Outcome::Refused(why)
            }
            (before, _) => {
                *self = before;
                Outcome::Stray
            }
        }
    }

    /// Counts `len` bytes of the server's file that arrived.
    pub fn chunk(&mut self, len: usize, now: Instant) -> Received {
        match std::mem::take(self) {
            Self::Receiving {
                path, got, total, ..
            } => {
                let got = got + len as u64;
                if got < total {
                    *self = Self::Receiving {
                        path,
                        since: now,
                        got,
                        total,
                    };
                    Received::More
                } else if got == total {
                    Received::Last
                } else {
                    Received::TooMuch
                }
            }
            other => {
                *self = other;
                Received::Stray
            }
        }
    }

    /// Whether the server's file has been awaited or received too slowly for longer than [`STALL_TIMEOUT`]; the link goes idle.
    pub fn stalled(&mut self, now: Instant) -> bool {
        match self {
            Self::Stopping { since, .. } | Self::Receiving { since, .. }
                if now.duration_since(*since) > STALL_TIMEOUT =>
            {
                *self = Self::Idle;
                true
            }
            _ => false,
        }
    }

    /// Whether the link has waited longer than [`ANSWER_TIMEOUT`] for a start's answer; it goes idle.
    pub fn timed_out(&mut self, now: Instant) -> bool {
        match self {
            Self::Asking { since } if now.duration_since(*since) > ANSWER_TIMEOUT => {
                *self = Self::Idle;
                true
            }
            _ => false,
        }
    }

    /// The connection ended: whatever waited will not be answered.
    pub fn reset(&mut self) -> Self {
        std::mem::take(self)
    }
}

#[cfg(test)]
mod tests;
