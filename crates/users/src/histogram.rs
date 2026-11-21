//! Latency percentiles from log-scaled buckets.
//!
//! Values below 128 are counted exactly. Above that, each power of two is split
//! into 64 buckets, so a bucket is at most 1/64 of its lower bound wide.
//! Percentiles report the bucket's midpoint, which is within 0.8 % of any value
//! in it. Count, min, max and mean are exact. Memory grows only with the
//! number of distinct buckets used.

use std::collections::BTreeMap;

use serde::Serialize;

const EXACT: u64 = 128;
const SUB: u64 = 64;

/// Bucket index of a value.
fn bucket(v: u64) -> u32 {
    if v < EXACT {
        return v as u32;
    }
    let bits = 64 - v.leading_zeros(); // v >= 128, so bits >= 8
    let shift = bits - 7; // top 7 bits kept: 64..=127
    let top = v >> shift;
    (EXACT + (u64::from(shift) - 1) * SUB + (top - SUB)) as u32
}

/// Lowest and highest value of a bucket.
fn bounds(i: u32) -> (u64, u64) {
    let i = u64::from(i);
    if i < EXACT {
        return (i, i);
    }
    let shift = (i - EXACT) / SUB + 1;
    let top = (i - EXACT) % SUB + SUB;
    let lo = top << shift;
    // Add the width minus one in one go: the top bucket ends at u64::MAX.
    (lo, lo + ((1u64 << shift) - 1))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Histogram {
    buckets: BTreeMap<u32, u64>,
    count: u64,
    sum: u128,
    min: u64,
    max: u64,
}

/// Summary of a histogram (values in the unit they were recorded in).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub count: u64,
    pub min: u64,
    pub mean: f64,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub max: u64,
}

impl Histogram {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, v: u64) {
        *self.buckets.entry(bucket(v)).or_insert(0) += 1;
        if self.count == 0 || v < self.min {
            self.min = v;
        }
        self.max = self.max.max(v);
        self.count += 1;
        self.sum += u128::from(v);
    }

    pub fn merge(&mut self, other: &Histogram) {
        if other.count == 0 {
            return;
        }
        for (b, n) in &other.buckets {
            *self.buckets.entry(*b).or_insert(0) += n;
        }
        if self.count == 0 || other.min < self.min {
            self.min = other.min;
        }
        self.max = self.max.max(other.max);
        self.count += other.count;
        self.sum += other.sum;
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    /// The value at quantile `q` (0..=1), as its bucket's midpoint, clamped to
    /// the exact min and max. `None` when empty.
    pub fn quantile(&self, q: f64) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        // Rank of the value, 1-based: the smallest value with at least q of
        // the values at or below it.
        let rank = ((q.clamp(0.0, 1.0) * self.count as f64).ceil() as u64).max(1);
        // The extremes are known exactly.
        if rank == 1 {
            return Some(self.min);
        }
        if rank >= self.count {
            return Some(self.max);
        }
        let mut seen = 0;
        for (b, n) in &self.buckets {
            seen += n;
            if seen >= rank {
                let (lo, hi) = bounds(*b);
                let mid = lo + (hi - lo) / 2;
                return Some(mid.clamp(self.min, self.max));
            }
        }
        Some(self.max)
    }

    pub fn summary(&self) -> Option<Summary> {
        (self.count > 0).then(|| Summary {
            count: self.count,
            min: self.min,
            mean: self.sum as f64 / self.count as f64,
            p50: self.quantile(0.50).unwrap_or(0),
            p90: self.quantile(0.90).unwrap_or(0),
            p99: self.quantile(0.99).unwrap_or(0),
            max: self.max,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_cover_every_value_once() {
        // Each value maps to a bucket whose bounds contain it, and bucket
        // ranges are contiguous.
        let mut probes: Vec<u64> = (0..2_000).collect();
        for shift in 7..64 {
            let p = 1u64 << shift;
            probes.extend([p - 1, p, p + 1, p + p / 3]);
        }
        probes.push(u64::MAX);
        for v in probes {
            let (lo, hi) = bounds(bucket(v));
            assert!(lo <= v && v <= hi, "{v} not in {lo}..={hi}");
            if lo >= EXACT {
                // Width at most 1/64 of the lower bound.
                assert!((hi - lo + 1) * SUB <= lo, "{v}: {lo}..={hi}");
            }
        }
        for b in 0..(bucket(u64::MAX)) {
            assert_eq!(bounds(b).1 + 1, bounds(b + 1).0, "gap after bucket {b}");
        }
    }

    #[test]
    fn percentiles_of_known_data() {
        let mut h = Histogram::new();
        assert_eq!(h.summary(), None);
        // 1..=100 exactly representable.
        for v in 1..=100 {
            h.record(v);
        }
        let s = h.summary().unwrap();
        assert_eq!((s.count, s.min, s.max), (100, 1, 100));
        assert_eq!((s.p50, s.p90, s.p99), (50, 90, 99));
        assert!((s.mean - 50.5).abs() < 1e-9);
    }

    #[test]
    fn large_values_are_within_one_percent() {
        let mut h = Histogram::new();
        // 10 000 latencies from 1 ms to 10 s in µs.
        let vals: Vec<u64> = (0..10_000u64).map(|i| 1_000 + i * 9_999).collect();
        for v in &vals {
            h.record(*v);
        }
        let mut sorted = vals.clone();
        sorted.sort_unstable();
        for q in [0.5, 0.9, 0.99] {
            let exact = sorted[((q * sorted.len() as f64).ceil() as usize).max(1) - 1] as f64;
            let got = h.quantile(q).unwrap() as f64;
            assert!(
                ((got - exact) / exact).abs() < 0.01,
                "q={q}: {got} vs {exact}"
            );
        }
        assert_eq!(h.quantile(1.0), Some(*sorted.last().unwrap()));
        assert_eq!(h.quantile(0.0), Some(sorted[0]));
    }

    #[test]
    fn merging_matches_recording_together() {
        let (mut a, mut b, mut all) = (Histogram::new(), Histogram::new(), Histogram::new());
        for v in [5u64, 900, 70_000, 3] {
            a.record(v);
            all.record(v);
        }
        for v in [1u64, 1_000_000, 42] {
            b.record(v);
            all.record(v);
        }
        a.merge(&b);
        a.merge(&Histogram::new());
        assert_eq!(a, all);
    }
}
