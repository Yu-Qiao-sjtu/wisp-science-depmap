//! Strict PDBQT parsing and writing.
//!
//! The parser treats the torsion tree as an ordered wire protocol, not as a
//! loose collection of records.  ROOT/BRANCH nesting, branch axes, atom
//! membership, serial identity, and TORSDOF are validated before a ligand is
//! returned.  Multi-model files must repeat exactly the same atom identity and
//! topology; only coordinates may vary.

use super::atom_type::{AdType, Element};
use super::pdb::RawAtom;
use std::collections::{HashMap, HashSet};

/// One PDBQT atom record: a PDB atom plus strict charge and AD wire identity.
#[derive(Debug, Clone, PartialEq)]
pub struct PdbqtAtom {
    pub raw: RawAtom,
    pub charge: f64,
    pub ad_type: AdType,
}

/// One rotatable branch. The axis is `parent_serial -> child_serial`; `parent`
/// is the enclosing branch, or `None` when the branch grows from ROOT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdbqtBranch {
    pub parent_serial: i32,
    pub child_serial: i32,
    pub parent: Option<usize>,
}

/// Ordered torsion-tree events. Atom indices refer to `PdbqtLigand::atoms` and
/// branch indices refer to `PdbqtLigand::branches`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyEvent {
    RootStart,
    Atom(usize),
    RootEnd,
    BranchStart(usize),
    BranchEnd(usize),
}

/// One validated PDBQT model.
#[derive(Debug, Clone, PartialEq)]
pub struct PdbqtLigand {
    /// MODEL serial when the source used MODEL/ENDMDL wrappers.
    pub model_id: Option<i32>,
    pub atoms: Vec<PdbqtAtom>,
    pub branches: Vec<PdbqtBranch>,
    /// Enclosing branch for each atom (`None` is the ROOT rigid fragment).
    pub atom_branches: Vec<Option<usize>>,
    /// Original ordered ROOT/atom/BRANCH event stream.
    pub topology: Vec<TopologyEvent>,
    /// File-declared TORSDOF. This is deliberately independent of branch count.
    pub torsdof: u32,
}

/// A rigid receptor PDBQT contains only atom records plus optional comments.
/// Ligand topology records are rejected rather than ignored.
#[derive(Debug, Clone, PartialEq)]
pub struct PdbqtReceptor {
    pub atoms: Vec<PdbqtAtom>,
}

#[derive(Debug)]
struct ModelBuilder {
    ligand: PdbqtLigand,
    serial_to_atom: HashMap<i32, usize>,
    branch_stack: Vec<usize>,
    root_seen: bool,
    root_open: bool,
    root_closed: bool,
    torsdof: Option<u32>,
}

impl ModelBuilder {
    fn new(model_id: Option<i32>) -> ModelBuilder {
        ModelBuilder {
            ligand: PdbqtLigand {
                model_id,
                atoms: Vec::new(),
                branches: Vec::new(),
                atom_branches: Vec::new(),
                topology: Vec::new(),
                torsdof: 0,
            },
            serial_to_atom: HashMap::new(),
            branch_stack: Vec::new(),
            root_seen: false,
            root_open: false,
            root_closed: false,
            torsdof: None,
        }
    }

    fn ensure_before_torsdof(&self) -> Result<(), String> {
        if self.torsdof.is_some() {
            Err("topology record after TORSDOF".into())
        } else {
            Ok(())
        }
    }

    fn root_start(&mut self) -> Result<(), String> {
        self.ensure_before_torsdof()?;
        if self.root_seen {
            return Err("duplicate ROOT record".into());
        }
        if !self.branch_stack.is_empty() || !self.ligand.atoms.is_empty() {
            return Err("ROOT must precede every atom and branch".into());
        }
        self.root_seen = true;
        self.root_open = true;
        self.ligand.topology.push(TopologyEvent::RootStart);
        Ok(())
    }

    fn root_end(&mut self) -> Result<(), String> {
        self.ensure_before_torsdof()?;
        if !self.root_open {
            return Err("ENDROOT without an open ROOT".into());
        }
        if self
            .ligand
            .atom_branches
            .iter()
            .all(|membership| membership.is_some())
        {
            return Err("ROOT rigid fragment is empty".into());
        }
        self.root_open = false;
        self.root_closed = true;
        self.ligand.topology.push(TopologyEvent::RootEnd);
        Ok(())
    }

    fn atom(&mut self, atom: PdbqtAtom) -> Result<(), String> {
        self.ensure_before_torsdof()?;
        let membership = if self.root_open {
            None
        } else if let Some(branch) = self.branch_stack.last().copied() {
            Some(branch)
        } else {
            return Err("atom record outside ROOT or BRANCH".into());
        };
        if atom.raw.serial <= 0 {
            return Err("atom serial must be positive".into());
        }
        if self.serial_to_atom.contains_key(&atom.raw.serial) {
            return Err(format!("duplicate atom serial {}", atom.raw.serial));
        }

        let index = self.ligand.atoms.len();
        self.serial_to_atom.insert(atom.raw.serial, index);
        self.ligand.atoms.push(atom);
        self.ligand.atom_branches.push(membership);
        self.ligand.topology.push(TopologyEvent::Atom(index));
        Ok(())
    }

    fn branch_start(&mut self, parent_serial: i32, child_serial: i32) -> Result<(), String> {
        self.ensure_before_torsdof()?;
        if !self.root_closed || self.root_open {
            return Err("BRANCH requires a completed ROOT block".into());
        }
        if parent_serial <= 0 || child_serial <= 0 || parent_serial == child_serial {
            return Err("BRANCH axis must contain two distinct positive serials".into());
        }
        let parent_branch = self.branch_stack.last().copied();
        let parent_atom = self
            .serial_to_atom
            .get(&parent_serial)
            .copied()
            .ok_or_else(|| format!("BRANCH parent serial {parent_serial} has not been declared"))?;
        if self.ligand.atom_branches[parent_atom] != parent_branch {
            return Err(format!(
                "BRANCH parent serial {parent_serial} is not in the enclosing rigid fragment"
            ));
        }
        if self.serial_to_atom.contains_key(&child_serial) {
            return Err(format!(
                "BRANCH child serial {child_serial} must be declared inside the branch"
            ));
        }
        if self.ligand.branches.iter().any(|branch| {
            branch.parent_serial == parent_serial && branch.child_serial == child_serial
        }) {
            return Err(format!(
                "duplicate BRANCH axis {parent_serial} {child_serial}"
            ));
        }

        let branch = self.ligand.branches.len();
        self.ligand.branches.push(PdbqtBranch {
            parent_serial,
            child_serial,
            parent: parent_branch,
        });
        self.ligand
            .topology
            .push(TopologyEvent::BranchStart(branch));
        self.branch_stack.push(branch);
        Ok(())
    }

