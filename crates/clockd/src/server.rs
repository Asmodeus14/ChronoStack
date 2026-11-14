//! Minimal HTTP/1.1 API (v1). Bound inside the container; published by Compose
//! on `127.0.0.1` only. Every endpoint except `/v1/health` needs the bearer token.

use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use chronostack_core::api::{
    AdvanceRequest, ApiError, ClockState, FaultList, FaultRequest, MAX_ADVANCE_NS, ProxyTarget,
};
use chronostack_core::duration::format_utc;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::clock::Clock;
use crate::events::{Events, event};
use crate::faults::Faults;
use crate::proxy;

pub fn log(line: &str) {
    println!("{line}");
}

type Reply = (u16, String);

/// Everything the API and the proxies share.
pub struct Ctx<'a> {
    pub clock: &'a Clock,
    pub token: &'a str,
    pub events: &'a Events,
    pub faults: &'a Faults,
    pub targets: &'a [ProxyTarget],
    pub changes: &'a Changes,
}

/// Wakes `/v1/clock/wait` long-polls when the clock changes.
pub struct Changes {
    lock: Mutex<()>,
    cv: Condvar,
    waiters: AtomicUsize,
}

/// Concurrent long-polls served (each holds a thread).
const MAX_WAITERS: usize = 64;
/// Longest a long-poll is held before it returns the unchanged state.
const MAX_WAIT: Duration = Duration::from_secs(30);

impl Changes {
    pub fn new() -> Changes {
        Changes {
            lock: Mutex::new(()),
            cv: Condvar::new(),
            waiters: AtomicUsize::new(0),
        }
    }

    fn notify(&self) {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        self.cv.notify_all();
    }

