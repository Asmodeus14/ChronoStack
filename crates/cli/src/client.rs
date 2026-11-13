//! HTTP client for the `clockd` API.

use std::time::Duration;

use anyhow::{Result, anyhow};
use chronostack_core::api::{AdvanceRequest, ApiError, ClockState};

pub struct Client {
    agent: ureq::Agent,
    base: String,
    token: String,
}

impl Client {
    pub fn new(base: String, token: String) -> Client {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(10))
            .build();
        Client { agent, base, token }
    }

    fn call(&self, method: &str, path: &str, body: Option<&AdvanceRequest>) -> Result<ClockState> {
        let req = self
            .agent
            .request(method, &format!("{}{path}", self.base))
            .set("Authorization", &format!("Bearer {}", self.token));
        let resp = match body {
            Some(b) => req.send_json(b),
            None if method == "POST" => req.send_string(""),
            None => req.call(),
        };
        match resp {
            Ok(r) => Ok(r.into_json()?),
            Err(ureq::Error::Status(code, r)) => {
                let msg = r
                    .into_json::<ApiError>()
                    .map(|e| e.error)
                    .unwrap_or_default();
                Err(anyhow!("clockd returned HTTP {code}: {msg}"))
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

    pub fn advance(&self, by_ns: i64) -> Result<ClockState> {
        self.call(
            "POST",
            "/v1/clock/advance",
            Some(&AdvanceRequest {
                by_ns: Some(by_ns),
                to_unix_ns: None,
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
