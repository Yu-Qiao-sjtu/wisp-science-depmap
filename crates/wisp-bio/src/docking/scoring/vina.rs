//! AutoDock Vina 1.2.7 Vina and Vinardo empirical scoring functions.
//!
//! Ordinary pair terms use the Vina XS atom identity carried by [`DockAtom`]:
//!
//! * `gauss1 = w * exp(-((d - offset) / width)^2)`
//! * `gauss2` uses the same shape and is disabled by Vinardo
//! * `repulsion = w * d^2` for negative surface distance
//! * hydrophobic and hydrogen-bond terms use linear ramps
//!
//! Here `d = distance - radius(first) - radius(second)`. Explicit hydrogen,
//! water, glue, and otherwise untyped atoms do not enter ordinary pair scoring.
//! Macrocycle glue attraction is intentionally not implemented in this R1
//! ordinary-pair scorer; glue identities therefore contribute zero safely.

use crate::docking::prep::atom_type::XsType;
use crate::docking::prep::{DockAtom, PreparedLigand, PreparedReceptor};

/// Maximum center-to-center distance for ordinary Vina-family pair terms.
pub const INTER_CUTOFF: f64 = 8.0;

/// Official XS surface radii from AutoDock Vina v1.2.7.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceRadii {
    pub c: f64,
    pub n: f64,
    pub o: f64,
    pub s: f64,
    pub p: f64,
    pub f: f64,
    pub cl: f64,
    pub br: f64,
    pub i: f64,
    pub si: f64,
    pub at: f64,
    pub metal: f64,
}

impl SurfaceRadii {
    /// Radius for an ordinary XS pair participant.
    ///
    /// Glue and water are deliberately absent from ordinary pair scoring.
    pub fn of(self, xs_type: XsType) -> Option<f64> {
        match xs_type {
            XsType::C_H | XsType::C_P => Some(self.c),
            XsType::N_P | XsType::N_D | XsType::N_A | XsType::N_DA => Some(self.n),
            XsType::O_P | XsType::O_D | XsType::O_A | XsType::O_DA => Some(self.o),
            XsType::S_P => Some(self.s),
            XsType::P_P => Some(self.p),
            XsType::F_H => Some(self.f),
            XsType::Cl_H => Some(self.cl),
            XsType::Br_H => Some(self.br),
            XsType::I_H => Some(self.i),
            XsType::Si => Some(self.si),
            XsType::At => Some(self.at),
            XsType::Met_D => Some(self.metal),
            XsType::G0 | XsType::G1 | XsType::G2 | XsType::G3 => None,
        }
    }

    fn validate(self) -> Result<(), String> {
        for (name, radius) in [
            ("C radius", self.c),
            ("N radius", self.n),
            ("O radius", self.o),
            ("S radius", self.s),
            ("P radius", self.p),
            ("F radius", self.f),
            ("Cl radius", self.cl),
            ("Br radius", self.br),
            ("I radius", self.i),
            ("Si radius", self.si),
            ("At radius", self.at),
            ("metal radius", self.metal),
        ] {
            require_finite(name, radius)?;
            if radius <= 0.0 {
                return Err(format!("{name} must be positive"));
            }
        }
        Ok(())
    }
}

/// One immutable Vina-family scoring model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VinaParams {
    /// Stable artifact/API identifier.
    pub model_id: &'static str,
    /// Human-readable model name.
    pub model_name: &'static str,
    pub w_gauss1: f64,
    pub w_gauss2: f64,
    pub w_repulsion: f64,
    pub w_hydrophobic: f64,
    pub w_hbond: f64,
    /// Coefficient in `1 + w_rot * num_tors`.
    pub w_rot: f64,
    pub gauss1_offset: f64,
    pub gauss1_width: f64,
    pub gauss2_offset: f64,
    pub gauss2_width: f64,
    /// Full-strength hydrophobic contact below this surface distance.
    pub hydrophobic_onset: f64,
    /// Hydrophobic contact vanishes at this surface distance.
    pub hydrophobic_offset: f64,
    /// Full-strength hydrogen bond below this surface distance.
    pub hbond_onset: f64,
    /// Hydrogen-bond contact vanishes at this surface distance.
    pub hbond_offset: f64,
    pub radii: SurfaceRadii,
}

impl VinaParams {
    /// Reject a model that could produce undefined or ambiguous scores.
    pub fn validate(&self) -> Result<(), String> {
        if self.model_id.trim().is_empty() {
            return Err("Vina model id must not be empty".into());
        }
        if self.model_name.trim().is_empty() {
            return Err("Vina model name must not be empty".into());
        }
        for (name, value) in [
            ("gauss1 weight", self.w_gauss1),
            ("gauss2 weight", self.w_gauss2),
            ("repulsion weight", self.w_repulsion),
            ("hydrophobic weight", self.w_hydrophobic),
            ("hydrogen-bond weight", self.w_hbond),
            ("rotor normalization weight", self.w_rot),
            ("gauss1 offset", self.gauss1_offset),
            ("gauss1 width", self.gauss1_width),
            ("gauss2 offset", self.gauss2_offset),
            ("gauss2 width", self.gauss2_width),
            ("hydrophobic onset", self.hydrophobic_onset),
            ("hydrophobic offset", self.hydrophobic_offset),
            ("hydrogen-bond onset", self.hbond_onset),
            ("hydrogen-bond offset", self.hbond_offset),
        ] {
            require_finite(name, value)?;
        }
        if self.gauss1_width <= 0.0 {
            return Err("gauss1 width must be positive".into());
        }
        if self.gauss2_width <= 0.0 {
            return Err("gauss2 width must be positive".into());
        }
        if self.hydrophobic_offset <= self.hydrophobic_onset {
            return Err("hydrophobic ramp offset must be greater than onset".into());
        }
        require_finite(
            "hydrophobic ramp span",
            self.hydrophobic_offset - self.hydrophobic_onset,
        )?;
        if self.hbond_offset <= self.hbond_onset {
            return Err("hydrogen-bond ramp offset must be greater than onset".into());
        }
        require_finite(
            "hydrogen-bond ramp span",
            self.hbond_offset - self.hbond_onset,
        )?;
        if self.w_rot < 0.0 {
            return Err("rotor normalization weight must be non-negative".into());
        }
        self.radii.validate()
    }

