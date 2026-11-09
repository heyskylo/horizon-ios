//! Safe async task offloading API for native mods using a dedicated worker pool.
//! API @API:mod_async_worker@.
use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::Game;
use horizon_client::mod_api::TaskCallback;
use horizon_threads::jobs::{Class, Pool, Rank, Registration, Source};

struct NativeTaskSource {
    queue: Mutex<VecDeque<(i64, Box<dyn FnOnce() + Send + 'static>)>>,
    next_id: AtomicI64,
}

impl Source for NativeTaskSource {
    fn next(&self) -> Option<Rank> {
        let q = self.queue.lock().ok()?;
        let (id, _) = q.front()?;
        Some(Rank::new(Class::Urgent, *id))
    }

    fn run(&self, _worker: usize) -> bool {
        let job = self.queue.lock().ok().and_then(|mut q| q.pop_front());
        if let Some((_, job)) = job {
            job();
            true
        } else {
            false
        }
    }

    fn label(&self) -> &'static str {
        "native-mod-tasks"
    }
}

struct TaskRunner {
    source: Arc<NativeTaskSource>,
    registration: Registration,
}

static MOD_POOL: OnceLock<Pool> = OnceLock::new();
static RUNNER: OnceLock<TaskRunner> = OnceLock::new();

/// Dedicated thread pool for native mod async tasks, separate from chunk generation.
fn mod_pool() -> &'static Pool {
    MOD_POOL.get_or_init(|| {
        let threads = std::env::var("HORIZON_MOD_JOB_THREADS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or_else(|| (horizon_threads::default_workers() / 2).clamp(1, 4));
        Pool::new(threads, "horizon-mod-worker")
    })
}

fn runner() -> &'static TaskRunner {
    RUNNER.get_or_init(|| {
        let source = Arc::new(NativeTaskSource {
            queue: Mutex::new(VecDeque::new()),
            next_id: AtomicI64::new(0),
        });
        let registration = mod_pool().register(source.clone());
        TaskRunner {
            source,
            registration,
        }
    })
}

/// Enqueue a background job onto the dedicated mod worker pool.
fn spawn_pool_job(job: Box<dyn FnOnce() + Send + 'static>) {
    let runner = runner();
    let id = runner.source.next_id.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut q) = runner.source.queue.lock() {
        q.push_back((id, job));
    }
    runner.registration.notify();
}

pub(crate) struct TaskCallbackEntry {
    pub(crate) callback: TaskCallback,
    pub(crate) mod_call: Option<crate::crash::NativeModCall>,
}

/// Per-game task queue. Each `Game` (both client window and server thread in single player)
/// owns its own `NativeTasks` holding deferred main-thread callbacks.
#[derive(Default)]
pub(crate) struct NativeTasks {
    callbacks: Arc<Mutex<VecDeque<TaskCallbackEntry>>>,
}

impl NativeTasks {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.callbacks.lock().is_ok_and(|q| q.is_empty())
    }

    pub(crate) fn drain_callbacks(&self) -> Vec<TaskCallbackEntry> {
        self.callbacks
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }

    /// Submit a background job to the dedicated mod worker pool. API @API:mod_async_worker@.
    pub(crate) fn spawn_background_job(&self, job: Box<dyn FnOnce() + Send + 'static>) {
        let mod_call = crate::crash::current_native_mod();
        spawn_pool_job(Box::new(move || {
            let _guard = mod_call.map(crate::crash::enter_native_mod);
            let _ = crate::crash::contain(AssertUnwindSafe(job));
        }));
    }

    /// Defer a callback to run on this game's thread during `native_tick`. API @API:mod_async_worker@.
    pub(crate) fn defer_main_thread(&self, callback: TaskCallback) {
        let mod_call = crate::crash::current_native_mod();
        if let Ok(mut q) = self.callbacks.lock() {
            q.push_back(TaskCallbackEntry { callback, mod_call });
        }
    }

    /// Offload heavy computation to the dedicated mod worker pool and run `callback` on this
    /// game's thread during `native_tick`.
    /// Panics in the background task or main-thread callback are caught and reported per mod.
    /// API @API:mod_async_worker@.
    pub(crate) fn spawn_task(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
        callback: TaskCallback,
    ) {
        let mod_call = crate::crash::current_native_mod();
        let target = Arc::downgrade(&self.callbacks);
        spawn_pool_job(Box::new(move || {
            let task_result = {
                let _guard = mod_call.map(crate::crash::enter_native_mod);
                crate::crash::contain(AssertUnwindSafe(task))
            };
            if task_result.is_ok()
                && let Some(arc) = target.upgrade()
                && let Ok(mut q) = arc.lock()
            {
                q.push_back(TaskCallbackEntry { callback, mod_call });
            }
        }));
    }
}

