//! `clockd` HTTP API v1 types (JSON).

use serde::{Deserialize, Serialize};

/// Snapshot of the virtual clock, returned by every clock endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockState {
    /// Virtual wall-clock time, Unix nanoseconds.
    pub virtual_ns: i64,
    /// Real wall-clock time of the controller, Unix nanoseconds.
    pub real_ns: i64,
    /// `virtual_ns - real_ns` (the human-meaningful offset).
    pub offset_ns: i64,
    pub frozen: bool,
    pub generation: u64,
    /// Hex session id of the clock page.
    pub session_id: String,
    pub layout_version: u32,
}

/// `POST /v1/clock/advance`. Exactly one field must be set; time only moves forward.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvanceRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_ns: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_unix_ns: Option<i64>,
}

/// Error body for non-2xx responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

/// Largest single advance accepted (100 years).
pub const MAX_ADVANCE_NS: i64 = 100 * 366 * 24 * 3600 * 1_000_000_000;

/// First port `clockd` uses for proxy targets inside the Compose network
/// (target *i* listens on `PROXY_BASE_PORT + i`). Never published to the host.
pub const PROXY_BASE_PORT: u16 = 7200;
/// Most proxy targets one session may declare.
pub const MAX_PROXY_TARGETS: usize = 32;
/// Events kept by `clockd` (ring buffer).
pub const EVENT_RING: usize = 1000;

/// An HTTP egress target routed through the `clockd` fault proxy. Passed to
/// `clockd` as JSON in `CHRONOSTACK_PROXIES`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyTarget {
    pub name: String,
    pub listen_port: u16,
    /// `http://host:port` inside the Compose network (no path).
    pub upstream: String,
}

/// An outage window on a proxy target, in virtual time: requests arriving while
/// `from_ns <= virtual now < to_ns` get `status` instead of being forwarded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    pub id: u64,
    pub target: String,
    pub from_ns: i64,
    pub to_ns: i64,
    pub status: u16,
}

/// `POST /v1/faults`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultRequest {
    pub target: String,
    pub from_ns: i64,
    pub to_ns: i64,
    /// HTTP status returned during the window (default 503).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

/// `GET /v1/faults`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultList {
    pub targets: Vec<ProxyTarget>,
    pub faults: Vec<Fault>,
}

/// One entry of the controller's event log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Increasing sequence number (starts at 1).
    pub seq: u64,
    pub real_ns: i64,
    pub virtual_ns: i64,
    pub generation: u64,
    /// `advance`, `freeze`, `unfreeze`, `fault_set`, `fault_clear`, `proxy`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// For `proxy`: `forwarded`, `fault` (outage window) or `upstream_error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// `GET /v1/events?since=<seq>`: events with `seq > since`, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsResponse {
    pub events: Vec<Event>,
    /// Highest sequence number assigned so far.
    pub last_seq: u64,
    /// True if events after `since` were already evicted from the ring buffer.
    pub truncated: bool,
}
