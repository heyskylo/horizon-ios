//! Owned, bounded IO producers. Dropping an owner drains accepted operations;
//! shutdown closes its sockets and polling joins only finished threads.
use crate::transport::Cancellation;
use std::{
    io,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::JoinHandle,
};

type Task = Box<dyn FnOnce() + Send>;
type Start = dyn Fn(Task) -> io::Result<JoinHandle<()>> + Send + Sync;
struct State {
    handles: Mutex<Vec<JoinHandle<()>>>,
    retired: AtomicBool,
    cancel: Arc<Cancellation>,
    extra_cancel: Option<Arc<dyn Fn() + Send + Sync>>,
    start: Arc<Start>,
}
fn registry() -> &'static Mutex<Vec<Arc<State>>> {
    static REGISTRY: OnceLock<Mutex<Vec<Arc<State>>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

/// A single lifetime's producer ownership; unrelated games have independent cancellation.
pub struct Workers(Arc<State>);
impl Default for Workers {
    fn default() -> Self {
        Self::new(None)
    }
}
impl Workers {
    pub fn new(extra_cancel: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        Self::with_start(
            extra_cancel,
            Arc::new(|task| {
                std::thread::Builder::new()
                    .name("Cosmetics IO".into())
                    .spawn(task)
            }),
        )
    }
    fn with_start(extra_cancel: Option<Arc<dyn Fn() + Send + Sync>>, start: Arc<Start>) -> Self {
        let state = Arc::new(State {
            handles: Mutex::default(),
            retired: AtomicBool::new(false),
            cancel: Arc::default(),
            extra_cancel,
            start,
        });
        let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        registry.retain(|old| !old.finished());
        registry.push(state.clone());
        Self(state)
    }
    pub fn cancellation(&self) -> Arc<Cancellation> {
        self.0.cancel.clone()
    }
    pub fn closed(&self) -> bool {
        self.0.retired.load(Ordering::Acquire)
    }
    /// Refusal and spawn failure settle the receiver immediately. No fallback thread.
    pub fn spawn<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Receiver<Result<T, String>> {
        let (tx, rx) = mpsc::channel();
        let mut handles = self.0.handles.lock().unwrap_or_else(|e| e.into_inner());
        reap(&mut handles);
        if self.closed() || handles.len() >= 4 {
            let _ = tx.send(Err("cosmetics worker unavailable".into()));
            return rx;
        }
        let failed = tx.clone();
        let cancel = self.cancellation();
        let task = Box::new(move || {
            let result = if cancel.cancelled() {
                Err("cosmetics request cancelled".into())
            } else {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                    .unwrap_or_else(|_| Err("cosmetics worker failed".into()))
            };
            let _ = tx.send(result);
        });
        match (self.0.start)(task) {
            Ok(handle) => handles.push(handle),
            Err(_) => {
                let _ = failed.send(Err("cosmetics worker unavailable".into()));
            }
        }
        rx
    }
    pub fn shutdown(&self) -> bool {
        self.0.retired.store(true, Ordering::Release);
        self.0.cancel.cancel();
        if let Some(cancel) = &self.0.extra_cancel {
            cancel();
        }
        self.0.finished()
    }
}
fn reap(handles: &mut Vec<JoinHandle<()>>) {
    let mut i = 0;
    while i < handles.len() {
        if handles[i].is_finished() {
            let _ = handles.swap_remove(i).join();
        } else {
            i += 1;
        }
    }
}
impl State {
    fn finished(&self) -> bool {
        if !self.retired.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut handles) = self.handles.try_lock() else {
            return false;
        };
        reap(&mut handles);
        handles.is_empty()
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        self.0.retired.store(true, Ordering::Release);
    }
}

/// Poll retired producers during ordinary navigation; their requested mutations drain.
pub fn poll_retired_workers() {
    if let Ok(mut registry) = registry().try_lock() {
        registry.retain(|state| !state.finished());
    }
}

/// Full application shutdown only, including owners already retired by navigation.
pub fn shutdown_workers() -> bool {
    let Ok(mut registry) = registry().try_lock() else {
        return false;
    };
    registry.retain(|state| {
        state.retired.store(true, Ordering::Release);
        state.cancel.cancel();
        if let Some(cancel) = &state.extra_cancel {
            cancel();
        }
        !state.finished()
    });
    registry.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn rejected_and_failed_jobs_settle_without_detached_fallback() {
        let workers = Workers::with_start(
            None,
            Arc::new(|_| Err(io::Error::other("controlled spawn failure"))),
        );
        assert!(workers.spawn(|| Ok(7)).recv().unwrap().is_err());
        assert!(workers.shutdown());
        assert!(workers.spawn(|| Ok(8)).recv().unwrap().is_err());
    }
    #[test]
    fn normal_navigation_drains_requested_mutation_and_bounds_producers() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let hook = cancelled.clone();
        let workers = Workers::new(Some(Arc::new(move || hook.store(true, Ordering::Release))));
        let retained = workers.0.clone();
        let mut release = Vec::new();
        let mut results = Vec::new();
        for value in 0..4 {
            let (tx, rx) = mpsc::channel();
            release.push(tx);
            results.push(workers.spawn(move || {
                rx.recv().unwrap();
                Ok(value)
            }));
        }
        assert!(workers.spawn(|| Ok(99)).recv().unwrap().is_err());
        drop(workers);
        assert!(!retained.finished());
        assert!(!cancelled.load(Ordering::Acquire));
        for tx in release {
            tx.send(()).unwrap();
        }
        for (value, result) in results.into_iter().enumerate() {
            assert_eq!(result.recv().unwrap(), Ok(value as i32));
        }
        let until = Instant::now() + Duration::from_secs(2);
        while !retained.finished() {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        assert!(!cancelled.load(Ordering::Acquire));
    }
    #[test]
    fn shutdown_joins_a_controlled_blocked_read_and_isolates_other_owners() {
        use std::{
            io::Read,
            net::{Shutdown, TcpListener, TcpStream},
        };
        let peer = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut socket = TcpStream::connect(peer.local_addr().unwrap()).unwrap();
        let close = socket.try_clone().unwrap();
        let (server, _) = peer.accept().unwrap();
        let owner = Workers::new(Some(Arc::new(move || {
            let _ = close.shutdown(Shutdown::Both);
        })));
        let other = Workers::default();
        let (entered, started) = mpsc::channel();
        let result = owner.spawn(move || {
            entered.send(()).unwrap();
            let _ = socket.read(&mut [0; 1]);
            Ok(17)
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(result.try_recv().is_err());
        let until = Instant::now() + Duration::from_secs(2);
        while !owner.shutdown() {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        assert_eq!(result.recv().unwrap(), Ok(17));
        assert_eq!(other.spawn(|| Ok(23)).recv().unwrap(), Ok(23));
        assert!(!other.closed());
        other.shutdown();
        drop(server);
    }
}
