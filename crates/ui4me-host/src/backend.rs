//! One managed application backend. A fixed-size log ring drains pipes even
//! when the GUI is busy; user output cannot become protocol traffic.
use anyhow::Result;
use std::collections::VecDeque;
use std::io::Read;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const LOG_LINES: usize = 512;
const LINE_BYTES: usize = 4096;

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogLine {
    pub stream: String,
    pub text: String,
}

pub struct Backend {
    child: crate::platform::NativeChild,
    logs: Arc<Mutex<VecDeque<LogLine>>>,
    dropped: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
}

impl Backend {
    pub fn spawn(
        config: &crate::config::BackendConfig,
        endpoint: &str,
        token: &str,
        app_id: &str,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let native = crate::platform::spawn_backend(config, endpoint, token, app_id)?;
        let logs = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_LINES)));
        let dropped = Arc::new(AtomicU64::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        for (name, reader) in [("stdout", native.stdout), ("stderr", native.stderr)] {
            let logs = logs.clone();
            let dropped = dropped.clone();
            let stopped = stopped.clone();
            let wake = wake.clone();
            thread::Builder::new()
                .name(format!("ui4me-backend-{name}"))
                .spawn(move || drain(reader, name, logs, dropped, stopped, wake))?;
        }
        Ok(Self {
            child: native.child,
            logs,
            dropped,
            stopped,
        })
    }
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        Ok(self.child.try_wait()?)
    }
    pub fn terminate(&mut self) -> Result<()> {
        Ok(self.child.terminate()?)
    }
    pub fn kill(&mut self) -> Result<()> {
        Ok(self.child.kill()?)
    }
    pub fn drain_logs(&self) -> Vec<LogLine> {
        let mut result: Vec<_> = self
            .logs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        let count = self.dropped.swap(0, Ordering::Relaxed);
        if count != 0 {
            result.insert(0, LogLine { stream: "host".into(), text: format!("{count} backend log records discarded because the bounded log ring was full") });
        }
        result
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.child.kill();
    }
}

fn push(logs: &Mutex<VecDeque<LogLine>>, dropped: &AtomicU64, name: &str, bytes: &[u8]) {
    let mut queue = logs.lock().unwrap_or_else(|e| e.into_inner());
    if queue.len() == LOG_LINES {
        queue.pop_front();
        dropped.fetch_add(1, Ordering::Relaxed);
    }
    queue.push_back(LogLine {
        stream: name.to_owned(),
        text: String::from_utf8_lossy(bytes)
            .trim_end_matches('\r')
            .to_owned(),
    });
}

fn drain(
    mut reader: Box<dyn Read + Send>,
    name: &str,
    logs: Arc<Mutex<VecDeque<LogLine>>>,
    dropped: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let mut buffer = [0_u8; 8192];
    let mut line = Vec::with_capacity(LINE_BYTES);
    let mut last_wake = Instant::now() - Duration::from_secs(1);
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(length) => {
                for byte in &buffer[..length] {
                    if *byte == b'\n' {
                        push(&logs, &dropped, name, &line);
                        line.clear();
                    } else {
                        line.push(*byte);
                        if line.len() == LINE_BYTES {
                            // Carry an incomplete UTF-8 code point into the
                            // next bounded record instead of corrupting valid
                            // output at an arbitrary byte boundary.
                            let prefix = match std::str::from_utf8(&line) {
                                Err(error) if error.error_len().is_none() => error.valid_up_to(),
                                _ => line.len(),
                            };
                            push(&logs, &dropped, name, &line[..prefix]);
                            line.drain(..prefix);
                        }
                    }
                }
                if last_wake.elapsed() >= Duration::from_millis(50) {
                    wake();
                    last_wake = Instant::now();
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        if stopped.load(Ordering::Acquire) {
            break;
        }
    }
    if !line.is_empty() {
        push(&logs, &dropped, name, &line);
    }
    wake();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_flood_is_drained_and_bounded_without_newlines() {
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let dropped = Arc::new(AtomicU64::new(0));
        let bytes = vec![b'x'; LINE_BYTES * (LOG_LINES + 8)];
        drain(
            Box::new(std::io::Cursor::new(bytes)),
            "stdout",
            logs.clone(),
            dropped.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(|| {}),
        );
        assert_eq!(logs.lock().unwrap().len(), LOG_LINES);
        assert_eq!(dropped.load(Ordering::Relaxed), 8);
    }

    #[test]
    fn unicode_survives_a_record_byte_boundary() {
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let dropped = Arc::new(AtomicU64::new(0));
        let value = format!("{}é😀\n", "x".repeat(LINE_BYTES - 1));
        drain(
            Box::new(std::io::Cursor::new(value.as_bytes().to_vec())),
            "stdout",
            logs.clone(),
            dropped,
            Arc::new(AtomicBool::new(false)),
            Arc::new(|| {}),
        );
        let combined = logs
            .lock()
            .unwrap()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>();
        assert_eq!(combined, value.trim_end_matches('\n'));
    }
}