    /// Denominator used by Vina's `num_tors_div` conformation-independent term.
    pub fn torsion_factor(&self, num_tors: f64) -> Result<f64, String> {
        self.validate()?;
        if !num_tors.is_finite() || num_tors < 0.0 {
            return Err("effective rotor count must be finite and non-negative".into());
        }
        let factor = 1.0 + self.w_rot * num_tors;
        if !factor.is_finite() || factor <= 0.0 {
            return Err("torsion normalization denominator must be finite and positive".into());
        }
        Ok(factor)
    }
}

/// Classic Vina model and official v1.2.7 XS radii.
pub const VINA_1_1: VinaParams = VinaParams {
    model_id: "vina",
    model_name: "AutoDock Vina",
    w_gauss1: -0.035579,
    w_gauss2: -0.005156,
    w_repulsion: 0.840245,
    w_hydrophobic: -0.035069,
    w_hbond: -0.587439,
    w_rot: 0.05846,
    gauss1_offset: 0.0,
    gauss1_width: 0.5,
    gauss2_offset: 3.0,
    gauss2_width: 2.0,
    hydrophobic_onset: 0.5,
    hydrophobic_offset: 1.5,
    hbond_onset: -0.7,
    hbond_offset: 0.0,
    radii: SurfaceRadii {
        c: 1.9,
        n: 1.8,
        o: 1.7,
        s: 2.0,
        p: 2.1,
        f: 1.5,
        cl: 1.8,
        br: 2.0,
        i: 2.2,
        si: 2.2,
        at: 2.3,
        metal: 1.2,
    },
};

/// Vinardo model and official v1.2.7 XS radii.
pub const VINARDO: VinaParams = VinaParams {
    model_id: "vinardo",
    model_name: "Vinardo",
    w_gauss1: -0.045,
    w_gauss2: 0.0,
    w_repulsion: 0.8,
    w_hydrophobic: -0.035,
    w_hbond: -0.6,
    w_rot: 0.05846,
    gauss1_offset: 0.0,
    gauss1_width: 0.8,
    gauss2_offset: 0.0,
    gauss2_width: 1.0,
    hydrophobic_onset: 0.0,
    hydrophobic_offset: 2.5,
    hbond_onset: -0.6,
    hbond_offset: 0.0,
    radii: SurfaceRadii {
        c: 2.0,
        n: 1.7,
        o: 1.6,
        s: 2.0,
        p: 2.1,
        f: 1.5,
        cl: 1.8,
        br: 2.0,
        i: 2.2,
        si: 2.2,
        at: 2.3,
        metal: 1.2,
    },
};

/// Weighted per-term energy of one atom pair.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PairBreakdown {
    pub gauss1: f64,
    pub gauss2: f64,
    pub repulsion: f64,
    pub hydrophobic: f64,
    pub hbond: f64,
}

impl PairBreakdown {
    pub fn total(&self) -> f64 {
        self.gauss1 + self.gauss2 + self.repulsion + self.hydrophobic + self.hbond
    }

    fn absorb(&mut self, other: PairBreakdown) {
        self.gauss1 += other.gauss1;
        self.gauss2 += other.gauss2;
        self.repulsion += other.repulsion;
        self.hydrophobic += other.hydrophobic;
        self.hbond += other.hbond;
    }

    pub(crate) fn require_finite(self, context: &str) -> Result<PairBreakdown, String> {
        for (name, value) in [
            ("gauss1", self.gauss1),
            ("gauss2", self.gauss2),
            ("repulsion", self.repulsion),
            ("hydrophobic", self.hydrophobic),
            ("hydrogen-bond", self.hbond),
            ("total", self.total()),
        ] {
            if !value.is_finite() {
                return Err(format!("{context} {name} must be finite"));
            }
        }
        Ok(self)
    }
}

/// Evaluate one ordinary XS atom pair at center-to-center `distance`.
///
/// Invalid parameters, distances, or non-finite outputs return an error. Absent
/// XS identities, explicit H, glue, water, and pairs at or beyond the cutoff
/// score zero rather than gaining fallback chemistry.
pub fn pair_energy(
    first: &DockAtom,
    second: &DockAtom,
    distance: f64,
    params: &VinaParams,
) -> Result<PairBreakdown, String> {
    params.validate()?;
    first.validate_scoring_identity("first pair atom")?;
    second.validate_scoring_identity("second pair atom")?;
    if !distance.is_finite() || distance < 0.0 {
        return Err("pair distance must be finite and non-negative".into());
    }
    pair_energy_unchecked(first, second, distance, params).require_finite("pair energy")
}

