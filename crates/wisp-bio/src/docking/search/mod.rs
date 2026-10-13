//! The conformational search: Monte-Carlo proposals with quasi-Newton
//! relaxation over the rigid-body and torsional degrees of freedom.
//!
//! * [`rng`] — the deterministic per-stream RNG (#274 layout).

pub mod rng;

pub use rng::{Rng, SplitMix64};
