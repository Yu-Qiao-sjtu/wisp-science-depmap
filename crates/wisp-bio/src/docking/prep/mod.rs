//! Docking structure preparation.
//!
//! PDBQT is the authoritative ligand input because it alone carries charges,
//! exact AutoDock wire types, and a torsion tree. Plain PDB ligand preparation
//! fails closed rather than presenting geometry-derived heuristics as a
//! scientifically prepared ligand. The receptor PDB compatibility path remains
//! available for rigid receptor input.

pub mod atom_type;
pub mod bonds;
pub mod charge;
pub mod pdb;
pub mod pdbqt;

use atom_type::{AdType, Bond, Element, XsType};
use pdb::RawAtom;
use pdbqt::PdbqtLigand;
use std::collections::{HashMap, HashSet};

/// One scoring-facing atom.
///
/// `wire_ad_type` preserves source PDBQT identity without carrying scoring
/// semantics. `xs_type` is the sole donor/acceptor/hydrophobic scoring
/// identity; explicit hydrogens have `xs_type == None` and remain in
/// `source_atoms` only.
#[derive(Debug, Clone, PartialEq)]
pub struct DockAtom {
    /// Index in `PreparedLigand::source_atoms` for ligands, or in the parsed PDB
    /// atom list for receptors.
    pub source_index: usize,
    pub serial: i32,
    pub name: String,
    pub element: Element,
    /// Exact source PDBQT identity when one exists.
    pub wire_ad_type: Option<AdType>,
    pub xs_type: Option<XsType>,
    pub position: [f64; 3],
    pub charge: f64,
    pub residue_name: String,
    pub chain: char,
    pub residue_seq: i32,
    pub insertion_code: char,
    pub is_hetero: bool,
}

impl DockAtom {
    pub fn is_hydrophobic(&self) -> bool {
        self.xs_type.is_some_and(XsType::is_hydrophobic)
    }

    pub fn is_donor(&self) -> bool {
        self.xs_type.is_some_and(XsType::is_donor)
    }

    pub fn is_acceptor(&self) -> bool {
        self.xs_type.is_some_and(XsType::is_acceptor)
    }
}

/// One rigid fragment induced by ROOT or a BRANCH body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigidFragment {
    /// `None` for ROOT; otherwise the PDBQT branch index.
    pub branch: Option<usize>,
    /// Parent fragment index; ROOT has no parent.
    pub parent: Option<usize>,
    /// Source atom indices in original ordered topology.
    pub source_atoms: Vec<usize>,
}

/// One authoritative PDBQT rotor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LigandBranch {
    pub parent_serial: i32,
    pub child_serial: i32,
    pub parent_source_index: usize,
    pub child_source_index: usize,
    pub parent_fragment: usize,
    pub child_fragment: usize,
    pub parent_branch: Option<usize>,
}

/// A prepared ligand with explicit source/scored mappings and authoritative
/// PDBQT topology.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedLigand {
    /// Every parsed PDBQT atom, including explicit H and glue/water identities.
    pub source_atoms: Vec<DockAtom>,
    /// Heavy/scorable atoms only, in source order.
    pub atoms: Vec<DockAtom>,
    /// Source-index to scored-index mapping. Explicit H and non-scoring glue
    /// atoms map to `None`.
    pub source_to_scored: Vec<Option<usize>>,
    /// Scored-index to source-index mapping.
    pub scored_to_source: Vec<usize>,
    /// Perceived source-atom connectivity, augmented with authoritative branch
    /// axis edges when geometry alone does not recover them.
    pub source_bonds: Vec<Bond>,
    /// Scored-heavy-atom projection of `source_bonds`.
    pub bonds: Vec<Bond>,
    pub rigid_fragments: Vec<RigidFragment>,
    pub branches: Vec<LigandBranch>,
    /// Compatibility projection of authoritative branch axes onto scored atoms.
    pub rotatable: Vec<Bond>,
    /// Vina's effective heavy-atom rotor count derived from authoritative
    /// branch axes. Each axis endpoint contributes 0.5 when the opposite
    /// endpoint has more than one bonded heavy atom.
    pub num_tors: f64,
    /// Independent TORSDOF declared by the file.
    pub torsdof: u32,
    /// Scored atom pairs separated by at most three bonds (1-2/1-3/1-4).
    pub intra_excluded: Vec<(usize, usize)>,
    /// Non-excluded scored pairs whose distance can vary across at least one
    /// authoritative branch. Same-rigid-fragment pairs are absent.
    pub distance_variable_pairs: Vec<(usize, usize)>,
}