fn pair_energy_unchecked(
    first: &DockAtom,
    second: &DockAtom,
    distance: f64,
    params: &VinaParams,
) -> PairBreakdown {
    if distance >= INTER_CUTOFF {
        return PairBreakdown::default();
    }
    let (Some(first_type), Some(second_type)) = (first.xs_type, second.xs_type) else {
        return PairBreakdown::default();
    };
    let (Some(first_radius), Some(second_radius)) =
        (params.radii.of(first_type), params.radii.of(second_type))
    else {
        return PairBreakdown::default();
    };

    let surface = distance - first_radius - second_radius;
    let gauss1 =
        params.w_gauss1 * (-((surface - params.gauss1_offset) / params.gauss1_width).powi(2)).exp();
    let gauss2 =
        params.w_gauss2 * (-((surface - params.gauss2_offset) / params.gauss2_width).powi(2)).exp();
    let repulsion = if surface < 0.0 {
        params.w_repulsion * surface * surface
    } else {
        0.0
    };
    let hydrophobic = if first_type.is_hydrophobic() && second_type.is_hydrophobic() {
        params.w_hydrophobic
            * linear_ramp(surface, params.hydrophobic_onset, params.hydrophobic_offset)
    } else {
        0.0
    };
    let donor_acceptor = (first_type.is_donor() && second_type.is_acceptor())
        || (second_type.is_donor() && first_type.is_acceptor());
    let hbond = if donor_acceptor {
        params.w_hbond * linear_ramp(surface, params.hbond_onset, params.hbond_offset)
    } else {
        0.0
    };
    PairBreakdown {
        gauss1,
        gauss2,
        repulsion,
        hydrophobic,
        hbond,
    }
}

/// AutoDock Vina's factor-32 fast lookup over squared center distance.
///
/// The reference implementation samples the analytic potential at
/// `sqrt(index / 32)` and stores the average of adjacent samples. Search and
/// score-only evaluations consume this fast table; the public [`pair_energy`]
/// remains the analytic-vector reference.
fn pair_energy_fast_r2(
    first: &DockAtom,
    second: &DockAtom,
    distance_squared: f64,
    params: &VinaParams,
) -> PairBreakdown {
    const FACTOR: f64 = 32.0;
    if !distance_squared.is_finite()
        || distance_squared < 0.0
        || distance_squared >= INTER_CUTOFF * INTER_CUTOFF
    {
        return PairBreakdown::default();
    }
    let index = (FACTOR * distance_squared).floor();
    let first_distance = (index / FACTOR).sqrt();
    let second_distance = ((index + 1.0) / FACTOR).sqrt();
    average_breakdown(
        pair_energy_unchecked(first, second, first_distance, params),
        pair_energy_unchecked(first, second, second_distance, params),
    )
}

fn average_breakdown(first: PairBreakdown, second: PairBreakdown) -> PairBreakdown {
    PairBreakdown {
        gauss1: (first.gauss1 + second.gauss1) / 2.0,
        gauss2: (first.gauss2 + second.gauss2) / 2.0,
        repulsion: (first.repulsion + second.repulsion) / 2.0,
        hydrophobic: (first.hydrophobic + second.hydrophobic) / 2.0,
        hbond: (first.hbond + second.hbond) / 2.0,
    }
}

fn soft_curl(mut terms: PairBreakdown, cap: f64) -> Result<PairBreakdown, String> {
    terms = terms.require_finite("pre-curl energy")?;
    if !cap.is_finite() || cap <= 0.0 {
        return Err("soft-curl cap must be finite and positive".into());
    }
    let energy = terms.total();
    if energy > 0.0 {
        let scale = cap / (cap + energy);
        terms.gauss1 *= scale;
        terms.gauss2 *= scale;
        terms.repulsion *= scale;
        terms.hydrophobic *= scale;
        terms.hbond *= scale;
    }
    terms.require_finite("post-curl energy")
}

/// Sum intermolecular pair energies between receptor and ligand positions.
pub fn evaluate_inter(
    receptor: &PreparedReceptor,
    ligand: &PreparedLigand,
    ligand_positions: &[[f64; 3]],
    params: &VinaParams,
) -> Result<PairBreakdown, String> {
    params.validate()?;
    validate_receptor(receptor)?;
    ligand.validate_scored_mapping()?;
    validate_positions(ligand, ligand_positions)?;
    let mut total = PairBreakdown::default();
    for (ligand_atom, position) in ligand.atoms.iter().zip(ligand_positions) {
        let mut atom_terms = PairBreakdown::default();
        for receptor_atom in &receptor.atoms {
            atom_terms.absorb(pair_energy_fast_r2(
                receptor_atom,
                ligand_atom,
                dist_sq(receptor_atom.position, *position)?,
                params,
            ));
        }
        total.absorb(soft_curl(atom_terms, 1000.0)?);
    }
    total.require_finite("intermolecular energy")
}