    fn branch_end(&mut self, parent_serial: i32, child_serial: i32) -> Result<(), String> {
        self.ensure_before_torsdof()?;
        let branch = self
            .branch_stack
            .last()
            .copied()
            .ok_or_else(|| "ENDBRANCH without an open BRANCH".to_string())?;
        let expected = self.ligand.branches[branch];
        if (parent_serial, child_serial) != (expected.parent_serial, expected.child_serial) {
            return Err(format!(
                "ENDBRANCH {parent_serial} {child_serial} does not match BRANCH {} {}",
                expected.parent_serial, expected.child_serial
            ));
        }
        let direct_atoms: Vec<usize> = self
            .ligand
            .atom_branches
            .iter()
            .enumerate()
            .filter_map(|(index, membership)| (*membership == Some(branch)).then_some(index))
            .collect();
        if direct_atoms.is_empty() {
            return Err(format!(
                "BRANCH {} {} is empty",
                expected.parent_serial, expected.child_serial
            ));
        }
        if !direct_atoms
            .iter()
            .any(|&index| self.ligand.atoms[index].raw.serial == expected.child_serial)
        {
            return Err(format!(
                "BRANCH child serial {} is not a direct member of its rigid fragment",
                expected.child_serial
            ));
        }
        self.branch_stack.pop();
        self.ligand.topology.push(TopologyEvent::BranchEnd(branch));
        Ok(())
    }

    fn set_torsdof(&mut self, value: u32) -> Result<(), String> {
        if self.torsdof.is_some() {
            return Err("duplicate TORSDOF record".into());
        }
        if !self.root_closed || self.root_open || !self.branch_stack.is_empty() {
            return Err("TORSDOF requires a closed ROOT and all branches closed".into());
        }
        self.torsdof = Some(value);
        Ok(())
    }

    fn finish(mut self) -> Result<PdbqtLigand, String> {
        if self.root_open {
            return Err("unclosed ROOT at end of model".into());
        }
        if !self.branch_stack.is_empty() {
            return Err("unclosed BRANCH at end of model".into());
        }
        if !self.root_seen || !self.root_closed {
            return Err("model must contain exactly one ROOT/ENDROOT block".into());
        }
        if self.ligand.atoms.is_empty() {
            return Err("model has no ATOM or HETATM records".into());
        }
        self.ligand.torsdof = self
            .torsdof
            .ok_or_else(|| "model must contain exactly one TORSDOF record".to_string())?;
        validate_ligand(&self.ligand)?;
        Ok(self.ligand)
    }
}

/// Parse a rigid receptor PDBQT body.
///
/// Rigid receptors contain only `ATOM`/`HETATM` records and optional blank,
/// `TER`, `END`, `REMARK`, or `WARNING` lines. Ligand topology records and unknown keywords
/// fail explicitly so a ligand cannot be misinterpreted as a rigid receptor.
pub fn parse_receptor_pdbqt(text: &str) -> Result<PdbqtReceptor, String> {
    let mut atoms = Vec::new();
    let mut serials = HashSet::new();
    for (line_index, line) in text.lines().enumerate() {
        let number = line_index + 1;
        match record_keyword(line) {
            "" | "TER" | "END" | "REMARK" | "WARNING" => {}
            "ATOM" | "HETATM" => {
                let atom = parse_atom_line(line).map_err(|message| line_error(number, &message))?;
                if atom.raw.serial <= 0 {
                    return Err(line_error(number, "atom serial must be positive"));
                }
                if !serials.insert(atom.raw.serial) {
                    return Err(line_error(
                        number,
                        &format!("duplicate atom serial {}", atom.raw.serial),
                    ));
                }
                atoms.push(atom);
            }
            "ROOT" | "ENDROOT" | "BRANCH" | "ENDBRANCH" | "TORSDOF" | "MODEL" | "ENDMDL" => {
                return Err(line_error(
                    number,
                    "ligand topology/model record is not valid in a rigid receptor PDBQT",
                ));
            }
            keyword => {
                return Err(line_error(
                    number,
                    &format!("unsupported rigid receptor PDBQT record `{keyword}`"),
                ));
            }
        }
    }
    if atoms.is_empty() {
        return Err("rigid receptor PDBQT has no ATOM or HETATM records".into());
    }
    Ok(PdbqtReceptor { atoms })
}

/// Parse a PDBQT body. Multi-model input is accepted only when all models have
/// identical atom identity and topology; the first pose is returned.
pub fn parse_pdbqt(text: &str) -> Result<PdbqtLigand, String> {
    let mut models = parse_pdbqt_models(text)?;
    if models.is_empty() {
        return Err("no PDBQT model found".into());
    }
    Ok(models.remove(0))
}

