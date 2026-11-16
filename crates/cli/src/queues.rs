//! Built-in barriers for job queues: BullMQ (and Bull) and Sidekiq.
//!
//! Each check is one `redis-cli EVAL` in the queue's Redis service. The script
//! returns how many jobs are still pending, read with Redis's own clock (which
//! ChronoStack virtualizes), so jobs that became due with the jump count as
//! pending until a worker has picked them up and finished them.

use crate::config::{BullmqSpec, SidekiqSpec};

/// Pending BullMQ jobs: waiting, paused, running, prioritized, and delayed jobs
/// that are already due. ARGV[1] is the key prefix, ARGV[2..] the queue names.
///
/// BullMQ scores delayed jobs as `timestamp_ms * 4096 + counter`; Bull (v3/v4)
/// and early BullMQ use the plain timestamp. The script looks at the oldest
/// delayed job's score to tell which one a queue uses.
const BULLMQ_PENDING: &str = r#"
local t = redis.call('TIME')
local now_ms = tonumber(t[1]) * 1000 + math.floor(tonumber(t[2]) / 1000)
local n = 0
for i = 2, #ARGV do
  local q = ARGV[1] .. ':' .. ARGV[i] .. ':'
  n = n + redis.call('LLEN', q .. 'wait') + redis.call('LLEN', q .. 'paused')
        + redis.call('LLEN', q .. 'active') + redis.call('ZCARD', q .. 'prioritized')
  local oldest = redis.call('ZRANGE', q .. 'delayed', 0, 0, 'WITHSCORES')
  if oldest[2] then
    local due = now_ms
    if tonumber(oldest[2]) >= 100000000000000 then
      due = now_ms * 4096 + 4095
    end
    n = n + redis.call('ZCOUNT', q .. 'delayed', '-inf', due)
  end
end
return n
"#;

/// Pending Sidekiq work: jobs in every queue, scheduled and retry jobs that are
/// already due, and jobs running in any process. ARGV[1] is the key prefix
/// (empty, or `<namespace>:`).
///
/// Returns `{pending, oldest_heartbeat_ms, now_ms}`. Sidekiq only publishes its
/// running jobs (`busy`) on its heartbeat, every few seconds, so a job taken
/// off a queue just now isn't visible yet; the caller waits for a heartbeat
/// newer than the moment the queues looked empty (`oldest_heartbeat_ms` is -1
/// when no Sidekiq process is registered, 0 when a registered process has no
/// record because its TTL ran out).
const SIDEKIQ_PENDING: &str = r#"
local p = ARGV[1]
local t = redis.call('TIME')
local now = tonumber(t[1]) + tonumber(t[2]) / 1000000
local n = 0
for _, q in ipairs(redis.call('SMEMBERS', p .. 'queues')) do
  n = n + redis.call('LLEN', p .. 'queue:' .. q)
end
n = n + redis.call('ZCOUNT', p .. 'schedule', '-inf', now)
      + redis.call('ZCOUNT', p .. 'retry', '-inf', now)
local oldest = -1
for _, proc in ipairs(redis.call('SMEMBERS', p .. 'processes')) do
  local h = redis.call('HMGET', p .. proc, 'busy', 'beat')
  n = n + (tonumber(h[1]) or 0)
  -- A listed process without its hash: the hash's 60 s TTL ran out (a clock
  -- jump expires it at once). Treat it as "no heartbeat yet" so the caller
  -- waits for the next one, which recreates it with the busy count.
  local beat = math.floor((tonumber(h[2]) or 0) * 1000)
  if oldest < 0 or beat < oldest then
    oldest = beat
  end
end
return {n, oldest, math.floor(now * 1000)}
"#;

/// `redis-cli` arguments for a BullMQ barrier.
pub fn bullmq_args(spec: &BullmqSpec) -> Vec<String> {
    let mut args = vec![
        "EVAL".to_string(),
        BULLMQ_PENDING.to_string(),
        "0".into(),
        spec.prefix.clone(),
    ];
    args.extend(spec.queues.iter().cloned());
    args
}

