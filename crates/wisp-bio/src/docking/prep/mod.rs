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

    pub(crate) fn validate_scoring_identity(&self, context: &str) -> Result<(), String> {
        if let Some(ad_type) = self.wire_ad_type {
            if ad_type.element() != self.element {
                return Err(format!(
                    "{context} AD type {} is incompatible with element {}",
                    ad_type.pdbqt_symbol(),
                    self.element.symbol()
                ));
            }
            if ad_type.is_explicit_hydrogen() && self.xs_type.is_some() {
                return Err(format!(
                    "{context} explicit hydrogen must not have an XS scoring type"
                ));
            }
        }
        if self.element == Element::H && self.xs_type.is_some() {
            return Err(format!(
                "{context} explicit hydrogen must not have an XS scoring type"
            ));
        }
        if let Some(xs_type) = self.xs_type {
            if !xs_type.is_compatible_with(self.element) {
                return Err(format!(
                    "{context} XS type {xs_type:?} is incompatible with element {}",
                    self.element.symbol()
                ));
            }
            if let Some(ad_type) = self.wire_ad_type {
                if !ad_type.is_compatible_with_xs(xs_type) {
                    return Err(format!(
                        "{context} AD type {} is incompatible with XS type {xs_type:?}",
                        ad_type.pdbqt_symbol()
                    ));
                }
            }
        }
        Ok(())
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
    pub(crate) source_atoms: Vec<DockAtom>,
    /// Heavy/scorable atoms only, in source order.
    pub(crate) atoms: Vec<DockAtom>,
    /// Source-index to scored-index mapping. Explicit H and non-scoring glue
    /// atoms map to `None`.
    pub(crate) source_to_scored: Vec<Option<usize>>,
    /// Scored-index to source-index mapping.
    pub(crate) scored_to_source: Vec<usize>,
    /// Perceived source-atom connectivity, augmented with authoritative branch
    /// axis edges when geometry alone does not recover them.
    pub(crate) source_bonds: Vec<Bond>,
    /// Scored-heavy-atom projection of `source_bonds`.
    pub(crate) bonds: Vec<Bond>,
    pub(crate) rigid_fragments: Vec<RigidFragment>,
    pub(crate) branches: Vec<LigandBranch>,
    /// Compatibility projection of authoritative branch axes onto scored atoms.
    pub(crate) rotatable: Vec<Bond>,
    /// Vina's effective heavy-atom rotor count derived from authoritative
    /// branch axes. Each axis endpoint contributes 0.5 when the opposite
    /// endpoint has more than one bonded heavy atom.
    pub(crate) num_tors: f64,
    /// Independent TORSDOF declared by the file.
    pub(crate) torsdof: u32,
    /// Scored atom pairs separated by at most three bonds (1-2/1-3/1-4).
    pub(crate) intra_excluded: Vec<(usize, usize)>,
    /// Non-excluded scored pairs whose distance can vary across at least one
    /// authoritative branch. Same-rigid-fragment pairs are absent.
    pub(crate) distance_variable_pairs: Vec<(usize, usize)>,
}

impl PreparedLigand {
    pub fn source_atoms(&self) -> &[DockAtom] {
        &self.source_atoms
    }

    pub fn atoms(&self) -> &[DockAtom] {
        &self.atoms
    }

    pub fn source_to_scored(&self) -> &[Option<usize>] {
        &self.source_to_scored
    }

    pub fn scored_to_source(&self) -> &[usize] {
        &self.scored_to_source
    }

    pub fn source_bonds(&self) -> &[Bond] {
        &self.source_bonds
    }

    pub fn bonds(&self) -> &[Bond] {
        &self.bonds
    }

    pub fn rigid_fragments(&self) -> &[RigidFragment] {
        &self.rigid_fragments
    }

    pub fn branches(&self) -> &[LigandBranch] {
        &self.branches
    }

    pub fn rotatable(&self) -> &[Bond] {
        &self.rotatable
    }

    pub fn num_tors(&self) -> f64 {
        self.num_tors
    }

    pub fn torsdof(&self) -> u32 {
        self.torsdof
    }

    pub fn intra_excluded(&self) -> &[(usize, usize)] {
        &self.intra_excluded
    }