/// Sum variable-distance intramolecular pair energies of the ligand.
pub fn evaluate_intra(
    ligand: &PreparedLigand,
    ligand_positions: &[[f64; 3]],
    params: &VinaParams,
) -> Result<PairBreakdown, String> {
    params.validate()?;
    ligand.validate_scored_mapping()?;
    validate_positions(ligand, ligand_positions)?;
    ligand.validate_pair_tables()?;
    let mut total = PairBreakdown::default();
    for i in 0..ligand.atoms.len() {
        for j in (i + 1)..ligand.atoms.len() {
            if !ligand.is_intra_eligible(i, j) {
                continue;
            }
            total.absorb(soft_curl(
                pair_energy_fast_r2(
                    &ligand.atoms[i],
                    &ligand.atoms[j],
                    dist_sq(ligand_positions[i], ligand_positions[j])?,
                    params,
                ),
                1000.0,
            )?);
        }
    }
    total.require_finite("intramolecular energy")
}

fn validate_receptor(receptor: &PreparedReceptor) -> Result<(), String> {
    if receptor.atoms.is_empty() {
        return Err("receptor has no scorable atoms".into());
    }
    if receptor.atoms.iter().any(|atom| atom.xs_type.is_none()) {
        return Err("receptor contains an atom without an XS scoring type".into());
    }
    for (index, atom) in receptor.atoms.iter().enumerate() {
        atom.validate_scoring_identity(&format!("receptor atom {index}"))?;
    }
    if receptor
        .atoms
        .iter()
        .flat_map(|atom| atom.position)
        .any(|coordinate| !coordinate.is_finite())
    {
        return Err("receptor contains a non-finite coordinate".into());
    }
    Ok(())
}

fn validate_positions(
    ligand: &PreparedLigand,
    ligand_positions: &[[f64; 3]],
) -> Result<(), String> {
    if ligand.atoms.is_empty() {
        return Err("ligand has no scorable atoms".into());
    }
    if ligand.atoms.iter().any(|atom| atom.xs_type.is_none()) {
        return Err("ligand contains an atom without an XS scoring type".into());
    }
    for (index, atom) in ligand.atoms.iter().enumerate() {
        atom.validate_scoring_identity(&format!("ligand atom {index}"))?;
    }
    if ligand_positions.len() != ligand.atoms.len() {
        return Err(format!(
            "ligand position count {} does not match scored atom count {}",
            ligand_positions.len(),
            ligand.atoms.len()
        ));
    }
    if ligand_positions
        .iter()
        .flatten()
        .any(|coordinate| !coordinate.is_finite())
    {
        return Err("ligand positions contain a non-finite coordinate".into());
    }
    Ok(())
}

/// 1 below `onset`, 0 at and beyond `offset`, linear between.
fn linear_ramp(x: f64, onset: f64, offset: f64) -> f64 {
    if x <= onset {
        1.0
    } else if x >= offset {
        0.0
    } else {
        (offset - x) / (offset - onset)
    }
}

fn require_finite(name: &str, value: f64) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{name} must be finite"))
    }
}