/// `redis-cli` arguments for a Sidekiq barrier.
pub fn sidekiq_args(spec: &SidekiqSpec) -> Vec<String> {
    let prefix = match spec.namespace.as_deref().map(str::trim) {
        Some(ns) if !ns.is_empty() => format!("{ns}:"),
        _ => String::new(),
    };
    vec![
        "EVAL".to_string(),
        SIDEKIQ_PENDING.to_string(),
        "0".into(),
        prefix,
    ]
}

/// Reads the BullMQ script's reply: `Some(pending)` for an integer reply.
pub fn pending(stdout: &str) -> Option<u64> {
    integer(stdout.trim())
}

/// The Sidekiq script's reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidekiqState {
    pub pending: u64,
    /// Oldest heartbeat of any Sidekiq process (ms, Redis clock); `None` when
    /// no process is registered.
    pub oldest_heartbeat_ms: Option<i64>,
    pub now_ms: i64,
}

/// Reads the Sidekiq script's reply (three integers, one per line).
pub fn sidekiq_state(stdout: &str) -> Option<SidekiqState> {
    let mut lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    let pending = integer(lines.next()?)?;
    let oldest: i64 = integer(lines.next()?)?;
    let now_ms = integer(lines.next()?)?;
    Some(SidekiqState {
        pending,
        oldest_heartbeat_ms: (oldest >= 0).then_some(oldest),
        now_ms,
    })
}

fn integer<T: std::str::FromStr>(v: &str) -> Option<T> {
    // redis-cli adds `(integer)` and list numbering when stdout is a terminal.
    let v = v.split_once(") ").map_or(v, |(head, rest)| {
        if head
            .trim_start()
            .chars()
            .all(|c| c.is_ascii_digit() || c == '(')
        {
            rest
        } else {
            v
        }
    });
    v.strip_prefix("(integer) ")
        .unwrap_or(v)
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullmq_args_pass_prefix_then_queues() {
        let spec = BullmqSpec {
            service: "redis".into(),
            queues: vec!["emails".into(), "cron-queue".into()],
            prefix: "{app:jobs}".into(),
        };
        let args = bullmq_args(&spec);
        assert_eq!(args[0], "EVAL");
        assert_eq!(args[2], "0");
        assert_eq!(&args[3..], ["{app:jobs}", "emails", "cron-queue"]);
    }

    #[test]
    fn sidekiq_namespace_becomes_a_key_prefix() {
        let with = SidekiqSpec {
            service: "redis".into(),
            namespace: Some("myapp".into()),
        };
        let without = SidekiqSpec {
            service: "redis".into(),
            namespace: None,
        };
        assert_eq!(sidekiq_args(&with)[3], "myapp:");
        assert_eq!(sidekiq_args(&without)[3], "");
    }

    #[test]
    fn reads_plain_and_annotated_integer_replies() {
        assert_eq!(pending("0\n"), Some(0));
        assert_eq!(pending("(integer) 3"), Some(3));
        assert_eq!(pending("ERR unknown command"), None);
    }

    #[test]
    fn reads_the_sidekiq_reply() {
        assert_eq!(
            sidekiq_state("2\n1795000000123\n1795000001000\n"),
            Some(SidekiqState {
                pending: 2,
                oldest_heartbeat_ms: Some(1_795_000_000_123),
                now_ms: 1_795_000_001_000,
            })
        );
        let no_process = sidekiq_state("0\n-1\n1795000001000").unwrap();
        assert_eq!(no_process.oldest_heartbeat_ms, None);
        assert_eq!(
            sidekiq_state("1) (integer) 0\n2) (integer) -1\n3) (integer) 5")
                .unwrap()
                .now_ms,
            5
        );
        assert_eq!(sidekiq_state("0\n"), None);
    }
}
