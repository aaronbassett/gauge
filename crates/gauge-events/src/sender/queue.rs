//! Append-only JSONL disk queue, modeled on Tome's telemetry queue:
//! one O_APPEND write per event, hard caps, atomic rewrite after delivery.

use std::io::Write as _;
use std::path::Path;

pub const MAX_LINE_BYTES: usize = 4096;
pub const MAX_QUEUE_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Appended,
    DroppedTooLong,
    DroppedQueueFull,
}

pub fn append_line(path: &Path, line: &str) -> std::io::Result<AppendOutcome> {
    if line.len() > MAX_LINE_BYTES {
        return Ok(AppendOutcome::DroppedTooLong);
    }
    let current = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if current + line.len() as u64 + 1 > MAX_QUEUE_BYTES {
        return Ok(AppendOutcome::DroppedQueueFull);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    f.write_all(&buf)?;
    Ok(AppendOutcome::Appended)
}

pub fn read_lines(path: &Path) -> std::io::Result<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s.lines().map(str::to_string).filter(|l| !l.is_empty()).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e),
    }
}

/// Write remaining lines to a temp file, then rename over the queue.
/// Crash before rename → old queue intact (resend = at-least-once).
pub fn rewrite_atomic(path: &Path, remaining: &[String]) -> std::io::Result<()> {
    let tmp = path.with_extension("jsonl.tmp");
    let mut content = String::new();
    for l in remaining {
        content.push_str(l);
        content.push('\n');
    }
    std::fs::write(&tmp, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_then_read_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        assert!(matches!(append_line(&q, "{\"a\":1}").unwrap(), AppendOutcome::Appended));
        assert!(matches!(append_line(&q, "{\"b\":2}").unwrap(), AppendOutcome::Appended));
        assert_eq!(read_lines(&q).unwrap(), vec!["{\"a\":1}", "{\"b\":2}"]);
    }

    #[test]
    fn oversized_line_is_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        let big = "x".repeat(MAX_LINE_BYTES + 1);
        assert!(matches!(append_line(&q, &big).unwrap(), AppendOutcome::DroppedTooLong));
        assert!(read_lines(&q).unwrap().is_empty());
    }

    #[test]
    fn full_queue_drops_new_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        let line = "y".repeat(MAX_LINE_BYTES - 1);
        loop {
            match append_line(&q, &line).unwrap() {
                AppendOutcome::Appended => continue,
                AppendOutcome::DroppedQueueFull => break,
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(std::fs::metadata(&q).unwrap().len() <= MAX_QUEUE_BYTES);
    }

    #[test]
    fn rewrite_atomic_replaces_content() {
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        append_line(&q, "one").unwrap();
        append_line(&q, "two").unwrap();
        rewrite_atomic(&q, &["two".to_string()]).unwrap();
        assert_eq!(read_lines(&q).unwrap(), vec!["two"]);
        rewrite_atomic(&q, &[]).unwrap();
        assert!(read_lines(&q).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn queue_file_is_0600() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let q = tmp.path().join("queue.jsonl");
        append_line(&q, "z").unwrap();
        assert_eq!(std::fs::metadata(&q).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
