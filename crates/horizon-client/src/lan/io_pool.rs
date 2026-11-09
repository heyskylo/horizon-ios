//! The network threads of the play state: a fixed pool, one thread per two
//! cores, each polling its players' sockets (epoll, kqueue or Windows' AFD,
//! through mio), where every player used to have a reader and a writer thread
//! of its own. The game thread only queues packet bodies in a player's
//! [`Outbox`] (over the network, bounded by [`MAX_PENDING`] bytes); framing, compression,
//! encryption, the socket writes, reading and the keep-alives happen here. A
//! connection stays on one thread, so its bytes keep their order. The
//! integrated server's own player, over an in-memory pipe, is served the same
//! way: the pipe tells the thread when bytes arrive.
use horizon_protocol::connection::{ConnReader, ConnWriter, Pipe, Socket};
use mio::{Events, Interest, Poll, Registry, Token, Waker};
use std::{
    io::{ErrorKind, Read, Write},
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// Packets queued for one client beyond this many bytes disconnect it.
pub(super) const MAX_PENDING: usize = 96 << 20;
/// How often a thread looks at its connections' clocks: keep-alives,
/// timeouts and the closes that wait for a disconnect's last bytes.
const SWEEP: Duration = Duration::from_millis(100);
/// Framed bytes a connection may hold unwritten before its thread stops
/// taking more from its outbox: the rest wait there, counted in `pending`, so
/// that a client which reads slowly has the hub hold its far chunks back
/// (`lan_far`'s backlog) and, past [`MAX_PENDING`], is disconnected.
const AHEAD: usize = 256 << 10;
/// How long a disconnect's last bytes are given before the socket closes.
const LINGER: Duration = Duration::from_millis(50);
const WAKER: Token = Token(usize::MAX);
/// The packets a connection keeps for the game while counting, each way: the game takes them
/// every tick ([`Outbox::take_packets`]), so only a tick stalled for long reaches it.
const TALLY_MAX: usize = 1 << 16;

/// Whether the network threads keep each play packet's id and size for the game
/// (`native_server_packets`): off unless a mod asks, and off a packet costs one relaxed load.
static COUNTING: AtomicBool = AtomicBool::new(false);

/// Start or stop keeping the play packets' ids and sizes.
pub(super) fn count_packets(on: bool) {
    COUNTING.store(on, Ordering::Relaxed);
}
/// Whether the play packets' ids and sizes are kept.
#[inline]
pub(super) fn counting_packets() -> bool {
    COUNTING.load(Ordering::Relaxed)
}
/// The VarInt packet id a body starts with.
fn body_id(body: &[u8]) -> Option<i32> {
    let mut value = 0u32;
    for (i, &b) in body.iter().take(5).enumerate() {
        value |= u32::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Some(value as i32);
        }
    }
    None
}
/// The bytes of `id` as a VarInt.
fn id_len(id: i32) -> usize {
    let mut v = id as u32;
    let mut len = 1;
    while v >= 0x80 {
        v >>= 7;
        len += 1;
    }
    len
}

/// The play state's side of a connection (conn.rs's `PlayConn`).
pub(super) trait Session: Send {
    /// A packet arrived.
    fn packet(&mut self, id: i32, payload: Vec<u8>) -> Verdict;
    /// A packet to send of its own accord now (a keep-alive).
    fn tick(&mut self, now: Instant) -> Option<Vec<u8>>;
    /// The connection is ending, for this reason: no packet arrives after it.
    fn left(&mut self, reason: String);
    /// The connection is closed, after [`Self::left`].
    fn closed(&mut self);
}

/// What becomes of a connection after one of its packets.
pub(super) enum Verdict {
    Go,
    /// Close it now, for this reason.
    Close(String),
    /// Send this body (a disconnect) after what is queued, then close, for this reason.
    Kick(String, Vec<u8>),
}

/// How an outbox ends, after the bodies queued before.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum End {
    /// Write everything queued (a disconnect last), then close.
    Disconnect,
    /// Close now: what is queued is dropped.
    Close,
}

/// Where an outbox's connection is.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum At {
    /// Not on its thread yet: the thread takes what waits when it arrives.
    #[default]
    Coming,
    Slot(usize),
    Closed,
}

