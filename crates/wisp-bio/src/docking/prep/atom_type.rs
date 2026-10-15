//! AutoDock wire types and Vina-family XS scoring types.
//!
//! PDBQT's AutoDock (`AD`) token is part of the file format and must survive a
//! parse/write cycle unchanged in meaning.  It is deliberately separate from
//! the environment-dependent XS type used by Vina scoring.  In particular,
//! `H` and `HD` are distinct AD identities even though explicit hydrogens do
//! not have an XS pair-interaction type.

use std::collections::HashSet;

/// Chemical element as far as docking preparation cares about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    H,
    B,
    C,
    N,
    O,
    F,
    Si,
    P,
    S,
    Cl,
    Br,
    I,
    At,
    Mg,
    Mn,
    Zn,
    Ca,
    Fe,
    Cu,
    Na,
    K,
    Hg,
    Co,
    U,
    Cd,
    Ni,
    Other,
}

impl Element {
    /// Parse a one- or two-letter element symbol, case-insensitively.
    pub fn from_symbol(symbol: &str) -> Element {
        match symbol.trim().to_ascii_uppercase().as_str() {
            "H" => Element::H,
            "B" => Element::B,
            "C" => Element::C,
            "N" => Element::N,
            "O" => Element::O,
            "F" => Element::F,
            "SI" => Element::Si,
            "P" => Element::P,
            "S" => Element::S,
            "CL" => Element::Cl,
            "BR" => Element::Br,
            "I" => Element::I,
            "AT" => Element::At,
            "MG" => Element::Mg,
            "MN" => Element::Mn,
            "ZN" => Element::Zn,
            "CA" => Element::Ca,
            "FE" => Element::Fe,
            "CU" => Element::Cu,
            "NA" => Element::Na,
            "K" => Element::K,
            "HG" => Element::Hg,
            "CO" => Element::Co,
            "U" => Element::U,
            "CD" => Element::Cd,
            "NI" => Element::Ni,
            _ => Element::Other,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Element::H => "H",
            Element::B => "B",
            Element::C => "C",
            Element::N => "N",
            Element::O => "O",
            Element::F => "F",
            Element::Si => "Si",
            Element::P => "P",
            Element::S => "S",
            Element::Cl => "Cl",
            Element::Br => "Br",
            Element::I => "I",
            Element::At => "At",
            Element::Mg => "Mg",
            Element::Mn => "Mn",
            Element::Zn => "Zn",
            Element::Ca => "Ca",
            Element::Fe => "Fe",
            Element::Cu => "Cu",
            Element::Na => "Na",
            Element::K => "K",
            Element::Hg => "Hg",
            Element::Co => "Co",
            Element::U => "U",
            Element::Cd => "Cd",
            Element::Ni => "Ni",
            Element::Other => "X",
        }
    }

    pub fn is_metal(self) -> bool {
        matches!(
            self,
            Element::Mg
                | Element::Mn
                | Element::Zn
                | Element::Ca
                | Element::Fe
                | Element::Cu
                | Element::Na
                | Element::K
                | Element::Hg
                | Element::Co
                | Element::U
                | Element::Cd
                | Element::Ni
        )
    }
}

/// Exact AutoDock atom identity carried on the PDBQT wire.
///
/// The variants cover the classic AD4/Vina tokens, the hydrogen variants seen
/// in prepared files, and Vina's macrocycle glue/water tokens.  Parsing is
/// closed: an unrecognized token is an error rather than a carbon fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdType {
    H,
    Hd,
    C,
    A,
    N,
    Na,
    O,
    Oa,
    S,
    Sa,
    P,
    F,
    Cl,
    Br,
    I,
    Mg,
    Mn,
    Zn,
    Ca,
    Fe,
    Cu,
    Sodium,
    K,
    Hg,
    Co,
    U,
    Cd,
    Ni,
    Se,
    Si,
    At,
    G0,
    G1,
    G2,
    G3,
    Cg0,
    Cg1,
    Cg2,
    Cg3,
    W,
}