impl PreparedLigand {
    pub fn is_intra_excluded(&self, first: usize, second: usize) -> bool {
        contains_pair(&self.intra_excluded, first, second)
    }

    pub fn is_distance_variable_pair(&self, first: usize, second: usize) -> bool {
        contains_pair(&self.distance_variable_pairs, first, second)
    }

    /// A pair is eligible for the variable intramolecular score only when a
    /// branch can change its distance and the 1-2/1-3/1-4 exclusion does not.
    pub fn is_intra_eligible(&self, first: usize, second: usize) -> bool {
        self.is_distance_variable_pair(first, second) && !self.is_intra_excluded(first, second)
    }
}

/// A validated rigid receptor prepared from authoritative PDBQT input.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedReceptor {
    pub atoms: Vec<DockAtom>,
    pub bonds: Vec<Bond>,
}

/// Geometry-derived PDB receptor used only by explicitly approximate workflows.
/// This type cannot be passed to [`crate::docking::engine::DockingEngine`].
#[derive(Debug, Clone, PartialEq)]
pub struct ApproximatePreparedReceptor {
    pub atoms: Vec<DockAtom>,
    pub bonds: Vec<Bond>,
}

/// Plain PDB does not carry authoritative Vina atom types or charges.
pub fn prepare_receptor(_pdb_text: &str) -> Result<PreparedReceptor, String> {
    Err("plain PDB receptor preparation is not a validated Vina input: provide a prepared PDBQT, or call prepare_receptor_pdb_approximate explicitly for non-scientific geometry workflows".into())
}

/// Approximate receptor preparation retained for geometry-only workflows.
///
/// This path guesses connectivity, XS types, and charges from PDB coordinates.
/// Its output must not be presented as a validated Vina preparation.
pub fn prepare_receptor_pdb_approximate(
    pdb_text: &str,
) -> Result<ApproximatePreparedReceptor, String> {
    let raw = pdb::parse_pdb_atoms(pdb_text)?;
    if raw.is_empty() {
        return Err("receptor has no ATOM or HETATM records".into());
    }
    let elements: Vec<Element> = raw.iter().map(|atom| atom.element).collect();
    let positions: Vec<[f64; 3]> = raw.iter().map(|atom| atom.position).collect();
    if positions
        .iter()
        .flatten()
        .any(|coordinate| !coordinate.is_finite())
    {
        return Err("receptor contains non-finite coordinates".into());
    }
    let bonds = bonds::perceive_bonds(&elements, &positions);
    let aromatic = bonds::aromatic_atoms(&elements, &positions, &bonds);
    let xs_types = atom_type::assign_xs_types(&elements, &positions, &bonds, &aromatic);
    let charges = charge::gasteiger_charges(&elements, &bonds, 0.0);
    if charges.iter().any(|charge| !charge.is_finite()) {
        return Err("receptor charge preparation produced a non-finite value".into());
    }
    let atoms = raw
        .into_iter()
        .enumerate()
        .map(|(source_index, atom)| {
            dock_atom(
                source_index,
                atom,
                None,
                xs_types[source_index],
                charges[source_index],
            )
        })
        .collect();
    Ok(ApproximatePreparedReceptor { atoms, bonds })
}

/// Prepare a rigid receptor from strict PDBQT atom records.
///
/// File charges and AD identities are preserved, and XS identities are derived
/// from the explicit PDBQT chemistry. Explicit hydrogens remain outside the
/// scored atom list.
pub fn prepare_receptor_pdbqt(pdbqt_text: &str) -> Result<PreparedReceptor, String> {
    let parsed = pdbqt::parse_receptor_pdbqt(pdbqt_text)?;
    let elements: Vec<Element> = parsed.atoms.iter().map(|atom| atom.raw.element).collect();
    let positions: Vec<[f64; 3]> = parsed.atoms.iter().map(|atom| atom.raw.position).collect();
    let ad_types: Vec<AdType> = parsed.atoms.iter().map(|atom| atom.ad_type).collect();
    let source_bonds = bonds::perceive_pdbqt_bonds(&ad_types, &positions, |_, _| true);
    let xs_types = atom_type::xs_types_from_ad(&ad_types, &elements, &source_bonds)?;

    let mut source_to_scored = vec![None; parsed.atoms.len()];
    let mut atoms = Vec::new();
    for (source_index, atom) in parsed.atoms.into_iter().enumerate() {
        if atom.raw.element == Element::H || xs_types[source_index].is_none() {
            continue;
        }
        source_to_scored[source_index] = Some(atoms.len());
        atoms.push(dock_atom(
            source_index,
            atom.raw,
            Some(atom.ad_type),
            xs_types[source_index],
            atom.charge,
        ));
    }
    if atoms.is_empty() {
        return Err("rigid receptor PDBQT has no scorable heavy atoms".into());
    }
    let bonds = project_bonds(&source_bonds, &source_to_scored);
    Ok(PreparedReceptor { atoms, bonds })
}