#[derive(Default)]
struct Queue {
    bodies: Vec<Vec<u8>>,
    end: Option<End>,
    at: At,
    /// In its thread's list of outboxes to serve.
    listed: bool,
}

/// One player's packets on their way to its network thread.
pub(super) struct Outbox {
    queue: Mutex<Queue>,
    /// The bytes queued and not framed yet (`PlayerSender::pending`).
    pending: Arc<AtomicUsize>,
    /// Whether [`MAX_PENDING`] applies. A client in this process (a memory connection) reads
    /// what it is sent as it comes, and a join's first view can pass the limit in one burst.
    capped: bool,
    worker: Arc<Worker>,
    /// While counting: each play packet framed (sent) and read (received) since the game last
    /// took them, as (id, body bytes).
    tally: Mutex<[Vec<(i32, usize)>; 2]>,
}
impl Outbox {
    /// An outbox on the least busy network thread, started the first time.
    pub(super) fn new(pending: Arc<AtomicUsize>, capped: bool) -> Result<Arc<Self>, String> {
        let workers = &pool()?.workers;
        let worker = workers
            .iter()
            .min_by_key(|w| w.load.load(Ordering::Relaxed))
            .expect("a pool has a thread");
        Ok(Arc::new(Self {
            queue: Mutex::default(),
            pending,
            capped,
            worker: worker.clone(),
            tally: Mutex::default(),
        }))
    }
    /// Keep `packets` going `way` (0 sent, 1 received), up to [`TALLY_MAX`].
    fn tally(&self, way: usize, packets: impl IntoIterator<Item = (i32, usize)>) {
        let mut tally = self.tally.lock().unwrap_or_else(PoisonError::into_inner);
        let list = &mut tally[way];
        let room = TALLY_MAX.saturating_sub(list.len());
        list.extend(packets.into_iter().take(room));
    }
    /// The play packets kept since the last call, sent then received, as (id, body bytes):
    /// the id before an older client's renumbering, the bytes before compression and
    /// encryption.
    pub(super) fn take_packets(&self) -> [Vec<(i32, usize)>; 2] {
        std::mem::take(&mut *self.tally.lock().unwrap_or_else(PoisonError::into_inner))
    }
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// Queue `bodies` (already counted in `pending`), then `end`. Nothing
    /// follows an end, but a close overrides a disconnect still writing.
    /// Past [`MAX_PENDING`] a capped connection closes. The thread to wake, if
    /// this is the first news it has since it last looked.
    pub(super) fn push(&self, bodies: Vec<Vec<u8>>, end: Option<End>) -> Option<&Arc<Worker>> {
        let mut q = self.queue();
        match (q.at, q.end) {
            (At::Closed, _) => return None,
            (_, None) => {
                if q.bodies.is_empty() {
                    q.bodies = bodies;
                } else {
                    q.bodies.extend(bodies);
                }
                q.end = if self.capped && self.pending.load(Ordering::Relaxed) > MAX_PENDING {
                    Some(End::Close)
                } else {
                    end
                };
            }
            (_, Some(End::Disconnect)) if end == Some(End::Close) => q.end = end,
            (_, Some(_)) => return None,
        }
        let At::Slot(slot) = q.at else {
            return None;
        };
        if std::mem::replace(&mut q.listed, true) {
            return None;
        }
        drop(q);
        self.worker
            .post(|i| i.serve.push(slot))
            .then_some(&self.worker)
    }
    /// [`Self::push`], waking the thread at once.
    pub(super) fn send(&self, bodies: Vec<Vec<u8>>, end: Option<End>) {
        if let Some(worker) = self.push(bodies, end) {
            worker.wake();
        }
    }
    /// What waits, for the thread.
    /// What waits, for the thread: the bodies and the end after them with
    /// `room` for them, else only a close. An end stays, so that nothing is
    /// queued after it.
    fn take(&self, room: bool) -> (Vec<Vec<u8>>, Option<End>) {
        let mut q = self.queue();
        q.listed = false;
        match room {
            true => (std::mem::take(&mut q.bodies), q.end),
            false => (Vec::new(), q.end.filter(|e| *e == End::Close)),
        }
    }
    fn attach(&self, slot: usize) {
        let mut q = self.queue();
        q.at = At::Slot(slot);
        q.listed = false;
    }
    fn closed(&self) {
        let mut q = self.queue();
        q.at = At::Closed;
        q.bodies = Vec::new();
        q.end = None;
    }
}

