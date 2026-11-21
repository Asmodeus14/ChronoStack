//! The engine against a small in-process HTTP API:
//!   POST /signup {email}   -> 201 {"token": "tok-<email>", "plan": "trial"}
//!   GET  /me  (Bearer tok) -> 200 {"plan": "pro"}, 401 without a valid token
//!   GET  /slow             -> 200 after 40 ms, tracking concurrent requests
//!   anything else          -> 404

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chronostack_users::engine::{Options, Report, run};
use chronostack_users::model::parse;

struct Server {
    url: String,
    peak: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
}

fn serve() -> Server {
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("bind"));
    let url = format!("http://{}", server.server_addr().to_ip().expect("ip"));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(AtomicUsize::new(0));
    for _ in 0..64 {
        let (server, active, peak, requests) = (
            Arc::clone(&server),
            Arc::clone(&active),
            Arc::clone(&peak),
            Arc::clone(&requests),
        );
        std::thread::spawn(move || {
            while let Ok(mut req) = server.recv() {
                requests.fetch_add(1, Ordering::SeqCst);
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let auth = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| h.value.as_str().to_string());
                let (status, out) = match (req.method().as_str(), req.url()) {
                    ("POST", "/signup") => {
                        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                        let email = v["email"].as_str().unwrap_or("?").to_string();
                        (
                            201,
                            serde_json::json!({"token": format!("tok-{email}"), "plan": "trial"})
                                .to_string(),
                        )
                    }
                    ("GET", "/me") => match auth {
                        Some(a) if a.starts_with("Bearer tok-") => {
                            (200, r#"{"plan":"pro"}"#.to_string())
                        }
                        _ => (401, r#"{"error":"no token"}"#.to_string()),
                    },
                    ("GET", "/slow") => {
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(40));
                        active.fetch_sub(1, Ordering::SeqCst);
                        (200, "ok".to_string())
                    }
                    _ => (404, "not found".to_string()),
                };
                let _ = req.respond(tiny_http::Response::from_string(out).with_status_code(status));
            }
        });
    }
    Server {
        url,
        peak,
        requests,
    }
}

fn opts(seed: u64) -> Options {
    Options {
        seed,
        run_id: "r1".into(),
        duration: None,
        request_timeout: Duration::from_secs(10),
        trace: true,
    }
}

fn go(yaml: &str, server: &Server, o: &Options) -> Report {
    let plan = parse(yaml).expect("valid journey file");
    let targets = BTreeMap::from([("api".to_string(), server.url.clone())]);
    run(&plan, &targets, o)
}

const TRIAL: &str = r#"
version: 1
targets: { api: "http://placeholder" }
cohorts:
  - { name: trialists, users: 6, journey: trial }
journeys:
  trial:
    - name: sign up
      http: { method: POST, url: "${api}/signup", json: { email: "u${user}-${run_id}" } }
      expect: { status: 201, json: { /plan: trial } }
      save: { token: /token }
    - think: { between: [1ms, 20ms] }
    - branch: { p: 0.5 }
    - name: me
      http: { url: "${api}/me", headers: { Authorization: "Bearer ${token}" } }
      expect: { json: { /plan: pro } }
"#;

#[test]
fn a_journey_saves_values_and_checks_responses() {
    let s = serve();
    let r = go(TRIAL, &s, &opts(42));
    assert!(r.passed(), "{r:#?}");
    let u = &r.users;
    assert_eq!(u.total, 6);
    assert_eq!(u.completed + u.left_at_branch, 6);
    assert_eq!(u.failed, 0);
    let by_name: BTreeMap<_, _> = r.steps.iter().map(|s| (s.name.as_str(), s)).collect();
    assert_eq!(by_name["sign up"].runs, 6);
    assert_eq!(by_name["me"].runs, u.completed);
    assert!(by_name["sign up"].latency_us.is_some());
    // The saved token was sent: /me answered 200 with plan=pro for everyone who got there.
    assert_eq!(by_name["me"].failed, 0);
}

#[test]
fn the_same_seed_gives_the_same_run() {
    let s = serve();
    let a = go(TRIAL, &s, &opts(7)).trace.unwrap();
    let b = go(TRIAL, &s, &opts(7)).trace.unwrap();
    assert_eq!(
        a, b,
        "same seed: same thinks, branches and requests per user"
    );
    // A different seed changes some choice (with 6 users and 2 random choices
    // each, the chance of an identical run is negligible).
    let c = go(TRIAL, &s, &opts(8)).trace.unwrap();
    assert_ne!(a, c);
}

