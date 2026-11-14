//! Event log: a bounded ring buffer of clock changes, fault changes and proxied
//! requests. The CLI reads it for reports (`GET /v1/events?since=`).

use std::collections::VecDeque;
use std::sync::Mutex;

use chronostack_core::api::{ClockState, EVENT_RING, Event, EventsResponse};

use crate::clock::real_ns;

pub struct Events {
    inner: Mutex<Inner>,
}

struct Inner {
    ring: VecDeque<Event>,
    last_seq: u64,
}

impl Events {
    pub fn new() -> Events {
        Events {
            inner: Mutex::new(Inner {
                ring: VecDeque::with_capacity(EVENT_RING),
                last_seq: 0,
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
}
