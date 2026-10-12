//! Rust-native molecular docking engine.
//!
//! Dismantles the mainstream docking tools into functional modules rebuilt
//! in Rust inside this crate, so docking never requires an external binary
//! at runtime. This milestone lands [`prep`]: structure preparation —
//! PDB/PDBQT parsing, AutoDock atom typing, Gasteiger charges, bond/ring/
//! aromaticity perception, and the united-atom merge. The scoring functions
//! and the shared engine contract follow in later milestones.
//!
//! NOTICE: the AutoDock typing conventions derive from AutoDock Vina
//! (Apache-2.0), Copyright AutoDock contributors.

pub mod prep;

pub use prep::{
    prepare_ligand_pdb, prepare_ligand_pdbqt, prepare_receptor, PreparedLigand, PreparedReceptor,
};
