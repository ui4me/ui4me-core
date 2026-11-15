//! HOST-07: termination requests from outside the application — `SIGTERM`, `SIGINT`, `SIGHUP`
//! on Linux; console control events and `WM_QUERYENDSESSION`/`WM_ENDSESSION` on Windows.
//!
//! The OS-specific watchers ([`crate::platform::install_termination_handlers`]) record a
//! [`Request`] here and wake the engine thread; the host controller turns the first request into
//! an orderly shutdown (`app.shutdown` with a short deadline) on its own thread, and a second one
//! into a force quit. A watcher that must not return before the application has finished (the
//! Windows session end) waits for [`Termination::finished`], which is signalled (through
//! [`FinishOnDrop`] in `main`) only after the host run has returned, its windows are destroyed
//! and its logs are flushed. Signal handlers are process-wide, so a host installs them once per
//! process; tests use their own [`Termination`] values.
//!
//! The orderly path needs a responsive engine thread. So that a hung engine can still be ended,
//! the watchers also run [`Termination::watch`] on a thread of their own (F-25 review #3): it
//! ends the process ([`watchdog_action`]) on the third distinct request, or when the host has
//! not finished [`WATCHDOG_MARGIN`] after the termination deadline.
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Upper bound of the shutdown deadline after a termination request. Process managers and
/// session ends allow only a few seconds (systemd `TimeoutStopSec=5s` for supervised hosts,
/// about 5 s for a Windows console close or session end) before they kill everything.
pub const DEADLINE: Duration = Duration::from_millis(3_000);

/// A further request within this time after the first is treated as the same one (a terminal
/// Ctrl+C plus the launcher's forwarded signal); a later one forces the quit.
pub const REPEAT_GRACE: Duration = Duration::from_millis(1_000);

/// The distinct termination request (repeats within [`REPEAT_GRACE`] merged) that ends the
/// process without the engine thread: the first starts the orderly shutdown, the second forces
/// the quit on the engine thread, the third no longer waits for it.
pub const WATCHDOG_REQUESTS: usize = 3;

/// How long after the termination deadline an unfinished host is ended without the engine
/// thread. The engine itself stops at the deadline when it is responsive.
pub const WATCHDOG_MARGIN: Duration = Duration::from_millis(2_000);

/// What the termination watchdog does next ([`watchdog_action`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchdogAction {
    /// No termination request yet: wait for one.
    Idle,
    /// The host is shutting down: check again after at most this long (or on a new request).
    Wait(Duration),
    /// End the process now with the recorded exit status.
    Exit,
    /// The host finished: nothing left to watch.
    Done,
}