impl AdType {
    /// Parse one case-sensitive PDBQT AD token. Unknown or empty tokens fail closed.
    ///
    /// Case matters: `NA` is acceptor nitrogen, while `Na` is metallic sodium.
    pub fn from_pdbqt_symbol(symbol: &str) -> Result<AdType, String> {
        let token = symbol.trim();
        let parsed = match token {
            "H" => AdType::H,
            "HD" => AdType::Hd,
            "C" => AdType::C,
            "A" => AdType::A,
            "N" => AdType::N,
            "NA" => AdType::Na,
            "O" => AdType::O,
            "OA" => AdType::Oa,
            "S" => AdType::S,
            "SA" => AdType::Sa,
            "P" => AdType::P,
            "F" => AdType::F,
            "Cl" => AdType::Cl,
            "Br" => AdType::Br,
            "I" => AdType::I,
            "Mg" => AdType::Mg,
            "Mn" => AdType::Mn,
            "Zn" => AdType::Zn,
            "Ca" => AdType::Ca,
            "Fe" => AdType::Fe,
            "Cu" => AdType::Cu,
            "Na" => AdType::Sodium,
            "K" => AdType::K,
            "Hg" => AdType::Hg,
            "Co" => AdType::Co,
            "U" => AdType::U,
            "Cd" => AdType::Cd,
            "Ni" => AdType::Ni,
            "Se" => AdType::Se,
            "Si" => AdType::Si,
            "At" => AdType::At,
            "G0" => AdType::G0,
            "G1" => AdType::G1,
            "G2" => AdType::G2,
            "G3" => AdType::G3,
            "CG0" => AdType::Cg0,
            "CG1" => AdType::Cg1,
            "CG2" => AdType::Cg2,
            "CG3" => AdType::Cg3,
            "W" => AdType::W,
            _ => return Err(format!("unsupported AutoDock atom type `{token}`")),
        };
        Ok(parsed)
    }

    /// Canonical token written to PDBQT.
    pub fn pdbqt_symbol(self) -> &'static str {
        match self {
            AdType::H => "H",
            AdType::Hd => "HD",
            AdType::C => "C",
            AdType::A => "A",
            AdType::N => "N",
            AdType::Na => "NA",
            AdType::O => "O",
            AdType::Oa => "OA",
            AdType::S => "S",
            AdType::Sa => "SA",
            AdType::P => "P",
            AdType::F => "F",
            AdType::Cl => "Cl",
            AdType::Br => "Br",
            AdType::I => "I",
            AdType::Mg => "Mg",
            AdType::Mn => "Mn",
            AdType::Zn => "Zn",
            AdType::Ca => "Ca",
            AdType::Fe => "Fe",
            AdType::Cu => "Cu",
            AdType::Sodium => "Na",
            AdType::K => "K",
            AdType::Hg => "Hg",
            AdType::Co => "Co",
            AdType::U => "U",
            AdType::Cd => "Cd",
            AdType::Ni => "Ni",
            AdType::Se => "Se",
            AdType::Si => "Si",
            AdType::At => "At",
            AdType::G0 => "G0",
            AdType::G1 => "G1",
            AdType::G2 => "G2",
            AdType::G3 => "G3",
            AdType::Cg0 => "CG0",
            AdType::Cg1 => "CG1",
            AdType::Cg2 => "CG2",
            AdType::Cg3 => "CG3",
            AdType::W => "W",
        }
    }

    /// Element implied by this AD identity.
    pub fn element(self) -> Element {
        match self {
            AdType::H | AdType::Hd => Element::H,
            AdType::C | AdType::A | AdType::Cg0 | AdType::Cg1 | AdType::Cg2 | AdType::Cg3 => {
                Element::C
            }
            AdType::N | AdType::Na => Element::N,
            AdType::O | AdType::Oa | AdType::W => Element::O,
            AdType::S | AdType::Sa | AdType::Se => Element::S,
            AdType::P => Element::P,
            AdType::F => Element::F,
            AdType::Cl => Element::Cl,
            AdType::Br => Element::Br,
            AdType::I => Element::I,
            AdType::Mg => Element::Mg,
            AdType::Mn => Element::Mn,
            AdType::Zn => Element::Zn,
            AdType::Ca => Element::Ca,
            AdType::Fe => Element::Fe,
            AdType::Cu => Element::Cu,
            AdType::Sodium => Element::Na,
            AdType::K => Element::K,
            AdType::Hg => Element::Hg,
            AdType::Co => Element::Co,
            AdType::U => Element::U,
            AdType::Cd => Element::Cd,
            AdType::Ni => Element::Ni,
            AdType::Si => Element::Si,
            AdType::At => Element::At,
            AdType::G0 | AdType::G1 | AdType::G2 | AdType::G3 => Element::Other,
        }
    }

    pub fn is_explicit_hydrogen(self) -> bool {
        matches!(self, AdType::H | AdType::Hd)
    }

    pub fn is_metal(self) -> bool {
        matches!(
            self,
            AdType::Mg
                | AdType::Mn
                | AdType::Zn
                | AdType::Ca
                | AdType::Fe
                | AdType::Cu
                | AdType::Sodium
                | AdType::K
                | AdType::Hg
                | AdType::Co
                | AdType::U
                | AdType::Cd
                | AdType::Ni
        )
    }
}