/// Parse all PDBQT models and enforce cross-model identity/topology equality.
pub fn parse_pdbqt_models(text: &str) -> Result<Vec<PdbqtLigand>, String> {
    let explicit_models = text
        .lines()
        .any(|line| matches!(record_keyword(line), "MODEL" | "ENDMDL"));
    let mut models = Vec::new();
    let mut current = (!explicit_models).then(|| ModelBuilder::new(None));
    let mut model_ids = HashSet::new();

    for (line_index, line) in text.lines().enumerate() {
        let number = line_index + 1;
        let keyword = record_keyword(line);
        let ignorable = matches!(keyword, "" | "REMARK" | "WARNING");

        if keyword == "MODEL" {
            if !explicit_models {
                unreachable!("MODEL discovery must select explicit mode");
            }
            if current.is_some() {
                return Err(line_error(number, "nested MODEL record"));
            }
            let id = parse_one::<i32>(line, "MODEL", "MODEL serial")
                .map_err(|message| line_error(number, &message))?;
            if id <= 0 {
                return Err(line_error(number, "MODEL serial must be positive"));
            }
            if !model_ids.insert(id) {
                return Err(line_error(number, &format!("duplicate MODEL serial {id}")));
            }
            current = Some(ModelBuilder::new(Some(id)));
            continue;
        }

        if keyword == "ENDMDL" {
            parse_no_args(line, "ENDMDL").map_err(|message| line_error(number, &message))?;
            let builder = current
                .take()
                .ok_or_else(|| line_error(number, "ENDMDL without MODEL"))?;
            let model = builder
                .finish()
                .map_err(|message| line_error(number, &message))?;
            models.push(model);
            continue;
        }

        if explicit_models && current.is_none() {
            if !ignorable {
                return Err(line_error(number, "PDBQT record outside MODEL"));
            }
            continue;
        }

        let Some(builder) = current.as_mut() else {
            continue;
        };
        let result = match keyword {
            "ROOT" => parse_no_args(line, "ROOT").and_then(|_| builder.root_start()),
            "ENDROOT" => parse_no_args(line, "ENDROOT").and_then(|_| builder.root_end()),
            "BRANCH" => parse_serial_pair(line, "BRANCH")
                .and_then(|(parent, child)| builder.branch_start(parent, child)),
            "ENDBRANCH" => parse_serial_pair(line, "ENDBRANCH")
                .and_then(|(parent, child)| builder.branch_end(parent, child)),
            "TORSDOF" => parse_one::<u32>(line, "TORSDOF", "TORSDOF value")
                .and_then(|value| builder.set_torsdof(value)),
            "ATOM" | "HETATM" => parse_atom_line(line).and_then(|atom| builder.atom(atom)),
            "" | "REMARK" | "WARNING" => Ok(()),
            keyword => Err(format!("unsupported ligand PDBQT record `{keyword}`")),
        };
        result.map_err(|message| line_error(number, &message))?;
    }

    if explicit_models {
        if current.is_some() {
            return Err("unclosed MODEL at end of PDBQT".into());
        }
        if models.is_empty() {
            return Err("PDBQT contains no complete MODEL".into());
        }
    } else {
        let builder = current.expect("implicit mode always owns one builder");
        models.push(builder.finish()?);
    }

    validate_model_consistency(&models)?;
    Ok(models)
}

fn record_keyword(line: &str) -> &str {
    if line.starts_with("ATOM  ") {
        "ATOM"
    } else if line.starts_with("HETATM") {
        "HETATM"
    } else {
        line.split_ascii_whitespace().next().unwrap_or("")
    }
}

fn line_error(number: usize, message: &str) -> String {
    format!("line {number}: {message}")
}

fn parse_no_args(line: &str, keyword: &str) -> Result<(), String> {
    let mut parts = line.split_ascii_whitespace();
    if parts.next() != Some(keyword) || parts.next().is_some() {
        return Err(format!("{keyword} record must not contain arguments"));
    }
    Ok(())
}

fn parse_one<T>(line: &str, keyword: &str, label: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    let mut parts = line.split_ascii_whitespace();
    if parts.next() != Some(keyword) {
        return Err(format!("bad {keyword} record"));
    }
    let value = parts
        .next()
        .ok_or_else(|| format!("missing {label}"))?
        .parse::<T>()
        .map_err(|_| format!("bad {label}"))?;
    if parts.next().is_some() {
        return Err(format!("{keyword} record has extra fields"));
    }
    Ok(value)
}

fn parse_serial_pair(line: &str, keyword: &str) -> Result<(i32, i32), String> {
    let mut parts = line.split_ascii_whitespace();
    if parts.next() != Some(keyword) {
        return Err(format!("bad {keyword} record"));
    }
    let parse = |token: Option<&str>| {
        token
            .ok_or_else(|| format!("missing {keyword} serial"))?
            .parse::<i32>()
            .map_err(|_| format!("bad {keyword} serial"))
    };
    let parent = parse(parts.next())?;
    let child = parse(parts.next())?;
    if parts.next().is_some() {
        return Err(format!("{keyword} record has extra fields"));
    }
    Ok((parent, child))
}

fn parse_atom_line(line: &str) -> Result<PdbqtAtom, String> {
    let mut raw = parse_raw_atom(line)?;
    let charge_text = line
        .get(68..76)
        .ok_or_else(|| "atom record is missing charge column 69-76".to_string())?
        .trim();
    let type_text = line
        .get(77..)
        .ok_or_else(|| "atom record is missing AD type starting at column 78".to_string())?
        .trim();
    if type_text.is_empty() {
        return Err("atom record is missing AD type".into());
    }
    if type_text.split_ascii_whitespace().count() != 1 {
        return Err("atom record has extra fields after AD type".into());
    }
    let charge = if charge_text.is_empty() {
        0.0
    } else {
        parse_fixed_decimal_value(charge_text, 8, 3, "charge")?
    };
    let ad_type = AdType::from_pdbqt_symbol(type_text)?;
    raw.element = ad_type.element();
    Ok(PdbqtAtom {
        raw,
        charge,
        ad_type,
    })
}

fn parse_raw_atom(line: &str) -> Result<RawAtom, String> {
    let altloc = line
        .get(16..17)
        .and_then(|text| text.chars().next())
        .unwrap_or(' ');
    if altloc != ' ' && altloc != 'A' {
        return Err(format!("unsupported alternate location `{altloc}`"));
    }
    let name = line
        .get(12..16)
        .ok_or_else(|| "short atom name field".to_string())?;
    let residue_name = line
        .get(17..20)
        .ok_or_else(|| "short residue name field".to_string())?;
    let chain = line
        .get(21..22)
        .and_then(|text| text.chars().next())
        .ok_or_else(|| "short chain field".to_string())?;
    let insertion_code = line
        .get(26..27)
        .and_then(|text| text.chars().next())
        .ok_or_else(|| "short insertion-code field".to_string())?;
    validate_fixed_text_fields(name, residue_name, chain, insertion_code)?;

    let position = [
        coordinate(line, 30, 38, "x coordinate")?,
        coordinate(line, 38, 46, "y coordinate")?,
        coordinate(line, 46, 54, "z coordinate")?,
    ];
    Ok(RawAtom {
        serial: column(line, 6, 11)?.parse().map_err(|_| "bad serial")?,
        name: name.to_string(),
        element: Element::Other,
        position,
        residue_name: residue_name.trim().to_string(),
        chain,
        residue_seq: column(line, 22, 26)?.parse().map_err(|_| "bad residue")?,
        insertion_code,
        is_hetero: line.starts_with("HETATM"),
    })
}

fn column(line: &str, start: usize, end: usize) -> Result<&str, String> {
    line.get(start..end)
        .map(str::trim)
        .ok_or_else(|| "short atom record".to_string())
}

