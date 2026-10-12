//! Docking structure preparation.
//!
//! Two pipelines feed the scoring layer:
//!
//! * **PDB path** — parse records, perceive bonds geometrically, detect
//!   aromatic rings, assign AutoDock atom types, compute Gasteiger charges,
//!   then merge nonpolar hydrogens into their parent heavy atoms.
//! * **PDBQT path** — trust the file's atom types and charges, perceive only
//!   the connectivity needed for intramolecular exclusions and the torsion
//!   count.
//!
//! Both paths converge on the same prepared types, so the scoring functions
//! and later the search treat input format as an implementation detail.

pub mod atom_type;
pub mod bonds;
pub mod charge;
pub mod pdb;
pub mod pdbqt;

use atom_type::{AtomType, Bond, Element};
use pdb::RawAtom;

/// A scoring-ready atom: the parsed record plus everything the Vina-family
/// functions need. `polar_carbon` marks C/A atoms bonded to heteroatoms;
/// they drop out of hydrophobic pair interactions.
#[derive(Debug, Clone, PartialEq)]
pub struct DockAtom {
    pub serial: i32,
    pub name: String,
    pub element: Element,
    pub ad_type: AtomType,
    pub polar_carbon: bool,
    pub position: [f64; 3],
    pub charge: f64,
    pub residue_name: String,
    pub chain: char,
    pub residue_seq: i32,
    pub insertion_code: char,
    pub is_hetero: bool,
}

impl DockAtom {
    /// True when this atom participates in hydrophobic pair scoring.
    pub fn is_hydrophobic(&self) -> bool {
        matches!(self.ad_type, AtomType::C | AtomType::A) && !self.polar_carbon
    }
}

/// A prepared ligand: united-atom model with connectivity bookkeeping.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedLigand {
    pub atoms: Vec<DockAtom>,
    pub bonds: Vec<Bond>,
    pub rotatable: Vec<Bond>,
    pub torsdof: u32,
    /// Atom pairs within two bonds; excluded from the intramolecular score.
    pub intra_excluded: Vec<(usize, usize)>,
}

/// A prepared receptor: rigid for the classic Vina workflow.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedReceptor {
    pub atoms: Vec<DockAtom>,
    pub bonds: Vec<Bond>,
}

/// Prepare a receptor from PDB text (typical protein side of a docking run).
pub fn prepare_receptor(pdb_text: &str) -> Result<PreparedReceptor, String> {
    let raw = pdb::parse_pdb_atoms(pdb_text)?;
    if raw.is_empty() {
        return Err("receptor has no ATOM or HETATM records".into());
    }
    let (atoms, bonds) = from_raw_atoms(raw)?;
    Ok(PreparedReceptor { atoms, bonds })
}

/// Prepare a ligand from PDB text, perceiving types and charges.
pub fn prepare_ligand_pdb(pdb_text: &str) -> Result<PreparedLigand, String> {
    let raw = pdb::parse_pdb_atoms(pdb_text)?;
    if raw.is_empty() {
        return Err("ligand has no ATOM or HETATM records".into());
    }
    finish_ligand(raw, None)
}

/// Prepare a ligand from PDBQT text, trusting the file's typing.
pub fn prepare_ligand_pdbqt(pdbqt_text: &str) -> Result<PreparedLigand, String> {
    let parsed = pdbqt::parse_pdbqt(pdbqt_text)?;
    if parsed.atoms.is_empty() {
        return Err("ligand PDBQT has no ATOM or HETATM records".into());
    }
    let raw: Vec<RawAtom> = parsed.atoms.iter().map(|atom| atom.raw.clone()).collect();
    let override_types: Vec<Option<(AtomType, f64)>> = parsed
        .atoms
        .iter()
        .map(|atom| Some((atom.ad_type, atom.charge)))
        .collect();
    let torsdof = parsed.torsdof;
    finish_ligand(raw, Some((override_types, torsdof)))
}