    /// Block until the clock generation differs from `after` or `timeout` passes.
    fn wait(&self, clock: &Clock, after: u64, timeout: Duration) -> ClockState {
        let deadline = Instant::now() + timeout.min(MAX_WAIT);
        let mut g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            let st = clock.state();
            let left = deadline.saturating_duration_since(Instant::now());
            if st.generation != after || left.is_zero() {
                return st;
            }
            g = self
                .cv
                .wait_timeout(g, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

fn json<T: serde::Serialize>(code: u16, v: &T) -> Reply {
    (code, serde_json::to_string(v).expect("serializable"))
}

fn ok(st: &ClockState) -> Reply {
    json(200, st)
}

fn err(code: u16, msg: impl Into<String>) -> Reply {
    json(code, &ApiError { error: msg.into() })
}

/// Constant-time comparison so the token can't be guessed byte by byte.
fn token_matches(given: &[u8], expected: &[u8]) -> bool {
    if given.len() != expected.len() {
        return false;
    }
    given
        .iter()
        .zip(expected)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn authorized(req: &Request, token: &str) -> bool {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .and_then(|h| h.value.as_str().strip_prefix("Bearer "))
        .is_some_and(|t| token_matches(t.as_bytes(), token.as_bytes()))
}

fn read_body(req: &mut Request) -> Result<String, Reply> {
    let mut body = String::new();
    req.as_reader()
        .take(64 * 1024)
        .read_to_string(&mut body)
        .map_err(|_| err(400, "unreadable body"))?;
    Ok(body)
}

/// Record a clock change in both the container log and the event log.
fn clock_event(ctx: &Ctx<'_>, kind: &str, extra: &str) -> Reply {
    let st = ctx.clock.state();
    log(&format!(
        r#"{{"event":"{kind}"{extra},"virtual_ns":{},"generation":{}}}"#,
        st.virtual_ns, st.generation
    ));
    ctx.events.push(&st, event(kind));
    ctx.changes.notify();
    ok(&st)
}

fn query_u64(url: &str, key: &str) -> Option<u64> {
    url.split_once('?')?
        .1
        .split('&')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
        .and_then(|v| v.parse().ok())
}

fn handle(ctx: &Ctx<'_>, req: &mut Request) -> Reply {
    let clock = ctx.clock;
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();
    if method == Method::Get && path == "/v1/health" {
        return (
            200,
            format!(r#"{{"ok":true,"version":"{}"}}"#, env!("CARGO_PKG_VERSION")),
        );
    }
    if !authorized(req, ctx.token) {
        return err(401, "missing or invalid bearer token");
    }
    match (method, path.as_str()) {
        (Method::Get, "/v1/clock") => ok(&clock.state()),
        (Method::Post, "/v1/clock/advance") => {
            let body = match read_body(req) {
                Ok(b) => b,
                Err(r) => return r,
            };
            let ar: AdvanceRequest = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => return err(400, format!("invalid JSON: {e}")),
            };
            let by = match (ar.by_ns, ar.to_unix_ns) {
                (Some(by), None) => by,
                (None, Some(to)) => to - clock.state().virtual_ns,
                _ => return err(400, "set exactly one of `by_ns` or `to_unix_ns`"),
            };
            if by <= 0 {
                return err(400, "time only moves forward: advance must be > 0");
            }
            if by > MAX_ADVANCE_NS {
                return err(400, "advance larger than 100 years refused");
            }
            clock.advance(by);
            clock_event(ctx, "advance", &format!(r#","by_ns":{by}"#))
        }
        (Method::Post, "/v1/clock/freeze") => {
            clock.freeze();
            clock_event(ctx, "freeze", "")
        }
        (Method::Post, "/v1/clock/unfreeze") => {
            clock.unfreeze();
            clock_event(ctx, "unfreeze", "")
        }
        (Method::Get, "/v1/faults") => json(
            200,
            &FaultList {
                targets: ctx.targets.to_vec(),
                faults: ctx.faults.list(),
            },
        ),
        (Method::Post, "/v1/faults") => {
            let body = match read_body(req) {
                Ok(b) => b,
                Err(r) => return r,
            };
            let fr: FaultRequest = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => return err(400, format!("invalid JSON: {e}")),
            };
            match ctx.faults.add(&fr, ctx.targets) {
                Ok(f) => {
                    let mut e = event("fault_set");
                    e.target = Some(f.target.clone());
                    e.fault_id = Some(f.id);
                    e.status = Some(f.status);
                    e.message = Some(format!(
                        "{} down {} .. {}",
                        f.target,
                        format_utc(f.from_ns),
                        format_utc(f.to_ns)
                    ));
                    ctx.events.push(&clock.state(), e);
                    log(&format!(
                        r#"{{"event":"fault_set","id":{},"target":"{}","from_ns":{},"to_ns":{}}}"#,
                        f.id, f.target, f.from_ns, f.to_ns
                    ));
                    json(201, &f)
                }
                Err(e) => err(400, e),
            }
        }
        (Method::Delete, p) if p == "/v1/faults" || p.starts_with("/v1/faults/") => {
            let id = match p.strip_prefix("/v1/faults/") {
                None => None,
                Some(s) => match s.parse::<u64>() {
                    Ok(v) => Some(v),
                    Err(_) => return err(400, "fault id must be a number"),
                },
            };
            let removed = ctx.faults.clear(id);
            if id.is_some() && removed.is_empty() {
                return err(404, "no such fault");
            }
            for f in &removed {
                let mut e = event("fault_clear");
                e.target = Some(f.target.clone());
                e.fault_id = Some(f.id);
                ctx.events.push(&clock.state(), e);
            }
            json(200, &serde_json::json!({ "removed": removed }))
        }
        (Method::Get, "/v1/events") => json(
            200,
            &ctx.events.since(query_u64(&url, "since").unwrap_or(0)),
        ),
        _ => err(404, "not found"),
    }
}

pub fn serve(ctx: &Ctx<'_>, listen: &str) -> i32 {
    let server = match Server::http(listen) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("clockd: cannot listen on {listen}: {e}");
            return 1;
        }
    };
    log(&format!(r#"{{"event":"listening","addr":"{listen}"}}"#));
    let shared = proxy::Shared {
        clock: ctx.clock,
        events: ctx.events,
        faults: ctx.faults,
    };
    std::thread::scope(|scope| {
        proxy::spawn(scope, ctx.targets, &shared);
        let json = Header::from_bytes("Content-Type", "application/json").expect("static header");
        for mut req in server.incoming_requests() {
            // Long-polls (`GET /v1/clock/wait?after=<generation>&timeout_ms=`)
            // run on their own thread so they never block the API.
            let url = req.url().to_string();
            if *req.method() == Method::Get
                && url.split('?').next() == Some("/v1/clock/wait")
                && authorized(&req, ctx.token)
            {
                if ctx.changes.waiters.fetch_add(1, Ordering::SeqCst) >= MAX_WAITERS {
                    ctx.changes.waiters.fetch_sub(1, Ordering::SeqCst);
                    let (code, body) = err(503, "too many clock watchers");
                    let _ = req.respond(Response::from_string(body).with_status_code(code));
                    continue;
                }
                let json = json.clone();
                scope.spawn(move || {
                    let after = query_u64(&url, "after").unwrap_or(u64::MAX);
                    let timeout =
                        Duration::from_millis(query_u64(&url, "timeout_ms").unwrap_or(25_000));
                    let st = ctx.changes.wait(ctx.clock, after, timeout);
                    ctx.changes.waiters.fetch_sub(1, Ordering::SeqCst);
                    let (code, body) = ok(&st);
                    let _ = req.respond(
                        Response::from_string(body)
                            .with_status_code(code)
                            .with_header(json),
                    );
                });
                continue;
            }
            let (code, body) = handle(ctx, &mut req);
            let _ = req.respond(
                Response::from_string(body)
                    .with_status_code(code)
                    .with_header(json.clone()),
            );
        }
    });
    0
}

#[cfg(test)]
mod tests {
    use super::{Changes, query_u64, token_matches};
    use crate::clock::Clock;
    use std::time::{Duration, Instant};

    #[test]
    fn long_poll_wakes_on_change_and_times_out() {
        let clock = Clock::anonymous();
        let changes = Changes::new();
        let g0 = clock.state().generation;
        // Different generation: returns at once.
        let t = Instant::now();
        assert_eq!(
            changes
                .wait(&clock, g0 + 99, Duration::from_secs(5))
                .generation,
            g0
        );
        assert!(t.elapsed() < Duration::from_millis(500));
        // Same generation, nothing happens: returns after the timeout.
        let t = Instant::now();
        changes.wait(&clock, g0, Duration::from_millis(200));
        assert!(t.elapsed() >= Duration::from_millis(190));
        // Same generation, the clock advances meanwhile: wakes early.
        std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                clock.advance(1_000_000_000);
                changes.notify();
            });
            let t = Instant::now();
            let st = changes.wait(&clock, g0, Duration::from_secs(10));
            assert_eq!(st.generation, g0 + 1);
            assert!(t.elapsed() < Duration::from_secs(5));
        });
    }

    #[test]
    fn token_comparison() {
        assert!(token_matches(b"abc", b"abc"));
        assert!(!token_matches(b"abd", b"abc"));
        assert!(!token_matches(b"ab", b"abc"));
        assert!(!token_matches(b"", b"abc"));
    }

    #[test]
    fn query_parsing() {
        assert_eq!(query_u64("/v1/events?since=42", "since"), Some(42));
        assert_eq!(query_u64("/v1/events?x=1&since=7", "since"), Some(7));
        assert_eq!(query_u64("/v1/events?sincex=7", "since"), None);
        assert_eq!(query_u64("/v1/events", "since"), None);
    }
}