/// Plain PDB cannot authoritatively provide ligand charge/type/topology.
pub fn prepare_ligand_pdb(_pdb_text: &str) -> Result<PreparedLigand, String> {
    Err("plain PDB ligand preparation is unsupported: provide a prepared PDBQT with explicit charges, AutoDock types, and torsion topology".into())
}

/// Prepare a ligand from a strictly validated PDBQT model.
pub fn prepare_ligand_pdbqt(pdbqt_text: &str) -> Result<PreparedLigand, String> {
    prepare_parsed_ligand(pdbqt::parse_pdbqt(pdbqt_text)?)
}

fn prepare_parsed_ligand(parsed: PdbqtLigand) -> Result<PreparedLigand, String> {
    pdbqt::validate_ligand(&parsed)?;
    let source_raw: Vec<RawAtom> = parsed.atoms.iter().map(|atom| atom.raw.clone()).collect();
    let elements: Vec<Element> = source_raw.iter().map(|atom| atom.element).collect();
    let positions: Vec<[f64; 3]> = source_raw.iter().map(|atom| atom.position).collect();
    let ad_types: Vec<AdType> = parsed.atoms.iter().map(|atom| atom.ad_type).collect();
    let charges: Vec<f64> = parsed.atoms.iter().map(|atom| atom.charge).collect();

    let serial_to_source: HashMap<i32, usize> = source_raw
        .iter()
        .enumerate()
        .map(|(index, atom)| (atom.serial, index))
        .collect();
    let mobility = source_mobility(&parsed, &serial_to_source);
    let mut source_bonds = bonds::perceive_pdbqt_bonds(&ad_types, &positions, |first, second| {
        mobility.is_non_variable(first, second)
    });
    for branch in &parsed.branches {
        let a = serial_to_source[&branch.parent_serial];
        let b = serial_to_source[&branch.child_serial];
        let axis = Bond { a, b }.canonical();
        if !source_bonds
            .iter()
            .any(|candidate| candidate.canonical() == axis)
        {
            return Err(format!(
                "BRANCH axis {} {} is not a covalent bond under the Vina PDBQT geometry rule",
                branch.parent_serial, branch.child_serial
            ));
        }
    }
    source_bonds.sort_unstable_by_key(|bond| {
        let canonical = bond.canonical();
        (canonical.a, canonical.b)
    });
    source_bonds.dedup_by_key(|bond| {
        let canonical = bond.canonical();
        (canonical.a, canonical.b)
    });

    let xs_types = atom_type::xs_types_from_ad(&ad_types, &elements, &source_bonds)?;
    let source_atoms: Vec<DockAtom> = source_raw
        .into_iter()
        .enumerate()
        .map(|(index, atom)| {
            dock_atom(
                index,
                atom,
                Some(ad_types[index]),
                xs_types[index],
                charges[index],
            )
        })
        .collect();

    let mut source_to_scored = vec![None; source_atoms.len()];
    let mut scored_to_source = Vec::new();
    let mut atoms = Vec::new();
    for atom in &source_atoms {
        if atom.element == Element::H || atom.xs_type.is_none() {
            continue;
        }
        source_to_scored[atom.source_index] = Some(atoms.len());
        scored_to_source.push(atom.source_index);
        atoms.push(atom.clone());
    }
    if atoms.is_empty() {
        return Err("ligand has no scorable heavy atoms".into());
    }

    let bonds = project_bonds(&source_bonds, &source_to_scored);
    let intra_excluded = bonds::bonded_exclusions(atoms.len(), &bonds);
    let rigid_fragments = build_rigid_fragments(&parsed);
    let branches = parsed
        .branches
        .iter()
        .enumerate()
        .filter(|(index, _)| pdbqt::branch_is_active(&parsed, *index))
        .map(|(index, branch)| {
            let parent_source_index = serial_to_source[&branch.parent_serial];
            let child_source_index = serial_to_source[&branch.child_serial];
            LigandBranch {
                parent_serial: branch.parent_serial,
                child_serial: branch.child_serial,
                parent_source_index,
                child_source_index,
                parent_fragment: branch
                    .parent
                    .map(|parent| {
                        effective_active_branch(&parsed, parent).map_or(0, |active| active + 1)
                    })
                    .unwrap_or(0),
                child_fragment: index + 1,
                parent_branch: branch
                    .parent
                    .and_then(|parent| effective_active_branch(&parsed, parent)),
            }
        })
        .collect::<Vec<_>>();
    let rotatable = branches
        .iter()
        .filter_map(|branch| {
            Some(Bond {
                a: source_to_scored[branch.parent_source_index]?,
                b: source_to_scored[branch.child_source_index]?,
            })
        })
        .collect();
    let distance_variable_pairs =
        distance_variable_pairs(atoms.len(), &scored_to_source, &mobility, &intra_excluded);
    let num_tors = vina_effective_num_tors(&branches, &source_bonds, &elements);

    Ok(PreparedLigand {
        source_atoms,
        atoms,
        source_to_scored,
        scored_to_source,
        source_bonds,
        bonds,
        rigid_fragments,
        branches,
        rotatable,
        num_tors,
        torsdof: parsed.torsdof,
        intra_excluded,
        distance_variable_pairs,
    })
}

