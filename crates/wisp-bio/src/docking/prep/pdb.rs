//! Minimal PDB ATOM/HETATM parsing for docking prep.
//!
//! Fixed-column parsing with no external dependency. The element comes from
//! columns 77-78 when present and falls back to the atom-name convention
//! (two-letter elements occupy columns 13-14, one-letter elements start at
//! column 14), which is what every structure file in practice relies on.

use super::atom_type::Element;

/// One parsed ATOM/HETATM record.
#[derive(Debug, Clone, PartialEq)]
pub struct RawAtom {
    pub serial: i32,
    pub name: String,
    pub element: Element,
    pub position: [f64; 3],
    pub residue_name: String,
    pub chain: char,
    pub residue_seq: i32,
    pub insertion_code: char,
    pub is_hetero: bool,
}

/// Parse every ATOM/HETATM record of a PDB file body.
///
/// Alternate locations other than the first (`A` or blank) are dropped so a
/// residue contributes one position per atom.
pub fn parse_pdb_atoms(text: &str) -> Result<Vec<RawAtom>, String> {
    let mut atoms = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let record = slice(line, 0, 6);
        if record != "ATOM" && record != "HETATM" {
            continue;
        }
        let altloc = char_at(line, 16);
        if altloc != ' ' && altloc != 'A' {
            continue;
        }
        let serial = slice(line, 6, 11)
            .trim()
            .parse::<i32>()
            .map_err(|_| format!("line {}: bad atom serial", number + 1))?;
        let name = line.get(12..16).unwrap_or("    ").to_string();
        let element_column = slice(line, 76, 78).trim().to_string();
        let element = if element_column.is_empty() {
            element_from_name(&name)
        } else {
            Element::from_symbol(&element_column)
        };
        let position = [
            coordinate(line, 30, 38, number + 1)?,
            coordinate(line, 38, 46, number + 1)?,
            coordinate(line, 46, 54, number + 1)?,
        ];
        atoms.push(RawAtom {
            serial,
            name,
            element,
            position,
            residue_name: slice(line, 17, 20).trim().to_string(),
            chain: char_at(line, 21),
            residue_seq: slice(line, 22, 26)
                .trim()
                .parse::<i32>()
                .map_err(|_| format!("line {}: bad residue sequence", number + 1))?,
            insertion_code: char_at(line, 26),
            is_hetero: record == "HETATM",
        });
    }
    Ok(atoms)
}

/// Element from the atom-name columns following the PDB convention.
fn element_from_name(name: &str) -> Element {
    let bytes: Vec<char> = name.chars().collect();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1].is_ascii_alphabetic() {
        // Two-letter element: "BR  ", "ZN  ", ... Calcium collides with the
        // alpha-carbon name, but the alpha carbon is " CA " with a leading
        // blank, so this branch only fires for genuine two-letter symbols.
        Element::from_symbol(&name[..2])
    } else {
        let first = bytes
            .iter()
            .find(|ch| ch.is_ascii_alphabetic())
            .copied()
            .unwrap_or(' ');
        Element::from_symbol(&first.to_string())
    }
}

fn coordinate(line: &str, start: usize, end: usize, number: usize) -> Result<f64, String> {
    slice(line, start, end)
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("line {}: bad coordinate", number))
}

fn slice(line: &str, start: usize, end: usize) -> &str {
    line.get(start..end).map(str::trim_end).unwrap_or("")
}

fn char_at(line: &str, index: usize) -> char {
    line.get(index..index + 1)
        .and_then(|text| text.chars().next())
        .unwrap_or(' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALPHA_CARBON: &str =
        "ATOM      1  CA  ALA A   1       1.000   2.000   3.000  1.00  0.00           C";
    const ZINC: &str =
        "HETATM    5 ZN    ZN A 300       4.000   5.000   6.000  1.00  0.00          ZN";

    #[test]
    fn parses_coordinates_residue_and_element_columns() {
        let atoms = parse_pdb_atoms(&(ALPHA_CARBON.to_string() + "\n" + ZINC + "\n")).unwrap();
        assert_eq!(atoms.len(), 2);
        assert_eq!(atoms[0].name, " CA ");
        assert_eq!(atoms[0].element, Element::C);
        assert_eq!(atoms[0].position, [1.0, 2.0, 3.0]);
        assert_eq!(atoms[0].residue_name, "ALA");
        assert_eq!(atoms[0].chain, 'A');
        assert!(!atoms[0].is_hetero);
        assert_eq!(atoms[1].element, Element::Zn);
        assert!(atoms[1].is_hetero);
        assert_eq!(atoms[1].position, [4.0, 5.0, 6.0]);
    }

    #[test]
    fn element_falls_back_to_the_name_convention() {
        // Two-letter elements start their name at column 13.
        let no_element = "ATOM      2 BR   LIG B   7       0.000   0.000   0.000  1.00  0.00";
        let atoms = parse_pdb_atoms(no_element).unwrap();
        assert_eq!(atoms[0].element, Element::Br);
        let methyl_h = "ATOM      3 1HB  ALA A   1       0.000   0.000   0.000  1.00  0.00";
        let atoms = parse_pdb_atoms(methyl_h).unwrap();
        assert_eq!(atoms[0].element, Element::H);
        let alpha = "ATOM      4  CA  ALA A   1       0.000   0.000   0.000  1.00  0.00";
        let atoms = parse_pdb_atoms(alpha).unwrap();
        assert_eq!(atoms[0].element, Element::C);
    }

    #[test]
    fn alternate_locations_keep_only_the_first() {
        let mut altloc_b = String::from(
            "ATOM      1  CA  ALA A   1       9.000   9.000   9.000  0.50  0.00           C",
        );
        altloc_b.replace_range(16..17, "B");
        let text = ALPHA_CARBON.to_string() + "\n" + &altloc_b + "\n";
        let atoms = parse_pdb_atoms(&text).unwrap();
        assert_eq!(atoms.len(), 1);
        assert_eq!(atoms[0].position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn non_atom_records_are_ignored() {
        let text = "REMARK   1 nothing here\nTITLE    demo\nEND\n";
        assert!(parse_pdb_atoms(text).unwrap().is_empty());
    }
}
