//! Event log: a bounded ring buffer of clock changes, fault changes and proxied
//! requests. The CLI reads it for reports (`GET /v1/events?since=`).
//!
//! The log is also appended to a JSON-lines file next to the clock page, so a
//! `clockd` restart keeps it (and its sequence numbers). The file is compacted
//! to the last [`EVENT_RING`] events whenever it grows to twice that.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use chronostack_core::api::{ClockState, EVENT_RING, Event, EventsResponse};

use crate::clock::real_ns;

pub struct Events {
    inner: Mutex<Inner>,
}

struct Inner {
    ring: VecDeque<Event>,
    last_seq: u64,
    path: Option<PathBuf>,
    file: Option<File>,
    /// Lines in the file (compaction trigger).
    lines: usize,
}

impl Events {
    /// In-memory only (tests).
    #[cfg(test)]
    pub fn new() -> Events {
        Events::open(None)
    }

    /// Load the persisted log from `path` (if any) and keep appending to it.
    /// Unreadable lines are skipped; a missing file starts an empty log.
    pub fn open(path: Option<PathBuf>) -> Events {
        let mut ring = VecDeque::with_capacity(EVENT_RING);
        let mut lines = 0;
        if let Some(f) = path.as_ref().and_then(|p| File::open(p).ok()) {
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                lines += 1;
                if let Ok(e) = serde_json::from_str::<Event>(&line) {
                    if ring.len() == EVENT_RING {
                        ring.pop_front();
                    }
                    ring.push_back(e);
                }
            }
        }
        let last_seq = ring.back().map(|e: &Event| e.seq).unwrap_or(0);
        let file = path
            .as_ref()
            .and_then(|p| OpenOptions::new().create(true).append(true).open(p).ok());
        Events {
            inner: Mutex::new(Inner {
                ring,
                last_seq,
                path,
                file,
                lines,
            }),
        }
    }

    /// Record an event. `seq`, `real_ns`, `virtual_ns` and `generation` are filled in here.
    pub fn push(&self, st: &ClockState, mut e: Event) -> Event {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        g.last_seq += 1;
        e.seq = g.last_seq;
        e.real_ns = real_ns();
        e.virtual_ns = st.virtual_ns;
        e.generation = st.generation;
        if g.ring.len() == EVENT_RING {
            g.ring.pop_front();
        }
        g.ring.push_back(e.clone());
        persist(&mut g, &e);
        e
    }

    pub fn since(&self, since: u64) -> EventsResponse {
        let g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let oldest = g
            .ring
            .front()
            .map(|e| e.seq)
            .unwrap_or(g.last_seq.saturating_add(1));
        EventsResponse {
            events: g.ring.iter().filter(|e| e.seq > since).cloned().collect(),
            last_seq: g.last_seq,
            truncated: since.saturating_add(1) < oldest && since < g.last_seq,
        }
    }
}

/// Append one event to the file; rewrite it with only the ring once it holds
/// twice as many lines. Failures are ignored: the in-memory log stays correct.
fn persist(g: &mut Inner, e: &Event) {
    let Ok(line) = serde_json::to_string(e) else {
        return;
    };
    if let Some(f) = g.file.as_mut() {
        if writeln!(f, "{line}").is_ok() {
            g.lines += 1;
        }
    }
    if g.lines >= 2 * EVENT_RING {
        if let Some(p) = g.path.clone() {
            let tmp = p.with_extension("tmp");
            let body: String = g
                .ring
                .iter()
                .filter_map(|e| serde_json::to_string(e).ok())
                .map(|l| l + "\n")
                .collect();
            if fs::write(&tmp, body).is_ok() && fs::rename(&tmp, &p).is_ok() {
                g.lines = g.ring.len();
                g.file = OpenOptions::new().append(true).open(&p).ok();
            }
        }
    }
}

/// An event with only `kind` set; callers fill the optional fields.
pub fn event(kind: &str) -> Event {
    Event {
        seq: 0,
        real_ns: 0,
        virtual_ns: 0,
        generation: 0,
        kind: kind.into(),
        target: None,
        method: None,
        path: None,
        status: None,
        outcome: None,
        fault_id: None,
        latency_ms: None,
        message: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(v: i64) -> ClockState {
        ClockState {
            virtual_ns: v,
            real_ns: 0,
            offset_ns: 0,
            frozen: false,
            generation: 7,
            session_id: String::new(),
            layout_version: 2,
        }
    }

    #[test]
    fn ring_keeps_last_events_and_reports_truncation() {
        let ev = Events::new();
        for i in 0..(EVENT_RING as i64 + 5) {
            ev.push(&st(i), event("advance"));
        }
        let all = ev.since(0);
        assert_eq!(all.events.len(), EVENT_RING);
        assert_eq!(all.events[0].seq, 6);
        assert_eq!(all.last_seq, EVENT_RING as u64 + 5);
        assert!(all.truncated, "events 1..5 were evicted");
        let tail = ev.since(all.last_seq - 2);
        assert_eq!(tail.events.len(), 2);
        assert!(!tail.truncated);
        assert_eq!(tail.events[1].generation, 7);
        assert!(ev.since(all.last_seq).events.is_empty());
    }

    #[test]
    fn survives_a_restart_and_compacts() {
        let dir = std::env::temp_dir().join(format!("cs-events-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        let _ = fs::remove_file(&path);
        {
            let ev = Events::open(Some(path.clone()));
            for i in 0..3 {
                ev.push(&st(i), event("advance"));
            }
        }
        let ev = Events::open(Some(path.clone()));
        let all = ev.since(0);
        assert_eq!(all.events.len(), 3, "reloaded from the file");
        assert_eq!(
            ev.push(&st(9), event("freeze")).seq,
            4,
            "sequence continues"
        );

        for i in 0..(2 * EVENT_RING as i64) {
            ev.push(&st(i), event("advance"));
        }
        let lines = fs::read_to_string(&path).unwrap().lines().count();
        assert!(lines < 2 * EVENT_RING, "compacted: {lines} lines");
        let reloaded = Events::open(Some(path.clone()));
        assert_eq!(reloaded.since(0).last_seq, ev.since(0).last_seq);
        assert_eq!(reloaded.since(0).events.len(), EVENT_RING);
        let _ = fs::remove_dir_all(dir);
    }
}
