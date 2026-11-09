//! Title-owned Realms work. Navigation keeps requested operations alive; the
//! application's shutdown gate cancels IO and joins only finished producers.
use super::{client::RealmsError, transport::Cancellation};
use std::{
    collections::VecDeque,
    fmt, io,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

pub(super) type Job<T> = Arc<Mutex<Option<T>>>;
type Task = Box<dyn FnOnce() + Send>;
type Start = dyn Fn(Task) -> io::Result<JoinHandle<()>> + Send + Sync;
const THREADS: usize = 4;
const BACKLOG: usize = 32;

pub(crate) trait WorkerOutput: Send + 'static {
    fn cancelled() -> Self;
    fn unavailable() -> Self;
}
impl<T: Send + 'static> WorkerOutput for Result<T, RealmsError> {
    fn cancelled() -> Self {
        Err(RealmsError::Cancelled)
    }
    fn unavailable() -> Self {
        Err(RealmsError::WorkerUnavailable)
    }
}
impl<T: Send + 'static> WorkerOutput for Result<T, super::upload::UploadError> {
    fn cancelled() -> Self {
        Err(super::upload::UploadError::Cancelled)
    }
    fn unavailable() -> Self {
        Err(super::upload::UploadError::Failed(
            "Realms worker unavailable".into(),
        ))
    }
}
impl<T: Send + 'static> WorkerOutput for Result<T, super::download::DownloadError> {
    fn cancelled() -> Self {
        Err(super::download::DownloadError::Cancelled)
    }
    fn unavailable() -> Self {
        Err(super::download::DownloadError::Failed(
            "Realms worker unavailable".into(),
        ))
    }
}
impl<R: WorkerOutput, const N: usize> WorkerOutput for [R; N] {
    fn cancelled() -> Self {
        std::array::from_fn(|_| R::cancelled())
    }
    fn unavailable() -> Self {
        std::array::from_fn(|_| R::unavailable())
    }
}

impl WorkerOutput for u64 {
    fn cancelled() -> Self {
        0
    }
    fn unavailable() -> Self {
        0
    }
}

struct State {
    jobs: Mutex<VecDeque<Task>>,
    wake: Condvar,
    retiring: AtomicBool,
    cancel: Arc<Cancellation>,
}
struct Pool {
    state: Arc<State>,
    handles: Mutex<Vec<JoinHandle<()>>>,
    start: Arc<Start>,
}

// Keep every service's JoinHandles until it has actually stopped, even after
// its title disappears. The registry owns no UI lease and cannot prolong one.
fn registry() -> &'static Mutex<Vec<Arc<Pool>>> {
    static REGISTRY: OnceLock<Mutex<Vec<Arc<Pool>>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

struct Owner(Arc<Pool>);
impl Drop for Owner {
    fn drop(&mut self) {
        // Normal screen/title navigation drains requested operations. It does
        // not cancel a server mutation that the player already confirmed.
        self.0.retire();
    }
}

#[derive(Clone)]
pub(crate) struct Workers(Arc<Owner>);
impl fmt::Debug for Workers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RealmsWorkers")
            .field("retiring", &self.0.0.state.retiring.load(Ordering::Acquire))
            .finish()
    }
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct WorkHandle(Arc<Pool>);
#[cfg(test)]
impl WorkHandle {
    #[cfg(test)]
    pub(crate) fn poll(&self) -> bool {
        self.0.finished()
    }
    #[cfg(test)]
    pub(crate) fn shutdown(&self) -> bool {
        self.0.shutdown()
    }
}

impl Workers {
    pub(crate) fn new() -> Self {
        Self::with_start(Arc::new(|task| {
            std::thread::Builder::new()
                .name("Realms IO".into())
                .spawn(task)
        }))
    }

    fn with_start(start: Arc<Start>) -> Self {
        let pool = Arc::new(Pool {
            state: Arc::new(State {
                jobs: Mutex::new(VecDeque::new()),
                wake: Condvar::new(),
                retiring: AtomicBool::new(false),
                cancel: Arc::new(Cancellation::default()),
            }),
            handles: Mutex::new(Vec::new()),
            start,
        });
        let mut registered = registry().lock().unwrap_or_else(|e| e.into_inner());
        registered.retain(|old| !old.finished());
        registered.push(pool.clone());
        Self(Arc::new(Owner(pool)))
    }