fn dock_atom(
    source_index: usize,
    atom: RawAtom,
    wire_ad_type: Option<AdType>,
    xs_type: Option<XsType>,
    charge: f64,
) -> DockAtom {
    DockAtom {
        source_index,
        serial: atom.serial,
        name: atom.name,
        element: atom.element,
        wire_ad_type,
        xs_type,
        position: atom.position,
        charge,
        residue_name: atom.residue_name,
        chain: atom.chain,
        residue_seq: atom.residue_seq,
        insertion_code: atom.insertion_code,
        is_hetero: atom.is_hetero,
    }
}

fn project_bonds(source_bonds: &[Bond], source_to_scored: &[Option<usize>]) -> Vec<Bond> {
    let mut projected: Vec<Bond> = source_bonds
        .iter()
        .filter_map(|bond| {
            Some(
                Bond {
                    a: source_to_scored.get(bond.a).copied().flatten()?,
                    b: source_to_scored.get(bond.b).copied().flatten()?,
                }
                .canonical(),
            )
        })
        .collect();
    projected.sort_unstable_by_key(|bond| (bond.a, bond.b));
    projected.dedup();
    projected
}

fn build_rigid_fragments(parsed: &PdbqtLigand) -> Vec<RigidFragment> {
    let fragments_by_source = effective_fragments(parsed);
    let mut fragments = Vec::with_capacity(parsed.branches.len() + 1);
    fragments.push(RigidFragment {
        branch: None,
        parent: None,
        source_atoms: Vec::new(),
    });
    fragments.extend(
        parsed
            .branches
            .iter()
            .enumerate()
            .map(|(index, branch)| RigidFragment {
                branch: Some(index),
                parent: Some(branch.parent.map_or(0, |parent| parent + 1)),
                source_atoms: Vec::new(),
            }),
    );
    for (source_index, fragment) in fragments_by_source.into_iter().enumerate() {
        fragments[fragment].source_atoms.push(source_index);
    }
    fragments
}

fn effective_active_branch(parsed: &PdbqtLigand, branch: usize) -> Option<usize> {
    let mut current = Some(branch);
    while let Some(index) = current {
        if pdbqt::branch_is_active(parsed, index) {
            return Some(index);
        }
        current = parsed.branches[index].parent;
    }
    None
}

fn effective_fragments(parsed: &PdbqtLigand) -> Vec<usize> {
    let effective_branch_fragment =
        |branch: usize| effective_active_branch(parsed, branch).map_or(0, |index| index + 1);
    let child_axis_to_parent = parsed
        .branches
        .iter()
        .map(|branch| {
            let parent_fragment = branch.parent.map(effective_branch_fragment).unwrap_or(0);
            (branch.child_serial, parent_fragment)
        })
        .collect::<HashMap<_, _>>();
    parsed
        .atoms
        .iter()
        .enumerate()
        .map(|(source_index, atom)| {
            child_axis_to_parent
                .get(&atom.raw.serial)
                .copied()
                .unwrap_or_else(|| {
                    parsed.atom_branches[source_index]
                        .map(effective_branch_fragment)
                        .unwrap_or(0)
                })
        })
        .collect()
}

