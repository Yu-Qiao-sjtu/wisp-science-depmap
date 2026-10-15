//! The docking engine contract and built-in Vina-family scoring engine.
//!
//! The contract distinguishes the pose-search objective from the affinity that
//! Vina reports after conformation-independent rotor normalization. Engines
//! reject invalid models and malformed position arrays instead of panicking.

use crate::docking::prep::{PreparedLigand, PreparedReceptor};
use crate::docking::scoring::{
    evaluate_inter, evaluate_intra, PairBreakdown, VinaParams, VINA_1_1,
};

/// Explicit decomposition of one scored pose.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScoreTerms {
    /// Weighted intermolecular pair terms.
    pub inter_terms: PairBreakdown,
    /// Weighted variable-distance intramolecular pair terms.
    pub intra_terms: PairBreakdown,
    pub raw_inter: f64,
    pub raw_intra: f64,
    /// Objective used to rank and optimize poses: `raw_inter + raw_intra`.
    pub search_energy: f64,
    /// Vina effective heavy-atom rotor count. Terminal axes may contribute 0.5.
    pub num_tors: f64,
    /// Positive divisor `1 + w_rot * num_tors`.
    pub torsion_factor: f64,
    /// Reported Vina-family affinity: `raw_inter / torsion_factor`.
    pub reported_affinity: f64,
}

impl ScoreTerms {
    pub fn from_breakdowns(
        inter_terms: PairBreakdown,
        intra_terms: PairBreakdown,
        num_tors: f64,
        params: &VinaParams,
    ) -> Result<ScoreTerms, String> {
        inter_terms.require_finite("intermolecular terms")?;
        intra_terms.require_finite("intramolecular terms")?;
        let raw_inter = inter_terms.total();
        let raw_intra = intra_terms.total();
        let search_energy = raw_inter + raw_intra;
        let torsion_factor = params.torsion_factor(num_tors)?;
        let reported_affinity = raw_inter / torsion_factor;
        for (name, value) in [
            ("raw intermolecular score", raw_inter),
            ("raw intramolecular score", raw_intra),
            ("search energy", search_energy),
            ("reported affinity", reported_affinity),
        ] {
            if !value.is_finite() {
                return Err(format!("{name} must be finite"));
            }
        }
        Ok(ScoreTerms {
            inter_terms,
            intra_terms,
            raw_inter,
            raw_intra,
            search_energy,
            num_tors,
            torsion_factor,
            reported_affinity,
        })
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
    /// Stable scoring-model identifier for artifacts and logs.
    fn name(&self) -> &'static str;

    /// Human-readable scoring-model name.
    fn display_name(&self) -> &'static str;

    /// Score a specific ligand placement.
    fn score(
        &self,
        receptor: &PreparedReceptor,
        ligand: &PreparedLigand,
        ligand_positions: &[[f64; 3]],
    ) -> Result<ScoreTerms, String>;

    /// Search for favorable placements, best search objective first.
    fn search(
        &self,
        receptor: &PreparedReceptor,
        ligand: &PreparedLigand,
        options: &SearchOptions,
    ) -> Result<Vec<ScoredPose>, String>;
}

/// The built-in Vina-family engine.
///
/// In this milestone `search` scores the input placement (score-only mode);
/// later search implementations must rank with [`ScoreTerms::search_energy`],
/// not the torsion-normalized reported affinity.
#[derive(Debug, Clone)]
pub struct VinaEngine {
    params: VinaParams,
}

impl VinaEngine {
    /// Construct and validate a scoring engine.
    pub fn new(params: VinaParams) -> Result<VinaEngine, String> {
        params.validate()?;
        Ok(VinaEngine { params })
    }

    pub fn try_new(params: VinaParams) -> Result<VinaEngine, String> {
        VinaEngine::new(params)
    }

    /// Infallible constructor for the validated built-in classic constants.
    pub fn classic() -> VinaEngine {
        VinaEngine { params: VINA_1_1 }
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
        self.params.model_id
    }

    fn display_name(&self) -> &'static str {
        self.params.model_name
    }

