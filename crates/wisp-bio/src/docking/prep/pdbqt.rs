//! PDBQT parsing and writing.
//!
//! PDBQT is the PDB atom record extended with a Gasteiger charge in columns
//! 71-76 and the AutoDock atom type in columns 78-79, plus torsion-tree
//! records (`ROOT`, `BRANCH`, `TORSDOF`) and `MODEL` blocks for multi-pose
//! output. The parser trusts the file's atom types; the writer emits the
//! charge and type columns so files round-trip through other docking tools.

use super::atom_type::{AtomType, Element};
use super::pdb::RawAtom;

/// One PDBQT atom record: a PDB atom plus charge and type.
#[derive(Debug, Clone, PartialEq)]
pub struct PdbqtAtom {
    pub raw: RawAtom,
    pub charge: f64,
    pub ad_type: AtomType,
}

/// A parsed ligand: atoms plus the torsion-tree bookkeeping.
#[derive(Debug, Clone, PartialEq)]
pub struct PdbqtLigand {
    pub atoms: Vec<PdbqtAtom>,
    /// Serial pairs from BRANCH records, in file order.
    pub branches: Vec<(i32, i32)>,
    /// Parent branch index for each branch (`None` means directly under ROOT).
    pub branch_parents: Vec<Option<usize>>,
    /// Enclosing branch index for each atom (`None` means the atom is in ROOT).
    pub atom_branches: Vec<Option<usize>>,
    pub torsdof: u32,
}

/// Parse a single-model PDBQT body.
pub fn parse_pdbqt(text: &str) -> Result<PdbqtLigand, String> {
    let mut models = parse_pdbqt_models(text)?;
    match models.len() {
        0 => Err("no ATOM or HETATM records in PDBQT".into()),
        _ => Ok(models.remove(0)),
    }
}

/// Parse every MODEL block; a file without MODEL records is one model.
/// Each model retains the nesting and atom membership of its torsion tree.
pub fn parse_pdbqt_models(text: &str) -> Result<Vec<PdbqtLigand>, String> {
    split_on_model_records(text)
}

/// The leading record keyword of a PDBQT line ("BRANCH", "TORSDOF", ...).
fn record_keyword(line: &str) -> &str {
    line.split_ascii_whitespace().next().unwrap_or("")
}

/// Re-parse keeping MODEL boundaries: atoms between MODEL/ENDMDL pairs.
fn split_on_model_records(text: &str) -> Result<Vec<PdbqtLigand>, String> {
    let empty_model = || PdbqtLigand {
        atoms: Vec::new(),
        branches: Vec::new(),
        branch_parents: Vec::new(),
        atom_branches: Vec::new(),
        torsdof: 0,
    };
    let mut models = Vec::new();
    let mut current: Option<PdbqtLigand> = None;
    let mut branch_stack: Vec<usize> = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let report = |message: &str| format!("line {}: {}", number + 1, message);
        match record_keyword(line) {
            "MODEL" => {
                if current.is_some() {
                    return Err(report("nested MODEL record"));
                }
                branch_stack.clear();
                current = Some(empty_model());
            }
            "ENDMDL" => {
                if !branch_stack.is_empty() {
                    return Err(report("unclosed BRANCH before ENDMDL"));
                }
                if let Some(model) = current.take() {
                    if !model.atoms.is_empty() {
                        models.push(model);
                    }
                }
            }
            "BRANCH" => {
                let mut parts = line.split_whitespace();
                let _ = parts.next();
                let a = parts
                    .next()
                    .and_then(|token| token.parse().ok())
                    .ok_or_else(|| report("bad BRANCH serial"))?;
                let b = parts
                    .next()
                    .and_then(|token| token.parse().ok())
                    .ok_or_else(|| report("bad BRANCH serial"))?;
                let model = current.get_or_insert_with(&empty_model);
                let branch = model.branches.len();
                model.branches.push((a, b));
                model.branch_parents.push(branch_stack.last().copied());
                branch_stack.push(branch);
            }
            "ENDBRANCH" => {
                branch_stack
                    .pop()
                    .ok_or_else(|| report("ENDBRANCH without BRANCH"))?;
            }
            "TORSDOF" => {
                let value = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|token| token.parse().ok())
                    .ok_or_else(|| report("bad TORSDOF value"))?;
                current.get_or_insert_with(&empty_model).torsdof = value;
            }
            "ATOM" | "HETATM" => {
                let atom = parse_atom_line(line).map_err(|message| report(&message))?;
                let model = current.get_or_insert_with(&empty_model);
                model.atoms.push(atom);
                model.atom_branches.push(branch_stack.last().copied());
            }
            _ => {}
        }
    }
    if !branch_stack.is_empty() {
        return Err("unclosed BRANCH at end of PDBQT".into());
    }
    if let Some(model) = current {
        if !model.atoms.is_empty() {
            models.push(model);
        }
    }
    Ok(models)
}