/// Vina XS scoring identity.
///
/// Donor, acceptor, and hydrophobic behavior is encoded by this type rather
/// than inferred from the PDBQT wire token at scoring time. Explicit hydrogen
/// atoms have no XS type and are omitted from `PreparedLigand::atoms`.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XsType {
    C_H,
    C_P,
    N_P,
    N_D,
    N_A,
    N_DA,
    O_P,
    O_D,
    O_A,
    O_DA,
    S_P,
    P_P,
    F_H,
    Cl_H,
    Br_H,
    I_H,
    Si,
    At,
    Met_D,
    G0,
    G1,
    G2,
    G3,
}

impl XsType {
    pub fn is_hydrophobic(self) -> bool {
        matches!(
            self,
            XsType::C_H | XsType::F_H | XsType::Cl_H | XsType::Br_H | XsType::I_H
        )
    }

    pub fn is_donor(self) -> bool {
        matches!(
            self,
            XsType::N_D | XsType::N_DA | XsType::O_D | XsType::O_DA | XsType::Met_D
        )
    }

    pub fn is_acceptor(self) -> bool {
        matches!(
            self,
            XsType::N_A | XsType::N_DA | XsType::O_A | XsType::O_DA
        )
    }
}

/// A bond between two atom indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bond {
    pub a: usize,
    pub b: usize,
}

impl Bond {
    pub fn canonical(self) -> Bond {
        if self.a <= self.b {
            self
        } else {
            Bond {
                a: self.b,
                b: self.a,
            }
        }
    }
}

