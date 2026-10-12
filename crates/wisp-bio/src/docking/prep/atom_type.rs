//! AutoDock atom typing for the Vina family of scoring functions.
//!
//! Dismantled from the AutoDock typing conventions (Apache-2.0, with NOTICE
//! attribution in the module root): a small closed set of atom types drives
//! every pair interaction, so typing is the first scoring-critical step.
//! Nonpolar hydrogens are typed `Hc` and merged into their parent heavy atom
//! by the prep layer (united-atom model); the scoring functions never see them.

/// Chemical element as far as docking cares about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Mg,
    Mn,
    Zn,
    Ca,
    Fe,
    Other,
}

impl Element {
    /// Parse a one- or two-letter element symbol, case-insensitively.
    pub fn from_symbol(symbol: &str) -> Element {
        let upper = symbol.trim().to_ascii_uppercase();
        match upper.as_str() {
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
            "MG" => Element::Mg,
            "MN" => Element::Mn,
            "ZN" => Element::Zn,
            "CA" => Element::Ca,
            "FE" => Element::Fe,
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
            Element::Mg => "Mg",
            Element::Mn => "Mn",
            Element::Zn => "Zn",
            Element::Ca => "Ca",
            Element::Fe => "Fe",
            Element::Other => "X",
        }
    }

    pub fn is_metal(self) -> bool {
        matches!(
            self,
            Element::Mg | Element::Mn | Element::Zn | Element::Ca | Element::Fe
        )
    }
}

/// The AutoDock atom types the Vina scoring family understands.
///
/// `Unknown` covers glue pseudoatoms and untyped elements; the scoring layer
/// skips them instead of guessing an interaction profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomType {
    /// Polar hydrogen bonded to N or O: the donor half of the H-bond term.
    H,
    /// Nonpolar hydrogen; merged away by prep before scoring.
    Hc,
    /// Aliphatic carbon.
    C,
    /// Aromatic carbon.
    A,
    /// Non-acceptor nitrogen (amide, protonated aromatic, charged).
    N,
    /// Acceptor nitrogen.
    Na,
    /// Oxygen. All docking-relevant oxygens are acceptors; the type survives
    /// for round-trips with files that distinguish them.
    O,
    /// Acceptor oxygen.
    Oa,
    /// Sulfur.
    S,
    /// Acceptor sulfur (bonded to oxygen: sulfoxide, sulfone, sulfonamide).
    Sa,
    /// Phosphorus.
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
    Unknown,
}

impl AtomType {
    /// Parse a PDBQT atom-type field such as `OA`, `NA`, `H`, `CL`.
    pub fn from_pdbqt_symbol(symbol: &str) -> AtomType {
        let upper = symbol.trim().to_ascii_uppercase();
        match upper.as_str() {
            "H" | "HD" | "HS" => AtomType::H,
            "H_C" => AtomType::Hc,
            "C" => AtomType::C,
            "A" => AtomType::A,
            "N" => AtomType::N,
            "NA" => AtomType::Na,
            "O" => AtomType::O,
            "OA" => AtomType::Oa,
            "S" => AtomType::S,
            "SA" => AtomType::Sa,
            "P" => AtomType::P,
            "F" => AtomType::F,
            "CL" => AtomType::Cl,
            "BR" => AtomType::Br,
            "I" => AtomType::I,
            "MG" => AtomType::Mg,
            "MN" => AtomType::Mn,
            "ZN" => AtomType::Zn,
            "CA" => AtomType::Ca,
            "FE" => AtomType::Fe,
            _ => AtomType::Unknown,
        }
    }