/// What a network thread is told between polls.
#[derive(Default)]
struct Inbox {
    /// Connections with something in their outbox.
    serve: Vec<usize>,
    /// Pipes with bytes to read.
    read: Vec<usize>,
    adopt: Vec<Adopted>,
}

/// One network thread, as the others see it.
pub(super) struct Worker {
    waker: Waker,
    inbox: Mutex<Inbox>,
    /// Its connections, to give a new one to the least busy.
    load: AtomicUsize,
}
impl Worker {
    /// Change the inbox: whether the thread needs waking for it (it was empty).
    fn post(&self, f: impl FnOnce(&mut Inbox)) -> bool {
        let mut inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        let empty = inbox.serve.is_empty() && inbox.read.is_empty() && inbox.adopt.is_empty();
        f(&mut inbox);
        empty
    }
    pub(super) fn wake(&self) {
        if let Err(e) = self.waker.wake() {
            log::warn!("[lan] waking a network thread: {e}");
        }
    }
}

struct Pool {
    workers: Vec<Arc<Worker>>,
}

/// The pool, started the first time: one thread per two cores.
fn pool() -> Result<&'static Pool, String> {
    static POOL: OnceLock<Result<Pool, String>> = OnceLock::new();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().div_ceil(2));
    POOL.get_or_init(|| start(threads).map_err(|e| format!("network threads: {e}")))
        .as_ref()
        .map_err(Clone::clone)
}

fn start(threads: usize) -> std::io::Result<Pool> {
    let mut workers = Vec::new();
    for n in 0..threads {
        let poll = Poll::new()?;
        let worker = Arc::new(Worker {
            waker: Waker::new(poll.registry(), WAKER)?,
            inbox: Mutex::default(),
            load: AtomicUsize::new(0),
        });
        let mine = worker.clone();
        std::thread::Builder::new()
            .name(format!("horizon-net-{n}"))
            .spawn(move || run(poll, &mine))?;
        workers.push(worker);
    }
    Ok(Pool { workers })
}

/// A connection handed to its thread.
struct Adopted {
    socket: Socket,
    reader: ConnReader,
    writer: ConnWriter,
    outbox: Arc<Outbox>,
    session: Box<dyn Session>,
    timeout: Duration,
}

/// Serve a connection from now on, on its outbox's thread: `timeout` is how
/// long it may read nothing, or have a write wait.
pub(super) fn adopt(
    socket: Socket,
    (reader, writer): (ConnReader, ConnWriter),
    outbox: Arc<Outbox>,
    session: Box<dyn Session>,
    timeout: Duration,
) {
    let worker = outbox.worker.clone();
    worker.load.fetch_add(1, Ordering::Relaxed);
    let adopted = Adopted {
        socket,
        reader,
        writer,
        outbox,
        session,
        timeout,
    };
    if worker.post(|i| i.adopt.push(adopted)) {
        worker.wake();
    }
}

