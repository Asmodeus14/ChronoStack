//! Minimal HTTP/1.1 API (v1). Bound inside the container; published by Compose
//! on `127.0.0.1` only. Every endpoint except `/v1/health` needs the bearer token.

use std::io::Read;

use chronostack_core::api::{AdvanceRequest, ApiError, ClockState, MAX_ADVANCE_NS};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::clock::Clock;

pub fn log(line: &str) {
    println!("{line}");
}

type Reply = (u16, String);

fn ok(st: &ClockState) -> Reply {
    (200, serde_json::to_string(st).expect("serializable"))
}

fn err(code: u16, msg: impl Into<String>) -> Reply {
    (
        code,
        serde_json::to_string(&ApiError { error: msg.into() }).expect("serializable"),
    )
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

fn handle(clock: &Clock, token: &str, req: &mut Request) -> Reply {
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();
    if method == Method::Get && path == "/v1/health" {
        return (200, r#"{"ok":true}"#.into());
    }
    if !authorized(req, token) {
        return err(401, "missing or invalid bearer token");
    }
    match (method, path.as_str()) {
        (Method::Get, "/v1/clock") => ok(&clock.state()),
        (Method::Post, "/v1/clock/advance") => {
            let mut body = String::new();
            if req
                .as_reader()
                .take(64 * 1024)
                .read_to_string(&mut body)
                .is_err()
            {
                return err(400, "unreadable body");
            }
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
            let st = clock.state();
            log(&format!(
                r#"{{"event":"advance","by_ns":{by},"virtual_ns":{},"generation":{}}}"#,
                st.virtual_ns, st.generation
            ));
            ok(&st)
        }
        (Method::Post, "/v1/clock/freeze") => {
            clock.freeze();
            let st = clock.state();
            log(&format!(
                r#"{{"event":"freeze","virtual_ns":{},"generation":{}}}"#,
                st.virtual_ns, st.generation
            ));
            ok(&st)
        }
        (Method::Post, "/v1/clock/unfreeze") => {
            clock.unfreeze();
            let st = clock.state();
            log(&format!(
                r#"{{"event":"unfreeze","virtual_ns":{},"generation":{}}}"#,
                st.virtual_ns, st.generation
            ));
            ok(&st)
        }
        _ => err(404, "not found"),
    }
}

pub fn serve(clock: &Clock, listen: &str, token: &str) -> i32 {
    let server = match Server::http(listen) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("clockd: cannot listen on {listen}: {e}");
            return 1;
        }
    };
    log(&format!(r#"{{"event":"listening","addr":"{listen}"}}"#));
    let json = Header::from_bytes("Content-Type", "application/json").expect("static header");
    for mut req in server.incoming_requests() {
        let (code, body) = handle(clock, token, &mut req);
        let _ = req.respond(
            Response::from_string(body)
                .with_status_code(code)
                .with_header(json.clone()),
        );
    }
    0
}

#[cfg(test)]
mod tests {
    use super::token_matches;

    #[test]
    fn token_comparison() {
        assert!(token_matches(b"abc", b"abc"));
        assert!(!token_matches(b"abd", b"abc"));
        assert!(!token_matches(b"ab", b"abc"));
        assert!(!token_matches(b"", b"abc"));
    }
}
