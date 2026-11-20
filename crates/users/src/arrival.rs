//! When each user of a cohort starts, as offsets from the start of the run.

use serde::{Deserialize, Serialize};

use crate::rng::Rng;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    /// Evenly spaced: user `i` of `n` starts at `i * window / n`.
    #[default]
    Uniform,
    /// A Poisson process with `n / window` arrivals per unit of time: random
    /// gaps, exponentially distributed. Later arrivals are clamped to the window.
    Poisson,
}

/// Start offsets (ns, ascending) for `users` users over `window_ns`.
/// `rng` is only used by [`Model::Poisson`].
pub fn offsets(model: Model, users: u32, window_ns: i64, rng: &mut Rng) -> Vec<i64> {
    let n = u64::from(users);
    let window = window_ns.max(0);
    if n == 0 {
        return vec![];
    }
    match model {
        Model::Uniform => (0..n)
            .map(|i| (i128::from(window) * i128::from(i) / i128::from(n)) as i64)
            .collect(),
        Model::Poisson => {
            let mean_gap = window as f64 / n as f64;
            let mut t = 0.0;
            (0..n)
                .map(|_| {
                    t += rng.exponential(mean_gap);
                    (t as i64).min(window)
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: i64 = 1_000_000_000;

    #[test]
    fn uniform_is_evenly_spaced() {
        let mut rng = Rng::new(1);
        assert_eq!(
            offsets(Model::Uniform, 4, 60 * SEC, &mut rng),
            [0, 15 * SEC, 30 * SEC, 45 * SEC]
        );
        assert_eq!(offsets(Model::Uniform, 3, 0, &mut rng), [0, 0, 0]);
        assert!(offsets(Model::Uniform, 0, SEC, &mut rng).is_empty());
    }

    #[test]
    fn poisson_is_seeded_sorted_and_inside_the_window() {
        let a = offsets(Model::Poisson, 500, 600 * SEC, &mut Rng::new(42));
        let b = offsets(Model::Poisson, 500, 600 * SEC, &mut Rng::new(42));
        assert_eq!(a, b, "same seed, same arrivals");
        assert_ne!(
            a,
            offsets(Model::Poisson, 500, 600 * SEC, &mut Rng::new(43))
        );
        assert!(a.windows(2).all(|w| w[0] <= w[1]));
        assert!(a.iter().all(|t| (0..=600 * SEC).contains(t)));
        // The mean gap is close to window / users (1.2 s) for 500 arrivals.
        let unclamped = a.iter().filter(|t| **t < 600 * SEC).count();
        let mean_gap = a[unclamped - 1] as f64 / unclamped as f64 / SEC as f64;
        assert!((1.0..1.4).contains(&mean_gap), "{mean_gap}");
    }
}