/// Assign XS types to a plain-PDB receptor from its perceived chemistry.
///
/// This remains a receptor-only compatibility path. Plain-PDB ligand
/// preparation is rejected because it cannot provide authoritative AD types,
/// charges, or a torsion tree.
pub fn assign_xs_types(
    elements: &[Element],
    positions: &[[f64; 3]],
    bonds: &[Bond],
    aromatic: &HashSet<usize>,
) -> Vec<Option<XsType>> {
    let neighbors = neighbor_lists(elements.len(), bonds);
    let carbonyl_carbon = |index: usize| -> bool {
        elements[index] == Element::C
            && neighbors[index].iter().any(|&other| {
                elements[other] == Element::O
                    && distance(positions[index], positions[other]) <= CARBONYL_O_DISTANCE
            })
    };

    (0..elements.len())
        .map(|index| {
            let bonded = &neighbors[index];
            let donor = matches!(elements[index], Element::N | Element::O)
                && bonded.iter().any(|&other| elements[other] == Element::H);
            match elements[index] {
                Element::H => None,
                Element::C => {
                    if aromatic.contains(&index) {
                        Some(XsType::C_H)
                    } else if bonded.iter().any(|&other| is_hetero(elements[other])) {
                        Some(XsType::C_P)
                    } else {
                        Some(XsType::C_H)
                    }
                }
                Element::N => {
                    let accepts = if aromatic.contains(&index) {
                        !bonded.iter().any(|&other| elements[other] == Element::H)
                    } else {
                        !bonded.iter().any(|&other| carbonyl_carbon(other)) && bonded.len() < 4
                    };
                    Some(match (donor, accepts) {
                        (false, false) => XsType::N_P,
                        (true, false) => XsType::N_D,
                        (false, true) => XsType::N_A,
                        (true, true) => XsType::N_DA,
                    })
                }
                Element::O => {
                    let accepts = !bonded.iter().any(|&other| elements[other] == Element::P);
                    Some(match (donor, accepts) {
                        (false, false) => XsType::O_P,
                        (true, false) => XsType::O_D,
                        (false, true) => XsType::O_A,
                        (true, true) => XsType::O_DA,
                    })
                }
                Element::S => Some(XsType::S_P),
                Element::P => Some(XsType::P_P),
                Element::F => Some(XsType::F_H),
                Element::Cl => Some(XsType::Cl_H),
                Element::Br => Some(XsType::Br_H),
                Element::I => Some(XsType::I_H),
                Element::Si => Some(XsType::Si),
                Element::At => Some(XsType::At),
                element if element.is_metal() => Some(XsType::Met_D),
                Element::B | Element::Other => None,
                Element::Mg
                | Element::Mn
                | Element::Zn
                | Element::Ca
                | Element::Fe
                | Element::Cu
                | Element::Na
                | Element::K
                | Element::Hg
                | Element::Co
                | Element::U
                | Element::Cd
                | Element::Ni => unreachable!("metal guard handles all docking metals"),
            }
        })
        .collect()
}

/// Convert strict PDBQT AD identities into environment-dependent XS types.
///
/// Only an `HD` atom with exactly one heavy neighbor, whose AD identity is N/O
/// compatible, can make that N/O a donor. Nonpolar `H` never confers donor
/// behavior.
pub fn xs_types_from_ad(
    ad_types: &[AdType],
    elements: &[Element],
    bonds: &[Bond],
) -> Result<Vec<Option<XsType>>, String> {
    if ad_types.len() != elements.len() {
        return Err("AD type and element arrays have different lengths".into());
    }
    let neighbors = neighbor_lists(elements.len(), bonds);
    let mut donor = vec![false; ad_types.len()];

    for (index, ad_type) in ad_types.iter().copied().enumerate() {
        if ad_type != AdType::Hd {
            continue;
        }
        let heavy_neighbors: Vec<usize> = neighbors[index]
            .iter()
            .copied()
            .filter(|&other| elements[other] != Element::H)
            .collect();
        if heavy_neighbors.len() != 1 {
            return Err(format!(
                "HD atom at source index {index} must have exactly one heavy N/O neighbor"
            ));
        }
        let parent = heavy_neighbors[0];
        if !matches!(
            ad_types[parent],
            AdType::N | AdType::Na | AdType::O | AdType::Oa
        ) {
            return Err(format!(
                "HD atom at source index {index} is attached to incompatible AD type {}",
                ad_types[parent].pdbqt_symbol()
            ));
        }
        donor[parent] = true;
    }

    ad_types
        .iter()
        .copied()
        .enumerate()
        .map(|(index, ad_type)| {
            let xs_type = match ad_type {
                AdType::H | AdType::Hd => None,
                AdType::C | AdType::A => {
                    if neighbors[index]
                        .iter()
                        .any(|&other| is_hetero(elements[other]))
                    {
                        Some(XsType::C_P)
                    } else {
                        Some(XsType::C_H)
                    }
                }
                AdType::N => Some(if donor[index] {
                    XsType::N_D
                } else {
                    XsType::N_P
                }),
                AdType::Na => Some(if donor[index] {
                    XsType::N_DA
                } else {
                    XsType::N_A
                }),
                AdType::O => Some(if donor[index] {
                    XsType::O_D
                } else {
                    XsType::O_P
                }),
                AdType::Oa => Some(if donor[index] {
                    XsType::O_DA
                } else {
                    XsType::O_A
                }),
                AdType::S | AdType::Sa | AdType::Se => Some(XsType::S_P),
                AdType::P => Some(XsType::P_P),
                AdType::F => Some(XsType::F_H),
                AdType::Cl => Some(XsType::Cl_H),
                AdType::Br => Some(XsType::Br_H),
                AdType::I => Some(XsType::I_H),
                AdType::Si => Some(XsType::Si),
                AdType::At => Some(XsType::At),
                AdType::Mg
                | AdType::Mn
                | AdType::Zn
                | AdType::Ca
                | AdType::Fe
                | AdType::Cu
                | AdType::Sodium
                | AdType::K
                | AdType::Hg
                | AdType::Co
                | AdType::U
                | AdType::Cd
                | AdType::Ni => Some(XsType::Met_D),
                AdType::G0
                | AdType::G1
                | AdType::G2
                | AdType::G3
                | AdType::Cg0
                | AdType::Cg1
                | AdType::Cg2
                | AdType::Cg3 => {
                    return Err(format!(
                        "macrocycle glue AD type {} is unsupported until the glue scoring term is implemented",
                        ad_type.pdbqt_symbol()
                    ));
                }
                AdType::W => {
                    return Err(
                        "hydrated-docking water AD type W is unsupported in the ordinary R1 scorer"
                            .into(),
                    );
                }
            };
            Ok(xs_type)
        })
        .collect()
}