fn run(mut poll: Poll, worker: &Arc<Worker>) {
    let mut events = Events::with_capacity(1024);
    let mut conns: Vec<Option<Conn>> = Vec::new();
    let mut free: Vec<usize> = Vec::new();
    let mut touched: Vec<usize> = Vec::new();
    let mut sweep_at = Instant::now() + SWEEP;
    loop {
        let wait = sweep_at.saturating_duration_since(Instant::now());
        if let Err(e) = poll.poll(&mut events, Some(wait))
            && e.kind() != ErrorKind::Interrupted
        {
            log::warn!("[lan] network poll: {e}");
            std::thread::sleep(SWEEP);
        }
        let now = Instant::now();
        for event in &events {
            let i = event.token().0;
            let Some(Some(c)) = conns.get_mut(i) else {
                continue;
            };
            if event.is_readable() || event.is_read_closed() || event.is_error() {
                c.read(now);
            }
            if event.is_writable() || event.is_write_closed() || event.is_error() {
                c.serve(now);
            }
            touched.push(i);
        }
        let inbox =
            std::mem::take(&mut *worker.inbox.lock().unwrap_or_else(PoisonError::into_inner));
        for adopted in inbox.adopt {
            let i = free.pop().unwrap_or(conns.len());
            if i == conns.len() {
                conns.push(None);
            }
            match Conn::register(adopted, poll.registry(), worker, i, now) {
                Ok(mut c) => {
                    c.outbox.attach(i);
                    c.read(now);
                    c.serve(now);
                    conns[i] = Some(c);
                    touched.push(i);
                }
                Err(()) => free.push(i),
            }
        }
        for i in inbox.read {
            if let Some(Some(c)) = conns.get_mut(i) {
                c.read(now);
                touched.push(i);
            }
        }
        for i in inbox.serve {
            if let Some(Some(c)) = conns.get_mut(i) {
                c.serve(now);
                touched.push(i);
            }
        }
        if now >= sweep_at {
            sweep_at = now + SWEEP;
            touched.clear();
            for (i, slot) in conns.iter_mut().enumerate() {
                if let Some(c) = slot {
                    c.sweep(now);
                    touched.push(i);
                }
            }
        }
        for i in touched.drain(..) {
            if conns[i].as_ref().is_some_and(|c| c.done(now)) {
                let c = conns[i].take().expect("checked");
                c.close(poll.registry(), worker);
                free.push(i);
            }
        }
    }
}

/// A connection's stream, polled.
enum Io {
    Tcp(mio::net::TcpStream),
    Memory(Pipe),
}
impl Read for Io {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.read(out),
            Self::Memory(p) => p.read(out),
        }
    }
}
impl Write for Io {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.write(data),
            Self::Memory(p) => p.write(data),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Why a read error ends a connection: a client that closed it is
/// Connection.channelInactive without details, multiplayer.disconnect.generic.
fn read_error(e: std::io::Error) -> String {
    match e.kind() {
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => "Disconnected".to_owned(),
        _ => horizon_protocol::Error::from(e).to_string(),
    }
}

/// A connection on its thread.
struct Conn {
    io: Io,
    reader: ConnReader,
    writer: ConnWriter,
    /// Framed bytes, written from `written` on.
    out: Vec<u8>,
    written: usize,
    outbox: Arc<Outbox>,
    session: Box<dyn Session>,
    timeout: Duration,
    last_read: Instant,
    /// Since when `out` has waited on a client that reads nothing.
    stalled: Option<Instant>,
    /// [`Session::left`] was called: what arrives now is dropped.
    left: bool,
    /// A disconnect is in `out`: nothing more is taken from the outbox.
    draining: bool,
    close_at: Option<Instant>,
}
impl Conn {
    fn register(
        a: Adopted,
        registry: &Registry,
        worker: &Arc<Worker>,
        i: usize,
        now: Instant,
    ) -> Result<Self, ()> {
        let io = match a.socket {
            Socket::Tcp(stream) => {
                let registered = stream.set_nonblocking(true).and_then(|()| {
                    let mut stream = mio::net::TcpStream::from_std(stream);
                    let interest = Interest::READABLE | Interest::WRITABLE;
                    registry.register(&mut stream, Token(i), interest)?;
                    Ok(stream)
                });
                match registered {
                    Ok(stream) => Io::Tcp(stream),
                    Err(e) => {
                        let mut session = a.session;
                        a.outbox.closed();
                        session.left(e.to_string());
                        session.closed();
                        worker.load.fetch_sub(1, Ordering::Relaxed);
                        return Err(());
                    }
                }
            }
            Socket::Memory(mut pipe) => {
                pipe.set_nonblocking(true);
                let worker = worker.clone();
                pipe.on_ready(move || {
                    if worker.post(|inbox| inbox.read.push(i)) {
                        worker.wake();
                    }
                });
                Io::Memory(pipe)
            }
        };
        Ok(Self {
            io,
            reader: a.reader,
            writer: a.writer,
            out: Vec::new(),
            written: 0,
            outbox: a.outbox,
            session: a.session,
            timeout: a.timeout,
            last_read: now,
            stalled: None,
            left: false,
            draining: false,
            close_at: None,
        })
    }

