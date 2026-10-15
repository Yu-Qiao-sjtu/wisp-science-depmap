//! The Vina and Vinardo empirical scoring functions.
//!
//! Both functions are pairwise-additive over interatomic "surface distances"
//! d = r - Ri - Rj (Trott & Olson, J Comput Chem 2010; Quiroga & Villarreal,
//! PLOS ONE 2016, Tables 1-3, which this port follows parameter by
//! parameter):
//!
//! * gauss1  w1 * exp(-((d - o1)/s1)^2)         steric attraction
//! * gauss2  w2 * exp(-((d - o2)/s2)^2)         long-range attraction (vina only)
//! * repulsion  w3 * d^2 when d < 0             steric clash
//! * hydrophobic  linear ramp, both atoms hydrophobic
//! * hbond  linear ramp, donor heavy atom to acceptor
//! * rot  normalizes the intermolecular score by `1 + w6 * N_rotatable_bonds`
//!
//! Atom pairs beyond 8 A do not interact. Every term decomposes into the
//! [`PairBreakdown`] so results can report per-term contributions.

use crate::docking::prep::atom_type::AtomType;
use crate::docking::prep::{DockAtom, PreparedLigand, PreparedReceptor};

/// Maximum interatomic distance for any pair interaction.
pub const INTER_CUTOFF: f64 = 8.0;

/// Surface radii per atom type for one parameter set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceRadii {
    pub c: f64,
    pub a: f64,
    pub n: f64,
    pub na: f64,
    pub o: f64,
    pub oa: f64,
    pub s: f64,
    pub sa: f64,
    pub p: f64,
    pub f: f64,
    pub cl: f64,
    pub br: f64,
    pub i: f64,
    pub h: f64,
    pub metal: f64,
}

impl SurfaceRadii {
    fn of(self, ad_type: AtomType) -> f64 {
        match ad_type {
            AtomType::H => self.h,
            AtomType::Hc => self.h,
            AtomType::C => self.c,
            AtomType::A => self.a,
            AtomType::N => self.n,
            AtomType::Na => self.na,
            AtomType::O => self.o,
            AtomType::Oa => self.oa,
            AtomType::S => self.s,
            AtomType::Sa => self.sa,
            AtomType::P => self.p,
            AtomType::F => self.f,
            AtomType::Cl => self.cl,
            AtomType::Br => self.br,
            AtomType::I => self.i,
            AtomType::Mg | AtomType::Mn | AtomType::Zn | AtomType::Ca | AtomType::Fe => self.metal,
            AtomType::Unknown => self.c,
        }
    }
}

/// One parameterized scoring function: term weights, term shape parameters,
/// and surface radii.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VinaParams {
    pub w_gauss1: f64,
    pub w_gauss2: f64,
    pub w_repulsion: f64,
    pub w_hydrophobic: f64,
    pub w_hbond: f64,
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
    pub radii: SurfaceRadii,
}

/// The classic Vina weights (Trott & Olson 2010).
pub const VINA_1_1: VinaParams = VinaParams {
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
    radii: SurfaceRadii {
        c: 1.9,
        a: 1.9,
        n: 1.8,
        na: 1.8,
        o: 1.7,
        oa: 1.7,
        s: 1.9,
        sa: 1.9,
        p: 2.0,
        f: 1.1,
        cl: 1.98,
        br: 2.22,
        i: 2.35,
        h: 1.2,
        metal: 0.65,
    },
};

/// The Vinardo reparametrization (Quiroga & Villarreal 2016): one gaussian,
/// widened hydrophobic reach, and adjusted radii.
pub const VINARDO: VinaParams = VinaParams {
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
    radii: SurfaceRadii {
        c: 2.0,
        a: 1.9,
        n: 1.7,
        na: 1.7,
        o: 1.6,
        oa: 1.6,
        s: 1.9,
        sa: 1.9,
        p: 2.0,
        f: 1.1,
        cl: 1.98,
        br: 2.22,
        i: 2.35,
        h: 1.2,
        metal: 0.65,
    },
};

/// Unweighted per-term energy of one atom pair; each field is already
/// multiplied by its weight so summing fields gives the pair energy.
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

    /// Add another breakdown field-wise. Public so the accelerated
    /// evaluation paths accumulate in the same order as the direct one.
    pub fn absorb(&mut self, other: PairBreakdown) {
        self.gauss1 += other.gauss1;
        self.gauss2 += other.gauss2;
        self.repulsion += other.repulsion;
        self.hydrophobic += other.hydrophobic;
        self.hbond += other.hbond;
    }
}

