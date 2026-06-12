use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::profile::MAX_RECORDS_PER_BATCH;
use crate::sender::encode::{encode_batch, QueuedEvent, SenderConfig};
use crate::sender::queue;
use crate::sender::transport::{post_batch, SenderError};

const STALE_LOCK_AFTER: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrainReport {
    pub sent: usize,
    pub remaining: usize,
    pub skipped_lock: bool,
}

struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn acquire_lock(path: &Path) -> std::io::Result<Option<LockGuard>> {
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => Ok(Some(LockGuard(path.to_path_buf()))),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let stale = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .map(|t| {
                    SystemTime::now().duration_since(t).unwrap_or_default() > STALE_LOCK_AFTER
                })
                .unwrap_or(true);
            if stale {
                let _ = std::fs::remove_file(path);
                match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
                    Ok(_) => Ok(Some(LockGuard(path.to_path_buf()))),
                    Err(_) => Ok(None),
                }
            } else {
                Ok(None)
            }
        }
        Err(e) => Err(e),
    }
}

/// Drain the queue: parse, batch, POST, atomically rewrite survivors.
/// At-least-once: lines are removed ONLY after their batch got a 2xx;
/// a crash between POST and rewrite resends on the next drain.
pub fn drain(cfg: &SenderConfig) -> Result<DrainReport, SenderError> {
    let lock_path = cfg.queue_path.with_extension("lock");
    let Some(_guard) = acquire_lock(&lock_path)? else {
        return Ok(DrainReport { sent: 0, remaining: 0, skipped_lock: true });
    };

    let lines = queue::read_lines(&cfg.queue_path)?;
    if lines.is_empty() {
        return Ok(DrainReport { sent: 0, remaining: 0, skipped_lock: false });
    }
    // unparseable lines are dropped permanently (they can never send)
    let events: Vec<QueuedEvent> = lines
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    let mut sent = 0usize;
    for chunk in events.chunks(MAX_RECORDS_PER_BATCH.min(100)) {
        let req = encode_batch(cfg, chunk);
        let status = post_batch(&cfg.endpoint, &req)?;
        if (200..300).contains(&status) {
            sent += chunk.len();
        } else {
            break; // keep this chunk and everything after it
        }
    }

    let remaining: Vec<String> = events[sent..]
        .iter()
        .map(|e| serde_json::to_string(e).expect("QueuedEvent serializes"))
        .collect();
    queue::rewrite_atomic(&cfg.queue_path, &remaining)?;
    Ok(DrainReport { sent, remaining: remaining.len(), skipped_lock: false })
}