    pub(crate) fn cancellation(&self) -> Arc<Cancellation> {
        self.0.0.state.cancel.clone()
    }

    pub(crate) fn closed(&self) -> bool {
        self.0.0.state.retiring.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn work_handle(&self) -> WorkHandle {
        WorkHandle(self.0.0.clone())
    }

    pub(crate) fn spawn<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, RealmsError> + Send + 'static,
    ) -> Job<Result<T, RealmsError>> {
        self.spawn_output(work)
    }

    pub(crate) fn spawn_output<R: WorkerOutput>(
        &self,
        work: impl FnOnce() -> R + Send + 'static,
    ) -> Job<R> {
        let job = Arc::new(Mutex::new(None));
        let pool = &self.0.0;
        if !pool.start_workers() {
            *job.lock().unwrap() = Some(if pool.state.cancel.cancelled() {
                R::cancelled()
            } else {
                R::unavailable()
            });
            return job;
        }
        let result_slot = job.clone();
        let cancel = pool.state.cancel.clone();
        let task: Task = Box::new(move || {
            let result = if cancel.cancelled() {
                R::cancelled()
            } else {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                    .unwrap_or_else(|_| R::unavailable())
            };
            let result = if cancel.cancelled() {
                R::cancelled()
            } else {
                result
            };
            *result_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        });
        let mut queue = pool.state.jobs.lock().unwrap_or_else(|e| e.into_inner());
        if pool.state.retiring.load(Ordering::Acquire) || queue.len() >= BACKLOG {
            drop(queue);
            drop(task);
            // The new result slot has not escaped this call yet, so settling
            // a refused submission cannot wait behind a UI reader.
            *job.lock().unwrap() = Some(if pool.state.cancel.cancelled() {
                R::cancelled()
            } else {
                R::unavailable()
            });
        } else {
            queue.push_back(task);
            drop(queue);
            pool.state.wake.notify_one();
        }
        job
    }
}

impl Pool {
    fn start_workers(&self) -> bool {
        if self.state.retiring.load(Ordering::Acquire) {
            return false;
        }
        let mut handles = self.handles.lock().unwrap_or_else(|e| e.into_inner());
        if !handles.is_empty() {
            return !self.state.retiring.load(Ordering::Acquire);
        }
        if self.state.retiring.load(Ordering::Acquire) {
            return false;
        }
        for _ in 0..THREADS {
            let state = self.state.clone();
            let task: Task = Box::new(move || {
                loop {
                    let task = {
                        let mut jobs = state.jobs.lock().unwrap_or_else(|e| e.into_inner());
                        loop {
                            // Accepted jobs settle on the owned worker, including
                            // cancellation results, before that producer exits.
                            if let Some(task) = jobs.pop_front() {
                                break task;
                            }
                            if state.retiring.load(Ordering::Acquire) {
                                return;
                            }
                            // Retirement never takes a blocking lock on the frame
                            // thread. A timed wait also covers a notify/check race.
                            jobs = state
                                .wake
                                .wait_timeout(jobs, Duration::from_millis(100))
                                .unwrap_or_else(|e| e.into_inner())
                                .0;
                        }
                    };
                    task();
                }
            });
            match (self.start)(task) {
                Ok(handle) => handles.push(handle),
                Err(_) => break,
            }
        }
        if handles.is_empty() {
            self.retire();
            return false;
        }
        true
    }

    fn retire(&self) {
        self.state.retiring.store(true, Ordering::Release);
        self.state.wake.notify_all();
    }

    fn finished(&self) -> bool {
        if !self.state.retiring.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut handles) = self.handles.try_lock() else {
            return false;
        };
        let mut i = 0;
        while i < handles.len() {
            if handles[i].is_finished() {
                let _ = handles.swap_remove(i).join();
            } else {
                i += 1;
            }
        }
        handles.is_empty()
    }

    fn shutdown(&self) -> bool {
        self.retire();
        self.state.cancel.cancel();
        self.finished()
    }
}