/// Type, charge, and merge a parsed atom list into docking atoms.
fn from_raw_atoms(raw: Vec<RawAtom>) -> Result<(Vec<DockAtom>, Vec<Bond>), String> {
    let elements: Vec<Element> = raw.iter().map(|atom| atom.element).collect();
    let positions: Vec<[f64; 3]> = raw.iter().map(|atom| atom.position).collect();
    let bonds = bonds::perceive_bonds(&elements, &positions);
    let aromatic = bonds::aromatic_atoms(&elements, &positions, &bonds);
    let types = atom_type::assign_types(&elements, &positions, &bonds, &aromatic);
    let charges = charge::gasteiger_charges(&elements, &bonds, 0.0);
    let (atoms, remap) = merge_nonpolar_hydrogens(raw, &types, &charges, &bonds);
    let remaining_bonds = reindex_bonds(&bonds, &remap);
    Ok((atoms, remaining_bonds))
}

fn finish_ligand(
    raw: Vec<RawAtom>,
    overrides: Option<(Vec<Option<(AtomType, f64)>>, u32)>,
) -> Result<PreparedLigand, String> {
    let (elements, positions): (Vec<Element>, Vec<[f64; 3]>) =
        raw.iter().map(|atom| (atom.element, atom.position)).unzip();
    let bonds = bonds::perceive_bonds(&elements, &positions);
    let aromatic = bonds::aromatic_atoms(&elements, &positions, &bonds);
    let has_overrides = overrides.is_some();
    let (types, charges, torsdof) = match overrides {
        Some((file_types, file_torsdof)) => {
            let mut types = atom_type::assign_types(&elements, &positions, &bonds, &aromatic);
            let mut charges = charge::gasteiger_charges(&elements, &bonds, 0.0);
            for (index, slot) in file_types.iter().enumerate() {
                if let Some((ad_type, charge)) = slot {
                    types[index] = *ad_type;
                    charges[index] = *charge;
                }
            }
            (types, charges, file_torsdof)
        }
        None => {
            let types = atom_type::assign_types(&elements, &positions, &bonds, &aromatic);
            let charges = charge::gasteiger_charges(&elements, &bonds, 0.0);
            let rotatable = bonds::rotatable_bonds(&elements, &positions, &bonds, &aromatic);
            (types, charges, rotatable.len() as u32)
        }
    };
    let (atoms, remap) = merge_nonpolar_hydrogens(raw, &types, &charges, &bonds);
    let remaining_bonds = reindex_bonds(&bonds, &remap);
    let remaining_elements: Vec<Element> = atoms.iter().map(|atom| atom.element).collect();
    let remaining_positions: Vec<[f64; 3]> = atoms.iter().map(|atom| atom.position).collect();
    let remaining_aromatic: std::collections::HashSet<usize> = atoms
        .iter()
        .enumerate()
        .filter(|(_, atom)| matches!(atom.ad_type, AtomType::A))
        .map(|(index, _)| index)
        .collect();
    let rotatable = bonds::rotatable_bonds(
        &remaining_elements,
        &remaining_positions,
        &remaining_bonds,
        &remaining_aromatic,
    );
    let torsdof = if has_overrides {
        torsdof
    } else {
        rotatable.len() as u32
    };
    let intra_excluded = bonds::bonded_exclusions(atoms.len(), &remaining_bonds);
    Ok(PreparedLigand {
        atoms,
        bonds: remaining_bonds,
        rotatable,
        torsdof,
        intra_excluded,
    })
}