    fn left(&mut self, reason: String) {
        if !std::mem::replace(&mut self.left, true) {
            self.session.left(reason);
        }
    }
    /// Close now.
    fn end(&mut self, now: Instant, reason: impl Into<String>) {
        self.left(reason.into());
        self.close_at = Some(now);
    }
    fn done(&self, now: Instant) -> bool {
        self.close_at.is_some_and(|at| now >= at)
    }

    /// Read and hand over every packet that arrived.
    fn read(&mut self, now: Instant) {
        loop {
            match self.reader.buffered() {
                Ok(Some((id, payload))) => {
                    if !self.left {
                        if counting_packets() {
                            self.outbox.tally(1, [(id, id_len(id) + payload.len())]);
                        }
                        match self.session.packet(id, payload) {
                            Verdict::Go => {}
                            Verdict::Close(reason) => return self.end(now, reason),
                            Verdict::Kick(reason, body) => self.kick(now, reason, &body),
                        }
                    }
                    continue;
                }
                Ok(None) => {}
                Err(e) => return self.end(now, e.to_string()),
            }
            if self.close_at.is_some() {
                return;
            }
            match self.reader.read_from(&mut self.io) {
                Ok(0) => return self.end(now, "Disconnected"),
                Ok(_) => self.last_read = now,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return self.end(now, read_error(e)),
            }
        }
    }

    /// Frame `bodies` into `out`, `queued` ones counted off `pending`: false
    /// if one could not be.
    fn frame(&mut self, now: Instant, bodies: Vec<Vec<u8>>, queued: bool) -> bool {
        if counting_packets() {
            let ids = bodies.iter().filter_map(|b| Some((body_id(b)?, b.len())));
            self.outbox.tally(0, ids);
        }
        for body in bodies {
            if queued {
                self.outbox.pending.fetch_sub(body.len(), Ordering::Relaxed);
            }
            if let Err(e) = super::conn::frame_body(&mut self.writer, &body, &mut self.out) {
                self.end(now, e.to_string());
                return false;
            }
        }
        true
    }

    /// Send `disconnect` after everything queued, then close.
    fn kick(&mut self, now: Instant, reason: String, disconnect: &[u8]) {
        self.left(reason);
        if !self.draining {
            let (bodies, end) = self.outbox.take(true);
            if end == Some(End::Close) {
                return self.end(now, "Disconnected");
            }
            // The hub's own disconnect, if it was the last queued, does for this one.
            let ours = (end.is_none()).then(|| disconnect.to_vec());
            if !self.frame(now, bodies, true) || !self.frame(now, ours.into_iter().collect(), false)
            {
                return;
            }
            self.draining = true;
        }
        self.serve(now);
    }

    /// Write what is framed, and frame what the outbox holds, while the
    /// client takes it.
    fn serve(&mut self, now: Instant) {
        loop {
            if !self.flush(now) || self.close_at.is_some() {
                return;
            }
            // A close is seen even while the client takes nothing.
            let room = !self.draining && self.out.len() - self.written <= AHEAD;
            let (bodies, end) = self.outbox.take(room);
            if end == Some(End::Close) {
                return self.end(now, "Disconnected");
            }
            if bodies.is_empty() {
                break;
            }
            if !self.frame(now, bodies, true) {
                return;
            }
            self.draining = end == Some(End::Disconnect);
        }
        if self.draining && self.written == self.out.len() && self.close_at.is_none() {
            self.close_at = Some(now + LINGER);
        }
    }