/// The watchdog's decision. `requests` counts distinct termination requests, `since_first` is
/// the time since the first one, `deadline` is the shutdown deadline it started, and `finished`
/// whether the host run has returned ([`Termination::finished`]).
pub fn watchdog_action(
    requests: usize,
    since_first: Duration,
    deadline: Duration,
    finished: bool,
) -> WatchdogAction {
    if finished {
        return WatchdogAction::Done;
    }
    if requests == 0 {
        return WatchdogAction::Idle;
    }
    if requests >= WATCHDOG_REQUESTS {
        return WatchdogAction::Exit;
    }
    let limit = deadline.saturating_add(WATCHDOG_MARGIN);
    match limit.checked_sub(since_first) {
        Some(left) if !left.is_zero() => WatchdogAction::Wait(left),
        _ => WatchdogAction::Exit,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// For logs: `SIGTERM`, `CTRL_C_EVENT`, `WM_ENDSESSION`, …
    pub source: &'static str,
    /// The host's exit status after an orderly shutdown caused by this request.
    pub exit_code: i32,
}

#[derive(Default)]
struct State {
    requests: Vec<Request>,
    finished: bool,
    /// When the first request arrived, and the last one counted as distinct.
    first: Option<Instant>,
    last_distinct: Option<Instant>,
    distinct: usize,
}

#[derive(Default)]
pub struct Termination {
    state: Mutex<State>,
    changed: Condvar,
    wake: Mutex<Option<Wake>>,
    /// The host's exit status cell, once a host watches this instance.
    exit_status: Mutex<Option<Arc<AtomicI32>>>,
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
    /// The exit status the host has recorded so far; the watchdog exits with it.
    pub fn set_exit_status(&self, status: Arc<AtomicI32>) {
        *self.exit_status.lock().unwrap_or_else(|e| e.into_inner()) = Some(status);
    }
    /// Record a request and wake the host. Never called from a signal handler.
    pub fn request(&self, request: Request) {
        self.request_at(request, Instant::now());
    }
    fn request_at(&self, request: Request, at: Instant) {
        {
            let mut state = self.state();
            state.first.get_or_insert(at);
            if state
                .last_distinct
                .is_none_or(|last| at.saturating_duration_since(last) >= REPEAT_GRACE)
            {
                state.last_distinct = Some(at);
                state.distinct += 1;
            }
            state.requests.push(request);
        }
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
    /// The status to exit with when the watchdog ends the process: the host's recorded status
    /// once it handled a request or failed (non-zero), otherwise the first request's.
    fn escalation_exit_code(&self, state: &State) -> i32 {
        let recorded = self
            .exit_status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map_or(0, |status| status.load(Ordering::Relaxed));
        match (recorded, state.requests.first()) {
            (0, Some(first)) => first.exit_code,
            (recorded, _) => recorded,
        }
    }
    /// The termination watchdog (F-25 review #3), run on a thread of its own by the OS watchers:
    /// it never needs the engine thread. Returns when the host finished; otherwise calls `exit`
    /// with the exit status once [`watchdog_action`] says so (and returns, for tests: the real
    /// `exit` does not return).
    pub fn watch(&self, deadline: Duration, exit: &dyn Fn(i32)) {
        let mut state = self.state();
        loop {
            let since_first = state.first.map_or(Duration::ZERO, |first| first.elapsed());
            match watchdog_action(state.distinct, since_first, deadline, state.finished) {
                WatchdogAction::Done => return,
                WatchdogAction::Idle => {
                    state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                WatchdogAction::Wait(left) => {
                    state = self
                        .changed
                        .wait_timeout(state, left)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
                WatchdogAction::Exit => {
                    let code = self.escalation_exit_code(&state);
                    let requests = state.distinct;
                    drop(state);
                    tracing::error!(
                        requests,
                        code,
                        "the host did not finish its shutdown; ending the process"
                    );
                    exit(code);
                    return;
                }
            }
        }
    }
    /// Start [`Self::watch`] with the termination [`DEADLINE`] on its own thread.
    pub fn spawn_watchdog(self: &Arc<Self>, exit: fn(i32)) -> std::io::Result<()> {
        let termination = self.clone();
        std::thread::Builder::new()
            .name("ui4me-termination-watchdog".into())
            .spawn(move || termination.watch(DEADLINE, &exit))
            .map(drop)
    }
}

/// `WM_QUERYENDSESSION` (the values of the Win32 constants, so the mapping is testable on
/// every OS; `platform::windows` asserts they match).
pub const WM_QUERYENDSESSION: u32 = 0x0011;
/// `WM_ENDSESSION`.
pub const WM_ENDSESSION: u32 = 0x0016;

/// How the hidden session window answers a session-end message (F-25 review #5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEndAction {
    /// Answer `TRUE` ("this application may end") and start nothing yet.
    AllowEnd,
    /// The session ends: record the request, hold the process until the host finished, and
    /// answer 0.
    Terminate(Request),
    /// The session end was cancelled: nothing to do; answer 0.
    Cancelled,
}

impl SessionEndAction {
    /// The window procedure's result.
    pub fn result(&self) -> isize {
        match self {
            Self::AllowEnd => 1,
            Self::Terminate(_) | Self::Cancelled => 0,
        }
    }
}

/// The action for a window message, or `None` for any other message (default handling).
pub fn session_end_action(message: u32, wparam: usize) -> Option<SessionEndAction> {
    match message {
        // Only a question: another application or the Restart Manager may still veto the end.
        WM_QUERYENDSESSION => Some(SessionEndAction::AllowEnd),
        WM_ENDSESSION if wparam != 0 => Some(SessionEndAction::Terminate(Request {
            source: "WM_ENDSESSION",
            exit_code: 0,
        })),
        WM_ENDSESSION => Some(SessionEndAction::Cancelled),
        _ => None,
    }
}

/// Signals [`Termination::finished`] when dropped. A run holds one for its whole duration so
/// every exit path (normal return, error, unwinding panic) releases the waiting watchers, and
/// only after the event loop has returned and destroyed its windows (F-25 review #4).
pub struct FinishOnDrop(Arc<Termination>);

impl FinishOnDrop {
    pub fn new(termination: Arc<Termination>) -> Self {
        Self(termination)
    }
}

impl Drop for FinishOnDrop {
    fn drop(&mut self) {
        self.0.finished();
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

    /// F-25 review #3: the watchdog's decision as a pure function.
    #[test]
    fn watchdog_waits_for_the_deadline_and_escalates_on_the_third_request() {
        use WatchdogAction::*;
        let s = Duration::from_secs;
        assert_eq!(watchdog_action(0, s(0), DEADLINE, false), Idle);
        assert_eq!(watchdog_action(0, s(60), DEADLINE, false), Idle);
        assert_eq!(watchdog_action(1, s(0), DEADLINE, false), Wait(s(5)));
        assert_eq!(watchdog_action(2, s(4), DEADLINE, false), Wait(s(1)));
        assert_eq!(watchdog_action(1, s(5), DEADLINE, false), Exit);
        assert_eq!(watchdog_action(1, s(9), DEADLINE, false), Exit);
        assert_eq!(watchdog_action(3, s(0), DEADLINE, false), Exit);
        assert_eq!(watchdog_action(4, s(0), DEADLINE, false), Exit);
        for requests in [0, 1, 3] {
            assert_eq!(watchdog_action(requests, s(9), DEADLINE, true), Done);
        }
    }

    fn sigint() -> Request {
        Request {
            source: "SIGINT",
            exit_code: 130,
        }
    }

    /// Repeats within [`REPEAT_GRACE`] (terminal Ctrl+C plus the launcher's forwarded signal)
    /// are one distinct request for the watchdog, as for the engine.
    #[test]
    fn immediate_repeats_are_one_distinct_request() {
        let termination = Termination::default();
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        for at in [0, 100, 1_200, 1_300] {
            termination.request_at(sigint(), t0 + ms(at));
        }
        assert_eq!(termination.state().distinct, 2);
        assert_eq!(termination.state().requests.len(), 4);
        termination.request_at(sigint(), t0 + ms(2_400));
        assert_eq!(termination.state().distinct, 3);
    }

    /// The watchdog ends the process with the recorded status on the third distinct request and
    /// when the deadline plus margin passed, and leaves a finished host alone.
    #[test]
    fn watchdog_ends_an_unfinished_host() {
        let exited = |termination: &Termination, deadline| {
            let code = std::cell::Cell::new(None);
            termination.watch(deadline, &|c| code.set(Some(c)));
            code.get()
        };
        let t0 = Instant::now() - Duration::from_secs(10);
        let third = Termination::default();
        for at in [0, 2, 4] {
            third.request_at(sigint(), t0 + Duration::from_secs(at));
        }
        assert_eq!(exited(&third, Duration::from_secs(3600)), Some(130));
        let status = Arc::new(AtomicI32::new(1));
        third.set_exit_status(status.clone());
        assert_eq!(exited(&third, Duration::from_secs(3600)), Some(1));

        let overdue = Termination::default();
        overdue.request_at(sigint(), Instant::now() - DEADLINE - WATCHDOG_MARGIN);
        assert_eq!(exited(&overdue, DEADLINE), Some(130));

        let finished = Termination::default();
        finished.request_at(sigint(), Instant::now() - DEADLINE - WATCHDOG_MARGIN);
        finished.finished();
        assert_eq!(exited(&finished, DEADLINE), None);
    }

    /// A host that finishes while the watchdog waits releases it without an exit.
    #[test]
    fn watchdog_returns_when_the_host_finishes_in_time() {
        let termination = Arc::new(Termination::default());
        let exits = Arc::new(AtomicUsize::new(0));
        let (watcher, counter) = (termination.clone(), exits.clone());
        let thread = std::thread::spawn(move || {
            watcher.watch(Duration::from_secs(60), &|_| {
                counter.fetch_add(1, Ordering::SeqCst);
            })
        });
        termination.request(sigint());
        std::thread::sleep(Duration::from_millis(20));
        termination.finished();
        thread.join().unwrap();
        assert_eq!(exits.load(Ordering::SeqCst), 0);
    }

    /// F-25 review #5: `WM_QUERYENDSESSION` is only a question (another application or the
    /// Restart Manager may still veto the logoff): it is answered "may end" without starting the
    /// shutdown. Only `WM_ENDSESSION` with `wParam != 0` (the session really ends) does.
    #[test]
    fn only_a_confirmed_session_end_starts_the_shutdown() {
        use SessionEndAction::*;
        assert_eq!(session_end_action(WM_QUERYENDSESSION, 0), Some(AllowEnd));
        assert_eq!(session_end_action(WM_QUERYENDSESSION, 1), Some(AllowEnd));
        assert_eq!(session_end_action(WM_ENDSESSION, 0), Some(Cancelled));
        assert_eq!(
            session_end_action(WM_ENDSESSION, 1),
            Some(Terminate(Request {
                source: "WM_ENDSESSION",
                exit_code: 0,
            }))
        );
        assert_eq!(session_end_action(0x0010 /* WM_CLOSE */, 1), None);
        assert_eq!(AllowEnd.result(), 1);
        assert_eq!(Cancelled.result(), 0);
        assert_eq!(session_end_action(WM_ENDSESSION, 1).unwrap().result(), 0);
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