/// Apply the united-atom model: fold every nonpolar hydrogen into its parent.
///
/// The hydrogen's charge moves to the parent so the molecule stays neutral;
/// its position disappears from the scored set entirely. Returns the merged
/// atoms plus the old-to-new index map used to rebuild bonds.
fn merge_nonpolar_hydrogens(
    raw: Vec<RawAtom>,
    types: &[AtomType],
    charges: &[f64],
    bonds: &[Bond],
) -> (Vec<DockAtom>, Vec<Option<usize>>) {
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); raw.len()];
    for bond in bonds {
        neighbors[bond.a].push(bond.b);
        neighbors[bond.b].push(bond.a);
    }
    let mut merged_charge = charges.to_vec();
    let mut dropped = vec![false; raw.len()];
    for bond in bonds {
        let (hydrogen, parent) = if types[bond.a] == AtomType::Hc {
            (bond.a, bond.b)
        } else if types[bond.b] == AtomType::Hc {
            (bond.b, bond.a)
        } else {
            continue;
        };
        dropped[hydrogen] = true;
        merged_charge[parent] += merged_charge[hydrogen];
        merged_charge[hydrogen] = 0.0;
    }
    let hetero_neighbor: Vec<bool> = raw
        .iter()
        .map(|atom| {
            atom.element != Element::H
                && matches!(
                    atom.element,
                    Element::N
                        | Element::O
                        | Element::S
                        | Element::P
                        | Element::F
                        | Element::Cl
                        | Element::Br
                        | Element::I
                )
        })
        .collect();
    let mut remap: Vec<Option<usize>> = vec![None; raw.len()];
    let mut atoms = Vec::with_capacity(raw.len());
    for (index, atom) in raw.into_iter().enumerate() {
        if dropped[index] {
            continue;
        }
        let polar_carbon = matches!(types[index], AtomType::C | AtomType::A)
            && neighbors[index].iter().any(|&other| hetero_neighbor[other]);
        remap[index] = Some(atoms.len());
        atoms.push(DockAtom {
            serial: atom.serial,
            name: atom.name,
            element: atom.element,
            ad_type: types[index],
            polar_carbon,
            position: atom.position,
            charge: merged_charge[index],
            residue_name: atom.residue_name,
            chain: atom.chain,
            residue_seq: atom.residue_seq,
            insertion_code: atom.insertion_code,
            is_hetero: atom.is_hetero,
        });
    }
    (atoms, remap)
}

