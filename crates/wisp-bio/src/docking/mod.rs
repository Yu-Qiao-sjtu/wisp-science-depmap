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
//! * [`trajectory`] — DCD output so pose series play in the viewer's
//!   stock trajectory pipeline (#276).
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
pub mod trajectory;

pub use engine::{DockingEngine, ScoreTerms, ScoredPose, SearchOptions, VinaEngine};
pub use prep::{
    prepare_ligand_pdb, prepare_ligand_pdbqt, prepare_receptor, DockAtom, PreparedLigand,
    PreparedReceptor,
};
pub use scoring::{
    evaluate_inter, evaluate_intra, pair_energy, PairBreakdown, SurfaceRadii, VinaParams, VINARDO,
    VINA_1_1,
};

/// One-shot builtin docking run over files (#275): prepare, search, and
/// write `pose-001.pdb`... frames plus a `scores.tsv` table into
/// `output_dir`. The receptor `ATOM` records are kept verbatim so the
/// viewer shows the same protein as the input; ligand atoms are rewritten
/// as `HETATM` records at the searched coordinates. Returns each mode
/// together with the frame path, best first.
pub fn dock_builtin(
    receptor_path: &std::path::Path,
    ligand_path: &std::path::Path,
    output_dir: &std::path::Path,
    options: &SearchOptions,
) -> Result<Vec<(std::path::PathBuf, ScoredPose)>, String> {
    let receptor_text = std::fs::read_to_string(receptor_path).map_err(|e| e.to_string())?;
    let ligand_text = std::fs::read_to_string(ligand_path).map_err(|e| e.to_string())?;
    let receptor = prepare_receptor(&receptor_text)?;
    let ligand = prepare_ligand_pdb(&ligand_text)?;
    let engine = VinaEngine::classic();
    let poses = engine.search(&receptor, &ligand, options);
    if poses.is_empty() {
        return Err("the builtin engine produced no poses".into());
    }
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    let receptor_lines: Vec<&str> = receptor_text
        .lines()
        .filter(|l| l.starts_with("ATOM  "))
        .collect();
    let mut table = String::from(
        "pose\tgauss1\tgauss2\trepulsion\thydrophobic\thbond\tinter\tintra\ttorsion\ttotal\n",
    );
    let mut out = Vec::with_capacity(poses.len());
    for (index, pose) in poses.iter().enumerate() {
        let terms = &pose.terms;
        table.push_str(&format!(
            "{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\n",
            index + 1,
            terms.gauss1,
            terms.gauss2,
            terms.repulsion,
            terms.hydrophobic,
            terms.hbond,
            terms.inter,
            terms.intra,
            terms.torsion,
            terms.total,
        ));
        let mut body = receptor_lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        for (atom_index, atom) in ligand.atoms.iter().enumerate() {
            body.push_str(&hetatm_line(atom, atom_index, pose.positions[atom_index]));
            body.push('\n');
        }
        body.push_str("END\n");
        let path = output_dir.join(format!("pose-{:03}.pdb", index + 1));
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        out.push((path, pose.clone()));
    }
    std::fs::write(output_dir.join("scores.tsv"), table).map_err(|e| e.to_string())?;
    Ok(out)
}

/// A ligand atom as a viewer-ready `HETATM` record in PDB column layout.
fn hetatm_line(atom: &DockAtom, index: usize, position: [f64; 3]) -> String {
    format!(
        "HETATM{serial:5} {name:<4} {residue:>3} {chain}{residue_seq:4}{icode}   {x:8.3}{y:8.3}{z:8.3}  1.00  0.00           {element:>2}",
        serial = index + 1,
        name = atom.name.trim_end(),
        residue = "LIG",
        chain = 'L',
        residue_seq = 1,
        icode = ' ',
        x = position[0],
        y = position[1],
        z = position[2],
        element = atom.element.symbol(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_builtin_writes_pose_frames_and_score_table() {
        let dir = std::env::temp_dir().join(format!("wisp-dock-builtin-{}", std::process::id()));
        let receptor_path = dir.join("receptor.pdb");
        let ligand_path = dir.join("ligand.pdb");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &receptor_path,
            "ATOM      1  CA  GLY A   1       0.000   0.000   0.000  1.00  0.00           C\n",
        )
        .unwrap();
        std::fs::write(
            &ligand_path,
            "HETATM    1  C   LIG L   1       8.000   8.000   8.000  1.00  0.00           C\n",
        )
        .unwrap();
        let output = dir.join("poses");
        let options = SearchOptions {
            seed: 7,
            exhaustiveness: 2,
            num_modes: 3,
        };
        let poses = dock_builtin(&receptor_path, &ligand_path, &output, &options).unwrap();
        assert!(!poses.is_empty());
        // Every frame exists, keeps the receptor line, and carries the
        // ligand at the searched coordinates.
        for (index, (path, pose)) in poses.iter().enumerate() {
            assert!(path.exists(), "missing {path:?}");
            let body = std::fs::read_to_string(path).unwrap();
            assert!(body.starts_with("ATOM  "));
            assert!(body.contains("HETATM    1  C   LIG"));
            let [x, _y, _z] = pose.positions[0];
            let x_text = format!("{x:8.3}");
            assert!(body.contains(&x_text), "frame {index} lacks {x_text}");
            assert!(body.ends_with("END\n"));
        }
        // The score table lists one row per pose with a header.
        let table = std::fs::read_to_string(output.join("scores.tsv")).unwrap();
        assert!(table.starts_with("pose\tgauss1\t"));
        assert_eq!(table.lines().count(), poses.len() + 1);
        // Determinism: the same seed and inputs reproduce the same files.
        let repeat_dir = dir.join("poses-repeat");
        let repeat = dock_builtin(&receptor_path, &ligand_path, &repeat_dir, &options).unwrap();
        assert_eq!(poses.len(), repeat.len());
        assert_eq!(poses[0].1, repeat[0].1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
