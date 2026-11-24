//! Outage windows on proxy targets, keyed to virtual time. Persisted next to the
//! clock page so they survive a `clockd` restart.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use chronostack_core::api::{FULL_RATE_PPM, Fault, FaultRequest, MAX_FAULT_DELAY_MS, ProxyTarget};

pub struct Faults {
    path: Option<PathBuf>,
    inner: Mutex<(Vec<Fault>, u64)>,
    /// Requests seen so far by each fault (the draw index; not persisted).
    counters: Mutex<HashMap<u64, u64>>,
}

/// What a fault does to one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Draw {
    pub delay_ms: u64,
    pub fail: bool,
}

/// SplitMix64: a well-mixed 64-bit value from a 64-bit input.
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The `n`-th request's draw for fault `f`: depends only on `f.seed` and `n`
/// (not on the window's id, which differs between runs), so a scenario that
/// sets the same seed fails the same requests on every run.
pub fn draw(f: &Fault, n: u64) -> Draw {
    let base = splitmix64(splitmix64(f.seed) ^ n.wrapping_mul(0xD1B5_4A32_D192_ED03));
    let fail = f.rate_ppm >= FULL_RATE_PPM || (base % 1_000_000) < u64::from(f.rate_ppm);
    let span = f.delay_max_ms.saturating_sub(f.delay_min_ms);
    let delay_ms = f.delay_min_ms
        + if span == 0 {
            0
        } else {
            splitmix64(base) % (span + 1)
        };
    Draw { delay_ms, fail }
}

impl Faults {
    /// Load persisted faults from `path` (missing or unreadable file = none).
    pub fn load(path: Option<PathBuf>) -> Faults {
        let faults: Vec<Fault> = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let next = faults.iter().map(|f| f.id).max().unwrap_or(0) + 1;
        Faults {
            path,
            inner: Mutex::new((faults, next)),
            counters: Mutex::new(HashMap::new()),
        }
    }

    fn save(&self, faults: &[Fault]) {
        if let Some(p) = &self.path {
            let tmp = p.with_extension("tmp");
            if let Ok(s) = serde_json::to_string(faults) {
                if fs::write(&tmp, s).is_ok() {
                    let _ = fs::rename(&tmp, p);
                }
            }
        }
    }

    pub fn add(&self, req: &FaultRequest, targets: &[ProxyTarget]) -> Result<Fault, String> {
        if !targets.iter().any(|t| t.name == req.target) {
            let names: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
            return Err(format!(
                "unknown proxy target `{}` (declared: {})",
                req.target,
                if names.is_empty() {
                    "none — add `proxies:` to chronostack.yml and re-run `chronostack up`"
                        .to_string()
                } else {
                    names.join(", ")
                }
            ));
        }
        if req.to_ns <= req.from_ns {
            return Err("the window must end after it starts".into());
        }
        let status = req.status.unwrap_or(503);
        if !(400..=599).contains(&status) {
            return Err("status must be a 4xx or 5xx code".into());
        }
        let (delay_min_ms, delay_max_ms) = match req.delay_ms {
            Some([a, b]) if a > b => return Err("delay: the minimum is above the maximum".into()),
            Some([_, b]) if b > MAX_FAULT_DELAY_MS => {
                return Err(format!(
                    "delay: at most {} s per request",
                    MAX_FAULT_DELAY_MS / 1000
                ));
            }
            Some([a, b]) => (a, b),
            None => (0, 0),
        };
        // Only a delay given: requests are slowed, not failed.
        let rate_ppm = match req.rate_ppm {
            Some(r) if r > FULL_RATE_PPM => return Err("rate must be between 0 and 1".into()),
            Some(r) => r,
            None if delay_max_ms > 0 && req.status.is_none() => 0,
            None => FULL_RATE_PPM,
        };
        if rate_ppm == 0 && delay_max_ms == 0 {
            return Err("this window would do nothing: give a rate above 0 or a delay".into());
        }
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if g.0.len() >= 1000 {
            return Err("too many faults (limit 1000); clear some first".into());
        }
        let f = Fault {
            id: g.1,
            target: req.target.clone(),
            from_ns: req.from_ns,
            to_ns: req.to_ns,
            status,
            rate_ppm,
            delay_min_ms,
            delay_max_ms,
            seed: req.seed.unwrap_or(g.1),
        };
        g.1 += 1;
        g.0.push(f.clone());
        self.save(&g.0);
        Ok(f)
    }