    /// Write what is framed until the client takes no more: false once the
    /// connection failed.
    fn flush(&mut self, now: Instant) -> bool {
        if self.close_at.is_some_and(|at| at <= now) {
            return false;
        }
        while self.written < self.out.len() {
            match self.io.write(&self.out[self.written..]) {
                Ok(0) => {
                    self.end(now, "Disconnected");
                    return false;
                }
                Ok(n) => {
                    self.written += n;
                    self.stalled = None;
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    self.stalled.get_or_insert(now);
                    return true;
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => {
                    self.end(now, "Disconnected");
                    return false;
                }
            }
        }
        self.out.clear();
        self.written = 0;
        self.stalled = None;
        if self.out.capacity() > 4 * AHEAD {
            self.out.shrink_to(AHEAD);
        }
        true
    }

    /// The connection's clocks: a keep-alive due, a client that sent nothing
    /// or took nothing for too long.
    fn sweep(&mut self, now: Instant) {
        if self.close_at.is_some() {
            return;
        }
        if self
            .stalled
            .is_some_and(|t| now.duration_since(t) > self.timeout)
        {
            return self.end(now, "Disconnected");
        }
        if !self.left && now.duration_since(self.last_read) > self.timeout {
            return self.end(now, horizon_protocol::Error::timed_out().to_string());
        }
        if !self.draining
            && let Some(body) = self.session.tick(now)
            && self.frame(now, vec![body], false)
        {
            self.serve(now);
        }
    }

    fn close(mut self, registry: &Registry, worker: &Worker) {
        match &mut self.io {
            Io::Tcp(stream) => {
                let _ = registry.deregister(stream);
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            Io::Memory(pipe) => {
                pipe.on_ready(|| {});
                pipe.shutdown();
            }
        }
        self.outbox.closed();
        self.left("Disconnected".to_owned());
        self.session.closed();
        worker.load.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol::connection::Connection;
    use std::sync::mpsc::{Receiver, Sender, channel};

    #[derive(Debug, PartialEq)]
    enum Seen {
        Packet(i32, Vec<u8>),
        Left(String),
        Closed,
    }
    struct Probe {
        seen: Sender<Seen>,
        /// The packet id answered with a kick.
        kick_on: Option<i32>,
        /// Sent at the first sweep.
        unprompted: Option<Vec<u8>>,
    }
    impl Session for Probe {
        fn packet(&mut self, id: i32, payload: Vec<u8>) -> Verdict {
            let _ = self.seen.send(Seen::Packet(id, payload));
            if self.kick_on == Some(id) {
                Verdict::Kick("kicked".into(), body(0x7f, 3))
            } else {
                Verdict::Go
            }
        }
        fn tick(&mut self, _: Instant) -> Option<Vec<u8>> {
            self.unprompted.take()
        }
        fn left(&mut self, reason: String) {
            let _ = self.seen.send(Seen::Left(reason));
        }
        fn closed(&mut self) {
            let _ = self.seen.send(Seen::Closed);
        }
    }

    /// A packet body: a one-byte id, then `len` bytes.
    fn body(id: u8, len: usize) -> Vec<u8> {
        std::iter::once(id)
            .chain((0..len).map(|j| (j * 31 + usize::from(id)) as u8))
            .collect()
    }
    fn packet(body: &[u8]) -> (i32, Vec<u8>) {
        (i32::from(body[0]), body[1..].to_vec())
    }

    struct Served {
        outbox: Arc<Outbox>,
        pending: Arc<AtomicUsize>,
        seen: Receiver<Seen>,
    }
    impl Served {
        fn send(&self, bodies: Vec<Vec<u8>>, end: Option<End>) {
            let len: usize = bodies.iter().map(Vec::len).sum();
            self.pending.fetch_add(len, Ordering::Relaxed);
            self.outbox.send(bodies, end);
        }
        fn next(&self) -> Seen {
            self.seen.recv_timeout(Duration::from_secs(10)).unwrap()
        }
    }
    fn serve(
        server: Connection,
        probe: impl FnOnce(Sender<Seen>) -> Probe,
        timeout: Duration,
    ) -> Served {
        serve_with(server, probe, timeout, true)
    }
    fn serve_with(
        server: Connection,
        probe: impl FnOnce(Sender<Seen>) -> Probe,
        timeout: Duration,
        capped: bool,
    ) -> Served {
        let pending = Arc::new(AtomicUsize::new(0));
        let outbox = Outbox::new(pending.clone(), capped).unwrap();
        let (tx, seen) = channel();
        let (socket, reader, writer) = server.detach();
        adopt(
            socket,
            (reader, writer),
            outbox.clone(),
            Box::new(probe(tx)),
            timeout,
        );
        Served {
            outbox,
            pending,
            seen,
        }
    }
    fn quiet(seen: Sender<Seen>) -> Probe {
        Probe {
            seen,
            kick_on: None,
            unprompted: None,
        }
    }

    /// A body's VarInt id reads back, a truncated one doesn't, and the sizes match.
    #[test]
    fn packet_ids_read_from_bodies() {
        for id in [0, 1, 0x7f, 0x80, 300, 0x3fff, 0x4000] {
            let mut body = Vec::new();
            let mut v = id as u32;
            loop {
                let b = (v & 0x7f) as u8;
                v >>= 7;
                if v == 0 {
                    body.push(b);
                    break;
                }
                body.push(b | 0x80);
            }
            assert_eq!(id_len(id), body.len(), "{id}");
            body.push(9);
            assert_eq!(body_id(&body), Some(id));
        }
        assert_eq!(body_id(&[0x80]), None);
        assert_eq!(body_id(&[]), None);
    }

    /// The tally keeps each way apart, stops at its cap and empties when taken.
    #[test]
    fn the_tally_keeps_both_ways_up_to_its_cap() {
        let outbox = Outbox::new(Arc::new(AtomicUsize::new(0)), true).unwrap();
        outbox.tally(0, [(0x27, 10), (0x08, 3)]);
        outbox.tally(1, [(0x1a, 5)]);
        assert_eq!(
            outbox.take_packets(),
            [vec![(0x27, 10), (0x08, 3)], vec![(0x1a, 5)]]
        );
        assert_eq!(outbox.take_packets(), [vec![], vec![]]);
        outbox.tally(0, (0..TALLY_MAX + 5).map(|_| (1, 1)));
        assert_eq!(outbox.take_packets()[0].len(), TALLY_MAX);
    }

    #[test]
    fn a_socket_carries_packets_both_ways_in_order_then_disconnects() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let mut server = Connection::from_stream(listener.accept().unwrap().0).unwrap();
        for c in [&mut client, &mut server] {
            c.set_compression(64);
            c.enable_encryption(&[5; 16]);
        }
        // The second arrives with the first, before the pool has the connection.
        client.send(&body(1, 10)).unwrap();
        client.send(&body(2, 10)).unwrap();
        assert_eq!(server.recv().unwrap(), packet(&body(1, 10)));
        let served = serve(server, quiet, Duration::from_secs(30));
        assert_eq!(served.next(), Seen::Packet(2, body(2, 10)[1..].to_vec()));
        // Small, compressed and shared (past 8 KiB) frames.
        let bodies: Vec<Vec<u8>> = (0..60)
            .map(|i| body(i as u8 + 3, [0, 63, 64, 300, 9000, 70_000][i % 6]))
            .collect();
        served.send(bodies.clone(), None);
        for b in &bodies {
            assert_eq!(client.recv().unwrap(), packet(b));
        }
        for i in 0..20u8 {
            client.send(&body(i + 3, 50)).unwrap();
        }
        for i in 0..20u8 {
            assert_eq!(
                served.next(),
                Seen::Packet(i32::from(i + 3), body(i + 3, 50)[1..].to_vec())
            );
        }
        assert_eq!(served.pending.load(Ordering::Relaxed), 0);
        served.send(vec![body(0x1d, 5)], Some(End::Disconnect));
        assert_eq!(client.recv().unwrap(), packet(&body(0x1d, 5)));
        assert!(client.recv().unwrap_err().is_end_of_stream());
        assert_eq!(served.next(), Seen::Left("Disconnected".into()));
        assert_eq!(served.next(), Seen::Closed);
    }

    #[test]
    fn a_kick_goes_after_what_was_queued_and_ends_the_packets() {
        let (a, b) = Pipe::pair();
        let mut client = Connection::local(a);
        let probe = |seen| Probe {
            seen,
            kick_on: Some(9),
            unprompted: Some(body(0x21, 2)),
        };
        let served = serve(Connection::local(b), probe, Duration::from_secs(30));
        served.send(vec![body(3, 10)], None);
        assert_eq!(client.recv().unwrap(), packet(&body(3, 10)));
        // What the session sends of its own accord.
        assert_eq!(client.recv().unwrap(), packet(&body(0x21, 2)));
        client.send(&body(9, 1)).unwrap();
        client.send(&body(10, 1)).unwrap();
        assert_eq!(client.recv().unwrap(), packet(&body(0x7f, 3)));
        assert!(client.recv().is_err());
        assert_eq!(served.next(), Seen::Packet(9, body(9, 1)[1..].to_vec()));
        assert_eq!(served.next(), Seen::Left("kicked".into()));
        assert_eq!(served.next(), Seen::Closed);
        // Nothing after the kick, and nothing queued once closed.
        assert!(served.seen.try_recv().is_err());
        assert!(served.outbox.push(vec![body(4, 1)], None).is_none());
    }

    #[test]
    fn a_silent_client_times_out() {
        let (a, b) = Pipe::pair();
        let mut client = Connection::local(a);
        let served = serve(Connection::local(b), quiet, Duration::from_millis(300));
        assert_eq!(served.next(), Seen::Left("Timed out".into()));
        assert_eq!(served.next(), Seen::Closed);
        assert!(client.recv().is_err());
    }

    /// A client that reads nothing holds its disconnect unwritten; a close
    /// then ends the connection at once.
    #[test]
    fn a_close_ends_a_disconnect_the_client_does_not_take() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let mut socket = listener.accept().unwrap().0;
        socket.set_nonblocking(true).unwrap();
        // Windows may buffer an entire large first write. Establish actual backpressure with
        // small framed writes before adoption, rather than assuming the batch below fills it.
        let fill = horizon_protocol::frame::encode(&body(0, 65535), None).unwrap();
        let mut filled = 0;
        let mut offset = 0;
        let fill_until = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < fill_until, "socket prefill timed out");
            match socket.write(&fill[offset..]) {
                Ok(n) => {
                    assert!(n > 0, "socket closed during prefill");
                    filled += n;
                    assert!(filled < MAX_PENDING, "socket did not apply backpressure");
                    offset = (offset + n) % fill.len();
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => panic!("socket prefill: {e}"),
            }
        }
        socket.set_nonblocking(false).unwrap();
        let server = Connection::from_stream(socket).unwrap();
        let served = serve(server, quiet, Duration::from_secs(30));
        // Fill the socket until the outbox waits. A fixed amount may all go
        // through: Windows takes a write of any size whole while its buffer
        // has room.
        let full = || {
            std::thread::sleep(Duration::from_millis(50));
            served.pending.load(Ordering::Relaxed) > 0
        };
        for i in 0..=u8::MAX {
            if full() && full() && full() {
                break;
            }
            assert!(i < u8::MAX, "the socket took everything");
            served.send(vec![body(i, 1 << 20)], None);
        }
        served.send(Vec::new(), Some(End::Disconnect));
        std::thread::sleep(Duration::from_millis(300));
        assert!(served.seen.try_recv().is_err(), "still writing");
        served.send(Vec::new(), Some(End::Close));
        assert_eq!(served.next(), Seen::Left("Disconnected".into()));
        assert_eq!(served.next(), Seen::Closed);
        drop(client);
    }

    #[test]
    fn a_close_or_too_much_queued_drops_what_waits() {
        for overflow in [false, true] {
            let (a, b) = Pipe::pair();
            let mut client = Connection::local(a);
            let served = serve(Connection::local(b), quiet, Duration::from_secs(30));
            if overflow {
                served.pending.store(MAX_PENDING, Ordering::Relaxed);
                served.send(vec![body(3, 10)], None);
            } else {
                served.send(Vec::new(), Some(End::Close));
            }
            assert!(client.recv().is_err());
            assert_eq!(served.next(), Seen::Left("Disconnected".into()));
            assert_eq!(served.next(), Seen::Closed);
        }
    }

    /// A client in this process takes a join's whole first view at once: queued past the
    /// limit, it is served and stays connected.
    #[test]
    fn an_uncapped_connection_outlasts_too_much_queued() {
        let (a, b) = Pipe::pair();
        let mut client = Connection::local(a);
        let served = serve_with(Connection::local(b), quiet, Duration::from_secs(30), false);
        served.pending.store(MAX_PENDING, Ordering::Relaxed);
        served.send(vec![body(3, 10)], None);
        assert_eq!(client.recv().unwrap().0, 3);
        assert!(served.seen.try_recv().is_err());
    }
}