fn coordinate(line: &str, start: usize, end: usize, label: &str) -> Result<f64, String> {
    parse_fixed_decimal_value(column(line, start, end)?, end - start, 3, label)
}

fn validate_model_consistency(models: &[PdbqtLigand]) -> Result<(), String> {
    let Some(reference) = models.first() else {
        return Ok(());
    };
    for (model_index, model) in models.iter().enumerate().skip(1) {
        let label = model.model_id.unwrap_or((model_index + 1) as i32);
        if model.branches != reference.branches
            || model.atom_branches != reference.atom_branches
            || model.topology != reference.topology
            || model.torsdof != reference.torsdof
        {
            return Err(format!(
                "MODEL {label} torsion topology does not match the first model"
            ));
        }
        if model.atoms.len() != reference.atoms.len() {
            return Err(format!(
                "MODEL {label} atom count does not match the first model"
            ));
        }
        for (atom_index, (atom, expected)) in model.atoms.iter().zip(&reference.atoms).enumerate() {
            let same_identity = atom.raw.serial == expected.raw.serial
                && atom.raw.name == expected.raw.name
                && atom.raw.element == expected.raw.element
                && atom.raw.residue_name == expected.raw.residue_name
                && atom.raw.chain == expected.raw.chain
                && atom.raw.residue_seq == expected.raw.residue_seq
                && atom.raw.insertion_code == expected.raw.insertion_code
                && atom.raw.is_hetero == expected.raw.is_hetero
                && atom.ad_type == expected.ad_type
                && atom.charge.to_bits() == expected.charge.to_bits();
            if !same_identity {
                return Err(format!(
                    "MODEL {label} atom identity at index {atom_index} does not match the first model"
                ));
            }
        }
    }
    Ok(())
}

/// Validate a constructed ligand before serialization or preparation.
pub fn validate_ligand(ligand: &PdbqtLigand) -> Result<(), String> {
    if ligand.atoms.is_empty() {
        return Err("cannot write an empty PDBQT ligand".into());
    }
    if ligand.atom_branches.len() != ligand.atoms.len() {
        return Err("atom branch-membership length does not match atom count".into());
    }
    if let Some(id) = ligand.model_id {
        if id <= 0 {
            return Err("MODEL serial must be positive".into());
        }
    }

    let mut serial_to_atom = HashMap::new();
    for (index, atom) in ligand.atoms.iter().enumerate() {
        if atom.raw.serial <= 0 {
            return Err(format!("atom {index} has a non-positive serial"));
        }
        if serial_to_atom.insert(atom.raw.serial, index).is_some() {
            return Err(format!("duplicate atom serial {}", atom.raw.serial));
        }
        validate_fixed_decimal_value(
            atom.charge,
            8,
            3,
            &format!("atom serial {} charge", atom.raw.serial),
        )?;
        for (label, coordinate) in [
            ("x coordinate", atom.raw.position[0]),
            ("y coordinate", atom.raw.position[1]),
            ("z coordinate", atom.raw.position[2]),
        ] {
            validate_fixed_decimal_value(
                coordinate,
                8,
                3,
                &format!("atom serial {} {label}", atom.raw.serial),
            )?;
        }
        if atom.raw.element != atom.ad_type.element() {
            return Err(format!(
                "atom serial {} element does not match AD type {}",
                atom.raw.serial,
                atom.ad_type.pdbqt_symbol()
            ));
        }
    }

    let mut root_open = false;
    let mut root_seen = false;
    let mut root_closed = false;
    let mut stack: Vec<usize> = Vec::new();
    let mut seen_atoms = vec![false; ligand.atoms.len()];
    let mut seen_branch_start = vec![false; ligand.branches.len()];
    let mut seen_branch_end = vec![false; ligand.branches.len()];
    let mut direct_atom_count = vec![0usize; ligand.branches.len()];
    let mut child_axis_seen = vec![false; ligand.branches.len()];

    for event in &ligand.topology {
        match *event {
            TopologyEvent::RootStart => {
                if root_seen || !stack.is_empty() || seen_atoms.iter().any(|seen| *seen) {
                    return Err("ROOT is duplicated or out of order".into());
                }
                root_seen = true;
                root_open = true;
            }
            TopologyEvent::Atom(index) => {
                let atom = ligand
                    .atoms
                    .get(index)
                    .ok_or_else(|| format!("topology references missing atom index {index}"))?;
                if seen_atoms[index] {
                    return Err(format!(
                        "topology emits atom serial {} more than once",
                        atom.raw.serial
                    ));
                }
                let expected_membership = if root_open {
                    None
                } else {
                    stack.last().copied().map(Some).ok_or_else(|| {
                        format!("atom serial {} is outside ROOT or BRANCH", atom.raw.serial)
                    })?
                };
                if ligand.atom_branches[index] != expected_membership {
                    return Err(format!(
                        "atom serial {} branch membership does not match topology order",
                        atom.raw.serial
                    ));
                }
                if let Some(branch) = expected_membership {
                    direct_atom_count[branch] += 1;
                    if atom.raw.serial == ligand.branches[branch].child_serial {
                        child_axis_seen[branch] = true;
                    }
                }
                seen_atoms[index] = true;
            }
            TopologyEvent::RootEnd => {
                if !root_open || !stack.is_empty() {
                    return Err("ENDROOT without the matching open ROOT".into());
                }
                root_open = false;
                root_closed = true;
            }
            TopologyEvent::BranchStart(branch_index) => {
                if !root_closed || root_open {
                    return Err("BRANCH appears before ENDROOT".into());
                }
                let branch = ligand.branches.get(branch_index).ok_or_else(|| {
                    format!("topology references missing branch index {branch_index}")
                })?;
                if seen_branch_start[branch_index] {
                    return Err(format!("branch index {branch_index} starts more than once"));
                }
                let expected_parent = stack.last().copied();
                if branch.parent != expected_parent {
                    return Err(format!("branch index {branch_index} has the wrong parent"));
                }
                let parent_atom = serial_to_atom
                    .get(&branch.parent_serial)
                    .copied()
                    .ok_or_else(|| {
                        format!("missing BRANCH parent serial {}", branch.parent_serial)
                    })?;
                if !seen_atoms[parent_atom] || ligand.atom_branches[parent_atom] != expected_parent
                {
                    return Err(format!(
                        "BRANCH parent serial {} is not in the enclosing rigid fragment",
                        branch.parent_serial
                    ));
                }
                let child_atom = serial_to_atom
                    .get(&branch.child_serial)
                    .copied()
                    .ok_or_else(|| {
                        format!("missing BRANCH child serial {}", branch.child_serial)
                    })?;
                if seen_atoms[child_atom] || ligand.atom_branches[child_atom] != Some(branch_index)
                {
                    return Err(format!(
                        "BRANCH child serial {} is not an undeclared direct branch member",
                        branch.child_serial
                    ));
                }
                seen_branch_start[branch_index] = true;
                stack.push(branch_index);
            }
            TopologyEvent::BranchEnd(branch_index) => {
                if stack.last().copied() != Some(branch_index) {
                    return Err(format!("branch index {branch_index} closes out of order"));
                }
                if direct_atom_count[branch_index] == 0 {
                    return Err(format!("branch index {branch_index} is empty"));
                }
                if !child_axis_seen[branch_index] {
                    return Err(format!(
                        "branch index {branch_index} does not contain its child-axis atom"
                    ));
                }
                stack.pop();
                seen_branch_end[branch_index] = true;
            }
        }
    }

    if root_open || !stack.is_empty() || !root_seen || !root_closed {
        return Err("incomplete ROOT/BRANCH topology".into());
    }
    if seen_atoms.iter().any(|seen| !seen) {
        return Err("topology does not emit every atom exactly once".into());
    }
    if seen_branch_start.iter().any(|seen| !seen) || seen_branch_end.iter().any(|seen| !seen) {
        return Err("topology does not open and close every branch exactly once".into());
    }
    if !ligand
        .atom_branches
        .iter()
        .any(|membership| membership.is_none())
    {
        return Err("ROOT rigid fragment is empty".into());
    }
    Ok(())
}