/// Remap bond endpoints onto the post-merge atom list, dropping bonds that
/// touched a merged hydrogen.
fn reindex_bonds(bonds: &[Bond], remap: &[Option<usize>]) -> Vec<Bond> {
    bonds
        .iter()
        .filter_map(|bond| {
            Some(Bond {
                a: remap.get(bond.a).copied().flatten()?,
                b: remap.get(bond.b).copied().flatten()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hexagon_pdb(element_bond: f64) -> String {
        let mut body = String::new();
        for index in 0..6 {
            let angle = index as f64 * std::f64::consts::FRAC_PI_3;
            let x = element_bond * angle.cos();
            let y = element_bond * angle.sin();
            body.push_str(&format!(
                "HETATM{serial:5}  C   BEN L   1    {x:8.3}{y:8.3}{z:8.3}  1.00  0.00           C\n",
                serial = index + 1,
                z = 0.0
            ));
        }
        body
    }

    #[test]
    fn benzene_preparation_merges_hydrogens_and_types_aromatic() {
        let mut body = hexagon_pdb(1.39);
        // One hydrogen per carbon, radially outward at 1.09 A.
        for index in 0..6 {
            let angle = index as f64 * std::f64::consts::FRAC_PI_3;
            let x = (1.39 + 1.09) * angle.cos();
            let y = (1.39 + 1.09) * angle.sin();
            body.push_str(&format!(
                "HETATM{serial:5}  H   BEN L   1    {x:8.3}{y:8.3}{z:8.3}  1.00  0.00           H\n",
                serial = index + 7,
                z = 0.0
            ));
        }
        let ligand = prepare_ligand_pdb(&body).unwrap();
        assert_eq!(ligand.atoms.len(), 6);
        assert!(ligand.atoms.iter().all(|atom| atom.ad_type == AtomType::A));
        assert_eq!(ligand.torsdof, 0);
        let charge_sum: f64 = ligand.atoms.iter().map(|atom| atom.charge).sum();
        assert!(charge_sum.abs() < 1e-9);
    }

    #[test]
    fn phenol_keeps_its_polar_hydrogen_and_marks_the_ring_polar() {
        // Benzene ring plus an OH: the C bonded to O is a polar carbon.
        let mut body = hexagon_pdb(1.39);
        let attach_angle = 0.0_f64;
        let ox = (1.39 + 1.36) * attach_angle.cos();
        let oy = (1.39 + 1.36) * attach_angle.sin();
        let hx = (1.39 + 1.36 + 0.96) * attach_angle.cos();
        let hy = (1.39 + 1.36 + 0.96) * attach_angle.sin();
        body.push_str(&format!(
            "HETATM    7  O   BEN L   1    {ox:8.3}{oy:8.3}{z:8.3}  1.00  0.00           O\nHETATM    8  H   BEN L   1    {hx:8.3}{hy:8.3}{z:8.3}  1.00  0.00           H\n",
            z = 0.0
        ));
        let ligand = prepare_ligand_pdb(&body).unwrap();
        // 6 ring carbons + O + polar H = 8 atoms survive.
        assert_eq!(ligand.atoms.len(), 8);
        let attached_carbon = ligand
            .atoms
            .iter()
            .find(|atom| atom.polar_carbon)
            .expect("the carbon bonded to O is polar");
        assert_eq!(attached_carbon.ad_type, AtomType::A);
        assert!(!attached_carbon.is_hydrophobic());
        assert!(
            ligand
                .atoms
                .iter()
                .filter(|a| a.ad_type == AtomType::A)
                .count()
                == 6
        );
        assert!(ligand.atoms.iter().any(|a| a.ad_type == AtomType::Oa));
        assert!(ligand.atoms.iter().any(|a| a.ad_type == AtomType::H));
    }

    #[test]
    fn butane_counts_two_rotatable_bonds_and_excludes_neighbors() {
        // C-C-C-C chain with enough spacing to avoid 1-3 shortcuts.
        let mut body = String::new();
        let xs = [0.0, 1.54, 2.51, 3.98]; // zig-zag with a rotatable middle
        let ys = [0.0, 0.0, 0.89, 0.89];
        for index in 0..4 {
            body.push_str(&format!(
                "HETATM{serial:5}  C   BUT L   1    {x:8.3}{y:8.3}{z:8.3}  1.00  0.00           C\n",
                serial = index + 1,
                x = xs[index],
                y = ys[index],
                z = 0.0
            ));
        }
        let ligand = prepare_ligand_pdb(&body).unwrap();
        assert_eq!(ligand.atoms.len(), 4);
        // Only the middle bond joins two non-terminal carbons.
        assert_eq!(ligand.torsdof, 1);
        assert_eq!(ligand.rotatable.len(), 1);
        assert!(ligand.intra_excluded.contains(&(0, 1)));
        assert!(ligand.intra_excluded.contains(&(0, 2)));
        assert!(!ligand.intra_excluded.contains(&(0, 3)));
    }

    #[test]
    fn pdbqt_input_trusts_file_types_and_torsdof() {
        let text = "\
HETATM    1  N   LIG X   1       0.000   0.000   0.000  1.00  0.00     0.100 NA
HETATM    2  C   LIG X   1       1.470   0.000   0.000  1.00  0.00    -0.100 C
ROOT
ENDROOT
TORSDOF 7
";
        let ligand = prepare_ligand_pdbqt(text).unwrap();
        assert_eq!(ligand.torsdof, 7);
        assert_eq!(ligand.atoms[0].ad_type, AtomType::Na);
        assert!((ligand.atoms[0].charge - 0.1).abs() < 5e-4);
        assert_eq!(ligand.atoms[1].ad_type, AtomType::C);
    }

    #[test]
    fn empty_input_reports_a_clear_error() {
        assert!(prepare_receptor("END\n").is_err());
        assert!(prepare_ligand_pdb("REMARK nothing\n").is_err());
    }
}