fn vina_effective_num_tors(
    branches: &[LigandBranch],
    source_bonds: &[Bond],
    elements: &[Element],
) -> f64 {
    let mut heavy_degree = vec![0usize; elements.len()];
    for bond in source_bonds {
        if elements[bond.a] != Element::H && elements[bond.b] != Element::H {
            heavy_degree[bond.a] += 1;
            heavy_degree[bond.b] += 1;
        }
    }
    branches
        .iter()
        .map(|branch| {
            if elements[branch.parent_source_index] == Element::H
                || elements[branch.child_source_index] == Element::H
            {
                return 0.0;
            }
            let mut contribution = 0.0;
            if elements[branch.child_source_index] != Element::H
                && heavy_degree[branch.child_source_index] > 1
            {
                contribution += 0.5;
            }
            if elements[branch.parent_source_index] != Element::H
                && heavy_degree[branch.parent_source_index] > 1
            {
                contribution += 0.5;
            }
            contribution
        })
        .sum()
}

#[derive(Debug, Clone)]
struct SourceMobility {
    count: usize,
    non_variable: HashSet<(usize, usize)>,
}

impl SourceMobility {
    fn new(count: usize) -> SourceMobility {
        SourceMobility {
            count,
            non_variable: HashSet::new(),
        }
    }

    fn mark_non_variable(&mut self, first: usize, second: usize) {
        if first == second {
            return;
        }
        debug_assert!(first < self.count && second < self.count);
        self.non_variable.insert(canonical_pair(first, second));
    }

    fn mark_group(&mut self, atoms: &[usize]) {
        for (offset, &first) in atoms.iter().enumerate() {
            for &second in &atoms[(offset + 1)..] {
                self.mark_non_variable(first, second);
            }
        }
    }

    fn is_non_variable(&self, first: usize, second: usize) -> bool {
        first == second || self.non_variable.contains(&canonical_pair(first, second))
    }
}

fn source_mobility(parsed: &PdbqtLigand, serial_to_source: &HashMap<i32, usize>) -> SourceMobility {
    let mut mobility = SourceMobility::new(parsed.atoms.len());
    let fragments = effective_fragments(parsed);
    let root = fragments
        .iter()
        .enumerate()
        .filter_map(|(index, fragment)| (*fragment == 0).then_some(index))
        .collect::<Vec<_>>();
    mobility.mark_group(&root);

    for (branch_index, branch) in parsed.branches.iter().enumerate() {
        if !pdbqt::branch_is_active(parsed, branch_index) {
            continue;
        }
        let body = fragments
            .iter()
            .enumerate()
            .filter_map(|(index, fragment)| (*fragment == branch_index + 1).then_some(index))
            .collect::<Vec<_>>();
        mobility.mark_group(&body);
        let parent_axis = serial_to_source[&branch.parent_serial];
        let child_axis = serial_to_source[&branch.child_serial];
        mobility.mark_non_variable(parent_axis, child_axis);
        for &atom in &body {
            mobility.mark_non_variable(parent_axis, atom);
            mobility.mark_non_variable(child_axis, atom);
        }
    }
    mobility
}

fn distance_variable_pairs(
    scored_count: usize,
    scored_to_source: &[usize],
    mobility: &SourceMobility,
    excluded: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    let excluded: HashSet<(usize, usize)> = excluded.iter().copied().collect();
    let mut pairs = Vec::new();
    for first in 0..scored_count {
        for second in (first + 1)..scored_count {
            if excluded.contains(&(first, second)) {
                continue;
            }
            if !mobility.is_non_variable(scored_to_source[first], scored_to_source[second]) {
                pairs.push((first, second));
            }
        }
    }
    pairs
}

fn canonical_pair(first: usize, second: usize) -> (usize, usize) {
    if first < second {
        (first, second)
    } else {
        (second, first)
    }
}