pub(crate) fn branch_is_active(ligand: &PdbqtLigand, branch: usize) -> bool {
    let child_serial = ligand.branches[branch].child_serial;
    let has_direct_body = ligand
        .atom_branches
        .iter()
        .enumerate()
        .any(|(index, membership)| {
            *membership == Some(branch) && ligand.atoms[index].raw.serial != child_serial
        });
    let has_child_branch = ligand
        .branches
        .iter()
        .any(|candidate| candidate.parent == Some(branch));
    has_direct_body || has_child_branch
}

/// Format one PDBQT atom line without truncation or non-finite fallback.
pub fn format_atom_line(atom: &PdbqtAtom) -> Result<String, String> {
    if atom.raw.serial <= 0 || atom.raw.serial.to_string().len() > 5 {
        return Err("atom serial does not fit the PDBQT serial field".into());
    }
    if atom.raw.residue_seq.to_string().len() > 4 {
        return Err("residue sequence does not fit the PDBQT field".into());
    }
    validate_fixed_text_fields(
        &atom.raw.name,
        &atom.raw.residue_name,
        atom.raw.chain,
        atom.raw.insertion_code,
    )?;
    if atom.raw.element != atom.ad_type.element() {
        return Err("atom element does not match its AD type".into());
    }
    validate_fixed_decimal_value(atom.raw.position[0], 8, 3, "x coordinate")?;
    validate_fixed_decimal_value(atom.raw.position[1], 8, 3, "y coordinate")?;
    validate_fixed_decimal_value(atom.raw.position[2], 8, 3, "z coordinate")?;
    validate_fixed_decimal_value(atom.charge, 8, 3, "charge")?;
    let x = fixed_decimal(atom.raw.position[0], 8, 3, "x coordinate")?;
    let y = fixed_decimal(atom.raw.position[1], 8, 3, "y coordinate")?;
    let z = fixed_decimal(atom.raw.position[2], 8, 3, "z coordinate")?;
    let charge = fixed_decimal(atom.charge, 8, 3, "charge")?;
    let record = if atom.raw.is_hetero {
        "HETATM"
    } else {
        "ATOM  "
    };
    Ok(format!(
        "{record}{serial:>5} {name:<4} {residue:>3} {chain}{residue_seq:>4}{icode}   {x}{y}{z}{occupancy:>6}{bfactor:>6}  {charge} {ad_type}",
        serial = atom.raw.serial,
        name = atom.raw.name,
        residue = atom.raw.residue_name,
        chain = atom.raw.chain,
        residue_seq = atom.raw.residue_seq,
        icode = atom.raw.insertion_code,
        occupancy = "1.00",
        bfactor = "0.00",
        ad_type = atom.ad_type.pdbqt_symbol(),
    ))
}

fn validate_fixed_text_fields(
    name: &str,
    residue_name: &str,
    chain: char,
    insertion_code: char,
) -> Result<(), String> {
    if !is_printable_ascii(name) || name.len() > 4 {
        return Err("atom name must be at most four printable ASCII bytes".into());
    }
    if !is_printable_ascii(residue_name) || residue_name.len() > 3 {
        return Err("residue name must be at most three printable ASCII bytes".into());
    }
    if !is_printable_ascii_char(chain) || !is_printable_ascii_char(insertion_code) {
        return Err("chain and insertion code must be printable ASCII".into());
    }
    Ok(())
}

fn is_printable_ascii(text: &str) -> bool {
    text.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}

fn is_printable_ascii_char(value: char) -> bool {
    value.is_ascii() && !value.is_ascii_control()
}

fn parse_fixed_decimal_value(
    text: &str,
    width: usize,
    precision: usize,
    label: &str,
) -> Result<f64, String> {
    let value = text
        .parse::<f64>()
        .map_err(|_| format!("bad {label} field"))?;
    validate_fixed_decimal_value(value, width, precision, label)?;
    Ok(value)
}

fn validate_fixed_decimal_value(
    value: f64,
    width: usize,
    precision: usize,
    label: &str,
) -> Result<(), String> {
    let rendered = fixed_decimal(value, width, precision, label)?;
    let reparsed = rendered
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("{label} canonical rendering is invalid"))?;
    if reparsed.to_bits() != value.to_bits() {
        return Err(format!(
            "{label} is not exactly representable in its PDBQT field"
        ));
    }
    Ok(())
}

fn fixed_decimal(
    value: f64,
    width: usize,
    precision: usize,
    label: &str,
) -> Result<String, String> {
    if !value.is_finite() {
        return Err(format!("{label} must be finite"));
    }
    let rendered = format!(
        "{value:>width$.precision$}",
        width = width,
        precision = precision
    );
    if rendered.len() != width {
        return Err(format!("{label} does not fit its PDBQT field"));
    }
    Ok(rendered)
}

