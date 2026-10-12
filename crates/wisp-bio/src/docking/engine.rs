//! The docking engine contract and the built-in scoring engine.
//!
//! [`DockingEngine`] is the seam between the control plane and every docking
//! backend: the built-in Vina-family engine today, the protein-protein and
//! peptide engines of later milestones, and optional external validation
//! engines. Engines report a per-term decomposition so results can explain
//! themselves in the UI, and `search` is deterministic for a given seed.

use crate::docking::prep::{PreparedLigand, PreparedReceptor};
use crate::docking::scoring::{
    evaluate_inter, evaluate_intra, PairBreakdown, VinaParams, VINA_1_1,
};

/// Per-term energy decomposition of one pose.
///
/// The five pair terms sum to `inter + intra`; `torsion` is the rotatable
/// bond penalty; `total = inter + intra + torsion`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScoreTerms {
    pub gauss1: f64,
    pub gauss2: f64,
    pub repulsion: f64,
    pub hydrophobic: f64,
    pub hbond: f64,
    pub torsion: f64,
    pub inter: f64,
    pub intra: f64,
    pub total: f64,
}

impl ScoreTerms {
    pub fn from_breakdowns(inter: PairBreakdown, intra: PairBreakdown, torsion: f64) -> ScoreTerms {
        let pair_total = |breakdown: &PairBreakdown| breakdown.total();
        let inter_total = pair_total(&inter);
        let intra_total = pair_total(&intra);
        ScoreTerms {
            gauss1: inter.gauss1 + intra.gauss1,
            gauss2: inter.gauss2 + intra.gauss2,
            repulsion: inter.repulsion + intra.repulsion,
            hydrophobic: inter.hydrophobic + intra.hydrophobic,
            hbond: inter.hbond + intra.hbond,
            torsion,
            inter: inter_total,
            intra: intra_total,
            total: inter_total + intra_total + torsion,
        }
    }
}

/// One scored ligand placement.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredPose {
    pub terms: ScoreTerms,
    /// Ligand atom positions in the same order as `PreparedLigand::atoms`.
    pub positions: Vec<[f64; 3]>,
}

/// Knobs every engine honors. Later milestones add search-space boxes and
/// constraints; the classic Vina knobs are here from the start.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOptions {
    pub seed: u64,
    pub exhaustiveness: u32,
    pub num_modes: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            seed: 0,
            exhaustiveness: 8,
            num_modes: 9,
        }
    }
}

/// The docking backend contract.
pub trait DockingEngine {
    /// Short engine identifier for artifacts and logs.
    fn name(&self) -> &'static str;

    /// Score a specific ligand placement.
    fn score(
        &self,
        receptor: &PreparedReceptor,
        ligand: &PreparedLigand,
        ligand_positions: &[[f64; 3]],
    ) -> ScoreTerms;

    /// Search for favorable placements, best first.
    fn search(
        &self,
        receptor: &PreparedReceptor,
        ligand: &PreparedLigand,
        options: &SearchOptions,
    ) -> Vec<ScoredPose>;
}

/// The built-in Vina-family engine.
///
/// In this milestone `search` scores the input placement (score-only mode);
/// the Monte-Carlo search lands with the next milestone behind the same
/// trait, so callers already target the final contract.
#[derive(Debug, Clone)]
pub struct VinaEngine {
    params: VinaParams,
}

impl VinaEngine {
    pub fn new(params: VinaParams) -> VinaEngine {
        VinaEngine { params }
    }

    pub fn classic() -> VinaEngine {
        VinaEngine::new(VINA_1_1)
    }

    pub fn params(&self) -> &VinaParams {
        &self.params
    }
}

impl Default for VinaEngine {
    fn default() -> Self {
        VinaEngine::classic()
    }
}