fn parse_atom_line(line: &str) -> Result<PdbqtAtom, String> {
    let raw = parse_raw_atom(line)?;
    // The trailing region holds the charge (cols 71-76) and the type
    // (cols 78-79); accept either right- or left-justified layouts.
    let tail = line.get(70..).map(str::trim).unwrap_or("").to_string();
    let mut tokens = tail.split_ascii_whitespace();
    let (charge, ad_type) = match (tokens.next(), tokens.next()) {
        (Some(first), Some(second)) => {
            let charge = first.parse::<f64>().map_err(|_| "bad charge column")?;
            (charge, AtomType::from_pdbqt_symbol(second))
        }
        (Some(first), None) => match first.parse::<f64>() {
            Ok(charge) => (charge, AtomType::Unknown),
            Err(_) => (0.0, AtomType::from_pdbqt_symbol(first)),
        },
        (None, _) => (0.0, AtomType::Unknown),
    };
    // The type field is more specific than the element column (which it
    // overlaps), so a recognized type decides the element outright.
    let element = if ad_type != AtomType::Unknown {
        ad_type.element()
    } else if raw.element != Element::Other {
        raw.element
    } else {
        Element::Other
    };
    Ok(PdbqtAtom {
        raw: RawAtom { element, ..raw },
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
        return Err("alternate location dropped".into());
    }
    Ok(RawAtom {
        serial: column(line, 6, 11)?.parse().map_err(|_| "bad serial")?,
        name: line.get(12..16).unwrap_or("    ").to_string(),
        element: line
            .get(76..78)
            .map(|text| Element::from_symbol(text.trim()))
            .filter(|element| *element != Element::Other)
            .unwrap_or(Element::Other),
        position: [
            coordinate(line, 30, 38)?,
            coordinate(line, 38, 46)?,
            coordinate(line, 46, 54)?,
        ],
        residue_name: line.get(17..20).unwrap_or("   ").trim().to_string(),
        chain: line
            .get(21..22)
            .and_then(|text| text.chars().next())
            .unwrap_or(' '),
        residue_seq: column(line, 22, 26)?.parse().map_err(|_| "bad residue")?,
        insertion_code: line
            .get(26..27)
            .and_then(|text| text.chars().next())
            .unwrap_or(' '),
        is_hetero: line.starts_with("HETATM"),
    })
}

fn column(line: &str, start: usize, end: usize) -> Result<&str, String> {
    let text = line
        .get(start..end)
        .map(str::trim)
        .ok_or_else(|| "short line".to_string())?;
    Ok(text)
}

fn coordinate(line: &str, start: usize, end: usize) -> Result<f64, String> {
    column(line, start, end)?
        .parse()
        .map_err(|_| "bad coordinate".to_string())
}

/// Format one PDBQT atom line.
pub fn format_atom_line(atom: &PdbqtAtom) -> String {
    let raw = &atom.raw;
    let record = if raw.is_hetero { "HETATM" } else { "ATOM  " };
    format!(
        "{record}{serial:5} {name:<4}{altloc}{residue:>3} {chain}{residue_seq:4}{icode}   {x:8.3}{y:8.3}{z:8.3}{occupancy:6.2}{bfactor:6.2}    {charge:6.3} {type:<2}",
        serial = raw.serial,
        name = raw.name.trim_end(),
        altloc = ' ',
        residue = raw.residue_name,
        chain = raw.chain,
        residue_seq = raw.residue_seq,
        icode = raw.insertion_code,
        x = raw.position[0],
        y = raw.position[1],
        z = raw.position[2],
        occupancy = 1.00,
        bfactor = 0.00,
        charge = atom.charge,
        type = atom.ad_type.pdbqt_symbol(),
    )
}

/// Serialize a ligand as a single-model PDBQT body.
pub fn write_pdbqt(ligand: &PdbqtLigand) -> String {
    fn write_branch(body: &mut String, ligand: &PdbqtLigand, branch: usize) {
        let (a, b) = ligand.branches[branch];
        body.push_str(&format!("BRANCH {a:4}{b:4}\n"));
        for (index, atom) in ligand.atoms.iter().enumerate() {
            if ligand.atom_branches.get(index).copied().flatten() == Some(branch) {
                body.push_str(&format_atom_line(atom));
                body.push('\n');
            }
        }
        for child in 0..ligand.branches.len() {
            if ligand.branch_parents.get(child).copied().flatten() == Some(branch) {
                write_branch(body, ligand, child);
            }
        }
        body.push_str(&format!("ENDBRANCH {a:4}{b:4}\n"));
    }

    let has_topology = ligand.atom_branches.len() == ligand.atoms.len()
        && ligand.branch_parents.len() == ligand.branches.len();
    let mut body = String::new();
    body.push_str("ROOT\n");
    for (index, atom) in ligand.atoms.iter().enumerate() {
        if !has_topology || ligand.atom_branches[index].is_none() {
            body.push_str(&format_atom_line(atom));
            body.push('\n');
        }
    }
    body.push_str("ENDROOT\n");
    if has_topology {
        for branch in 0..ligand.branches.len() {
            if ligand.branch_parents[branch].is_none() {
                write_branch(&mut body, ligand, branch);
            }
        }
    }
    body.push_str(&format!("TORSDOF {}\n", ligand.torsdof));
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ligand_line(position: [f64; 3], charge: f64, ad_type: AtomType) -> String {
        let atom = PdbqtAtom {
            raw: RawAtom {
                serial: 1,
                name: " C1 ".into(),
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
        };
        format_atom_line(&atom)
    }

    #[test]
    fn atom_lines_round_trip_charge_type_and_coordinates() {
        let line = ligand_line([1.234, -5.678, 9.012], -0.246, AtomType::Oa);
        let parsed = parse_pdbqt(&(line.clone() + "\nTORSDOF 2\n")).unwrap();
        assert_eq!(parsed.atoms.len(), 1);
        assert_eq!(parsed.torsdof, 2);
        let atom = &parsed.atoms[0];
        assert_eq!(atom.ad_type, AtomType::Oa);
        assert!((atom.charge - (-0.246)).abs() < 5e-4);
        assert!((atom.raw.position[0] - 1.234).abs() < 5e-4);
        assert!((atom.raw.position[2] - 9.012).abs() < 5e-4);
        assert!(atom.raw.is_hetero);
        // Writing the parsed atom reproduces the same line.
        let rewritten = write_pdbqt(&parsed);
        assert!(rewritten.lines().any(|candidate| candidate == line));
    }

    #[test]
    fn model_blocks_split_into_separate_poses() {
        let first = ligand_line([0.0, 0.0, 0.0], 0.1, AtomType::C);
        let second = ligand_line([3.0, 0.0, 0.0], 0.1, AtomType::C);
        let text = format!(
            "MODEL 1\nROOT\n{first}\nENDROOT\nTORSDOF 0\nENDMDL\nMODEL 2\nROOT\n{second}\nENDROOT\nTORSDOF 0\nENDMDL\n"
        );
        let models = parse_pdbqt_models(&text).unwrap();
        assert_eq!(models.len(), 2);
        assert!((models[0].atoms[0].raw.position[0] - 0.0).abs() < 1e-9);
        assert!((models[1].atoms[0].raw.position[0] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn branch_records_parse_into_serial_pairs() {
        let atom = ligand_line([0.0, 0.0, 0.0], 0.0, AtomType::C);
        let text = format!("ROOT\n{atom}\nENDROOT\nBRANCH   1   3\nENDBRANCH   1   3\nTORSDOF 1\n");
        let parsed = parse_pdbqt(&text).unwrap();
        assert_eq!(parsed.branches, vec![(1, 3)]);
        assert_eq!(parsed.torsdof, 1);
    }

    #[test]
    fn nested_branch_atoms_round_trip_inside_their_blocks() {
        let root = ligand_line([0.0, 0.0, 0.0], 0.0, AtomType::C);
        let mut child = ligand_line([1.5, 0.0, 0.0], 0.0, AtomType::C);
        child.replace_range(6..11, "    2");
        let mut grandchild = ligand_line([3.0, 0.0, 0.0], 0.0, AtomType::Oa);
        grandchild.replace_range(6..11, "    3");
        let text = format!(
            "ROOT\n{root}\nENDROOT\nBRANCH   1   2\n{child}\nBRANCH   2   3\n{grandchild}\nENDBRANCH   2   3\nENDBRANCH   1   2\nTORSDOF 2\n"
        );
        let parsed = parse_pdbqt(&text).unwrap();
        assert_eq!(parsed.branch_parents, vec![None, Some(0)]);
        assert_eq!(parsed.atom_branches, vec![None, Some(0), Some(1)]);

        let written = write_pdbqt(&parsed);
        let lines: Vec<&str> = written.lines().collect();
        let branch = lines
            .iter()
            .position(|line| line.starts_with("BRANCH") && line.ends_with("   2"))
            .unwrap();
        let child_atom = lines
            .iter()
            .position(|line| line.starts_with("HETATM    2"))
            .unwrap();
        let nested = lines
            .iter()
            .position(|line| line.starts_with("BRANCH") && line.ends_with("   3"))
            .unwrap();
        let grandchild_atom = lines
            .iter()
            .position(|line| line.starts_with("HETATM    3"))
            .unwrap();
        let nested_end = lines
            .iter()
            .position(|line| line.starts_with("ENDBRANCH") && line.ends_with("   3"))
            .unwrap();
        let branch_end = lines
            .iter()
            .position(|line| line.starts_with("ENDBRANCH") && line.ends_with("   2"))
            .unwrap();
        assert!(branch < child_atom && child_atom < nested);
        assert!(nested < grandchild_atom && grandchild_atom < nested_end);
        assert!(nested_end < branch_end);
        assert_eq!(parse_pdbqt(&written).unwrap(), parsed);
    }

    #[test]
    fn missing_tail_columns_do_not_fail_the_parse() {
        let bare = "ATOM      1  N   LIG A   1       1.000   2.000   3.000  1.00  0.00";
        let parsed = parse_pdbqt(bare).unwrap();
        assert_eq!(parsed.atoms[0].charge, 0.0);
        assert_eq!(parsed.atoms[0].ad_type, AtomType::Unknown);
    }
}