fn neighbor_lists(count: usize, bonds: &[Bond]) -> Vec<Vec<usize>> {
    let mut neighbors = vec![Vec::new(); count];
    for bond in bonds {
        if bond.a < count && bond.b < count {
            neighbors[bond.a].push(bond.b);
            neighbors[bond.b].push(bond.a);
        }
    }
    neighbors
}

fn is_hetero(element: Element) -> bool {
    !matches!(element, Element::H | Element::C | Element::Other)
}

/// C=O bond length cutoff separating a carbonyl from a single-bonded O.
pub const CARBONYL_O_DISTANCE: f64 = 1.30;

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bond(a: usize, b: usize) -> Bond {
        Bond { a, b }
    }

    #[test]
    fn every_supported_ad_token_round_trips_without_collapsing_identity() {
        let types = [
            AdType::H,
            AdType::Hd,
            AdType::C,
            AdType::A,
            AdType::N,
            AdType::Na,
            AdType::O,
            AdType::Oa,
            AdType::S,
            AdType::Sa,
            AdType::P,
            AdType::F,
            AdType::Cl,
            AdType::Br,
            AdType::I,
            AdType::Mg,
            AdType::Mn,
            AdType::Zn,
            AdType::Ca,
            AdType::Fe,
            AdType::Cu,
            AdType::Sodium,
            AdType::K,
            AdType::Hg,
            AdType::Co,
            AdType::U,
            AdType::Cd,
            AdType::Ni,
            AdType::Se,
            AdType::Si,
            AdType::At,
            AdType::G0,
            AdType::G1,
            AdType::G2,
            AdType::G3,
            AdType::Cg0,
            AdType::Cg1,
            AdType::Cg2,
            AdType::Cg3,
            AdType::W,
        ];
        for expected in types {
            assert_eq!(
                AdType::from_pdbqt_symbol(expected.pdbqt_symbol()).unwrap(),
                expected
            );
        }
        assert_ne!(AdType::H, AdType::Hd);
        assert_eq!(AdType::from_pdbqt_symbol("Se").unwrap(), AdType::Se);
        assert_eq!(AdType::Se.element(), Element::S);
        assert_eq!(AdType::from_pdbqt_symbol("NA").unwrap(), AdType::Na);
        assert_eq!(AdType::from_pdbqt_symbol("Na").unwrap(), AdType::Sodium);
        assert_ne!(AdType::Na, AdType::Sodium);
        let xs = xs_types_from_ad(
            &[AdType::Na, AdType::Sodium],
            &[Element::N, Element::Na],
            &[],
        )
        .unwrap();
        assert_eq!(xs, vec![Some(XsType::N_A), Some(XsType::Met_D)]);
    }

    #[test]
    fn unknown_ad_tokens_fail_instead_of_becoming_carbon() {
        let error = AdType::from_pdbqt_symbol("XX").unwrap_err();
        assert!(error.contains("unsupported AutoDock atom type"));
        assert!(AdType::from_pdbqt_symbol("").is_err());
        assert!(AdType::from_pdbqt_symbol("na").is_err());
        assert!(AdType::from_pdbqt_symbol("HS").is_err());
        assert!(AdType::from_pdbqt_symbol("H_C").is_err());
    }

    #[test]
    fn only_legal_attached_hd_confers_donor_xs_identity() {
        let elements = [Element::N, Element::H, Element::H, Element::O];
        let ad_types = [AdType::Na, AdType::Hd, AdType::H, AdType::Oa];
        let bonds = [bond(0, 1), bond(0, 2)];
        let xs = xs_types_from_ad(&ad_types, &elements, &bonds).unwrap();
        assert_eq!(xs[0], Some(XsType::N_DA));
        assert_eq!(xs[1], None);
        assert_eq!(xs[2], None);
        assert_eq!(xs[3], Some(XsType::O_A));
        assert_eq!(xs[0], Some(XsType::N_DA));
        assert!(xs[0].unwrap().is_acceptor());
    }

    #[test]
    fn hd_attached_to_carbon_fails_closed() {
        let error = xs_types_from_ad(
            &[AdType::C, AdType::Hd],
            &[Element::C, Element::H],
            &[bond(0, 1)],
        )
        .unwrap_err();
        assert!(error.contains("incompatible AD type C"));
    }

    #[test]
    fn all_carbon_ad_types_use_hetero_neighbors_for_xs_polarity() {
        let elements = [Element::C, Element::O];
        let bonds = [bond(0, 1)];
        let aliphatic = xs_types_from_ad(&[AdType::C, AdType::Oa], &elements, &bonds).unwrap();
        let aromatic = xs_types_from_ad(&[AdType::A, AdType::Oa], &elements, &bonds).unwrap();
        assert_eq!(aliphatic[0], Some(XsType::C_P));
        assert_eq!(aromatic[0], Some(XsType::C_P));
        assert!(!aliphatic[0].unwrap().is_hydrophobic());
        assert!(!aromatic[0].unwrap().is_hydrophobic());

        let hydrocarbon = xs_types_from_ad(&[AdType::A], &[Element::C], &[]).unwrap();
        assert_eq!(hydrocarbon[0], Some(XsType::C_H));
        assert!(hydrocarbon[0].unwrap().is_hydrophobic());
    }

    #[test]
    fn unsupported_water_and_macrocycle_glue_fail_closed() {
        for ad_type in [AdType::W, AdType::G0, AdType::Cg0] {
            let error = xs_types_from_ad(&[ad_type], &[ad_type.element()], &[]).unwrap_err();
            assert!(
                error.contains("unsupported"),
                "unexpected error for {}: {error}",
                ad_type.pdbqt_symbol()
            );
        }
    }

    #[test]
    fn receptor_assignment_encodes_donor_acceptor_and_hydrophobic_behavior() {
        let elements = [Element::O, Element::H, Element::C, Element::Cl];
        let positions = [
            [0.0, 0.0, 0.0],
            [0.0, 0.96, 0.0],
            [1.4, 0.0, 0.0],
            [2.9, 0.0, 0.0],
        ];
        let bonds = [bond(0, 1), bond(0, 2), bond(2, 3)];
        let xs = assign_xs_types(&elements, &positions, &bonds, &HashSet::new());
        assert_eq!(xs[0], Some(XsType::O_DA));
        assert_eq!(xs[1], None);
        assert_eq!(xs[2], Some(XsType::C_P));
        assert_eq!(xs[3], Some(XsType::Cl_H));
    }
}