#[test]
fn a_failed_expectation_stops_the_user() {
    let s = serve();
    let yaml = r#"
version: 1
targets: { api: "http://placeholder" }
cohorts: [{ name: c, users: 3, journey: j }]
journeys:
  j:
    - name: no token
      http: { url: "${api}/me" }
    - name: never reached
      http: { url: "${api}/me" }
"#;
    let r = go(yaml, &s, &opts(1));
    assert!(!r.passed());
    assert_eq!(r.users.failed, 3);
    assert_eq!(r.steps[0].failed, 3);
    assert!(
        r.steps[0].errors[0].contains("unexpected HTTP 401"),
        "{:?}",
        r.steps[0].errors
    );
    assert_eq!(r.steps[1].runs, 0, "a failed user stops");
}

#[test]
fn missing_values_and_non_json_responses_fail_clearly() {
    let s = serve();
    let yaml = r#"
version: 1
targets: { api: "http://placeholder" }
cohorts: [{ name: a, users: 1, journey: save_missing }, { name: b, users: 1, journey: not_json }]
journeys:
  save_missing:
    - http: { method: POST, url: "${api}/signup", json: { email: x } }
      save: { id: /id }
  not_json:
    - http: { url: "${api}/slow" }
      expect: { json: { /ok: true } }
"#;
    let r = go(yaml, &s, &opts(1));
    let errs: Vec<String> = r.steps.iter().flat_map(|s| s.errors.clone()).collect();
    assert!(
        errs.iter()
            .any(|e| e.contains("save id: /id not in the response")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("response is not JSON")),
        "{errs:?}"
    );
    assert_eq!(r.users.failed, 2);
}

#[test]
fn max_parallel_limits_active_users() {
    let s = serve();
    let yaml = r#"
version: 1
max_parallel: 4
targets: { api: "http://placeholder" }
cohorts: [{ name: c, users: 20, journey: j }]
journeys:
  j:
    - repeat: { times: 2, steps: [{ http: { url: "${api}/slow" } }] }
"#;
    let r = go(yaml, &s, &opts(1));
    assert!(r.passed(), "{r:#?}");
    assert_eq!(s.requests.load(Ordering::SeqCst), 40);
    let peak = s.peak.load(Ordering::SeqCst);
    assert!(peak <= 4, "peak {peak} > max_parallel 4");
    assert!(peak >= 2, "requests ran in parallel (peak {peak})");
    // Steps inside repeat are numbered after it and counted per run.
    assert_eq!(
        (r.steps[0].index, r.steps[0].kind, r.steps[0].runs),
        (1, "repeat", 20)
    );
    assert_eq!(
        (r.steps[1].index, r.steps[1].kind, r.steps[1].runs),
        (2, "http", 40)
    );
}

#[test]
fn duration_stops_the_run() {
    let s = serve();
    let yaml = r#"
version: 1
max_think: 1h
targets: { api: "http://placeholder" }
cohorts:
  - { name: c, users: 10, arrive: { over: 10s }, journey: j }
journeys:
  j:
    - http: { url: "${api}/slow" }
    - think: 5s
    - http: { url: "${api}/slow" }
"#;
    let mut o = opts(1);
    o.duration = Some(Duration::from_millis(1_500));
    let started = std::time::Instant::now();
    let r = go(yaml, &s, &o);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert!(r.stopped_by_duration);
    // Users arrive every second: about 2 started (at 0 s and 1 s), and were
    // stopped during their 5 s think; the rest never started.
    assert_eq!(r.users.stopped + r.users.not_started, 10, "{:?}", r.users);
    assert!(
        r.users.stopped >= 1 && r.users.not_started >= 7,
        "{:?}",
        r.users
    );
    assert_eq!(r.users.failed, 0);
}

#[test]
fn long_thinks_are_capped() {
    let s = serve();
    let yaml = r#"
version: 1
max_think: 50ms
targets: { api: "http://placeholder" }
cohorts: [{ name: c, users: 2, journey: j }]
journeys:
  j:
    - think: 13d
    - http: { url: "${api}/slow" }
"#;
    let started = std::time::Instant::now();
    let r = go(yaml, &s, &opts(1));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(r.passed(), "{r:#?}");
    assert_eq!(r.thinks_capped, 2);
}
