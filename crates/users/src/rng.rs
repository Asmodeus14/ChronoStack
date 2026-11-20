//! A small seeded random generator: xoshiro256\*\* (Blackman and Vigna), with
//! its state filled by SplitMix64.
//!
//! Written out here rather than taken from a crate so a seed gives the same
//! values on every platform and in every ChronoStack version: a run's choices
//! (who arrives when, which branch a user takes, how long they think) can be
//! reproduced from its seed.

/// SplitMix64, used to expand one `u64` seed into a full generator state.
#[derive(Debug, Clone)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// xoshiro256\*\*.
#[derive(Debug, Clone)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// A generator for one seed.
    pub fn new(seed: u64) -> Self {
        let mut sm = SplitMix64::new(seed);
        Rng {
            s: [sm.next_u64(), sm.next_u64(), sm.next_u64(), sm.next_u64()],
        }
    }

    /// An independent generator for one part of a run, e.g. `(cohort, user)`:
    /// the run seed and each id are mixed through SplitMix64, so neighbouring
    /// ids don't give related sequences.
    pub fn derive(seed: u64, ids: &[u64]) -> Self {
        let mut x = SplitMix64::new(seed).next_u64();
        for id in ids {
            x = SplitMix64::new(x ^ id.wrapping_mul(0x9E37_79B9_7F4A_7C15)).next_u64();
        }
        Rng::new(x)
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform in `[0, 1)`, from the top 53 bits.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in `lo..=hi` (`lo <= hi`), without modulo bias.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(lo <= hi, "range({lo}, {hi})");
        let span = hi - lo;
        if span == u64::MAX {
            return self.next_u64();
        }
        let n = span + 1;
        // Reject the top, partial copy of 0..n so every value is equally likely.
        let zone = u64::MAX - (u64::MAX % n) - 1;
        loop {
            let x = self.next_u64();
            if x <= zone {
                return lo + x % n;
            }
        }
    }

    /// `true` with probability `p` (clamped to `[0, 1]`).
    pub fn chance(&mut self, p: f64) -> bool {
        self.next_f64() < p.clamp(0.0, 1.0)
    }

    /// Exponentially distributed with the given mean (for Poisson arrivals).
    pub fn exponential(&mut self, mean: f64) -> f64 {
        // 1 - u is in (0, 1], so the log is finite.
        -mean * (1.0 - self.next_f64()).ln()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values from an independent Python implementation of the
    // published algorithms. SplitMix64(1234567) is its well-known test vector.
    #[test]
    fn splitmix64_reference() {
        let mut g = SplitMix64::new(1_234_567);
        let got: Vec<u64> = (0..5).map(|_| g.next_u64()).collect();
        assert_eq!(
            got,
            [
                6457827717110365317,
                3203168211198807973,
                9817491932198370423,
                4593380528125082431,
                16408922859458223821
            ]
        );
    }

    #[test]
    fn xoshiro_reference() {
        let mut g = Rng::new(0);
        let got: Vec<u64> = (0..5).map(|_| g.next_u64()).collect();
        assert_eq!(
            got,
            [
                11091344671253066420,
                13793997310169335082,
                1900383378846508768,
                7684712102626143532,
                13521403990117723737
            ]
        );
        let mut g = Rng::new(42);
        let got: Vec<u64> = (0..5).map(|_| g.next_u64()).collect();
        assert_eq!(
            got,
            [
                1546998764402558742,
                6990951692964543102,
                12544586762248559009,
                17057574109182124193,
                18295552978065317476
            ]
        );
    }

    #[test]
    fn derived_generators_are_stable_and_distinct() {
        let a: Vec<u64> = {
            let mut g = Rng::derive(42, &[0, 7]);
            (0..3).map(|_| g.next_u64()).collect()
        };
        let again: Vec<u64> = {
            let mut g = Rng::derive(42, &[0, 7]);
            (0..3).map(|_| g.next_u64()).collect()
        };
        assert_eq!(a, again);
        for other in [[0, 8], [1, 7], [7, 0]] {
            let mut g = Rng::derive(42, &other);
            assert_ne!(a[0], g.next_u64(), "{other:?}");
        }
        let mut g = Rng::derive(43, &[0, 7]);
        assert_ne!(a[0], g.next_u64());
    }

    #[test]
    fn helpers_stay_in_range() {
        let mut g = Rng::new(7);
        for _ in 0..10_000 {
            let f = g.next_f64();
            assert!((0.0..1.0).contains(&f));
            let r = g.range(3, 9);
            assert!((3..=9).contains(&r));
            assert!(g.exponential(2.0) >= 0.0);
        }
        assert_eq!(g.range(5, 5), 5);
        assert!(!g.chance(0.0));
        assert!(g.chance(1.0));
        // Roughly fair: 10 000 draws of 0..=1 land within a few % of half.
        let ones = (0..10_000).filter(|_| g.range(0, 1) == 1).count();
        assert!((4_700..5_300).contains(&ones), "{ones}");
    }
}
