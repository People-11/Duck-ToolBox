//! Command history in `duckd.log`, one JSON line per invocation.
//!
//! Only the outcome is recorded (command, status, error), never the `data` payload, so
//! profile secrets and device identifiers do not end up in a world-readable log.

use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{paths::AppPaths, unix_now};

const MAX_LOG_BYTES: u64 = 512 * 1024;
/// Bound log responses for native WebUI stdout bridges.
pub const MAX_LOG_PAGE_SIZE: usize = 20;
const MAX_LOG_MESSAGE_CHARS: usize = 512;

#[derive(Debug, Serialize)]
pub struct LogPage {
    pub entries: Vec<LogEntry>,
    pub next_offset: Option<usize>,
}

fn bound_message(message: &mut Option<String>) {
    if let Some(message) = message
        && let Some((byte, _)) = message.char_indices().nth(MAX_LOG_MESSAGE_CHARS)
    {
        message.truncate(byte);
        message.push('…');
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: u64,
    pub command: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl LogEntry {
    pub fn from_envelope(envelope: &Value) -> Self {
        let text = |value: &Value| value.as_str().map(str::to_owned);
        Self {
            ts: envelope["ts"].as_u64().unwrap_or_else(unix_now),
            command: text(&envelope["command"]).unwrap_or_default(),
            ok: envelope["ok"].as_bool().unwrap_or(false),
            code: text(&envelope["error"]["code"]),
            message: text(&envelope["error"]["message"]),
        }
    }
}

/// Appends one entry, rotating the log once it grows past 512 KiB. Logging is best effort:
/// failures never affect the command result.
pub fn append(paths: &AppPaths, envelope: &Value) {
    if paths.ensure_runtime_dirs().is_err() {
        return;
    }
    rotate_if_needed(paths);

    let mut entry = LogEntry::from_envelope(envelope);
    bound_message(&mut entry.message);
    let Ok(line) = serde_json::to_string(&entry) else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log_path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// The newest `limit` entries, newest first. Older releases logged whole envelopes; those
/// lines are reduced to entries, and anything unparsable is skipped.
pub fn tail(paths: &AppPaths, limit: usize) -> Result<Vec<LogEntry>> {
    let mut entries = read_entries(paths)?;
    entries.truncate(limit);
    Ok(entries)
}

/// A bounded slice of history. The next offset is absent at the end of the log.
pub fn page(paths: &AppPaths, limit: usize, offset: usize) -> Result<LogPage> {
    let entries = read_entries(paths)?;
    let end = offset
        .saturating_add(limit.clamp(1, MAX_LOG_PAGE_SIZE))
        .min(entries.len());
    Ok(LogPage {
        entries: entries.get(offset..end).unwrap_or(&[]).to_vec(),
        next_offset: (offset < entries.len() && end < entries.len()).then_some(end),
    })
}

fn read_entries(paths: &AppPaths) -> Result<Vec<LogEntry>> {
    let file = match std::fs::File::open(&paths.log_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut entries: Vec<LogEntry> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .filter_map(|value| {
            let mut entry = if value.get("error").is_some() || value.get("data").is_some() {
                LogEntry::from_envelope(&value)
            } else {
                serde_json::from_value(value).ok()?
            };
            // Legacy logs may contain unbounded error strings.
            bound_message(&mut entry.message);
            Some(entry)
        })
        .collect();
    entries.reverse();
    Ok(entries)
}

fn rotate_if_needed(paths: &AppPaths) {
    let Ok(metadata) = std::fs::metadata(&paths.log_path) else {
        return;
    };
    if metadata.len() < MAX_LOG_BYTES {
        return;
    }

    let rotated = paths.logs_dir.join("duckd.log.1");
    let _ = std::fs::remove_file(&rotated);
    let _ = std::fs::rename(&paths.log_path, rotated);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{MAX_LOG_MESSAGE_CHARS, MAX_LOG_PAGE_SIZE, append, page, tail};
    use crate::AppPaths;

    #[test]
    fn bounded_pages_and_utf8_errors() {
        let dir = std::env::temp_dir().join(format!("duck-log-pages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = AppPaths::for_root(&dir, &dir);
        for ts in 0..45 {
            append(
                &paths,
                &json!({
                    "ok": false, "command": "demo",
                    "error": { "code": "failed", "message": "界".repeat(900) },
                    "ts": ts,
                }),
            );
        }
        let first = page(&paths, 200, 0).unwrap();
        assert_eq!(first.entries.len(), MAX_LOG_PAGE_SIZE);
        assert_eq!(first.entries[0].ts, 44);
        assert_eq!(first.next_offset, Some(20));
        assert_eq!(
            first.entries[0].message.as_ref().unwrap().chars().count(),
            MAX_LOG_MESSAGE_CHARS + 1
        );
        assert!(first.entries[0].message.as_ref().unwrap().ends_with('…'));
        let second = page(&paths, 20, 20).unwrap();
        assert_eq!(second.entries[0].ts, 24);
        assert_eq!(second.next_offset, Some(40));
        let last = page(&paths, 20, 40).unwrap();
        assert_eq!(last.entries.len(), 5);
        assert_eq!(last.next_offset, None);
        assert!(page(&paths, 20, usize::MAX).unwrap().entries.is_empty());
        assert_eq!(page(&paths, 0, 0).unwrap().entries.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn records_outcomes_without_payloads() {
        let dir = std::env::temp_dir().join(format!("duck-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = AppPaths::for_root(&dir, &dir);

        append(
            &paths,
            &json!({ "ok": true, "command": "rkp.profile.show", "data": { "seed_hex": "secret" }, "ts": 1 }),
        );
        append(
            &paths,
            &json!({ "ok": false, "command": "rkp.keybox", "error": { "code": "bad", "message": "nope" }, "ts": 2 }),
        );

        let raw = std::fs::read_to_string(&paths.log_path).unwrap();
        assert!(!raw.contains("secret"));

        let entries = tail(&paths, 10).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].command, "rkp.keybox");
        assert_eq!(entries[0].code.as_deref(), Some("bad"));
        assert!(entries[1].ok);
        assert_eq!(tail(&paths, 1).unwrap().len(), 1);
    }
}
