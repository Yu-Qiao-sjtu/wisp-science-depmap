//! The conformational search: Monte-Carlo proposals with quasi-Newton
//! relaxation over the rigid-body and torsional degrees of freedom.
//!
//! * [`rng`] — the deterministic per-stream RNG (#274 layout).
//! * [`conformation`] — rigid transform, torsion tree, pose materialization
//!   (#272).
//! * [`quasi_newton`] — BFGS local minimization (#271).
//! * [`monte_carlo`] — the Metropolis outer loop, multi-start streams, mode
//!   pooling and RMSD deduplication (#271).
//! * [`grid`] — receptor cell-list and intramolecular pair precalculation
//!   that accelerate scoring without changing its results (#273).

pub mod conformation;
pub mod grid;
pub mod monte_carlo;
pub mod quasi_newton;
pub mod rng;

pub use conformation::{wrap_angle, Conformation, LigandFrame, Quaternion, TorsionTree};
pub use monte_carlo::{pose_rmsd, search_poses, SearchBox, SearchSchedule};
pub use rng::{Rng, SplitMix64};
