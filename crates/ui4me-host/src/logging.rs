//! Bounded logging for an event-loop process. Neither a verbose backend nor
//! a slow or unread redirected stderr pipe can block the GUI's log writes.

use crate::backend::LogLine;
use anyhow::Result;
use std::io::Write;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tracing_appender::non_blocking::{ErrorCounter, NonBlocking, NonBlockingBuilder, WorkerGuard};

const BUFFERED_RECORDS: usize = 128;
const BACKEND_RECORDS_PER_PUMP: usize = 32;
static SINK_DROPPED: OnceLock<ErrorCounter> = OnceLock::new();
static BACKEND_OMITTED: AtomicU64 = AtomicU64::new(0);

fn writer<T: Write + Send + 'static>(output: T) -> (NonBlocking, WorkerGuard) {
    NonBlockingBuilder::default()
        .buffered_lines_limit(BUFFERED_RECORDS)
        .lossy(true)
        .thread_name("ui4me-log-writer")
        .finish(output)
}

/// Retain this until the native event loop has stopped. Flushing is bounded;
/// a broken sink is allowed to lose log records instead of retaining an app.
pub struct LogGuard(Option<WorkerGuard>);

impl Drop for LogGuard {
    fn drop(&mut self) {
        let Some(guard) = self.0.take() else {
            return;
        };
        // Upstream's WorkerGuard has bounded channel waits, but its timeout
        // diagnostic writes synchronously to stdout. Dispose it away from the
        // caller too, so even two blocked output pipes cannot stall shutdown.
        // ManuallyDrop intentionally leaks the guard if spawning this final
        // helper fails: the process is exiting and must not perform blocking IO.
        let guard = ManuallyDrop::new(guard);
        let (finished, wait) = std::sync::mpsc::sync_channel(1);
        if std::thread::Builder::new()
            .name("ui4me-log-shutdown".into())
            .spawn(move || {
                drop(ManuallyDrop::into_inner(guard));
                let _ = finished.send(());
            })
            .is_ok()
        {
            let _ = wait.recv_timeout(Duration::from_millis(1500));
        }
    }
}

/// Initialize JSON diagnostics after environment setup and before the GUI.
/// Libraries embedding the host may install their own subscriber instead.
pub fn init() -> Result<LogGuard> {
    let (output, guard) = writer(std::io::stderr());
    let dropped = output.error_counter();
    let guard = LogGuard(Some(guard));
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ui4me_host=info".into()),
        )
        .json()
        .with_ansi(false)
        .with_writer(output)
        .try_init()
        .map_err(|error| anyhow::anyhow!("initialize structured logging: {error}"))?;
    let _ = SINK_DROPPED.set(dropped);
    Ok(guard)
}

/// Limit formatting/queue work per pump as well as the output queue itself.
/// Backend pipes are already continuously drained by their own reader threads.
pub fn backend_logs(lines: Vec<LogLine>) {
    let omitted = lines.len().saturating_sub(BACKEND_RECORDS_PER_PUMP);
    for line in lines.into_iter().take(BACKEND_RECORDS_PER_PUMP) {
        tracing::info!(source = "backend", stream = %line.stream, text = %line.text, "backend output");
    }
    if omitted != 0 {
        BACKEND_OMITTED.fetch_add(omitted as u64, Ordering::Relaxed);
        tracing::warn!(
            source = "backend",
            omitted_records = omitted,
            "backend log batch exceeded the GUI formatting budget"
        );
    }
}

pub fn diagnostics() -> serde_json::Value {
    serde_json::json!({
        "sink_dropped_records": SINK_DROPPED.get().map_or(0, ErrorCounter::dropped_lines),
        "backend_omitted_records": BACKEND_OMITTED.load(Ordering::Relaxed),
        "sink_capacity_records": BUFFERED_RECORDS,
        "backend_records_per_pump": BACKEND_RECORDS_PER_PUMP,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    struct BlockedOutput {
        started: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
        first: bool,
    }
    impl Write for BlockedOutput {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.first {
                self.first = false;
                self.started.send(()).unwrap();
                self.release.recv().unwrap();
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_blocked_output_drops_excess_records_without_stalling_the_caller() {
        let (started, waiting) = mpsc::sync_channel(1);
        let (release, blocked) = mpsc::sync_channel(1);
        let (mut output, guard) = writer(BlockedOutput {
            started,
            release: blocked,
            first: true,
        });
        output.write_all(b"first\n").unwrap();
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        let began = Instant::now();
        for _ in 0..1024 {
            output.write_all(b"busy backend\n").unwrap();
        }
        let elapsed = began.elapsed();
        let dropped = output.error_counter().dropped_lines();
        release.send(()).unwrap();
        drop(output);
        drop(LogGuard(Some(guard)));
        assert!(elapsed < Duration::from_secs(1));
        assert_eq!(dropped, 1024 - BUFFERED_RECORDS);
    }
}
