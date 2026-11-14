//! Minimal HTTP/1.1 API (v1). Bound inside the container; published by Compose
//! on `127.0.0.1` only. Every endpoint except `/v1/health` needs the bearer token.

use std::io::Read;

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
        return (200, r#"{"ok":true}"#.into());
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
    use super::{query_u64, token_matches};

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