    fn score(
        &self,
        receptor: &PreparedReceptor,
        ligand: &PreparedLigand,
        ligand_positions: &[[f64; 3]],
    ) -> Result<ScoreTerms, String> {
        let num_tors = ligand.validated_num_tors()?;
        let inter_terms = evaluate_inter(receptor, ligand, ligand_positions, &self.params)?;
        let intra_terms = evaluate_intra(ligand, ligand_positions, &self.params)?;
        ScoreTerms::from_breakdowns(inter_terms, intra_terms, num_tors, &self.params)
    }

    fn search(
        &self,
        receptor: &PreparedReceptor,
        ligand: &PreparedLigand,
        _options: &SearchOptions,
    ) -> Result<Vec<ScoredPose>, String> {
        let positions: Vec<[f64; 3]> = ligand.atoms.iter().map(|atom| atom.position).collect();
        let terms = self.score(receptor, ligand, &positions)?;
        Ok(vec![ScoredPose { terms, positions }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docking::prep::atom_type::{AdType, XsType};
    use crate::docking::prep::{prepare_ligand_pdbqt, prepare_receptor_pdbqt};
    use crate::docking::scoring::VINARDO;

    #[test]
    fn score_terms_separate_search_objective_from_reported_affinity() {
        let terms = ScoreTerms::from_breakdowns(
            PairBreakdown {
                gauss1: -6.0,
                ..PairBreakdown::default()
            },
            PairBreakdown {
                repulsion: 2.0,
                ..PairBreakdown::default()
            },
            2.0,
            &VINA_1_1,
        )
        .unwrap();
        assert_eq!(terms.raw_inter, -6.0);
        assert_eq!(terms.raw_intra, 2.0);
        assert_eq!(terms.search_energy, -4.0);
        assert_eq!(terms.num_tors, 2.0);
        assert!((terms.torsion_factor - (1.0 + 2.0 * VINA_1_1.w_rot)).abs() < 1e-12);
        assert!((terms.reported_affinity - (-6.0 / terms.torsion_factor)).abs() < 1e-12);
    }

    #[test]
    fn score_terms_reject_non_finite_components_before_aggregation() {
        let error = ScoreTerms::from_breakdowns(
            PairBreakdown {
                repulsion: f64::INFINITY,
                ..PairBreakdown::default()
            },
            PairBreakdown::default(),
            0.0,
            &VINA_1_1,
        )
        .unwrap_err();
        assert!(error.contains("intermolecular terms repulsion must be finite"));

        let error = ScoreTerms::from_breakdowns(
            PairBreakdown {
                gauss1: f64::MAX,
                gauss2: f64::MAX,
                ..PairBreakdown::default()
            },
            PairBreakdown::default(),
            0.0,
            &VINA_1_1,
        )
        .unwrap_err();
        assert!(error.contains("intermolecular terms total must be finite"));
    }

    #[test]
    fn rotor_normalization_uses_num_tors_not_file_torsdof() {
        let receptor = carbon_receptor();
        let ligand = ligand_with_one_branch_and_torsdof(7);
        assert_eq!(ligand.num_tors, 1.0);
        assert_eq!(ligand.torsdof, 7);
        let terms = VinaEngine::classic()
            .score(&receptor, &ligand, &ligand_positions(&ligand))
            .unwrap();
        assert_eq!(terms.num_tors, 1.0);
        assert!((terms.torsion_factor - (1.0 + VINA_1_1.w_rot)).abs() < 1e-12);

        let mut malformed = ligand.clone();
        malformed.num_tors = 0.0;
        assert!(VinaEngine::classic()
            .score(&receptor, &malformed, &ligand_positions(&malformed))
            .unwrap_err()
            .contains("does not match topology-derived value"));

        let mut missing_topology = ligand.clone();
        missing_topology.branches.clear();
        missing_topology.rotatable.clear();
        missing_topology.num_tors = 0.0;
        assert!(VinaEngine::classic()
            .score(
                &receptor,
                &missing_topology,
                &ligand_positions(&missing_topology),
            )
            .unwrap_err()
            .contains("active branches do not match"));
    }

    #[test]
    fn search_ranking_and_reported_affinity_can_disagree_across_rotor_counts() {
        let rigid = ScoreTerms::from_breakdowns(
            PairBreakdown {
                gauss1: -5.0,
                ..PairBreakdown::default()
            },
            PairBreakdown::default(),
            0.0,
            &VINA_1_1,
        )
        .unwrap();
        let flexible = ScoreTerms::from_breakdowns(
            PairBreakdown {
                gauss1: -8.0,
                ..PairBreakdown::default()
            },
            PairBreakdown {
                repulsion: 3.2,
                ..PairBreakdown::default()
            },
            10.0,
            &VINA_1_1,
        )
        .unwrap();
        assert!(rigid.search_energy < flexible.search_energy);
        assert!(flexible.reported_affinity < rigid.reported_affinity);
    }

    #[test]
    fn engine_scores_strict_synthetic_pdbqt_and_reports_model_identity() {
        let receptor = carbon_receptor();
        let ligand = one_atom_ligand(3.8, AdType::C);
        let engine = VinaEngine::classic();
        assert_eq!(engine.name(), "vina");
        assert_eq!(engine.display_name(), "AutoDock Vina");
        let terms = engine
            .score(&receptor, &ligand, &ligand_positions(&ligand))
            .unwrap();
        assert!(terms.raw_inter < 0.0);
        assert_eq!(terms.raw_intra, 0.0);
        assert_eq!(terms.search_energy, terms.raw_inter);
        assert_eq!(terms.reported_affinity, terms.raw_inter);
        assert!(terms.inter_terms.hydrophobic < 0.0);
    }

    #[test]
    fn score_is_invariant_under_joint_rotation_and_translation() {
        let receptor = carbon_receptor();
        let ligand = one_atom_ligand(3.8, AdType::C);
        let engine = VinaEngine::classic();
        let baseline = engine
            .score(&receptor, &ligand, &ligand_positions(&ligand))
            .unwrap();

        let transformed_receptor = PreparedReceptor {
            atoms: receptor
                .atoms
                .iter()
                .cloned()
                .map(|mut atom| {
                    atom.position = transform(atom.position);
                    atom
                })
                .collect(),
            bonds: receptor.bonds.clone(),
        };
        let transformed_positions = ligand_positions(&ligand)
            .into_iter()
            .map(transform)
            .collect::<Vec<_>>();
        let transformed = engine
            .score(&transformed_receptor, &ligand, &transformed_positions)
            .unwrap();
        assert!((baseline.raw_inter - transformed.raw_inter).abs() < 1e-12);
        assert!((baseline.search_energy - transformed.search_energy).abs() < 1e-12);
        assert!((baseline.reported_affinity - transformed.reported_affinity).abs() < 1e-12);
    }

    #[test]
    fn invalid_params_and_position_lengths_fail_explicitly() {
        let mut params = VINA_1_1;
        params.gauss1_width = 0.0;
        assert!(VinaEngine::new(params)
            .unwrap_err()
            .contains("gauss1 width"));

        let mut params = VINA_1_1;
        params.radii.c = f64::NAN;
        assert!(VinaEngine::new(params).unwrap_err().contains("finite"));

        let mut params = VINA_1_1;
        params.w_rot = f64::INFINITY;
        assert!(VinaEngine::new(params).unwrap_err().contains("finite"));

        let mut params = VINA_1_1;
        params.w_rot = -1.0;
        assert!(VinaEngine::new(params)
            .unwrap_err()
            .contains("non-negative"));
        assert!(params
            .torsion_factor(1.0)
            .unwrap_err()
            .contains("non-negative"));
        assert!(ScoreTerms::from_breakdowns(
            PairBreakdown::default(),
            PairBreakdown::default(),
            1.0,
            &params,
        )
        .unwrap_err()
        .contains("non-negative"));

        let mut params = VINA_1_1;
        params.hydrophobic_offset = params.hydrophobic_onset;
        assert!(VinaEngine::new(params).unwrap_err().contains("ramp"));

        let mut params = VINA_1_1;
        params.hydrophobic_onset = -f64::MAX;
        params.hydrophobic_offset = f64::MAX;
        assert!(VinaEngine::new(params)
            .unwrap_err()
            .contains("hydrophobic ramp span must be finite"));

        let mut params = VINA_1_1;
        params.hbond_onset = -f64::MAX;
        params.hbond_offset = f64::MAX;
        assert!(VinaEngine::new(params)
            .unwrap_err()
            .contains("hydrogen-bond ramp span must be finite"));

        let mut params = VINA_1_1;
        params.radii.metal = 0.0;
        assert!(VinaEngine::new(params).unwrap_err().contains("positive"));

        let receptor = carbon_receptor();
        let ligand = one_atom_ligand(3.8, AdType::C);
        assert!(VinaEngine::classic()
            .score(&receptor, &ligand, &[])
            .unwrap_err()
            .contains("position count"));

        for invalid in [f64::NAN, f64::INFINITY] {
            let mut receptor = receptor.clone();
            receptor.atoms[0].position[0] = invalid;
            assert!(VinaEngine::classic()
                .score(&receptor, &ligand, &ligand_positions(&ligand))
                .unwrap_err()
                .contains("receptor contains a non-finite coordinate"));
        }
    }

    #[test]
    fn vinardo_constructor_preserves_its_model_and_vectors() {
        let engine = VinaEngine::new(VINARDO).unwrap();
        assert_eq!(engine.name(), "vinardo");
        assert_eq!(engine.display_name(), "Vinardo");
        let receptor = carbon_receptor();
        let ligand = one_atom_ligand(4.0, AdType::C);
        let terms = engine
            .score(&receptor, &ligand, &ligand_positions(&ligand))
            .unwrap();
        let rounded = |value: f64| (value * 1000.0).round() / 1000.0;
        assert_eq!(rounded(terms.inter_terms.gauss1), -0.045);
        assert_eq!(terms.inter_terms.gauss2, 0.0);
        assert_eq!(rounded(terms.inter_terms.hydrophobic), -0.035);
    }

    #[test]
    fn search_returns_the_input_pose_scored_by_search_energy() {
        let receptor = carbon_receptor();
        let ligand = one_atom_ligand(3.8, AdType::C);
        let poses = VinaEngine::classic()
            .search(&receptor, &ligand, &SearchOptions::default())
            .unwrap();
        assert_eq!(poses.len(), 1);
        assert_eq!(poses[0].positions, vec![[3.8, 0.0, 0.0]]);
        assert!(poses[0].terms.search_energy < 0.0);
        assert!(poses[0].terms.reported_affinity < 0.0);
    }

    fn carbon_receptor() -> PreparedReceptor {
        let receptor = prepare_receptor_pdbqt(
            "ATOM      1  C1  REC A   1       0.000   0.000   0.000  1.00  0.00     0.000 C \n",
        )
        .unwrap();
        assert_eq!(receptor.atoms[0].xs_type, Some(XsType::C_H));
        receptor
    }

    fn one_atom_ligand(x: f64, ad_type: AdType) -> PreparedLigand {
        prepare_ligand_pdbqt(&format!(
            "ROOT\n{}\nENDROOT\nTORSDOF 0\n",
            atom_line(1, x, ad_type)
        ))
        .unwrap()
    }

    fn ligand_with_one_branch_and_torsdof(torsdof: u32) -> PreparedLigand {
        let atoms = [
            atom_line(1, 3.8, AdType::C),
            atom_line(2, 5.3, AdType::C),
            atom_line(3, 6.8, AdType::C),
            atom_line(4, 8.3, AdType::C),
            atom_line(5, 9.8, AdType::C),
            atom_line(6, 11.3, AdType::C),
        ];
        prepare_ligand_pdbqt(&format!(
            "ROOT\n{}\n{}\n{}\n{}\nENDROOT\nBRANCH 4 5\n{}\n{}\nENDBRANCH 4 5\nTORSDOF {torsdof}\n",
            atoms[0], atoms[1], atoms[2], atoms[3], atoms[4], atoms[5]
        ))
        .unwrap()
    }

    fn atom_line(serial: i32, x: f64, ad_type: AdType) -> String {
        format!(
            "HETATM{serial:5} X{serial:<3} LIG L   1    {x:8.3}{y:8.3}{z:8.3}  1.00  0.00  {charge:8.3} {kind:<2}",
            y = 0.0,
            z = 0.0,
            charge = 0.0,
            kind = ad_type.pdbqt_symbol(),
        )
    }

    fn ligand_positions(ligand: &PreparedLigand) -> Vec<[f64; 3]> {
        ligand.atoms.iter().map(|atom| atom.position).collect()
    }

    fn transform([x, y, z]: [f64; 3]) -> [f64; 3] {
        [-y + 10.0, x - 3.0, z + 2.5]
    }
}
