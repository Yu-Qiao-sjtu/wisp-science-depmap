//! Rust-native molecular docking engine.
//!
//! Dismantles the mainstream docking tools into functional modules rebuilt
//! in Rust inside this crate, so docking never requires an external binary
//! at runtime:
//!
//! * [`prep`] — structure preparation: PDB/PDBQT parsing, AutoDock atom
//!   typing, Gasteiger charges, bond/ring/aromaticity perception, and the
//!   united-atom merge.
//! * [`scoring`] — the pairwise scoring functions. The Vina and Vinardo
//!   parameter sets are ported from AutoDock Vina (Apache-2.0) and the
//!   Vinardo paper; see the module docs for the parameter provenance.
//! * [`search`] — the conformational search: deterministic RNG streams,
//!   the torsion tree, BFGS relaxation, the Metropolis outer loop, and the
//!   grid-accelerated evaluation paths.
//! * [`engine`] — the [`DockingEngine`] contract shared by every backend,
//!   the per-term [`ScoreTerms`] decomposition, and the built-in engine.
//!
//! NOTICE: the Vina scoring constants and typing conventions derive from
//! AutoDock Vina (Apache-2.0), Copyright AutoDock contributors, and from
//! Quiroga & Villarreal, PLOS ONE 2016 (Vinardo). The search scheme ports
//! the published Metropolis/quasi-Newton design from the same Apache-2.0
//! sources.

pub mod engine;
pub mod prep;
pub mod scoring;
pub mod search;

pub use engine::{DockingEngine, ScoreTerms, ScoredPose, SearchOptions, VinaEngine};
pub use prep::{
    prepare_ligand_pdb, prepare_ligand_pdbqt, prepare_receptor, PreparedLigand, PreparedReceptor,
};
pub use scoring::{
    evaluate_inter, evaluate_intra, pair_energy, PairBreakdown, SurfaceRadii, VinaParams, VINARDO,
    VINA_1_1,
};
