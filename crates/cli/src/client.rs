//! HTTP client for the `clockd` API.

use std::time::Duration;

use anyhow::{Result, anyhow};
use chronostack_core::api::{AdvanceRequest, ApiError, ClockState};

/// An HTTP agent with a global timeout that returns 4xx/5xx responses as `Ok`
/// (callers inspect the status themselves).
pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

pub struct Client {
    agent: ureq::Agent,
    base: String,
    token: String,
}

impl Client {
    pub fn new(base: String, token: String) -> Client {
        Client {
            agent: agent(Duration::from_secs(10)),
            base,
            token,
        }
    }

    fn call(&self, method: &str, path: &str, body: Option<&AdvanceRequest>) -> Result<ClockState> {
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {}", self.token);
        let resp = if method == "POST" {
            let req = self.agent.post(&url).header("Authorization", &auth);
            match body {
                Some(b) => req.send_json(b),
                None => req.send_empty(),
            }
        } else {
            self.agent.get(&url).header("Authorization", &auth).call()
        };
        match resp {
            Ok(mut r) if r.status().is_success() => Ok(r.body_mut().read_json()?),
            Ok(mut r) => {
                let msg = r
                    .body_mut()
                    .read_json::<ApiError>()
                    .map(|e| e.error)
                    .unwrap_or_default();
                Err(anyhow!(
                    "clockd returned HTTP {}: {msg}",
                    r.status().as_u16()
                ))
            }
            Err(e) => Err(anyhow!(
                "cannot reach clockd at {} ({e}) — is the stack up? try `chronostack up`",
                self.base
            )),
        }
    }

    pub fn clock(&self) -> Result<ClockState> {
        self.call("GET", "/v1/clock", None)
    }

    pub fn advance_to(&self, to_unix_ns: i64) -> Result<ClockState> {
        self.call(
            "POST",
            "/v1/clock/advance",
            Some(&AdvanceRequest {
                by_ns: None,
                to_unix_ns: Some(to_unix_ns),
            }),
        )
    }

    pub fn freeze(&self) -> Result<ClockState> {
        self.call("POST", "/v1/clock/freeze", None)
    }

    pub fn unfreeze(&self) -> Result<ClockState> {
        self.call("POST", "/v1/clock/unfreeze", None)
    }

    /// Wait for the API to answer (after `up`).
    pub fn wait_ready(&self, timeout: Duration) -> Result<ClockState> {
        let start = std::time::Instant::now();
        loop {
            match self.clock() {
                Ok(st) => return Ok(st),
                Err(e) if start.elapsed() > timeout => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_millis(250)),
            }
        }
    }
}