/// Serialize one validated model. Invalid constructed topology is rejected;
/// there is no topology or atom-type fallback path.
pub fn write_pdbqt(ligand: &PdbqtLigand) -> Result<String, String> {
    validate_ligand(ligand)?;
    let mut body = String::new();
    if let Some(model_id) = ligand.model_id {
        body.push_str(&format!("MODEL {model_id}\n"));
    }
    for event in &ligand.topology {
        match *event {
            TopologyEvent::RootStart => body.push_str("ROOT\n"),
            TopologyEvent::Atom(index) => {
                body.push_str(&format_atom_line(&ligand.atoms[index])?);
                body.push('\n');
            }
            TopologyEvent::RootEnd => body.push_str("ENDROOT\n"),
            TopologyEvent::BranchStart(index) => {
                let branch = ligand.branches[index];
                body.push_str(&format!(
                    "BRANCH {:>4} {:>4}\n",
                    branch.parent_serial, branch.child_serial
                ));
            }
            TopologyEvent::BranchEnd(index) => {
                let branch = ligand.branches[index];
                body.push_str(&format!(
                    "ENDBRANCH {:>4} {:>4}\n",
                    branch.parent_serial, branch.child_serial
                ));
            }
        }
    }
    body.push_str(&format!("TORSDOF {}\n", ligand.torsdof));
    if ligand.model_id.is_some() {
        body.push_str("ENDMDL\n");
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom_line(serial: i32, position: [f64; 3], charge: f64, ad_type: AdType) -> String {
        format_atom_line(&PdbqtAtom {
            raw: RawAtom {
                serial,
                name: "C".into(),
                element: ad_type.element(),
                position,
                residue_name: "LIG".into(),
                chain: 'A',
                residue_seq: 1,
                insertion_code: ' ',
                is_hetero: true,
            },
            charge,
            ad_type,
        })
        .unwrap()
    }

    fn branched_body() -> String {
        let root = atom_line(1, [0.0, 0.0, 0.0], 0.1, AdType::C);
        let child = atom_line(2, [1.5, 0.0, 0.0], -0.1, AdType::C);
        let child_body = atom_line(4, [1.5, 1.4, 0.0], 0.0, AdType::C);
        let grandchild = atom_line(3, [3.0, 0.0, 0.0], 0.0, AdType::Oa);
        let grandchild_body = atom_line(5, [4.3, 0.0, 0.0], 0.0, AdType::C);
        format!(
            "ROOT\n{root}\nENDROOT\nBRANCH   1   2\n{child}\n{child_body}\nBRANCH   2   3\n{grandchild}\n{grandchild_body}\nENDBRANCH   2   3\nENDBRANCH   1   2\nTORSDOF 9\n"
        )
    }

    #[test]
    fn rigid_receptor_accepts_atoms_and_rejects_ligand_topology() {
        let carbon = atom_line(1, [0.0, 0.0, 0.0], 0.1, AdType::C);
        let oxygen = atom_line(2, [3.0, 0.0, 0.0], -0.1, AdType::Oa);
        let parsed = parse_receptor_pdbqt(&format!(
            "REMARK rigid receptor\n{carbon}\nWARNING test warning\n{oxygen}\n"
        ))
        .unwrap();
        assert_eq!(parsed.atoms.len(), 2);
        assert_eq!(parsed.atoms[0].ad_type, AdType::C);
        assert_eq!(parsed.atoms[1].ad_type, AdType::Oa);

        assert!(
            parse_receptor_pdbqt(&format!("ROOT\n{carbon}\nENDROOT\nTORSDOF 0\n"))
                .unwrap_err()
                .contains("ligand topology")
        );
        assert!(parse_receptor_pdbqt(&format!("{carbon}\n{carbon}\n"))
            .unwrap_err()
            .contains("duplicate atom serial"));
        assert!(parse_receptor_pdbqt(&format!("{carbon}\nTER\nEND\n")).is_ok());
        assert!(parse_receptor_pdbqt("CONECT 1 2\n")
            .unwrap_err()
            .contains("unsupported rigid receptor"));
    }

    #[test]
    fn atom_parser_rejects_control_characters_in_fixed_text_fields() {
        let line = atom_line(1, [0.0; 3], 0.0, AdType::C);
        for (offset, byte) in [(12, b'\t'), (18, 0), (21, b'\r'), (26, b'\t')] {
            let mut bytes = line.as_bytes().to_vec();
            bytes[offset] = byte;
            let invalid = String::from_utf8(bytes).unwrap();
            assert!(
                parse_pdbqt(&format!("ROOT\n{invalid}\nENDROOT\nTORSDOF 0\n"))
                    .unwrap_err()
                    .contains("printable ASCII")
            );
            assert!(parse_receptor_pdbqt(&format!("{invalid}\n"))
                .unwrap_err()
                .contains("printable ASCII"));
        }
    }

    #[test]
    fn atom_writer_rejects_control_characters_in_fixed_text_fields() {
        let base = PdbqtAtom {
            raw: RawAtom {
                serial: 1,
                name: "C".into(),
                element: Element::C,
                position: [0.0; 3],
                residue_name: "LIG".into(),
                chain: 'A',
                residue_seq: 1,
                insertion_code: ' ',
                is_hetero: true,
            },
            charge: 0.0,
            ad_type: AdType::C,
        };

        let mut invalid = base.clone();
        invalid.raw.name = "C\n".into();
        assert!(format_atom_line(&invalid)
            .unwrap_err()
            .contains("printable ASCII"));

        let mut invalid = base.clone();
        invalid.raw.residue_name = "L\rG".into();
        assert!(format_atom_line(&invalid)
            .unwrap_err()
            .contains("printable ASCII"));

        let mut invalid = base.clone();
        invalid.raw.chain = '\t';
        assert!(format_atom_line(&invalid)
            .unwrap_err()
            .contains("printable ASCII"));

        let mut invalid = base.clone();
        invalid.raw.insertion_code = '\0';
        assert!(format_atom_line(&invalid)
            .unwrap_err()
            .contains("printable ASCII"));

        let mut blank_fields = base;
        blank_fields.raw.name = "    ".into();
        blank_fields.raw.residue_name = "   ".into();
        blank_fields.raw.chain = ' ';
        assert!(format_atom_line(&blank_fields).is_ok());
    }

    #[test]
    fn atom_numeric_fields_must_be_losslessly_representable_as_f8_3() {
        let line = atom_line(1, [1.234, 0.0, 0.0], -0.246, AdType::C);
        for (range, replacement) in [
            (30..38, "1.234567"),
            (30..38, "   1e100"),
            (68..76, "0.123456"),
            (68..76, "   1e100"),
        ] {
            let mut invalid = line.clone();
            invalid.replace_range(range, replacement);
            assert!(
                parse_pdbqt(&format!("ROOT\n{invalid}\nENDROOT\nTORSDOF 0\n"))
                    .unwrap_err()
                    .contains("PDBQT field")
            );
            assert!(parse_receptor_pdbqt(&format!("{invalid}\n"))
                .unwrap_err()
                .contains("PDBQT field"));
        }

        let mut scientific = line.clone();
        scientific.replace_range(30..38, "    1e-3");
        let parsed = parse_pdbqt(&format!("ROOT\n{scientific}\nENDROOT\nTORSDOF 0\n")).unwrap();
        assert_eq!(parsed.atoms[0].raw.position[0], 0.001);
        assert_eq!(parse_pdbqt(&write_pdbqt(&parsed).unwrap()).unwrap(), parsed);

        let mut constructed = parse_pdbqt(&format!("ROOT\n{line}\nENDROOT\nTORSDOF 0\n")).unwrap();
        constructed.atoms[0].raw.position[0] = 1.234567;
        assert!(validate_ligand(&constructed)
            .unwrap_err()
            .contains("PDBQT field"));
        assert!(write_pdbqt(&constructed)
            .unwrap_err()
            .contains("PDBQT field"));
        constructed.atoms[0].raw.position[0] = 1.234;
        constructed.atoms[0].charge = 0.123456;
        assert!(validate_ligand(&constructed)
            .unwrap_err()
            .contains("PDBQT field"));
    }

    #[test]
    fn atom_lines_round_trip_charge_type_and_coordinates() {
        let line = atom_line(1, [1.234, -5.678, 9.012], -0.246, AdType::Oa);
        let parsed = parse_pdbqt(&format!("ROOT\n{line}\nENDROOT\nTORSDOF 2\n")).unwrap();
        assert_eq!(parsed.atoms[0].ad_type, AdType::Oa);
        assert_eq!(parsed.torsdof, 2);
        assert!((parsed.atoms[0].charge + 0.246).abs() < 5e-4);
        assert_eq!(
            write_pdbqt(&parsed).unwrap().lines().nth(1),
            Some(line.as_str())
        );
    }

    #[test]
    fn hydrogen_halogen_and_metal_wire_identity_is_preserved() {
        for (serial, ad_type) in [
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
        ]
        .into_iter()
        .enumerate()
        {
            let line = atom_line(serial as i32 + 1, [serial as f64, 0.0, 0.0], 0.0, ad_type);
            let parsed = parse_pdbqt(&format!("ROOT\n{line}\nENDROOT\nTORSDOF 0\n")).unwrap();
            assert_eq!(parsed.atoms[0].ad_type, ad_type);
            assert!(write_pdbqt(&parsed)
                .unwrap()
                .contains(ad_type.pdbqt_symbol()));
        }
    }

    #[test]
    fn ordered_nested_topology_round_trips_exactly() {
        let text = branched_body();
        let parsed = parse_pdbqt(&text).unwrap();
        assert_eq!(parsed.branches.len(), 2);
        assert_eq!(parsed.branches[1].parent, Some(0));
        assert_eq!(
            parsed.atom_branches,
            vec![None, Some(0), Some(0), Some(1), Some(1)]
        );
        assert_eq!(parsed.torsdof, 9);
        let written = write_pdbqt(&parsed).unwrap();
        assert_eq!(parse_pdbqt(&written).unwrap(), parsed);
    }

    #[test]
    fn branch_child_axis_may_follow_another_direct_atom() {
        let root = atom_line(1, [0.0, 0.0, 0.0], 0.0, AdType::C);
        let body = atom_line(3, [1.5, 1.5, 0.0], 0.0, AdType::C);
        let child = atom_line(2, [1.5, 0.0, 0.0], 0.0, AdType::C);
        let text = format!(
            "ROOT\n{root}\nENDROOT\nBRANCH 1 2\n{body}\n{child}\nENDBRANCH 1 2\nTORSDOF 1\n"
        );
        let parsed = parse_pdbqt(&text).unwrap();
        assert_eq!(
            parsed
                .atoms
                .iter()
                .map(|atom| atom.raw.serial)
                .collect::<Vec<_>>(),
            vec![1, 3, 2]
        );
        assert_eq!(parsed.atom_branches, vec![None, Some(0), Some(0)]);

        let written = write_pdbqt(&parsed).unwrap();
        assert!(written.find(&body).unwrap() < written.find(&child).unwrap());
        assert_eq!(parse_pdbqt(&written).unwrap(), parsed);
    }

    #[test]
    fn unknown_ligand_records_fail_instead_of_being_ignored() {
        let atom = atom_line(1, [0.0, 0.0, 0.0], 0.0, AdType::C);
        let error =
            parse_pdbqt(&format!("ROOT\n{atom}\nTYPOLOGY 1 2\nENDROOT\nTORSDOF 0\n")).unwrap_err();
        assert!(error.contains("unsupported ligand PDBQT record `TYPOLOGY`"));
    }

    #[test]
    fn strict_root_branch_and_torsdof_ordering_is_enforced() {
        let atom = atom_line(1, [0.0, 0.0, 0.0], 0.0, AdType::C);
        for bad in [
            format!("{atom}\nROOT\nENDROOT\nTORSDOF 0\n"),
            format!("ROOT\n{atom}\nTORSDOF 0\nENDROOT\n"),
            format!("ROOT\n{atom}\nENDROOT\n"),
            format!("ROOT\n{atom}\nENDROOT\nTORSDOF 0\nTORSDOF 0\n"),
        ] {
            assert!(parse_pdbqt(&bad).is_err(), "unexpectedly accepted:\n{bad}");
        }
    }

    #[test]
    fn branch_end_axis_membership_and_nonempty_rules_are_enforced() {
        let root = atom_line(1, [0.0, 0.0, 0.0], 0.0, AdType::C);
        let wrong_child = atom_line(3, [1.5, 0.0, 0.0], 0.0, AdType::C);
        let axis_only = atom_line(2, [1.5, 0.0, 0.0], 0.0, AdType::C);
        let cases = [
            format!("ROOT\n{root}\nENDROOT\nBRANCH 1 2\nENDBRANCH 1 2\nTORSDOF 1\n"),
            format!("ROOT\n{root}\nENDROOT\nBRANCH 1 2\n{wrong_child}\nENDBRANCH 1 2\nTORSDOF 1\n"),
            format!(
                "ROOT\n{root}\nENDROOT\nBRANCH 1 2\n{}\nENDBRANCH 1 3\nTORSDOF 1\n",
                atom_line(2, [1.5, 0.0, 0.0], 0.0, AdType::C)
            ),
        ];
        for text in cases {
            assert!(
                parse_pdbqt(&text).is_err(),
                "unexpectedly accepted:\n{text}"
            );
        }

        let disabled = parse_pdbqt(&format!(
            "ROOT\n{root}\nENDROOT\nBRANCH 1 2\n{axis_only}\nENDBRANCH 1 2\nTORSDOF 1\n"
        ))
        .unwrap();
        assert!(!branch_is_active(&disabled, 0));
        assert_eq!(
            parse_pdbqt(&write_pdbqt(&disabled).unwrap()).unwrap(),
            disabled
        );

        let nested_axis = atom_line(3, [3.0, 0.0, 0.0], 0.0, AdType::C);
        let chain = parse_pdbqt(&format!(
            "ROOT\n{root}\nENDROOT\nBRANCH 1 2\n{axis_only}\nBRANCH 2 3\n{nested_axis}\nENDBRANCH 2 3\nENDBRANCH 1 2\nTORSDOF 2\n"
        ))
        .unwrap();
        assert!(branch_is_active(&chain, 0));
        assert!(!branch_is_active(&chain, 1));
    }

    #[test]
    fn fixed_charge_and_type_columns_are_case_sensitive() {
        let sodium = atom_line(1, [0.0, 0.0, 0.0], 0.0, AdType::Sodium);
        let parsed = parse_pdbqt(&format!("ROOT\n{sodium}\nENDROOT\nTORSDOF 0\n")).unwrap();
        assert_eq!(parsed.atoms[0].ad_type, AdType::Sodium);
        assert_eq!(&sodium[77..79].trim(), &"Na");

        let blank_charge = format!("{}{}{}", &sodium[..68], "        ", &sodium[76..]);
        let parsed = parse_pdbqt(&format!("ROOT\n{blank_charge}\nENDROOT\nTORSDOF 0\n")).unwrap();
        assert_eq!(parsed.atoms[0].charge, 0.0);

        let bad_case = sodium.replace(" Na", " NA");
        let parsed = parse_pdbqt(&format!("ROOT\n{bad_case}\nENDROOT\nTORSDOF 0\n")).unwrap();
        assert_eq!(parsed.atoms[0].ad_type, AdType::Na);
        assert_ne!(parsed.atoms[0].ad_type, AdType::Sodium);

        let extra = format!("{sodium} unexpected");
        assert!(parse_pdbqt(&format!("ROOT\n{extra}\nENDROOT\nTORSDOF 0\n"))
            .unwrap_err()
            .contains("extra fields"));
    }

    #[test]
    fn duplicate_serial_unknown_type_and_nonfinite_numbers_fail_closed() {
        let atom = atom_line(1, [0.0, 0.0, 0.0], 0.0, AdType::C);
        let duplicate = format!("ROOT\n{atom}\n{atom}\nENDROOT\nTORSDOF 0\n");
        assert!(parse_pdbqt(&duplicate).is_err());

        let mut unknown = atom.clone();
        unknown.replace_range(unknown.len() - 1.., "X");
        assert!(parse_pdbqt(&format!("ROOT\n{unknown}\nENDROOT\nTORSDOF 0\n")).is_err());

        let nan_charge = atom.replace(" 0.000 C", "   NaN C");
        assert!(parse_pdbqt(&format!("ROOT\n{nan_charge}\nENDROOT\nTORSDOF 0\n")).is_err());
    }

    #[test]
    fn multi_model_coordinates_may_change_but_identity_and_topology_may_not() {
        let first = branched_body();
        let second = first.replace("   0.000   0.000   0.000", "   0.250   0.000   0.000");
        let text = format!("MODEL 1\n{first}ENDMDL\nMODEL 2\n{second}ENDMDL\n");
        let models = parse_pdbqt_models(&text).unwrap();
        assert_eq!(models.len(), 2);
        assert_ne!(
            models[0].atoms[0].raw.position,
            models[1].atoms[0].raw.position
        );

        let changed_type = second.replacen(" 0.100 C", " 0.100 A", 1);
        let bad = format!("MODEL 1\n{first}ENDMDL\nMODEL 2\n{changed_type}ENDMDL\n");
        assert!(parse_pdbqt_models(&bad).is_err());

        let changed_topology = second.replace("TORSDOF 9", "TORSDOF 8");
        let bad = format!("MODEL 1\n{first}ENDMDL\nMODEL 2\n{changed_topology}ENDMDL\n");
        assert!(parse_pdbqt_models(&bad).is_err());
    }

    #[test]
    fn branch_serials_keep_an_explicit_separator_at_four_and_five_digits() {
        for (parent, child, body) in [(999, 1000, 1001), (99998, 99999, 99997)] {
            let root = atom_line(parent, [0.0, 0.0, 0.0], 0.0, AdType::C);
            let axis = atom_line(child, [1.5, 0.0, 0.0], 0.0, AdType::C);
            let body_atom = atom_line(body, [3.0, 0.0, 0.0], 0.0, AdType::C);
            let parsed = parse_pdbqt(&format!(
                "ROOT\n{root}\nENDROOT\nBRANCH {parent} {child}\n{axis}\n{body_atom}\nENDBRANCH {parent} {child}\nTORSDOF 1\n"
            ))
            .unwrap();
            let written = write_pdbqt(&parsed).unwrap();
            assert!(written.contains(&format!("BRANCH {parent:>4} {child:>4}")));
            assert_eq!(parse_pdbqt(&written).unwrap(), parsed);
        }
    }

    #[test]
    fn writer_rejects_incomplete_topology_instead_of_falling_back() {
        let mut ligand = parse_pdbqt(&branched_body()).unwrap();
        ligand.atom_branches.pop();
        assert!(write_pdbqt(&ligand).is_err());

        let mut ligand = parse_pdbqt(&branched_body()).unwrap();
        ligand
            .topology
            .retain(|event| *event != TopologyEvent::BranchEnd(1));
        assert!(write_pdbqt(&ligand).is_err());
    }
}