fn dist_sq(a: [f64; 3], b: [f64; 3]) -> Result<f64, String> {
    let delta = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    if delta.iter().any(|value| !value.is_finite()) {
        return Err("pair coordinate difference must be finite".into());
    }
    let squared = [
        delta[0] * delta[0],
        delta[1] * delta[1],
        delta[2] * delta[2],
    ];
    if squared.iter().any(|value| !value.is_finite()) {
        return Err("pair squared coordinate difference must be finite".into());
    }
    let distance_squared = squared[0] + squared[1] + squared[2];
    if !distance_squared.is_finite() {
        return Err("pair squared distance must be finite".into());
    }
    Ok(distance_squared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docking::prep::atom_type::{AdType, Element};
    use crate::docking::prep::{prepare_ligand_pdbqt, LigandBranch, PreparedLigand, RigidFragment};

    fn atom(xs_type: Option<XsType>, position: [f64; 3]) -> DockAtom {
        let element = match xs_type {
            Some(XsType::C_H | XsType::C_P) => Element::C,
            Some(XsType::N_P | XsType::N_D | XsType::N_A | XsType::N_DA) => Element::N,
            Some(XsType::O_P | XsType::O_D | XsType::O_A | XsType::O_DA) => Element::O,
            Some(XsType::S_P) => Element::S,
            Some(XsType::P_P) => Element::P,
            Some(XsType::F_H) => Element::F,
            Some(XsType::Cl_H) => Element::Cl,
            Some(XsType::Br_H) => Element::Br,
            Some(XsType::I_H) => Element::I,
            Some(XsType::Si) => Element::Si,
            Some(XsType::At) => Element::At,
            Some(XsType::Met_D) => Element::Zn,
            Some(XsType::G0 | XsType::G1 | XsType::G2 | XsType::G3) | None => Element::Other,
        };
        DockAtom {
            source_index: 0,
            serial: 1,
            name: "X".into(),
            element,
            wire_ad_type: None,
            xs_type,
            position,
            charge: 0.0,
            residue_name: "LIG".into(),
            chain: 'A',
            residue_seq: 1,
            insertion_code: ' ',
            is_hetero: false,
        }
    }

    fn empty_ligand(mut atoms: Vec<DockAtom>) -> PreparedLigand {
        for (index, atom) in atoms.iter_mut().enumerate() {
            atom.source_index = index;
            atom.serial = index as i32 + 1;
        }
        let count = atoms.len();
        PreparedLigand {
            source_atoms: atoms.clone(),
            atoms,
            source_to_scored: (0..count).map(Some).collect(),
            scored_to_source: (0..count).collect(),
            source_bonds: Vec::new(),
            bonds: Vec::new(),
            rigid_fragments: vec![RigidFragment {
                branch: None,
                parent: None,
                source_atoms: (0..count).collect(),
            }],
            branches: Vec::<LigandBranch>::new(),
            rotatable: Vec::new(),
            num_tors: 0.0,
            torsdof: 0,
            intra_excluded: Vec::new(),
            distance_variable_pairs: Vec::new(),
        }
    }

    #[test]
    fn official_vina_and_vinardo_radii_cover_every_ordinary_xs_class() {
        let ordinary = [
            (XsType::C_H, 1.9, 2.0),
            (XsType::C_P, 1.9, 2.0),
            (XsType::N_P, 1.8, 1.7),
            (XsType::N_D, 1.8, 1.7),
            (XsType::N_A, 1.8, 1.7),
            (XsType::N_DA, 1.8, 1.7),
            (XsType::O_P, 1.7, 1.6),
            (XsType::O_D, 1.7, 1.6),
            (XsType::O_A, 1.7, 1.6),
            (XsType::O_DA, 1.7, 1.6),
            (XsType::S_P, 2.0, 2.0),
            (XsType::P_P, 2.1, 2.1),
            (XsType::F_H, 1.5, 1.5),
            (XsType::Cl_H, 1.8, 1.8),
            (XsType::Br_H, 2.0, 2.0),
            (XsType::I_H, 2.2, 2.2),
            (XsType::Si, 2.2, 2.2),
            (XsType::At, 2.3, 2.3),
            (XsType::Met_D, 1.2, 1.2),
        ];
        for (xs_type, vina, vinardo) in ordinary {
            assert_eq!(VINA_1_1.radii.of(xs_type), Some(vina));
            assert_eq!(VINARDO.radii.of(xs_type), Some(vinardo));
        }
        for xs_type in [XsType::G0, XsType::G1, XsType::G2, XsType::G3] {
            assert_eq!(VINA_1_1.radii.of(xs_type), None);
            assert_eq!(VINARDO.radii.of(xs_type), None);
        }
    }

    #[test]
    fn vina_and_vinardo_pair_vectors_match_their_parameters() {
        let carbon = atom(Some(XsType::C_H), [0.0; 3]);
        let vina = pair_energy_unchecked(&carbon, &carbon, 3.8, &VINA_1_1);
        let expected_vina = PairBreakdown {
            gauss1: -0.035579,
            gauss2: -0.005156 * (-2.25f64).exp(),
            repulsion: 0.0,
            hydrophobic: -0.035069,
            hbond: 0.0,
        };
        assert_breakdown_close(vina, expected_vina);

        let vinardo = pair_energy_unchecked(&carbon, &carbon, 4.0, &VINARDO);
        let expected_vinardo = PairBreakdown {
            gauss1: -0.045,
            gauss2: 0.0,
            repulsion: 0.0,
            hydrophobic: -0.035,
            hbond: 0.0,
        };
        assert_breakdown_close(vinardo, expected_vinardo);
    }

    #[test]
    fn halogens_are_hydrophobic_and_polar_carbon_is_not() {
        let halogen = atom(Some(XsType::Cl_H), [0.0; 3]);
        let hydrophobic_carbon = atom(Some(XsType::C_H), [0.0; 3]);
        let polar_carbon = atom(Some(XsType::C_P), [0.0; 3]);
        assert!(
            pair_energy_unchecked(&halogen, &hydrophobic_carbon, 3.7, &VINA_1_1).hydrophobic < 0.0
        );
        assert_eq!(
            pair_energy_unchecked(&halogen, &polar_carbon, 3.7, &VINA_1_1).hydrophobic,
            0.0
        );
    }

    #[test]
    fn metal_is_donor_only_and_hydrogen_bonds_to_an_acceptor() {
        let metal = atom(Some(XsType::Met_D), [0.0; 3]);
        let acceptor = atom(Some(XsType::O_A), [0.0; 3]);
        let donor = atom(Some(XsType::N_D), [0.0; 3]);
        assert!(pair_energy_unchecked(&metal, &acceptor, 2.5, &VINA_1_1).hbond < 0.0);
        assert_eq!(
            pair_energy_unchecked(&metal, &donor, 2.5, &VINA_1_1).hbond,
            0.0
        );
    }

    #[test]
    fn explicit_hydrogen_and_glue_water_are_omitted() {
        let explicit_h = DockAtom {
            element: Element::H,
            wire_ad_type: Some(AdType::Hd),
            xs_type: None,
            ..atom(None, [0.0; 3])
        };
        let carbon = atom(Some(XsType::C_H), [0.0; 3]);
        assert_eq!(
            pair_energy_unchecked(&explicit_h, &carbon, 2.0, &VINA_1_1),
            PairBreakdown::default()
        );
        for xs_type in [XsType::G0, XsType::G1, XsType::G2, XsType::G3] {
            assert_eq!(
                pair_energy_unchecked(&atom(Some(xs_type), [0.0; 3]), &carbon, 2.0, &VINA_1_1),
                PairBreakdown::default()
            );
        }
    }

    #[test]
    fn donor_and_acceptor_semantics_come_only_from_xs_type() {
        let donor = atom(Some(XsType::N_D), [0.0; 3]);
        let acceptor = atom(Some(XsType::O_A), [0.0; 3]);
        let plain_nitrogen = atom(Some(XsType::N_P), [0.0; 3]);
        assert!(pair_energy_unchecked(&donor, &acceptor, 3.0, &VINA_1_1).hbond < 0.0);
        assert_eq!(
            pair_energy_unchecked(&plain_nitrogen, &acceptor, 3.0, &VINA_1_1).hbond,
            0.0
        );
    }

    #[test]
    fn positive_energies_use_vina_soft_curl_at_the_reference_aggregation_granularity() {
        let positive = PairBreakdown {
            repulsion: 20.0,
            gauss1: -5.0,
            ..PairBreakdown::default()
        };
        let curled = soft_curl(positive, 1000.0).unwrap();
        let scale = 1000.0 / 1015.0;
        assert!((curled.repulsion - 20.0 * scale).abs() < 1e-12);
        assert!((curled.gauss1 - (-5.0 * scale)).abs() < 1e-12);
        assert_eq!(
            soft_curl(
                PairBreakdown {
                    gauss1: -1.0,
                    ..PairBreakdown::default()
                },
                1000.0
            )
            .unwrap()
            .gauss1,
            -1.0
        );

        let ligand = empty_ligand(vec![atom(Some(XsType::C_H), [0.0; 3])]);
        let receptor_atom = atom(Some(XsType::C_H), [0.0; 3]);
        let receptor = PreparedReceptor {
            atoms: vec![receptor_atom.clone(), receptor_atom],
            bonds: Vec::new(),
        };
        let pair = pair_energy_fast_r2(&receptor.atoms[0], &ligand.atoms[0], 0.0, &VINA_1_1);
        let expected = soft_curl(
            PairBreakdown {
                gauss1: pair.gauss1 * 2.0,
                gauss2: pair.gauss2 * 2.0,
                repulsion: pair.repulsion * 2.0,
                hydrophobic: pair.hydrophobic * 2.0,
                hbond: pair.hbond * 2.0,
            },
            1000.0,
        )
        .unwrap();
        let actual = evaluate_inter(&receptor, &ligand, &[[0.0; 3]], &VINA_1_1).unwrap();
        assert_breakdown_close(actual, expected);
    }

    #[test]
    fn aggregate_overflow_is_rejected_before_soft_curl() {
        let large = 0.4 * f64::MAX;
        assert!(soft_curl(
            PairBreakdown {
                gauss1: large,
                gauss2: large,
                hydrophobic: large,
                ..PairBreakdown::default()
            },
            1000.0,
        )
        .unwrap_err()
        .contains("pre-curl energy total must be finite"));

        let mut params = VINA_1_1;
        params.w_gauss1 = large;
        params.w_gauss2 = large;
        params.w_hydrophobic = large;
        params.gauss1_offset = 0.0;
        params.gauss2_offset = 0.0;

        let ligand = empty_ligand(vec![atom(Some(XsType::C_H), [3.8, 0.0, 0.0])]);
        let receptor = PreparedReceptor {
            atoms: vec![atom(Some(XsType::C_H), [0.0; 3])],
            bonds: Vec::new(),
        };
        assert!(
            pair_energy(&receptor.atoms[0], &ligand.atoms[0], 3.8, &params)
                .unwrap_err()
                .contains("pair energy total must be finite")
        );
        assert!(
            evaluate_inter(&receptor, &ligand, &[[3.8, 0.0, 0.0]], &params)
                .unwrap_err()
                .contains("pre-curl energy total must be finite")
        );

        let mut intra_ligand = empty_ligand(vec![
            atom(Some(XsType::C_H), [0.0; 3]),
            atom(Some(XsType::C_H), [3.8, 0.0, 0.0]),
        ]);
        intra_ligand.distance_variable_pairs = vec![(0, 1)];
        assert!(
            evaluate_intra(&intra_ligand, &[[0.0; 3], [3.8, 0.0, 0.0]], &params,)
                .unwrap_err()
                .contains("pre-curl energy total must be finite")
        );
    }

    #[test]
    fn fast_lookup_uses_original_squared_distance_at_bucket_boundaries() {
        let carbon = atom(Some(XsType::C_H), [0.0; 3]);
        let exact_r2 = 3.0 / 32.0;
        let exact = pair_energy_fast_r2(&carbon, &carbon, exact_r2, &VINA_1_1);
        let expected = average_breakdown(
            pair_energy_unchecked(&carbon, &carbon, (3.0_f64 / 32.0).sqrt(), &VINA_1_1),
            pair_energy_unchecked(&carbon, &carbon, (4.0_f64 / 32.0).sqrt(), &VINA_1_1),
        );
        assert_breakdown_close(exact, expected);

        let below_r2 = f64::from_bits(exact_r2.to_bits() - 1);
        let below = pair_energy_fast_r2(&carbon, &carbon, below_r2, &VINA_1_1);
        let below_expected = average_breakdown(
            pair_energy_unchecked(&carbon, &carbon, (2.0_f64 / 32.0).sqrt(), &VINA_1_1),
            pair_energy_unchecked(&carbon, &carbon, (3.0_f64 / 32.0).sqrt(), &VINA_1_1),
        );
        assert_breakdown_close(below, below_expected);

        let above_r2 = f64::from_bits(exact_r2.to_bits() + 1);
        assert_breakdown_close(
            pair_energy_fast_r2(&carbon, &carbon, above_r2, &VINA_1_1),
            expected,
        );
    }

    #[test]
    fn intra_uses_only_authoritative_variable_pairs() {
        let mut ligand = empty_ligand(vec![
            atom(Some(XsType::C_H), [0.0, 0.0, 0.0]),
            atom(Some(XsType::C_H), [3.8, 0.0, 0.0]),
            atom(Some(XsType::C_H), [0.0, 3.8, 0.0]),
        ]);
        ligand.distance_variable_pairs = vec![(0, 1)];
        ligand.intra_excluded = vec![(0, 2)];
        let positions = ligand
            .atoms
            .iter()
            .map(|atom| atom.position)
            .collect::<Vec<_>>();
        let intra = evaluate_intra(&ligand, &positions, &VINA_1_1).unwrap();
        let expected = pair_energy_fast_r2(
            &ligand.atoms[0],
            &ligand.atoms[1],
            3.8_f64.powi(2),
            &VINA_1_1,
        );
        assert_breakdown_close(intra, expected);
    }

    #[test]
    fn public_intra_evaluation_validates_pair_table_invariants() {
        let positions = [[0.0; 3], [3.8, 0.0, 0.0], [0.0, 3.8, 0.0]];
        let mut ligand = empty_ligand(vec![
            atom(Some(XsType::C_H), positions[0]),
            atom(Some(XsType::C_H), positions[1]),
            atom(Some(XsType::C_H), positions[2]),
        ]);

        for (pairs, expected) in [
            (vec![(1, 0)], "canonical"),
            (vec![(0, 2), (0, 1)], "sorted and unique"),
            (vec![(0, 1), (0, 1)], "sorted and unique"),
            (vec![(0, 3)], "out of bounds"),
        ] {
            ligand.distance_variable_pairs = pairs;
            assert!(evaluate_intra(&ligand, &positions, &VINA_1_1)
                .unwrap_err()
                .contains(expected));
        }

        ligand.distance_variable_pairs = vec![(0, 1)];
        ligand.intra_excluded = vec![(1, 0)];
        assert!(evaluate_intra(&ligand, &positions, &VINA_1_1)
            .unwrap_err()
            .contains("intra-exclusion pair"));

        ligand.intra_excluded = vec![(0, 1)];
        assert!(evaluate_intra(&ligand, &positions, &VINA_1_1)
            .unwrap_err()
            .contains("must not overlap"));
    }

    #[test]
    fn public_evaluation_rejects_invalid_models_positions_and_empty_inputs() {
        let ligand = empty_ligand(vec![atom(Some(XsType::C_H), [0.0; 3])]);
        let receptor = PreparedReceptor {
            atoms: vec![atom(Some(XsType::C_H), [0.0; 3])],
            bonds: Vec::new(),
        };
        assert!(evaluate_inter(&receptor, &ligand, &[], &VINA_1_1)
            .unwrap_err()
            .contains("position count"));
        assert!(evaluate_intra(&ligand, &[[f64::NAN, 0.0, 0.0]], &VINA_1_1)
            .unwrap_err()
            .contains("non-finite"));

        for (receptor_position, ligand_position, expected) in [
            (
                [f64::MAX, 0.0, 0.0],
                [-f64::MAX, 0.0, 0.0],
                "coordinate difference",
            ),
            (
                [f64::MAX, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                "squared coordinate difference",
            ),
            ([1.0e154, 1.0e154, 0.0], [0.0, 0.0, 0.0], "squared distance"),
        ] {
            let extreme_receptor = PreparedReceptor {
                atoms: vec![atom(Some(XsType::C_H), receptor_position)],
                bonds: Vec::new(),
            };
            assert!(
                evaluate_inter(&extreme_receptor, &ligand, &[ligand_position], &VINA_1_1)
                    .unwrap_err()
                    .contains(expected)
            );
        }

        let mut extreme_ligand = empty_ligand(vec![
            atom(Some(XsType::C_H), [0.0; 3]),
            atom(Some(XsType::C_H), [0.0; 3]),
        ]);
        extreme_ligand.distance_variable_pairs = vec![(0, 1)];
        assert!(evaluate_intra(
            &extreme_ligand,
            &[[f64::MAX, 0.0, 0.0], [-f64::MAX, 0.0, 0.0]],
            &VINA_1_1,
        )
        .unwrap_err()
        .contains("coordinate difference"));

        let empty_receptor = PreparedReceptor {
            atoms: Vec::new(),
            bonds: Vec::new(),
        };
        assert!(
            evaluate_inter(&empty_receptor, &ligand, &[[0.0; 3]], &VINA_1_1)
                .unwrap_err()
                .contains("no scorable atoms")
        );
        let ligand_without_atoms = empty_ligand(Vec::new());
        assert!(evaluate_intra(&ligand_without_atoms, &[], &VINA_1_1)
            .unwrap_err()
            .contains("no scorable atoms"));

        let mut untyped_receptor = receptor.clone();
        untyped_receptor.atoms[0].xs_type = None;
        assert!(
            evaluate_inter(&untyped_receptor, &ligand, &[[0.0; 3]], &VINA_1_1)
                .unwrap_err()
                .contains("without an XS scoring type")
        );

        let mut inconsistent_receptor = receptor.clone();
        inconsistent_receptor.atoms[0].element = Element::H;
        inconsistent_receptor.atoms[0].wire_ad_type = Some(AdType::Hd);
        inconsistent_receptor.atoms[0].xs_type = Some(XsType::C_H);
        assert!(
            evaluate_inter(&inconsistent_receptor, &ligand, &[[0.0; 3]], &VINA_1_1)
                .unwrap_err()
                .contains("explicit hydrogen")
        );
        assert!(pair_energy(
            &inconsistent_receptor.atoms[0],
            &ligand.atoms[0],
            3.8,
            &VINA_1_1,
        )
        .unwrap_err()
        .contains("explicit hydrogen"));

        let mut inconsistent_ligand = ligand.clone();
        inconsistent_ligand.atoms[0].wire_ad_type = Some(AdType::Oa);
        assert!(evaluate_intra(&inconsistent_ligand, &[[0.0; 3]], &VINA_1_1)
            .unwrap_err()
            .contains("AD type OA is incompatible with element C"));

        let mut source_disagreement = ligand.clone();
        source_disagreement.atoms[0].element = Element::O;
        source_disagreement.atoms[0].wire_ad_type = Some(AdType::Oa);
        source_disagreement.atoms[0].xs_type = Some(XsType::O_A);
        assert!(
            evaluate_inter(&receptor, &source_disagreement, &[[0.0; 3]], &VINA_1_1,)
                .unwrap_err()
                .contains("identity does not match source atom")
        );

        let mut invalid_params = VINA_1_1;
        invalid_params.gauss1_width = 0.0;
        assert!(
            evaluate_inter(&receptor, &ligand, &[[0.0; 3]], &invalid_params)
                .unwrap_err()
                .contains("gauss1 width")
        );
        assert!(evaluate_intra(&ligand, &[[0.0; 3]], &invalid_params)
            .unwrap_err()
            .contains("gauss1 width"));
        assert!(
            pair_energy(&receptor.atoms[0], &ligand.atoms[0], 0.0, &invalid_params)
                .unwrap_err()
                .contains("gauss1 width")
        );
        assert!(
            pair_energy(&receptor.atoms[0], &ligand.atoms[0], f64::NAN, &VINA_1_1,)
                .unwrap_err()
                .contains("finite and non-negative")
        );

        let mut overflowing_params = VINA_1_1;
        overflowing_params.w_repulsion = f64::MAX;
        assert!(pair_energy(
            &receptor.atoms[0],
            &ligand.atoms[0],
            0.0,
            &overflowing_params,
        )
        .unwrap_err()
        .contains("pair energy repulsion must be finite"));
        assert!(
            evaluate_inter(&receptor, &ligand, &[[0.0; 3]], &overflowing_params,)
                .unwrap_err()
                .contains("pre-curl energy repulsion must be finite")
        );

        let mut intra_ligand = empty_ligand(vec![
            atom(Some(XsType::C_H), [0.0; 3]),
            atom(Some(XsType::C_H), [0.0; 3]),
        ]);
        intra_ligand.distance_variable_pairs = vec![(0, 1)];
        assert!(
            evaluate_intra(&intra_ligand, &[[0.0; 3], [0.0; 3]], &overflowing_params,)
                .unwrap_err()
                .contains("pre-curl energy repulsion must be finite")
        );
    }

    #[test]
    fn prepared_explicit_hd_is_absent_from_scored_atoms() {
        let n = pdbqt_atom_line(1, 0.0, AdType::Na);
        let hd = pdbqt_atom_line(2, 1.0, AdType::Hd);
        let c = pdbqt_atom_line(3, -1.45, AdType::C);
        let ligand =
            prepare_ligand_pdbqt(&format!("ROOT\n{n}\n{hd}\n{c}\nENDROOT\nTORSDOF 0\n")).unwrap();
        assert_eq!(ligand.source_atoms[1].xs_type, None);
        assert_eq!(ligand.atoms.len(), 2);
        assert!(ligand.atoms.iter().all(|atom| atom.xs_type.is_some()));
    }

    fn pdbqt_atom_line(serial: i32, x: f64, ad_type: AdType) -> String {
        format!(
            "HETATM{serial:5} X{serial:<3} LIG L   1    {x:8.3}{y:8.3}{z:8.3}  1.00  0.00  {charge:8.3} {kind:<2}",
            y = 0.0,
            z = 0.0,
            charge = 0.0,
            kind = ad_type.pdbqt_symbol(),
        )
    }

    fn assert_breakdown_close(actual: PairBreakdown, expected: PairBreakdown) {
        assert!((actual.gauss1 - expected.gauss1).abs() < 1e-12);
        assert!((actual.gauss2 - expected.gauss2).abs() < 1e-12);
        assert!((actual.repulsion - expected.repulsion).abs() < 1e-12);
        assert!((actual.hydrophobic - expected.hydrophobic).abs() < 1e-12);
        assert!((actual.hbond - expected.hbond).abs() < 1e-12);
    }
}