    pub fn distance_variable_pairs(&self) -> &[(usize, usize)] {
        &self.distance_variable_pairs
    }

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

    pub(crate) fn validate_scored_mapping(&self) -> Result<(), String> {
        if self.source_to_scored.len() != self.source_atoms.len() {
            return Err(
                "ligand source-to-scored mapping length does not match source atoms".into(),
            );
        }
        if self.scored_to_source.len() != self.atoms.len() {
            return Err(
                "ligand scored-to-source mapping length does not match scored atoms".into(),
            );
        }

        if self
            .scored_to_source
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err("ligand scored-to-source mapping must preserve source order".into());
        }

        let mut source_serials = HashSet::new();
        let mut seen_scored = vec![false; self.atoms.len()];
        for (source_index, source_atom) in self.source_atoms.iter().enumerate() {
            if source_atom.source_index != source_index {
                return Err(format!(
                    "ligand source atom {source_index} carries mismatched source index {}",
                    source_atom.source_index
                ));
            }
            if !source_serials.insert(source_atom.serial) {
                return Err(format!(
                    "ligand has duplicate source atom serial {}",
                    source_atom.serial
                ));
            }
            source_atom.validate_scoring_identity(&format!("ligand source atom {source_index}"))?;
            match self.source_to_scored[source_index] {
                Some(scored_index) => {
                    let scored_atom = self.atoms.get(scored_index).ok_or_else(|| {
                        format!(
                            "ligand source atom {source_index} maps to missing scored atom {scored_index}"
                        )
                    })?;
                    if seen_scored[scored_index] {
                        return Err(format!(
                            "ligand scored atom {scored_index} is mapped from more than one source atom"
                        ));
                    }
                    seen_scored[scored_index] = true;
                    if self.scored_to_source[scored_index] != source_index {
                        return Err(format!(
                            "ligand source/scored mappings disagree at scored atom {scored_index}"
                        ));
                    }
                    if source_atom.element == Element::H || source_atom.xs_type.is_none() {
                        return Err(format!(
                            "ligand non-scoring source atom {source_index} maps to scored atom {scored_index}"
                        ));
                    }
                    scored_atom
                        .validate_scoring_identity(&format!("ligand scored atom {scored_index}"))?;
                    if !same_scoring_identity(source_atom, scored_atom)
                        || scored_atom.source_index != source_index
                    {
                        return Err(format!(
                            "ligand scored atom {scored_index} identity does not match source atom {source_index}"
                        ));
                    }
                }
                None => {
                    if source_atom.element != Element::H && source_atom.xs_type.is_some() {
                        return Err(format!(
                            "ligand scorable source atom {source_index} is missing from scored atoms"
                        ));
                    }
                }
            }
        }
        if seen_scored.iter().any(|seen| !seen) {
            return Err("ligand scored atom is absent from source-to-scored mapping".into());
        }
        Ok(())
    }

    pub(crate) fn validate_pair_tables(&self) -> Result<(), String> {
        validate_pair_table("intra-exclusion", &self.intra_excluded, self.atoms.len())?;
        validate_pair_table(
            "distance-variable",
            &self.distance_variable_pairs,
            self.atoms.len(),
        )?;
        if self
            .distance_variable_pairs
            .iter()
            .any(|pair| self.intra_excluded.binary_search(pair).is_ok())
        {
            return Err(
                "distance-variable and intra-exclusion pair tables must not overlap".into(),
            );
        }
        Ok(())
    }

    pub(crate) fn validated_num_tors(&self) -> Result<f64, String> {
        self.validate_scored_mapping()?;
        self.validate_pair_tables()?;
        if !self.num_tors.is_finite() || self.num_tors < 0.0 {
            return Err("ligand effective rotor count must be finite and non-negative".into());
        }
        let source_count = self.source_atoms.len();
        if source_count == 0 {
            return Err("ligand has no source atoms for torsion validation".into());
        }
        let mut source_bonds = HashSet::new();
        for bond in &self.source_bonds {
            if bond.a >= source_count || bond.b >= source_count {
                return Err(format!(
                    "ligand source bond ({}, {}) is out of bounds for {source_count} atoms",
                    bond.a, bond.b
                ));
            }
            if bond.a == bond.b {
                return Err(format!(
                    "ligand source bond ({}, {}) must connect distinct atoms",
                    bond.a, bond.b
                ));
            }
            if !source_bonds.insert(bond.canonical()) {
                return Err(format!(
                    "ligand source bond ({}, {}) is duplicated",
                    bond.a, bond.b
                ));
            }
        }

        let expected_bonds = project_bonds(&self.source_bonds, &self.source_to_scored);
        if self.bonds != expected_bonds {
            return Err("ligand scored bonds do not match the source-bond projection".into());
        }
        let expected_excluded = bonds::bonded_exclusions(self.atoms.len(), &self.bonds);
        if self.intra_excluded != expected_excluded {
            return Err("ligand intra-exclusion pairs do not match scored bonds".into());
        }

        if self.rigid_fragments.is_empty() {
            return Err("ligand has no ROOT rigid fragment".into());
        }
        if self.rigid_fragments[0].branch.is_some() || self.rigid_fragments[0].parent.is_some() {
            return Err("ligand ROOT rigid fragment has invalid metadata".into());
        }
        let mut fragment_membership = vec![None; source_count];
        let mut wire_branch_ids = HashSet::new();
        for (fragment_index, fragment) in self.rigid_fragments.iter().enumerate() {
            if fragment_index > 0 {
                let branch_index = fragment.branch.ok_or_else(|| {
                    format!("ligand rigid fragment {fragment_index} has no branch identity")
                })?;
                if !wire_branch_ids.insert(branch_index) {
                    return Err(format!(
                        "ligand rigid fragments reuse wire branch identity {branch_index}"
                    ));
                }
                let parent = fragment.parent.ok_or_else(|| {
                    format!("ligand rigid fragment {fragment_index} has no parent")
                })?;
                if parent >= fragment_index {
                    return Err(format!(
                        "ligand rigid fragment {fragment_index} has invalid parent {parent}"
                    ));
                }
            }
            for &source_index in &fragment.source_atoms {
                if source_index >= source_count {
                    return Err(format!(
                        "ligand rigid fragment {fragment_index} references missing source atom {source_index}"
                    ));
                }
                if fragment_membership[source_index]
                    .replace(fragment_index)
                    .is_some()
                {
                    return Err(format!(
                        "ligand source atom {source_index} appears in multiple rigid fragments"
                    ));
                }
            }
        }
        if fragment_membership.iter().any(Option::is_none) {
            return Err("ligand rigid fragments do not cover every source atom".into());
        }

        let expected_active_fragments = self
            .rigid_fragments
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(index, fragment)| (!fragment.source_atoms.is_empty()).then_some(index))
            .collect::<HashSet<_>>();
        let mut actual_active_fragments = HashSet::new();
        let mut expected_rotatable = Vec::new();
        for (index, branch) in self.branches.iter().enumerate() {
            if branch.parent_source_index >= source_count
                || branch.child_source_index >= source_count
                || branch.parent_source_index == branch.child_source_index
            {
                return Err(format!(
                    "ligand branch {index} has invalid source atom endpoints"
                ));
            }
            if self.source_atoms[branch.parent_source_index].serial != branch.parent_serial
                || self.source_atoms[branch.child_source_index].serial != branch.child_serial
            {
                return Err(format!(
                    "ligand branch {index} serials do not match its source atom endpoints"
                ));
            }
            if branch.parent_fragment >= self.rigid_fragments.len()
                || branch.child_fragment >= self.rigid_fragments.len()
                || branch.child_fragment == 0
                || branch.parent_fragment == branch.child_fragment
            {
                return Err(format!(
                    "ligand branch {index} has invalid rigid-fragment endpoints"
                ));
            }
            if !actual_active_fragments.insert(branch.child_fragment) {
                return Err(format!(
                    "ligand branch {index} reuses active child fragment {}",
                    branch.child_fragment
                ));
            }
            let child_axis_fragment = fragment_membership[branch.child_source_index]
                .ok_or_else(|| format!("ligand branch {index} child axis has no rigid fragment"))?;
            if child_axis_fragment != branch.parent_fragment {
                return Err(format!(
                    "ligand branch {index} child axis is not rigid with its parent fragment"
                ));
            }
            if self.rigid_fragments[branch.child_fragment]
                .source_atoms
                .is_empty()
            {
                return Err(format!("ligand branch {index} has no movable child body"));
            }
            match branch.parent_branch {
                Some(parent) if parent >= index => {
                    return Err(format!(
                        "ligand branch {index} has an invalid parent branch"
                    ));
                }
                Some(parent) => {
                    if self.branches[parent].child_fragment != branch.parent_fragment {
                        return Err(format!(
                            "ligand branch {index} parent branch does not own its parent fragment"
                        ));
                    }
                }
                None => {
                    if branch.parent_fragment != 0 {
                        return Err(format!(
                            "ligand branch {index} without a parent branch must grow from ROOT"
                        ));
                    }
                }
            }
            let axis = Bond {
                a: branch.parent_source_index,
                b: branch.child_source_index,
            }
            .canonical();
            if !source_bonds.contains(&axis) {
                return Err(format!(
                    "ligand branch {index} axis is absent from source bonds"
                ));
            }
            if let (Some(parent), Some(child)) = (
                self.source_to_scored[branch.parent_source_index],
                self.source_to_scored[branch.child_source_index],
            ) {
                expected_rotatable.push(Bond {
                    a: parent,
                    b: child,
                });
            }
        }
        if actual_active_fragments != expected_active_fragments {
            return Err(
                "ligand active branches do not match non-empty child rigid fragments".into(),
            );
        }
        if self.rotatable != expected_rotatable {
            return Err("ligand rotatable bonds do not match active branch axes".into());
        }

        let mut mobility = SourceMobility::new(source_count);
        for fragment in &self.rigid_fragments {
            mobility.mark_group(&fragment.source_atoms);
        }
        for branch in &self.branches {
            mobility.mark_non_variable(branch.parent_source_index, branch.child_source_index);
            for &source_index in &self.rigid_fragments[branch.child_fragment].source_atoms {
                mobility.mark_non_variable(branch.parent_source_index, source_index);
                mobility.mark_non_variable(branch.child_source_index, source_index);
            }
        }
        let expected_variable_pairs = distance_variable_pairs(
            self.atoms.len(),
            &self.scored_to_source,
            &mobility,
            &self.intra_excluded,
        );
        if self.distance_variable_pairs != expected_variable_pairs {
            return Err(
                "ligand variable-distance pairs do not match rigid-fragment mobility".into(),
            );
        }

        let elements = self
            .source_atoms
            .iter()
            .map(|atom| atom.element)
            .collect::<Vec<_>>();
        let expected = vina_effective_num_tors(&self.branches, &self.source_bonds, &elements);
        if self.num_tors != expected {
            return Err(format!(
                "ligand effective rotor count {} does not match topology-derived value {expected}",
                self.num_tors
            ));
        }
        Ok(expected)
    }
}

