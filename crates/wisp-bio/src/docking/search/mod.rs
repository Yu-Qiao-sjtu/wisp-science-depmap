//! The conformational search: Monte-Carlo proposals with quasi-Newton
//! relaxation over the rigid-body and torsional degrees of freedom.
//!
//! * [`rng`] — the deterministic per-stream RNG (#274 layout).
//! * [`conformation`] — rigid transform, torsion tree, pose materialization
//!   (#272).

pub mod conformation;
pub mod rng;

pub use conformation::{wrap_angle, Conformation, LigandFrame, Quaternion, TorsionTree};
pub use rng::{Rng, SplitMix64};