impl DockingEngine for VinaEngine {
    fn name(&self) -> &'static str {
        "builtin-vina"
    }

    fn score(
        &self,
        receptor: &PreparedReceptor,
        ligand: &PreparedLigand,
        ligand_positions: &[[f64; 3]],
    ) -> ScoreTerms {
        let inter = evaluate_inter(receptor, ligand, ligand_positions, &self.params);
        let intra = evaluate_intra(ligand, ligand_positions, &self.params);
        let torsion = self.params.w_rot * ligand.torsdof as f64;
        ScoreTerms::from_breakdowns(inter, intra, torsion)
    }

    fn search(
        &self,
        receptor: &PreparedReceptor,
        ligand: &PreparedLigand,
        _options: &SearchOptions,
    ) -> Vec<ScoredPose> {
        let positions: Vec<[f64; 3]> = ligand.atoms.iter().map(|atom| atom.position).collect();
        let terms = self.score(receptor, ligand, &positions);
        vec![ScoredPose { terms, positions }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docking::prep::atom_type::{AtomType, Element};
    use crate::docking::prep::{prepare_ligand_pdb, prepare_receptor, DockAtom};

    #[test]
    fn score_terms_decompose_into_inter_intra_and_torsion() {
        let terms = ScoreTerms::from_breakdowns(
            PairBreakdown {
                gauss1: -1.0,
                gauss2: -2.0,
                repulsion: 0.5,
                hydrophobic: -3.0,
                hbond: -4.0,
            },
            PairBreakdown {
                gauss1: -0.25,
                gauss2: -0.5,
                repulsion: 0.125,
                hydrophobic: -0.75,
                hbond: -1.0,
            },
            0.5,
        );
        assert!((terms.inter - (-9.5)).abs() < 1e-12);
        assert!((terms.intra - (-2.375)).abs() < 1e-12);
        assert!((terms.torsion - 0.5).abs() < 1e-12);
        assert!((terms.total - (terms.inter + terms.intra + terms.torsion)).abs() < 1e-12);
        assert!((terms.gauss1 - (-1.25)).abs() < 1e-12);
        assert!((terms.hbond - (-5.0)).abs() < 1e-12);
    }

    #[test]
    fn engine_scores_a_placed_ligand_against_a_receptor() {
        // One hydrophobic carbon receptor atom and a single-carbon ligand at
        // surface contact: the pair values must show up in the total.
        let receptor = prepare_receptor(
            "ATOM      1  CA  GLY A   1       0.000   0.000   0.000  1.00  0.00           C\n",
        )
        .unwrap();
        assert_eq!(receptor.atoms[0].ad_type, AtomType::C);
        let ligand = prepare_ligand_pdb(
            "HETATM    1  C   LIG L   1       3.800   0.000   0.000  1.00  0.00           C\n",
        )
        .unwrap();
        let engine = VinaEngine::classic();
        let positions: Vec<[f64; 3]> = ligand.atoms.iter().map(|a| a.position).collect();
        let terms = engine.score(&receptor, &ligand, &positions);
        assert!((terms.inter - pair_total_at_contact()).abs() < 1e-12);
        assert_eq!(terms.intra, 0.0); // one atom: no pairs at all
        assert_eq!(terms.torsion, 0.0);
        assert!((terms.total - terms.inter).abs() < 1e-12);
    }

    fn pair_total_at_contact() -> f64 {
        let mut first = DockAtom {
            serial: 1,
            name: "C".into(),
            element: Element::C,
            ad_type: AtomType::C,
            polar_carbon: false,
            position: [0.0; 3],
            charge: 0.0,
            residue_name: "GLY".into(),
            chain: 'A',
            residue_seq: 1,
            insertion_code: ' ',
            is_hetero: false,
        };
        first.element = Element::C;
        let second = DockAtom {
            position: [3.8, 0.0, 0.0],
            ..first.clone()
        };
        crate::docking::scoring::pair_energy(&first, &second, 3.8, &VINA_1_1).total()
    }

    #[test]
    fn search_returns_the_input_pose_scored() {
        // One hydrophobic carbon each at surface contact: the search result
        // carries the pair energy and the unchanged placement.
        let receptor = prepare_receptor(
            "ATOM      1  CA  GLY A   1       0.000   0.000   0.000  1.00  0.00           C\n",
        )
        .unwrap();
        let ligand = prepare_ligand_pdb(
            "HETATM    1  C   LIG L   1       3.800   0.000   0.000  1.00  0.00           C\n",
        )
        .unwrap();
        let engine = VinaEngine::classic();
        let poses = engine.search(&receptor, &ligand, &SearchOptions::default());
        assert_eq!(poses.len(), 1);
        assert_eq!(poses[0].positions, vec![[3.8, 0.0, 0.0]]);
        assert!(poses[0].terms.total < 0.0);
        assert!(poses[0].terms.hydrophobic < 0.0);
    }
}