/// Evaluate one atom pair at distance `distance`.
///
/// `polar_carbon` flags carbons excluded from hydrophobic interactions; the
/// hydrogen-bond term fires between a donor heavy atom and an acceptor. Hydrogen
/// atoms are omitted from every Vina pair term.
pub fn pair_energy(
    first: &DockAtom,
    second: &DockAtom,
    distance: f64,
    params: &VinaParams,
) -> PairBreakdown {
    if distance >= INTER_CUTOFF
        || first.element == crate::docking::prep::atom_type::Element::H
        || second.element == crate::docking::prep::atom_type::Element::H
        || first.ad_type == AtomType::Unknown
        || second.ad_type == AtomType::Unknown
    {
        return PairBreakdown::default();
    }
    let surface = distance - params.radii.of(first.ad_type) - params.radii.of(second.ad_type);
    let gauss1 =
        params.w_gauss1 * (-((surface - params.gauss1_offset) / params.gauss1_width).powi(2)).exp();
    let gauss2 =
        params.w_gauss2 * (-((surface - params.gauss2_offset) / params.gauss2_width).powi(2)).exp();
    let repulsion = if surface < 0.0 {
        params.w_repulsion * surface * surface
    } else {
        0.0
    };
    let hydrophobic = if first.is_hydrophobic() && second.is_hydrophobic() {
        params.w_hydrophobic
            * linear_ramp(surface, params.hydrophobic_onset, params.hydrophobic_offset)
    } else {
        0.0
    };
    let donor_acceptor = (first.hbond_donor && second.ad_type.is_acceptor())
        || (second.hbond_donor && first.ad_type.is_acceptor());
    let hbond = if donor_acceptor {
        params.w_hbond * linear_ramp(surface, params.hbond_onset, 0.0)
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

/// Sum intermolecular pair energies between receptor and ligand positions.
pub fn evaluate_inter(
    receptor: &PreparedReceptor,
    ligand: &PreparedLigand,
    ligand_positions: &[[f64; 3]],
    params: &VinaParams,
) -> PairBreakdown {
    let mut total = PairBreakdown::default();
    for receptor_atom in &receptor.atoms {
        for (index, ligand_atom) in ligand.atoms.iter().enumerate() {
            let distance = dist(receptor_atom.position, ligand_positions[index]);
            total.absorb(pair_energy(receptor_atom, ligand_atom, distance, params));
        }
    }
    total
}

/// Sum intramolecular pair energies of the ligand, skipping pairs within two
/// bonds of each other.
pub fn evaluate_intra(
    ligand: &PreparedLigand,
    ligand_positions: &[[f64; 3]],
    params: &VinaParams,
) -> PairBreakdown {
    let excluded: std::collections::HashSet<(usize, usize)> =
        ligand.intra_excluded.iter().copied().collect();
    let mut total = PairBreakdown::default();
    for i in 0..ligand.atoms.len() {
        for j in (i + 1)..ligand.atoms.len() {
            if excluded.contains(&(i, j)) {
                continue;
            }
            let distance = dist(ligand_positions[i], ligand_positions[j]);
            total.absorb(pair_energy(
                &ligand.atoms[i],
                &ligand.atoms[j],
                distance,
                params,
            ));
        }
    }
    total
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docking::prep::atom_type::Element;

    fn atom(ad_type: AtomType, position: [f64; 3]) -> DockAtom {
        DockAtom {
            serial: 1,
            name: "X".into(),
            element: Element::Other,
            ad_type,
            hbond_donor: false,
            polar_carbon: false,
            position,
            charge: 0.0,
            residue_name: "LIG".into(),
            chain: 'A',
            residue_seq: 1,
            insertion_code: ' ',
            is_hetero: false,
        }
    }

    #[test]
    fn vina_pair_values_match_the_published_parameters() {
        // Two hydrophobic carbons exactly at surface contact (d = 0):
        // r = 1.9 + 1.9 = 3.8 A.
        let pair = pair_energy(
            &atom(AtomType::C, [0.0; 3]),
            &atom(AtomType::C, [3.8, 0.0, 0.0]),
            3.8,
            &VINA_1_1,
        );
        assert!((pair.gauss1 - (-0.035579)).abs() < 1e-12); // exp(0)
        assert!((pair.gauss2 - (-0.005156 * (-2.25f64).exp())).abs() < 1e-12); // exp(-((0-3)/2)^2)
        assert_eq!(pair.repulsion, 0.0);
        assert!((pair.hydrophobic - (-0.035069)).abs() < 1e-12); // full below 0.5
        assert_eq!(pair.hbond, 0.0);
        assert_eq!(pair.total(), pair.gauss1 + pair.gauss2 + pair.hydrophobic);
    }

    #[test]
    fn overlapping_atoms_repel_quadratically() {
        // Carbons 0.4 A inside contact: d = -0.4.
        let pair = pair_energy(
            &atom(AtomType::C, [0.0; 3]),
            &atom(AtomType::C, [3.4, 0.0, 0.0]),
            3.4,
            &VINA_1_1,
        );
        assert!((pair.repulsion - 0.840245 * 0.16).abs() < 1e-12);
        assert!(pair.repulsion > 0.0);
    }

    #[test]
    fn hydrogen_bond_uses_the_donor_heavy_atom_and_omits_hydrogens() {
        let mut donor = atom(AtomType::N, [0.0; 3]);
        donor.element = Element::N;
        donor.hbond_donor = true;
        let mut acceptor = atom(AtomType::Oa, [3.0, 0.0, 0.0]);
        acceptor.element = Element::O;
        // N/O radii sum to 3.5 A, so d = -0.5 and the ramp is 5/7.
        let pair = pair_energy(&donor, &acceptor, 3.0, &VINA_1_1);
        assert!((pair.hbond - (-0.587439 * 5.0 / 7.0)).abs() < 1e-12);

        let mut hydrogen = atom(AtomType::H, [1.0, 0.0, 0.0]);
        hydrogen.element = Element::H;
        let omitted = pair_energy(&hydrogen, &acceptor, 2.0, &VINA_1_1);
        assert_eq!(omitted, PairBreakdown::default());

        donor.hbond_donor = false;
        assert_eq!(pair_energy(&donor, &acceptor, 3.0, &VINA_1_1).hbond, 0.0);
    }

    #[test]
    fn polar_carbons_do_not_score_as_hydrophobic() {
        let mut polar = atom(AtomType::C, [0.0; 3]);
        polar.polar_carbon = true;
        let pair = pair_energy(&polar, &atom(AtomType::C, [3.8, 0.0, 0.0]), 3.8, &VINA_1_1);
        assert_eq!(pair.hydrophobic, 0.0);
        let plain = pair_energy(
            &atom(AtomType::C, [0.0; 3]),
            &atom(AtomType::C, [3.8, 0.0, 0.0]),
            3.8,
            &VINA_1_1,
        );
        assert!(plain.hydrophobic < 0.0);
    }

    #[test]
    fn pairs_beyond_the_cutoff_do_not_interact() {
        let far = pair_energy(
            &atom(AtomType::C, [0.0; 3]),
            &atom(AtomType::C, [8.0, 0.0, 0.0]),
            8.0,
            &VINA_1_1,
        );
        assert_eq!(far.total(), 0.0);
        let near = pair_energy(
            &atom(AtomType::C, [0.0; 3]),
            &atom(AtomType::C, [7.99, 0.0, 0.0]),
            7.99,
            &VINA_1_1,
        );
        assert!(near.gauss2 < 0.0); // long-range gaussian still reaches
    }

    #[test]
    fn vinardo_uses_its_own_radii_and_single_gaussian() {
        // Carbons at 4.0 A: vina surface 0.2, vinardo surface 0.0.
        let vina = pair_energy(
            &atom(AtomType::C, [0.0; 3]),
            &atom(AtomType::C, [4.0, 0.0, 0.0]),
            4.0,
            &VINA_1_1,
        );
        let vinardo = pair_energy(
            &atom(AtomType::C, [0.0; 3]),
            &atom(AtomType::C, [4.0, 0.0, 0.0]),
            4.0,
            &VINARDO,
        );
        assert!((vinardo.gauss1 - (-0.045)).abs() < 1e-12);
        assert_eq!(vinardo.gauss2, 0.0);
        assert!((vinardo.hydrophobic - (-0.035)).abs() < 1e-12); // onset 0.0
                                                                 // Same geometry scores differently under the two parameter sets.
        assert!((vina.total() - vinardo.total()).abs() > 1e-6);
    }

    #[test]
    fn pair_energy_is_symmetric() {
        let one = pair_energy(
            &atom(AtomType::C, [0.0; 3]),
            &atom(AtomType::Na, [3.7, 0.0, 0.0]),
            3.7,
            &VINA_1_1,
        );
        let two = pair_energy(
            &atom(AtomType::Na, [0.0; 3]),
            &atom(AtomType::C, [3.7, 0.0, 0.0]),
            3.7,
            &VINA_1_1,
        );
        assert!((one.total() - two.total()).abs() < 1e-12);
    }
}