/// Full-application Quit only. Do not call for one guest or ordinary title
/// navigation: other active titles may still be using the Realms service.
pub fn shutdown_workers() -> bool {
    let Ok(mut registered) = registry().try_lock() else {
        return false;
    };
    registered.retain(|pool| !pool.shutdown());
    registered.is_empty()
}

pub fn poll_retired_workers() {
    if let Ok(mut registered) = registry().try_lock() {
        registered.retain(|pool| !pool.finished());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Instant};

    fn wait(mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !ready() {
            assert!(Instant::now() < deadline, "Realms producer did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn spawn_failure_settles_the_future_without_a_detached_thread() {
        let workers = Workers::with_start(Arc::new(|_| {
            Err(io::Error::other("controlled spawn failure"))
        }));
        let job = workers.spawn(|| Ok::<_, RealmsError>(17));
        assert_eq!(
            job.lock().unwrap().take(),
            Some(Err(RealmsError::WorkerUnavailable))
        );
        let mut screen = crate::realms::players_fixture(0);
        screen.workers = workers.clone();
        screen.state = crate::realms::FetchState::Loading(workers.spawn(|| Ok(Vec::new())));
        let _ = screen.poll();
        assert!(matches!(
            screen.state,
            crate::realms::FetchState::Error(RealmsError::WorkerUnavailable)
        ));
        assert!(workers.work_handle().shutdown());
    }

    #[test]
    fn dropping_a_title_owner_does_not_block_or_abandon_an_active_producer() {
        let workers = Workers::new();
        let handle = workers.work_handle();
        let (entered, ready) = mpsc::channel();
        let (release, hold) = mpsc::channel();
        let job = workers.spawn(move || {
            entered.send(()).unwrap();
            hold.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(23)
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let start = Instant::now();
        drop(workers);
        assert!(start.elapsed() < Duration::from_millis(100));
        assert!(!handle.poll());
        assert!(job.lock().unwrap().is_none());
        // Graceful retirement preserves a requested operation. Its result is
        // published before the owning gate can report that producer finished.
        release.send(()).unwrap();
        wait(|| handle.poll());
        assert_eq!(job.lock().unwrap().take(), Some(Ok(23)));
    }

    #[test]
    fn shutdown_waits_for_active_producers_and_settles_the_bounded_backlog() {
        let workers = Workers::new();
        let handle = workers.work_handle();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (entered, ready) = mpsc::channel();
        let mut active = Vec::new();
        for _ in 0..THREADS {
            let (entered, gate) = (entered.clone(), gate.clone());
            active.push(workers.spawn(move || {
                entered.send(()).unwrap();
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
                Ok(())
            }));
        }
        for _ in 0..THREADS {
            ready.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        let executed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut queued = Vec::new();
        for _ in 0..BACKLOG {
            let executed = executed.clone();
            queued.push(workers.spawn(move || {
                executed.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }));
        }
        let full = workers.spawn(|| Ok(()));
        assert_eq!(
            full.lock().unwrap().take(),
            Some(Err(RealmsError::WorkerUnavailable))
        );
        assert!(
            !handle.shutdown(),
            "a cancellation flag alone is not producer completion"
        );
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        wait(|| handle.shutdown());
        assert_eq!(executed.load(Ordering::Relaxed), 0);
        for job in active.into_iter().chain(queued) {
            assert_eq!(
                job.lock().unwrap().take(),
                Some(Err(RealmsError::Cancelled))
            );
        }
    }

    #[test]
    fn a_panicking_operation_settles_its_result_and_keeps_the_service_usable() {
        let workers = Workers::new();
        let failed = workers
            .spawn(|| -> Result<(), RealmsError> { panic!("controlled Realms fixture panic") });
        wait(|| failed.lock().unwrap().is_some());
        assert_eq!(
            failed.lock().unwrap().take(),
            Some(Err(RealmsError::WorkerUnavailable))
        );
        let next = workers.spawn(|| Ok(31));
        wait(|| next.lock().unwrap().is_some());
        assert_eq!(next.lock().unwrap().take(), Some(Ok(31)));
        wait(|| workers.work_handle().shutdown());
    }
}
