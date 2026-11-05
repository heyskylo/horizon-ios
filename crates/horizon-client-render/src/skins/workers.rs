//! Owned bounded-concurrency skin IO. Jobs retain their own result futures.
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

type Work = Box<dyn FnOnce(&AtomicBool, &AtomicBool) + Send>;
struct Job {
    scope: Arc<AtomicBool>,
    run: Option<Work>,
    cancel: Option<Box<dyn FnOnce() + Send>>,
}
impl Job {
    fn execute(mut self, stopping: &AtomicBool) {
        if !stopping.load(Ordering::Acquire) && !self.scope.load(Ordering::Acquire) {
            if let Some(run) = self.run.take() {
                run(stopping, &self.scope);
            }
            self.cancel.take();
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel();
        }
    }
}
#[derive(Default)]
struct Shared {
    jobs: Mutex<VecDeque<Job>>,
    wake: Condvar,
    stopping: AtomicBool,
}
pub(super) struct Pool {
    shared: Arc<Shared>,
    handles: Mutex<Vec<JoinHandle<()>>>,
    name: String,
    count: usize,
}
impl Pool {
    pub(super) fn new(name: &str, count: usize) -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::new(Shared::default()),
            handles: Mutex::new(Vec::new()),
            name: name.into(),
            count,
        })
    }
    fn start_workers(&self) {
        let mut handles = self.handles.lock().unwrap_or_else(|e| e.into_inner());
        if !handles.is_empty() || self.stopped() {
            return;
        }
        for _ in 0..self.count {
            if self.stopped() {
                break;
            }
            let state = self.shared.clone();
            let spawned = std::thread::Builder::new()
                .name(self.name.clone())
                .spawn(move || {
                    loop {
                        let job = {
                            let mut jobs = state.jobs.lock().unwrap_or_else(|e| e.into_inner());
                            loop {
                                if state.stopping.load(Ordering::Acquire) {
                                    return;
                                }
                                if let Some(job) = jobs.pop_front() {
                                    break job;
                                }
                                jobs = state.wake.wait(jobs).unwrap_or_else(|e| e.into_inner());
                            }
                        };
                        // A failed codec/service callback settles that job's future
                        // through Job::drop without killing capacity for later jobs.
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            job.execute(&state.stopping);
                        }));
                    }
                });
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(_) => break,
            }
        }
        if handles.is_empty() {
            self.request_shutdown();
        }
    }
    pub(super) fn stopped(&self) -> bool {
        self.shared.stopping.load(Ordering::Acquire)
    }
    pub(super) fn submit(
        &self,
        scope: Arc<AtomicBool>,
        run: impl FnOnce(&AtomicBool, &AtomicBool) + Send + 'static,
        cancel: impl FnOnce() + Send + 'static,
    ) {
        let job = Job {
            scope,
            run: Some(Box::new(run)),
            cancel: Some(Box::new(cancel)),
        };
        let mut jobs = self.shared.jobs.lock().unwrap_or_else(|e| e.into_inner());
        if self.stopped() || job.scope.load(Ordering::Acquire) {
            drop(jobs);
            drop(job);
        } else {
            // The lightweight backlog retains every accepted lookup; the
            // fixed worker count bounds concurrent HTTP/decoding pressure.
            jobs.push_back(job);
            drop(jobs);
            self.start_workers();
            self.shared.wake.notify_one();
        }
    }
    pub(super) fn cancel_scope(&self, scope: &Arc<AtomicBool>) {
        scope.store(true, Ordering::Release);
        let removed = {
            let mut jobs = self.shared.jobs.lock().unwrap_or_else(|e| e.into_inner());
            let mut kept = VecDeque::new();
            let mut removed = Vec::new();
            for job in jobs.drain(..) {
                if Arc::ptr_eq(&job.scope, scope) {
                    removed.push(job);
                } else {
                    kept.push_back(job);
                }
            }
            *jobs = kept;
            removed
        };
        drop(removed);
    }
    pub(super) fn request_shutdown(&self) {
        let jobs = {
            let mut jobs = self.shared.jobs.lock().unwrap_or_else(|e| e.into_inner());
            self.shared.stopping.store(true, Ordering::Release);
            std::mem::take(&mut *jobs)
        };
        self.shared.wake.notify_all();
        drop(jobs);
    }
    pub(super) fn shutdown(&self) -> bool {
        self.request_shutdown();
        let mut handles = self.handles.lock().unwrap_or_else(|e| e.into_inner());
        let mut index = 0;
        while index < handles.len() {
            if handles[index].is_finished() {
                let _ = handles.swap_remove(index).join();
            } else {
                index += 1;
            }
        }
        handles.is_empty()
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        self.request_shutdown();
        for handle in self
            .handles
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{atomic::AtomicUsize, mpsc},
        time::{Duration, Instant},
    };
    fn wait(mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !ready() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    fn bounded_workers_finish_every_valid_queued_job_without_ui_polling() {
        let pool = Pool::new("skin pressure", 4);
        assert!(pool.handles.lock().unwrap().is_empty());
        let scope = Arc::new(AtomicBool::new(false));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let canceled = Arc::new(AtomicUsize::new(0));
        let (entered, entered_rx) = mpsc::channel();
        for _ in 0..501 {
            let (gate, active, peak, completed, entered, canceled) = (
                gate.clone(),
                active.clone(),
                peak.clone(),
                completed.clone(),
                entered.clone(),
                canceled.clone(),
            );
            pool.submit(
                scope.clone(),
                move |_, _| {
                    let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(count, Ordering::SeqCst);
                    entered.send(()).unwrap();
                    let mut open = gate.0.lock().unwrap();
                    while !*open {
                        open = gate.1.wait(open).unwrap();
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                },
                move || {
                    canceled.fetch_add(1, Ordering::SeqCst);
                },
            );
        }
        let started = (0..4).all(|_| entered_rx.recv_timeout(Duration::from_secs(2)).is_ok());
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert!(started);
        wait(|| completed.load(Ordering::SeqCst) == 501);
        assert_eq!(peak.load(Ordering::SeqCst), 4);
        assert_eq!(canceled.load(Ordering::SeqCst), 0);
        wait(|| pool.shutdown());
    }
    #[test]
    fn shutdown_cancels_waiting_work_without_network_and_retains_active_work() {
        let pool = Pool::new("skin shutdown", 1);
        let scope = Arc::new(AtomicBool::new(false));
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        pool.submit(
            scope.clone(),
            move |_, _| {
                entered.send(()).unwrap();
                release_rx.recv().unwrap();
            },
            || {},
        );
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let network = Arc::new(AtomicUsize::new(0));
        let canceled = Arc::new(AtomicUsize::new(0));
        for _ in 0..100 {
            let (network, canceled) = (network.clone(), canceled.clone());
            pool.submit(
                scope.clone(),
                move |_, _| {
                    network.fetch_add(1, Ordering::SeqCst);
                },
                move || {
                    canceled.fetch_add(1, Ordering::SeqCst);
                },
            );
        }
        let stopped = pool.shutdown();
        let (network_after, canceled_after) = (network.clone(), canceled.clone());
        pool.submit(
            scope,
            move |_, _| {
                network_after.fetch_add(1, Ordering::SeqCst);
            },
            move || {
                canceled_after.fetch_add(1, Ordering::SeqCst);
            },
        );
        release.send(()).unwrap();
        assert!(!stopped);
        assert_eq!(network.load(Ordering::SeqCst), 0);
        assert_eq!(canceled.load(Ordering::SeqCst), 101);
        wait(|| pool.shutdown());
    }
    #[test]
    fn session_scope_cancellation_does_not_stop_another_live_scope() {
        let pool = Pool::new("skin scopes", 1);
        let old = Arc::new(AtomicBool::new(false));
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        pool.submit(
            old.clone(),
            move |_, _| {
                entered.send(()).unwrap();
                release_rx.recv().unwrap();
            },
            || {},
        );
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let canceled = Arc::new(AtomicUsize::new(0));
        let fetched = Arc::new(AtomicUsize::new(0));
        for _ in 0..20 {
            let (canceled, fetched) = (canceled.clone(), fetched.clone());
            pool.submit(
                old.clone(),
                move |_, _| {
                    fetched.fetch_add(1, Ordering::SeqCst);
                },
                move || {
                    canceled.fetch_add(1, Ordering::SeqCst);
                },
            );
        }
        pool.cancel_scope(&old);
        let (done, done_rx) = mpsc::channel();
        pool.submit(
            Arc::new(AtomicBool::new(false)),
            move |_, _| {
                done.send(()).unwrap();
            },
            || {},
        );
        release.send(()).unwrap();
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(canceled.load(Ordering::SeqCst), 20);
        assert_eq!(fetched.load(Ordering::SeqCst), 0);
        assert!(!pool.stopped());
        wait(|| pool.shutdown());
    }
    #[test]
    fn an_idle_service_stops_without_starting_threads() {
        let pool = Pool::new("unused skin service", 4);
        assert!(pool.shutdown());
        let (canceled, canceled_rx) = mpsc::channel();
        pool.submit(
            Arc::new(AtomicBool::new(false)),
            |_, _| panic!("network after shutdown"),
            move || {
                canceled.send(()).unwrap();
            },
        );
        canceled_rx.recv().unwrap();
        assert!(pool.handles.lock().unwrap().is_empty());
    }
}