impl Game<'_> {
    /// Submit a background job to the dedicated mod worker pool. API @API:mod_async_worker@.
    pub(super) fn native_spawn_background_job(&self, job: Box<dyn FnOnce() + Send + 'static>) {
        self.native_tasks.spawn_background_job(job);
    }

    /// Defer a callback to run on this game's thread during `native_tick`. API @API:mod_async_worker@.
    pub(super) fn native_defer_main_thread(&self, callback: TaskCallback) {
        self.native_tasks.defer_main_thread(callback);
    }

    /// Offload heavy computation to the dedicated mod worker pool and run `callback` on this
    /// game's thread during `native_tick`. API @API:mod_async_worker@.
    pub(super) fn native_spawn_task(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
        callback: TaskCallback,
    ) {
        self.native_tasks.spawn_task(task, callback);
    }

    /// Process deferred task callbacks on the game/server thread.
    /// Each callback is executed inside `contain` and tagged with the mod that scheduled it,
    /// so a panicking mod callback is logged without taking down the main game loop.
    pub(super) fn native_process_tasks(&mut self) {
        let callbacks = self.native_tasks.drain_callbacks();
        for entry in callbacks {
            let _guard = entry.mod_call.map(crate::crash::enter_native_mod);
            let _ = crate::crash::contain(AssertUnwindSafe(|| {
                (entry.callback)(self);
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::mod_api::{Host, HostTaskExt};
    use std::sync::atomic::{AtomicBool, AtomicI32};
    use std::time::{Duration, Instant};

    struct TestHost {
        tasks: NativeTasks,
    }

    impl TestHost {
        fn new() -> Self {
            Self {
                tasks: NativeTasks::new(),
            }
        }

        fn process_tasks(&mut self) {
            let callbacks = self.tasks.drain_callbacks();
            for entry in callbacks {
                let _guard = entry.mod_call.map(crate::crash::enter_native_mod);
                let _ = crate::crash::contain(AssertUnwindSafe(|| {
                    (entry.callback)(self);
                }));
            }
        }
    }

    impl Host for TestHost {
        fn spawn_background_job(&self, job: Box<dyn FnOnce() + Send + 'static>) {
            self.tasks.spawn_background_job(job);
        }

        fn defer_main_thread(&self, callback: TaskCallback) {
            self.tasks.defer_main_thread(callback);
        }

        fn spawn_task(&self, task: Box<dyn FnOnce() + Send + 'static>, callback: TaskCallback) {
            self.tasks.spawn_task(task, callback);
        }
    }

    #[test]
    fn background_job_runs_on_dedicated_mod_pool() {
        let host = TestHost::new();
        let thread_name = Arc::new(Mutex::new(None));
        let thread_name_clone = thread_name.clone();
        host.spawn_background_job(Box::new(move || {
            let name = std::thread::current().name().map(|s| s.to_string());
            *thread_name_clone.lock().unwrap() = name;
        }));
        let start = Instant::now();
        while thread_name.lock().unwrap().is_none() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let name = thread_name
            .lock()
            .unwrap()
            .clone()
            .expect("job should have run");
        assert!(
            name.starts_with("horizon-mod-worker"),
            "expected mod worker thread, got {name}"
        );
    }

    #[test]
    fn two_distinct_games_do_not_interfere() {
        let mut host_client = TestHost::new();
        let mut host_server = TestHost::new();

        let client_seen = Arc::new(AtomicI32::new(0));
        let server_seen = Arc::new(AtomicI32::new(0));

        let c_seen = client_seen.clone();
        host_client.spawn_task_with(
            || 10,
            move |_host, val| {
                c_seen.store(val, Ordering::SeqCst);
            },
        );

        let s_seen = server_seen.clone();
        host_server.spawn_task_with(
            || 20,
            move |_host, val| {
                s_seen.store(val, Ordering::SeqCst);
            },
        );

        let start = Instant::now();
        while (host_client.tasks.is_empty() || host_server.tasks.is_empty())
            && start.elapsed() < Duration::from_secs(3)
        {
            std::thread::sleep(Duration::from_millis(5));
        }

        // Process only client tasks
        host_client.process_tasks();
        assert_eq!(client_seen.load(Ordering::SeqCst), 10);
        assert_eq!(server_seen.load(Ordering::SeqCst), 0);

        // Now process server tasks
        host_server.process_tasks();
        assert_eq!(server_seen.load(Ordering::SeqCst), 20);
    }

    #[test]
    fn dropped_game_safely_cancels_callbacks() {
        let host_client = TestHost::new();
        let mut host_server = TestHost::new();

        let server_seen = Arc::new(AtomicI32::new(0));
        let s_seen = server_seen.clone();

        // Client drops immediately after spawning
        host_client.spawn_task_with(|| 10, |_host, _val| {});
        drop(host_client);

        // Server should still function perfectly
        host_server.spawn_task_with(
            || 42,
            move |_host, val| {
                s_seen.store(val, Ordering::SeqCst);
            },
        );

        let start = Instant::now();
        while host_server.tasks.is_empty() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
        }

        host_server.process_tasks();
        assert_eq!(server_seen.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn background_panic_is_contained_and_worker_survives() {
        let mut host = TestHost::new();
        // Submit panicking job
        host.spawn_background_job(Box::new(|| {
            panic!("intentional mod background panic");
        }));

        // Submit subsequent healthy task
        let seen = Arc::new(AtomicBool::new(false));
        let seen_clone = seen.clone();
        host.spawn_task_with(
            || 1,
            move |_host, _val| {
                seen_clone.store(true, Ordering::SeqCst);
            },
        );

        let start = Instant::now();
        while host.tasks.is_empty() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
        }

        host.process_tasks();
        assert!(
            seen.load(Ordering::SeqCst),
            "worker should survive panic and process subsequent task"
        );
    }

    #[test]
    fn main_thread_callback_panic_is_contained() {
        let mut host = TestHost::new();
        let second_ran = Arc::new(AtomicBool::new(false));
        let second_clone = second_ran.clone();

        // First callback panics
        host.defer_main_thread(Box::new(|_host| {
            panic!("intentional mod main-thread panic");
        }));

        // Second callback succeeds
        host.defer_main_thread(Box::new(move |_host| {
            second_clone.store(true, Ordering::SeqCst);
        }));

        // Process tasks: first panics (contained), second still executes!
        host.process_tasks();
        assert!(second_ran.load(Ordering::SeqCst));
    }

    #[test]
    fn mod_call_attribution_preserved() {
        let host = TestHost::new();
        let reported = Arc::new(Mutex::new(None));
        let rep_clone = reported.clone();

        let guard = crate::crash::enter_native_mod(crate::crash::NativeModCall {
            id: Some("test_mod"),
            index: 0,
            hook: "test_hook",
        });

        host.spawn_background_job(Box::new(move || {
            let current = crate::crash::current_native_mod();
            *rep_clone.lock().unwrap() = current.map(|c| (c.id, c.hook));
        }));
        drop(guard);

        let start = Instant::now();
        while reported.lock().unwrap().is_none() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let res = *reported.lock().unwrap();
        assert_eq!(res, Some((Some("test_mod"), "test_hook")));
    }
}