    pub fn list(&self) -> Vec<Fault> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .0
            .clone()
    }

    /// Remove one fault (`Some(id)`) or all of them. Returns the removed faults.
    pub fn clear(&self, id: Option<u64>) -> Vec<Fault> {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let (removed, kept): (Vec<Fault>, Vec<Fault>) =
            g.0.drain(..).partition(|f| id.is_none_or(|i| f.id == i));
        g.0 = kept;
        self.save(&g.0);
        removed
    }

    /// The next draw for fault `f` (counts the request).
    pub fn next_draw(&self, f: &Fault) -> Draw {
        let mut c = self.counters.lock().unwrap_or_else(|p| p.into_inner());
        let n = c.entry(f.id).or_insert(0);
        let d = draw(f, *n);
        *n += 1;
        d
    }

    /// The fault active for `target` at virtual time `v`, if any.
    pub fn active(&self, target: &str, v: i64) -> Option<Fault> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .0
            .iter()
            .find(|f| f.target == target && f.from_ns <= v && v < f.to_ns)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Vec<ProxyTarget> {
        vec![ProxyTarget {
            name: "hooks".into(),
            listen_port: 7200,
            upstream: "http://r:4000".into(),
        }]
    }

    fn req(from: i64, to: i64) -> FaultRequest {
        FaultRequest {
            target: "hooks".into(),
            from_ns: from,
            to_ns: to,
            status: None,
            rate_ppm: None,
            delay_ms: None,
            seed: None,
        }
    }

    #[test]
    fn windows_are_half_open_and_persisted() {
        let dir = std::env::temp_dir().join(format!("cs-faults-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("faults.json");
        let f = Faults::load(Some(path.clone()));
        let a = f.add(&req(100, 200), &targets()).unwrap();
        assert_eq!((a.id, a.status), (1, 503));
        assert!(f.active("hooks", 99).is_none());
        assert_eq!(f.active("hooks", 100).unwrap().id, 1);
        assert_eq!(f.active("hooks", 199).unwrap().id, 1);
        assert!(f.active("hooks", 200).is_none(), "end is exclusive");
        assert!(f.active("other", 150).is_none());

        let reloaded = Faults::load(Some(path.clone()));
        assert_eq!(reloaded.list(), vec![a.clone()]);
        let b = reloaded.add(&req(300, 400), &targets()).unwrap();
        assert_eq!(b.id, 2, "ids continue after a restart");
        assert_eq!(reloaded.clear(Some(1)), vec![a]);
        assert_eq!(reloaded.list(), vec![b]);
        assert_eq!(reloaded.clear(None).len(), 1);
        assert!(Faults::load(Some(path)).list().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_bad_requests() {
        let f = Faults::load(None);
        assert!(f.add(&req(200, 100), &targets()).is_err());
        let mut r = req(1, 2);
        r.target = "nope".into();
        assert!(f.add(&r, &targets()).unwrap_err().contains("hooks"));
        let mut r = req(1, 2);
        r.status = Some(200);
        assert!(f.add(&r, &targets()).is_err());
        let mut r = req(1, 2);
        r.rate_ppm = Some(1_000_001);
        assert!(f.add(&r, &targets()).unwrap_err().contains("rate"));
        let mut r = req(1, 2);
        r.delay_ms = Some([3000, 1000]);
        assert!(f.add(&r, &targets()).unwrap_err().contains("minimum"));
        let mut r = req(1, 2);
        r.delay_ms = Some([0, 60_000]);
        assert!(f.add(&r, &targets()).unwrap_err().contains("at most"));
        let mut r = req(1, 2);
        r.rate_ppm = Some(0);
        assert!(f.add(&r, &targets()).unwrap_err().contains("nothing"));
    }

    #[test]
    fn defaults_outage_and_latency_only() {
        let f = Faults::load(None);
        let outage = f.add(&req(1, 2), &targets()).unwrap();
        assert!(outage.is_outage());
        assert_eq!(outage.describe(), "HTTP 503");
        let mut r = req(1, 2);
        r.delay_ms = Some([500, 3000]);
        let slow = f.add(&r, &targets()).unwrap();
        assert_eq!(
            slow.rate_ppm, 0,
            "only a delay: requests are slowed, not failed"
        );
        assert_eq!(slow.describe(), "0.5-3 s delay");
        r.rate_ppm = Some(300_000);
        r.status = Some(500);
        let flaky = f.add(&r, &targets()).unwrap();
        assert_eq!(
            flaky.describe(),
            "HTTP 500 for 30% of requests, 0.5-3 s delay"
        );
        assert_eq!(flaky.seed, flaky.id, "default seed: the fault's id");
    }

    #[test]
    fn draws_are_seeded_and_match_the_rate() {
        let f = Fault {
            id: 7,
            target: "hooks".into(),
            from_ns: 0,
            to_ns: 1,
            status: 503,
            rate_ppm: 300_000,
            delay_min_ms: 100,
            delay_max_ms: 900,
            seed: 42,
        };
        let a: Vec<Draw> = (0..2000).map(|n| draw(&f, n)).collect();
        let b: Vec<Draw> = (0..2000).map(|n| draw(&f, n)).collect();
        assert_eq!(a, b, "same seed, same draws");
        let fails = a.iter().filter(|d| d.fail).count();
        assert!((540..=660).contains(&fails), "about 30% of 2000: {fails}");
        assert!(a.iter().all(|d| (100..=900).contains(&d.delay_ms)));
        let same_seed_other_window = Fault {
            id: 99,
            ..f.clone()
        };
        assert_eq!(
            (0..50)
                .map(|n| draw(&same_seed_other_window, n))
                .collect::<Vec<_>>(),
            a[..50].to_vec(),
            "the window's id doesn't change the draws"
        );
        let other = Fault {
            seed: 43,
            ..f.clone()
        };
        assert_ne!(
            (0..50).map(|n| draw(&other, n)).collect::<Vec<_>>(),
            a[..50].to_vec(),
            "another seed, other draws"
        );
        let outage = Fault {
            rate_ppm: FULL_RATE_PPM,
            delay_min_ms: 0,
            delay_max_ms: 0,
            ..f
        };
        assert!((0..100).all(|n| draw(&outage, n)
            == Draw {
                delay_ms: 0,
                fail: true
            }));
    }

    #[test]
    fn old_fault_files_still_load() {
        let old = r#"[{"id":1,"target":"hooks","from_ns":1,"to_ns":2,"status":503}]"#;
        let f: Vec<Fault> = serde_json::from_str(old).unwrap();
        assert!(f[0].is_outage());
        assert_eq!(f[0].rate_ppm, FULL_RATE_PPM);
    }
}
