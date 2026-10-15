//! Rust-native molecular docking engine.
//!
//! Dismantles the mainstream docking tools into functional modules rebuilt
//! in Rust inside this crate, so docking never requires an external binary
//! at runtime:
//!
//! * [`prep`] — strict PDBQT preparation and parser-only/explicitly approximate
//!   PDB compatibility helpers. Validated Vina scoring requires prepared PDBQT.
//! * [`scoring`] — the pairwise scoring functions. The Vina and Vinardo
//!   parameter sets are ported from AutoDock Vina (Apache-2.0) and the
//!   Vinardo paper; see the module docs for the parameter provenance.
//! * [`engine`] — the [`DockingEngine`] contract shared by every backend,
//!   the per-term [`ScoreTerms`] decomposition, and the built-in scoring
//!   engine.
//!
//! NOTICE: the Vina scoring constants and typing conventions derive from
//! AutoDock Vina (Apache-2.0), Copyright AutoDock contributors, and from
//! Quiroga & Villarreal, PLOS ONE 2016 (Vinardo). The search algorithms of
//! later milestones port the published Metropolis/quasi-Newton scheme from
//! the same Apache-2.0 sources.

pub mod engine;
pub mod prep;
pub mod scoring;

pub use engine::{DockingEngine, ScoreTerms, ScoredPose, SearchOptions, VinaEngine};
pub use prep::atom_type::{AdType, XsType};
pub use prep::{
    prepare_ligand_pdb, prepare_ligand_pdbqt, prepare_receptor, prepare_receptor_pdb_approximate,
    prepare_receptor_pdbqt, ApproximatePreparedReceptor, DockAtom, LigandBranch, PreparedLigand,
    PreparedReceptor, RigidFragment,
};
pub use scoring::{
    evaluate_inter, evaluate_intra, pair_energy, PairBreakdown, SurfaceRadii, VinaParams, VINARDO,
    VINA_1_1,
};
