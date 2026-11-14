//! HTTP fault proxy (ADR-010). Each declared target gets its own port inside
//! the Compose network. Requests are forwarded to the upstream unchanged unless
//! an outage window covers the current *virtual* time, in which case the proxy
//! answers with the window's status code (default 503). Every request is
//! written to the event log. If forwarding fails the proxy answers 502: it
//! fails closed, never silently bypassing a fault.

use std::io::Read;
use std::time::{Duration, Instant};

use chronostack_core::api::ProxyTarget;
use chronostack_core::duration::format_utc;
use tiny_http::{Header, Request, Response, Server};

use crate::clock::Clock;
use crate::events::{Events, event};
use crate::faults::Faults;
use crate::server::log;

/// Largest request or response body forwarded.
const MAX_BODY: u64 = 16 * 1024 * 1024;
/// Worker threads per target.
const WORKERS: usize = 4;

/// Headers that describe one connection, not the message (RFC 9110 §7.6.1).
fn hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-connection"
            | "transfer-encoding"
            | "te"
            | "trailer"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

pub struct Shared<'a> {
    pub clock: &'a Clock,
    pub events: &'a Events,
    pub faults: &'a Faults,
}

/// Run every target's proxy on scoped threads. Returns once all listeners are up
/// (or failed to bind; failures are logged and the API keeps working).
pub fn spawn<'scope, 'env>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    targets: &'env [ProxyTarget],
    shared: &'env Shared<'env>,
) {
    for t in targets {
        let addr = format!("0.0.0.0:{}", t.listen_port);
        let server = match Server::http(&addr) {
            Ok(s) => std::sync::Arc::new(s),
            Err(e) => {
                eprintln!("clockd: proxy `{}` cannot listen on {addr}: {e}", t.name);
                continue;
            }
        };
        log(&format!(
            r#"{{"event":"proxy_listening","target":"{}","addr":"{addr}","upstream":"{}"}}"#,
            t.name, t.upstream
        ));
        for _ in 0..WORKERS {
            let server = server.clone();
            scope.spawn(move || {
                let agent: ureq::Agent = ureq::Agent::config_builder()
                    .http_status_as_error(false)
                    .max_redirects(0)
                    .timeout_global(Some(Duration::from_secs(30)))
                    .build()
                    .into();
                for req in server.incoming_requests() {
                    handle(t, shared, &agent, req);
                }
            });
        }
    }
}

fn respond(req: Request, status: u16, headers: Vec<Header>, body: Vec<u8>) {
    let mut r = Response::from_data(body).with_status_code(status);
    for h in headers {
        r = r.with_header(h);
    }
    let _ = req.respond(r);
}

fn json_header() -> Header {
    Header::from_bytes("Content-Type", "application/json").expect("static header")
}

fn handle(t: &ProxyTarget, shared: &Shared<'_>, agent: &ureq::Agent, mut req: Request) {
    let start = Instant::now();
    let method = req.method().as_str().to_string();
    let path = req.url().to_string();
    let st = shared.clock.state();
    let mut ev = event("proxy");
    ev.target = Some(t.name.clone());
    ev.method = Some(method.clone());
    ev.path = Some(path.chars().take(300).collect());

    if let Some(f) = shared.faults.active(&t.name, st.virtual_ns) {
        ev.status = Some(f.status);
        ev.outcome = Some("fault".into());
        ev.fault_id = Some(f.id);
        ev.latency_ms = Some(start.elapsed().as_millis() as u64);
        shared.events.push(&st, ev);
        let body = serde_json::json!({
            "error": format!("ChronoStack fault: `{}` is down until {} (virtual time)", t.name, format_utc(f.to_ns)),
            "fault_id": f.id,
        });
        respond(
            req,
            f.status,
            vec![json_header()],
            body.to_string().into_bytes(),
        );
        return;
    }

    let mut body = Vec::new();
    if req
        .as_reader()
        .take(MAX_BODY)
        .read_to_end(&mut body)
        .is_err()
    {
        ev.outcome = Some("upstream_error".into());
        ev.status = Some(400);
        ev.message = Some("could not read the request body".into());
        shared.events.push(&st, ev);
        respond(req, 400, vec![], Vec::new());
        return;
    }
    let mut b = ureq::http::Request::builder()
        .method(method.as_str())
        .uri(format!("{}{path}", t.upstream));
    for h in req.headers() {
        if !hop_by_hop(h.field.as_str().as_str()) {
            b = b.header(h.field.as_str().as_str(), h.value.as_str());
        }
    }
    let result = if body.is_empty() {
        b.body(())
            .map_err(|e| e.to_string())
            .and_then(|r| agent.run(r).map_err(|e| e.to_string()))
    } else {
        b.body(&body[..])
            .map_err(|e| e.to_string())
            .and_then(|r| agent.run(r).map_err(|e| e.to_string()))
    };
    match result {
        Ok(mut resp) => {
            let status = resp.status().as_u16();
            let headers: Vec<Header> = resp
                .headers()
                .iter()
                .filter(|(k, _)| !hop_by_hop(k.as_str()))
                .filter_map(|(k, v)| Header::from_bytes(k.as_str().as_bytes(), v.as_bytes()).ok())
                .collect();
            let data = resp
                .body_mut()
                .with_config()
                .limit(MAX_BODY)
                .read_to_vec()
                .unwrap_or_default();
            ev.status = Some(status);
            ev.outcome = Some("forwarded".into());
            ev.latency_ms = Some(start.elapsed().as_millis() as u64);
            shared.events.push(&st, ev);
            respond(req, status, headers, data);
        }
        Err(e) => {
            ev.status = Some(502);
            ev.outcome = Some("upstream_error".into());
            ev.message = Some(e.chars().take(300).collect());
            ev.latency_ms = Some(start.elapsed().as_millis() as u64);
            shared.events.push(&st, ev);
            let body = serde_json::json!({ "error": format!("ChronoStack proxy: upstream `{}` failed: {e}", t.upstream) });
            respond(req, 502, vec![json_header()], body.to_string().into_bytes());
        }
    }
}
