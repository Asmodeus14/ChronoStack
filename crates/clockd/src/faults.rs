//! Outage windows on proxy targets, keyed to virtual time. Persisted next to the
//! clock page so they survive a `clockd` restart.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use chronostack_core::api::{Fault, FaultRequest, ProxyTarget};

pub struct Faults {
    path: Option<PathBuf>,
    inner: Mutex<(Vec<Fault>, u64)>,
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
    }
}