/// A validated rigid receptor prepared from authoritative PDBQT input.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedReceptor {
    pub(crate) atoms: Vec<DockAtom>,
    pub(crate) bonds: Vec<Bond>,
}

impl PreparedReceptor {
    pub fn atoms(&self) -> &[DockAtom] {
        &self.atoms
    }

    pub fn bonds(&self) -> &[Bond] {
        &self.bonds
    }
}

/// Geometry-derived PDB receptor used only by explicitly approximate workflows.
/// This type cannot be passed to [`crate::docking::engine::DockingEngine`].
#[derive(Debug, Clone, PartialEq)]
pub struct ApproximatePreparedReceptor {
    pub(crate) atoms: Vec<DockAtom>,
    pub(crate) bonds: Vec<Bond>,
}

impl ApproximatePreparedReceptor {
    pub fn atoms(&self) -> &[DockAtom] {
        &self.atoms
    }

    pub fn bonds(&self) -> &[Bond] {
        &self.bonds
    }
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
    let active_branch_indices = (0..parsed.branches.len())
        .filter(|index| pdbqt::branch_is_active(&parsed, *index))
        .collect::<Vec<_>>();
    let mut original_to_active = vec![None; parsed.branches.len()];
    for (active_index, &original_index) in active_branch_indices.iter().enumerate() {
        original_to_active[original_index] = Some(active_index);
    }
    let branches = active_branch_indices
        .iter()
        .map(|&index| {
            let branch = &parsed.branches[index];
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
                    .and_then(|parent| effective_active_branch(&parsed, parent))
                    .and_then(|original_parent| original_to_active[original_parent]),
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

fn same_scoring_identity(source: &DockAtom, scored: &DockAtom) -> bool {
    source.serial == scored.serial
        && source.name == scored.name
        && source.element == scored.element
        && source.wire_ad_type == scored.wire_ad_type
        && source.xs_type == scored.xs_type
        && source.charge.to_bits() == scored.charge.to_bits()
        && source.residue_name == scored.residue_name
        && source.chain == scored.chain
        && source.residue_seq == scored.residue_seq
        && source.insertion_code == scored.insertion_code
        && source.is_hetero == scored.is_hetero
}

fn validate_pair_table(
    label: &str,
    pairs: &[(usize, usize)],
    atom_count: usize,
) -> Result<(), String> {
    let mut previous = None;
    for &(first, second) in pairs {
        if first >= second {
            return Err(format!(
                "{label} pair ({first}, {second}) must be canonical with first < second"
            ));
        }
        if second >= atom_count {
            return Err(format!(
                "{label} pair ({first}, {second}) is out of bounds for {atom_count} scored atoms"
            ));
        }
        if previous.is_some_and(|candidate| candidate >= (first, second)) {
            return Err(format!(
                "{label} pairs must be sorted and unique; ({first}, {second}) is out of order"
            ));
        }
        previous = Some((first, second));
    }
    Ok(())
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
    pairs.contains(&pair)
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
    fn active_branch_parent_indices_are_remapped_after_inactive_wire_branches() {
        let root_one = atom_line(1, 0.0, AdType::C, 0.0);
        let root_two = atom_line(10, 4.0, AdType::C, 0.0);
        let inactive_axis = atom_line(2, 1.5, AdType::C, 0.0);
        let parent_axis = atom_line(11, 5.5, AdType::C, 0.0);
        let parent_body = atom_line(12, 7.0, AdType::C, 0.0);
        let child_axis = atom_line(13, 8.5, AdType::C, 0.0);
        let child_body = atom_line(14, 10.0, AdType::C, 0.0);
        let ligand = prepare_ligand_pdbqt(&format!(
            "ROOT\n{root_one}\n{root_two}\nENDROOT\nBRANCH 1 2\n{inactive_axis}\nENDBRANCH 1 2\nBRANCH 10 11\n{parent_axis}\n{parent_body}\nBRANCH 12 13\n{child_axis}\n{child_body}\nENDBRANCH 12 13\nENDBRANCH 10 11\nTORSDOF 3\n"
        ))
        .unwrap();
        assert_eq!(ligand.branches.len(), 2);
        assert_eq!(ligand.branches[0].parent_branch, None);
        assert_eq!(ligand.branches[1].parent_branch, Some(0));
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
    fn child_axis_keeps_parent_mobility_when_it_follows_branch_body_atoms() {
        let root_a = atom_line(1, -4.0, AdType::C, 0.0);
        let root_b = atom_line(2, 1.5, AdType::C, 0.0);
        let child_body = atom_line(4, 0.7, AdType::C, 0.0);
        let child_axis = atom_line(3, 0.8, AdType::C, 0.0);
        let ligand = prepare_ligand_pdbqt(&format!(
            "ROOT\n{root_a}\n{root_b}\nENDROOT\nBRANCH 2 3\n{child_body}\n{child_axis}\nENDBRANCH 2 3\nTORSDOF 1\n"
        ))
        .unwrap();
        assert_eq!(ligand.rigid_fragments[0].source_atoms, vec![0, 1, 3]);
        assert_eq!(ligand.rigid_fragments[1].source_atoms, vec![2]);
        assert!(ligand.is_distance_variable_pair(0, 2));
        assert!(!ligand.is_distance_variable_pair(0, 1));
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

        let mut unsorted = ligand.clone();
        unsorted.intra_excluded = vec![(1, 3), (0, 2)];
        assert!(unsorted.is_intra_excluded(1, 3));
        assert!(unsorted.is_intra_excluded(2, 0));
    }
}