fn contains_pair(pairs: &[(usize, usize)], first: usize, second: usize) -> bool {
    if first == second {
        return false;
    }
    let pair = if first < second {
        (first, second)
    } else {
        (second, first)
    };
    pairs.binary_search(&pair).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom_line(serial: i32, x: f64, ad_type: AdType, charge: f64) -> String {
        pdbqt::format_atom_line(&pdbqt::PdbqtAtom {
            raw: RawAtom {
                serial,
                name: format!("X{serial}"),
                element: ad_type.element(),
                position: [x, 0.0, 0.0],
                residue_name: "LIG".into(),
                chain: 'L',
                residue_seq: 1,
                insertion_code: ' ',
                is_hetero: true,
            },
            charge,
            ad_type,
        })
        .unwrap()
    }

    #[test]
    fn plain_pdb_ligand_preparation_fails_closed() {
        let pdb =
            "HETATM    1  C   LIG L   1       0.000   0.000   0.000  1.00  0.00           C\n";
        let error = prepare_ligand_pdb(pdb).unwrap_err();
        assert!(error.contains("prepared PDBQT"));
    }

    #[test]
    fn source_atoms_preserve_explicit_hydrogens_but_scored_mapping_omits_them() {
        let n = atom_line(1, 0.0, AdType::Na, 0.1);
        let hd = atom_line(2, 1.0, AdType::Hd, 0.05);
        let c = atom_line(3, -1.45, AdType::C, -0.15);
        let text = format!("ROOT\n{n}\n{hd}\n{c}\nENDROOT\nTORSDOF 0\n");
        let ligand = prepare_ligand_pdbqt(&text).unwrap();
        assert_eq!(ligand.source_atoms.len(), 3);
        assert_eq!(ligand.atoms.len(), 2);
        assert_eq!(ligand.source_to_scored, vec![Some(0), None, Some(1)]);
        assert_eq!(ligand.scored_to_source, vec![0, 2]);
        assert_eq!(ligand.source_atoms[1].wire_ad_type, Some(AdType::Hd));
        assert_eq!(ligand.source_atoms[1].xs_type, None);
        assert_eq!(ligand.atoms[0].xs_type, Some(XsType::N_DA));
        assert!(ligand.atoms[0].is_donor());
        assert!(ligand.atoms[0].is_acceptor());
    }

    #[test]
    fn nonpolar_h_does_not_make_a_heavy_atom_a_donor() {
        let n = atom_line(1, 0.0, AdType::Na, 0.0);
        let h = atom_line(2, 1.0, AdType::H, 0.0);
        let text = format!("ROOT\n{n}\n{h}\nENDROOT\nTORSDOF 0\n");
        let ligand = prepare_ligand_pdbqt(&text).unwrap();
        assert_eq!(ligand.atoms[0].xs_type, Some(XsType::N_A));
        assert!(!ligand.atoms[0].is_donor());
    }

    #[test]
    fn active_branch_with_hydrogen_axis_does_not_contribute_num_tors() {
        let root = atom_line(1, 0.0, AdType::C, 0.0);
        let hydrogen_axis = atom_line(2, 1.0, AdType::H, 0.0);
        let body = atom_line(3, 2.0, AdType::C, 0.0);
        let ligand = prepare_ligand_pdbqt(&format!(
            "ROOT\n{root}\nENDROOT\nBRANCH 1 2\n{hydrogen_axis}\n{body}\nENDBRANCH 1 2\nTORSDOF 1\n"
        ))
        .unwrap();
        assert_eq!(ligand.branches.len(), 1);
        assert_eq!(ligand.num_tors, 0.0);
        assert_eq!(ligand.torsdof, 1);
    }

    #[test]
    fn branch_topology_controls_num_tors_independently_of_file_torsdof() {
        let root = atom_line(1, 0.0, AdType::C, 0.0);
        let child = atom_line(2, 1.5, AdType::C, 0.0);
        let child_body = atom_line(4, 1.5, AdType::C, 0.0);
        let grandchild = atom_line(3, 3.0, AdType::Oa, 0.0);
        let grandchild_body = atom_line(5, 4.3, AdType::C, 0.0);
        let text = format!(
            "ROOT\n{root}\nENDROOT\nBRANCH 1 2\n{child}\n{child_body}\nBRANCH 2 3\n{grandchild}\n{grandchild_body}\nENDBRANCH 2 3\nENDBRANCH 1 2\nTORSDOF 7\n"
        );
        let ligand = prepare_ligand_pdbqt(&text).unwrap();
        assert_eq!(ligand.num_tors, 2.0);
        assert_eq!(ligand.torsdof, 7);
        assert_eq!(ligand.branches.len(), 2);
        assert_eq!(ligand.rigid_fragments.len(), 3);
        assert_eq!(ligand.rigid_fragments[0].source_atoms, vec![0, 1]);
        assert_eq!(ligand.rigid_fragments[1].source_atoms, vec![2, 3]);
        assert_eq!(ligand.rigid_fragments[2].source_atoms, vec![4]);
    }

    #[test]
    fn branch_axis_is_parent_rigid_but_non_axis_body_atoms_remain_variable() {
        let root_a = atom_line(1, -4.0, AdType::C, 0.0);
        let root_b = atom_line(2, 1.5, AdType::C, 0.0);
        let child_axis = atom_line(3, 0.8, AdType::C, 0.0);
        let child_body = atom_line(4, 0.7, AdType::C, 0.0);
        let text = format!(
            "ROOT\n{root_a}\n{root_b}\nENDROOT\nBRANCH 2 3\n{child_axis}\n{child_body}\nENDBRANCH 2 3\nTORSDOF 1\n"
        );
        let ligand = prepare_ligand_pdbqt(&text).unwrap();
        assert_eq!(ligand.rigid_fragments[0].source_atoms, vec![0, 1, 2]);
        assert_eq!(ligand.rigid_fragments[1].source_atoms, vec![3]);
        assert!(!ligand.is_distance_variable_pair(0, 2));
        assert!(ligand.is_distance_variable_pair(0, 3));
    }

    #[test]
    fn intra_eligibility_requires_variable_distance_and_preserves_one_four_exclusions() {
        // ROOT: 1-4 are kept in one rigid node. BRANCH 4->5 has child-axis 5
        // in the parent node and movable body atom 6 in the child node. 1-5 is
        // fixed by the axis relation; 1-6 is variable and beyond the 1-4 graph
        // exclusion, so it is eligible.
        let atoms = (1..=6)
            .map(|serial| atom_line(serial, (serial - 1) as f64 * 1.5, AdType::C, 0.0))
            .collect::<Vec<_>>();
        let text = format!(
            "ROOT\n{}\n{}\n{}\n{}\nENDROOT\nBRANCH 4 5\n{}\n{}\nENDBRANCH 4 5\nTORSDOF 1\n",
            atoms[0], atoms[1], atoms[2], atoms[3], atoms[4], atoms[5]
        );
        let ligand = prepare_ligand_pdbqt(&text).unwrap();
        assert!(ligand.is_intra_excluded(0, 3));
        assert!(!ligand.is_distance_variable_pair(0, 3));
        assert!(!ligand.is_distance_variable_pair(0, 4));
        assert!(ligand.is_distance_variable_pair(0, 5));
        assert_eq!(
            ligand.is_intra_eligible(0, 5),
            !ligand.is_intra_excluded(0, 5)
        );
        assert_eq!(
            ligand.is_intra_eligible(1, 5),
            !ligand.is_intra_excluded(1, 5)
        );
        assert!(!ligand.is_intra_eligible(0, 3)); // same rigid node
    }

    #[test]
    fn receptor_pdb_path_is_explicitly_approximate() {
        let pdb =
            "ATOM      1  C   GLY A   1       0.000   0.000   0.000  1.00  0.00           C\n";
        assert!(prepare_receptor(pdb)
            .unwrap_err()
            .contains("not a validated Vina input"));
        let receptor = prepare_receptor_pdb_approximate(pdb).unwrap();
        assert_eq!(receptor.atoms.len(), 1);
        assert_eq!(receptor.atoms[0].wire_ad_type, None);
        assert_eq!(receptor.atoms[0].xs_type, Some(XsType::C_H));
    }

    #[test]
    fn pair_lookup_is_order_independent() {
        let ligand = PreparedLigand {
            source_atoms: Vec::new(),
            atoms: Vec::new(),
            source_to_scored: Vec::new(),
            scored_to_source: Vec::new(),
            source_bonds: Vec::new(),
            bonds: Vec::new(),
            rigid_fragments: Vec::new(),
            branches: Vec::new(),
            rotatable: Vec::new(),
            num_tors: 0.0,
            torsdof: 0,
            intra_excluded: vec![(1, 3)],
            distance_variable_pairs: vec![(0, 2)],
        };
        assert!(ligand.is_intra_excluded(3, 1));
        assert!(ligand.is_distance_variable_pair(2, 0));
    }
}
