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
