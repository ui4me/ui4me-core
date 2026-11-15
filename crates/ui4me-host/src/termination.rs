//! HOST-07: termination requests from outside the application — `SIGTERM`, `SIGINT`, `SIGHUP`
//! on Linux; console control events and `WM_QUERYENDSESSION`/`WM_ENDSESSION` on Windows.
//!
//! The OS-specific watchers ([`crate::platform::install_termination_handlers`]) only record a
//! [`Request`] here and wake the engine thread; the host controller turns the first request into
//! an orderly shutdown (`app.shutdown` with a short deadline) on its own thread, and a second one
//! into a force quit. A watcher that must not return before the application has finished (the
//! Windows session end) waits for [`Termination::finished`], which the host signals when it
//! stops. Signal handlers are process-wide, so a host installs them once per process; tests use
//! their own [`Termination`] values.
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Upper bound of the shutdown deadline after a termination request. Process managers and
/// session ends allow only a few seconds (systemd `TimeoutStopSec=5s` for supervised hosts,
/// about 5 s for a Windows console close or session end) before they kill everything.
pub const DEADLINE: Duration = Duration::from_millis(3_000);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// For logs: `SIGTERM`, `CTRL_C_EVENT`, `WM_QUERYENDSESSION`, …
    pub source: &'static str,
    /// The host's exit status after an orderly shutdown caused by this request.
    pub exit_code: i32,
}

#[derive(Default)]
struct State {
    requests: Vec<Request>,
    finished: bool,
}

#[derive(Default)]
pub struct Termination {
    state: Mutex<State>,
    changed: Condvar,
    wake: Mutex<Option<Wake>>,
}

impl Termination {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// The process-wide instance the OS watchers report to.
    pub fn global() -> Arc<Termination> {
        static GLOBAL: OnceLock<Arc<Termination>> = OnceLock::new();
        GLOBAL.get_or_init(Default::default).clone()
    }
    /// Who is woken for a new request (the host's event-loop proxy).
    pub fn set_wake(&self, wake: Wake) {
        *self.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(wake);
    }
    /// Record a request and wake the host. Never called from a signal handler.
    pub fn request(&self, request: Request) {
        self.state().requests.push(request);
        self.changed.notify_all();
        let wake = self.wake.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(wake) = wake {
            wake();
        }
    }
    /// Requests after the first `seen` ones; advances `seen`.
    pub fn take_new(&self, seen: &mut usize) -> Vec<Request> {
        let state = self.state();
        let new = state.requests.get(*seen..).unwrap_or_default().to_vec();
        *seen = state.requests.len();
        new
    }
    /// The host has stopped (or is about to exit): release waiting watchers.
    pub fn finished(&self) {
        self.state().finished = true;
        self.changed.notify_all();
    }
    /// Wait until [`Self::finished`] or `timeout`; returns whether the host finished.
    pub fn wait_finished(&self, timeout: Duration) -> bool {
        let state = self.state();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, timeout, |s| !s.finished)
            .unwrap_or_else(|e| e.into_inner());
        state.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn requests_wake_once_each_and_are_taken_once() {
        let termination = Termination::default();
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = wakes.clone();
        termination.set_wake(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let mut seen = 0;
        assert!(termination.take_new(&mut seen).is_empty());
        let term = Request {
            source: "SIGTERM",
            exit_code: 143,
        };
        termination.request(term.clone());
        assert_eq!(termination.take_new(&mut seen), std::slice::from_ref(&term));
        assert!(termination.take_new(&mut seen).is_empty());
        termination.request(term.clone());
        assert_eq!(termination.take_new(&mut seen).len(), 1);
        assert_eq!(wakes.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_waiting_watcher_is_released_when_the_host_finishes() {
        let termination = Arc::new(Termination::default());
        assert!(!termination.wait_finished(Duration::from_millis(10)));
        let waiter = termination.clone();
        let thread = std::thread::spawn(move || waiter.wait_finished(Duration::from_secs(10)));
        std::thread::sleep(Duration::from_millis(20));
        termination.finished();
        assert!(thread.join().unwrap());
    }
}