    /// The symbol written into PDBQT files.
    pub fn pdbqt_symbol(self) -> &'static str {
        match self {
            AtomType::H => "H",
            AtomType::Hc => "H",
            AtomType::C => "C",
            AtomType::A => "A",
            AtomType::N => "N",
            AtomType::Na => "NA",
            AtomType::O => "O",
            AtomType::Oa => "OA",
            AtomType::S => "S",
            AtomType::Sa => "SA",
            AtomType::P => "P",
            AtomType::F => "F",
            AtomType::Cl => "CL",
            AtomType::Br => "BR",
            AtomType::I => "I",
            AtomType::Mg => "MG",
            AtomType::Mn => "MN",
            AtomType::Zn => "ZN",
            AtomType::Ca => "CA",
            AtomType::Fe => "FE",
            AtomType::Unknown => "C",
        }
    }

    /// Element backing this atom type.
    pub fn element(self) -> Element {
        match self {
            AtomType::H | AtomType::Hc => Element::H,
            AtomType::C | AtomType::A => Element::C,
            AtomType::N | AtomType::Na => Element::N,
            AtomType::O | AtomType::Oa => Element::O,
            AtomType::S | AtomType::Sa => Element::S,
            AtomType::P => Element::P,
            AtomType::F => Element::F,
            AtomType::Cl => Element::Cl,
            AtomType::Br => Element::Br,
            AtomType::I => Element::I,
            AtomType::Mg => Element::Mg,
            AtomType::Mn => Element::Mn,
            AtomType::Zn => Element::Zn,
            AtomType::Ca => Element::Ca,
            AtomType::Fe => Element::Fe,
            AtomType::Unknown => Element::Other,
        }
    }

    /// True when this atom is the donor half of the scoring H-bond term.
    pub fn is_donor_hydrogen(self) -> bool {
        self == AtomType::H
    }

    /// True when this atom is the acceptor half of the scoring H-bond term.
    pub fn is_acceptor(self) -> bool {
        matches!(
            self,
            AtomType::Na
                | AtomType::Oa
                | AtomType::Sa
                | AtomType::Mg
                | AtomType::Mn
                | AtomType::Zn
                | AtomType::Ca
                | AtomType::Fe
        )
    }
}

/// Assign AutoDock types from connectivity.
///
/// `aromatic` marks atoms in perceived aromatic rings; `bonded` gives the
/// neighbor index list for every atom. Carbonyl oxygens (O=C shorter than
/// [`CARBONYL_O_DISTANCE`]) and sulfonyl sulfurs (S bonded to O) are detected
/// here because they decide acceptor and amide typing.
pub fn assign_types(
    elements: &[Element],
    positions: &[[f64; 3]],
    bonds: &[Bond],
    aromatic: &std::collections::HashSet<usize>,
) -> Vec<AtomType> {
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); elements.len()];
    for bond in bonds {
        neighbors[bond.a].push(bond.b);
        neighbors[bond.b].push(bond.a);
    }
    let carbonyl_carbon = |index: usize| -> bool {
        // A carbon with a double-bond-length oxygen is a carbonyl carbon.
        elements[index] == Element::C
            && neighbors[index].iter().any(|&other| {
                elements[other] == Element::O
                    && distance(positions[index], positions[other]) <= CARBONYL_O_DISTANCE
            })
    };
    let types = (0..elements.len())
        .map(|index| {
            let element = elements[index];
            let bonded = &neighbors[index];
            match element {
                Element::H => {
                    let polar = bonded
                        .iter()
                        .any(|&other| matches!(elements[other], Element::N | Element::O));
                    if polar {
                        AtomType::H
                    } else {
                        AtomType::Hc
                    }
                }
                Element::C => {
                    if aromatic.contains(&index) {
                        AtomType::A
                    } else {
                        AtomType::C
                    }
                }
                Element::N => {
                    if aromatic.contains(&index) {
                        // Aromatic N accepts unless it carries a hydrogen.
                        let has_h = bonded.iter().any(|&other| elements[other] == Element::H);
                        if has_h {
                            AtomType::N
                        } else {
                            AtomType::Na
                        }
                    } else if bonded.iter().any(|&other| carbonyl_carbon(other)) {
                        // Amide nitrogen: lone pair delocalized, not an acceptor.
                        AtomType::N
                    } else if bonded.len() >= 4 {
                        // Quaternary or otherwise saturated: not an acceptor.
                        AtomType::N
                    } else {
                        AtomType::Na
                    }
                }
                Element::O => {
                    if bonded.iter().any(|&other| elements[other] == Element::P) {
                        // Phosphate oxygen: kept as plain O for parity with
                        // files typed by the classic preparation scripts.
                        AtomType::O
                    } else {
                        AtomType::Oa
                    }
                }
                Element::S => {
                    let oxygen_neighbor = bonded.iter().any(|&other| elements[other] == Element::O);
                    if oxygen_neighbor {
                        AtomType::Sa
                    } else {
                        AtomType::S
                    }
                }
                Element::P => AtomType::P,
                Element::F => AtomType::F,
                Element::Cl => AtomType::Cl,
                Element::Br => AtomType::Br,
                Element::I => AtomType::I,
                Element::Mg => AtomType::Mg,
                Element::Mn => AtomType::Mn,
                Element::Zn => AtomType::Zn,
                Element::Ca => AtomType::Ca,
                Element::Fe => AtomType::Fe,
                Element::B | Element::Si | Element::Other => AtomType::Unknown,
            }
        })
        .collect();
    types
}

/// C=O bond length cutoff separating a carbonyl from a single-bonded O.
pub const CARBONYL_O_DISTANCE: f64 = 1.30;

