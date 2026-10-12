//! Rust-native molecular docking engine.
//!
//! Dismantles the mainstream docking tools into functional modules rebuilt
//! in Rust inside this crate, so docking never requires an external binary
//! at runtime. This milestone lands [`prep`] and [`scoring`]: structure
//! preparation (PDB/PDBQT parsing, AutoDock atom typing, Gasteiger charges,
//! bond/ring/aromaticity perception, the united-atom merge) and the pairwise
//! scoring functions. The shared engine contract follows in a later
//! milestone.
//!
//! NOTICE: the AutoDock typing conventions and the Vina scoring constants
//! derive from AutoDock Vina (Apache-2.0), Copyright AutoDock contributors,
//! and from Quiroga & Villarreal, PLOS ONE 2016 (Vinardo); see the scoring
//! module docs for the parameter provenance.

pub mod prep;
pub mod scoring;

pub use prep::{
    prepare_ligand_pdb, prepare_ligand_pdbqt, prepare_receptor, PreparedLigand, PreparedReceptor,
};
pub use scoring::{
    evaluate_inter, evaluate_intra, pair_energy, PairBreakdown, SurfaceRadii, VinaParams, VINARDO,
    VINA_1_1,
};
