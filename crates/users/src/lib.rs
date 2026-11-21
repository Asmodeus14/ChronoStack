//! Seeded user journeys for ChronoStack.
//!
//! A journey file describes *who* does *what*: cohorts of simulated users,
//! when they arrive, and the steps each one takes (HTTP requests, think times,
//! probabilistic branches, repeats). This crate holds the parts that don't do
//! I/O, so they can be tested on their own:
//!
//! - [`model`]: the journey file format and its validation.
//! - [`rng`]: a seeded random generator that gives the same values on every
//!   platform and in every version (xoshiro256\*\*, seeded with SplitMix64).
//! - [`arrival`]: when each user of a cohort starts (uniform or Poisson).
//! - [`template`]: `${name}` substitution, the same syntax as scenarios.
//! - [`histogram`]: latency percentiles from log-scaled buckets.
//! - [`engine`]: runs a validated file in real time over HTTP.

pub mod arrival;
pub mod engine;
pub mod histogram;
pub mod model;
pub mod rng;
pub mod template;