/// A bond between two atom indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bond {
    pub a: usize,
    pub b: usize,
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn bond(a: usize, b: usize) -> Bond {
        Bond { a, b }
    }

    #[test]
    fn pdbqt_symbols_round_trip_for_the_scoring_types() {
        for expected in [
            AtomType::H,
            AtomType::C,
            AtomType::A,
            AtomType::N,
            AtomType::Na,
            AtomType::O,
            AtomType::Oa,
            AtomType::S,
            AtomType::Sa,
            AtomType::P,
            AtomType::F,
            AtomType::Cl,
            AtomType::Br,
            AtomType::I,
            AtomType::Mg,
            AtomType::Mn,
            AtomType::Zn,
            AtomType::Ca,
            AtomType::Fe,
        ] {
            assert_eq!(
                AtomType::from_pdbqt_symbol(expected.pdbqt_symbol()),
                expected
            );
        }
        // Donor variants seen in the wild collapse onto the scoring type.
        assert_eq!(AtomType::from_pdbqt_symbol("HD"), AtomType::H);
        assert_eq!(AtomType::from_pdbqt_symbol("oa"), AtomType::Oa);
        assert_eq!(AtomType::from_pdbqt_symbol("G0"), AtomType::Unknown);
    }

    #[test]
    fn hydrogens_split_into_polar_and_nonpolar() {
        // A hydroxyl group: O at origin, H at 0.96, plus a methyl H far away.
        let elements = vec![Element::O, Element::H, Element::C, Element::H];
        let positions = [
            [0.0, 0.0, 0.0],
            [0.0, 0.96, 0.0],
            [1.4, -0.4, 0.0],
            [2.4, -0.4, 0.0],
        ];
        let bonds = vec![bond(0, 1), bond(0, 2), bond(2, 3)];
        let types = assign_types(&elements, &positions, &bonds, &HashSet::new());
        assert_eq!(types[0], AtomType::Oa);
        assert_eq!(types[1], AtomType::H);
        assert_eq!(types[2], AtomType::C);
        assert_eq!(types[3], AtomType::Hc);
    }

    #[test]
    fn pyridine_nitrogen_accepts_and_protonated_ring_nitrogen_does_not() {
        let elements = vec![Element::N, Element::H, Element::C];
        let positions = [[0.0, 0.0, 0.0], [0.0, 1.01, 0.0], [1.33, -0.1, 0.0]];
        let bonds = vec![bond(0, 1), bond(0, 2)];
        let mut ring = HashSet::new();
        ring.insert(0);
        let types = assign_types(&elements, &positions, &bonds, &ring);
        assert_eq!(types[0], AtomType::N);
        // Without the hydrogen the same ring nitrogen is an acceptor.
        let bonds = vec![bond(0, 2)];
        let types = assign_types(&elements, &positions, &bonds, &ring);
        assert_eq!(types[0], AtomType::Na);
    }

    #[test]
    fn amide_nitrogen_is_not_an_acceptor_but_amine_nitrogen_is() {
        // Formamide N-C(=O): carbonyl oxygen 1.23 A from the carbon.
        let elements = vec![Element::N, Element::C, Element::O, Element::C];
        let positions = [
            [0.0, 0.0, 0.0],
            [1.35, 0.2, 0.0],
            [1.75, 1.35, 0.0],
            [-1.45, -0.2, 0.0],
        ];
        let bonds = vec![bond(0, 1), bond(1, 2), bond(0, 3)];
        let types = assign_types(&elements, &positions, &bonds, &HashSet::new());
        assert_eq!(types[0], AtomType::N);
        assert_eq!(types[1], AtomType::C);
        assert_eq!(types[2], AtomType::Oa);
        // Trim the oxygen to single-bond length and the nitrogen starts accepting.
        let positions = [
            [0.0, 0.0, 0.0],
            [1.35, 0.2, 0.0],
            [2.0, 1.8, 0.0],
            [-1.45, -0.2, 0.0],
        ];
        let types = assign_types(&elements, &positions, &bonds, &HashSet::new());
        assert_eq!(types[0], AtomType::Na);
    }

    #[test]
    fn sulfonyl_sulfur_and_metals_type_by_element() {
        let elements = vec![Element::S, Element::O, Element::Zn];
        let positions = [[0.0, 0.0, 0.0], [1.44, 0.0, 0.0], [4.0, 0.0, 0.0]];
        let bonds = vec![bond(0, 1)];
        let types = assign_types(&elements, &positions, &bonds, &HashSet::new());
        assert_eq!(types[0], AtomType::Sa);
        assert_eq!(types[2], AtomType::Zn);
        assert!(types[2].is_acceptor());
        assert!(AtomType::H.is_donor_hydrogen());
    }
}
