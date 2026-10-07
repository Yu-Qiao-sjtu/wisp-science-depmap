//! Rigid-body docking that writes one complex PDB per sample.
//!
//! The ligand starts away from the receptor and each sample moves it closer
//! while it spins. Every sample is a complete structure file so the viewer can
//! replace the current frame. This is geometric placement, not an affinity model.

use std::fs;
use std::path::{Path, PathBuf};

const MAX_SAMPLES: usize = 24;

#[derive(Clone)]
struct Atom {
    line: String,
    x: f64,
    y: f64,
    z: f64,
}

pub fn write_docking_frames(
    receptor_path: &Path,
    ligand_path: &Path,
    output_dir: &Path,
    samples: usize,
) -> Result<Vec<PathBuf>, String> {
    let samples = samples.clamp(1, MAX_SAMPLES);
    let receptor = read_pdb(receptor_path)?;
    let ligand = read_pdb(ligand_path)?;
    if receptor.is_empty() {
        return Err("receptor has no ATOM or HETATM records".into());
    }
    if ligand.is_empty() {
        return Err("ligand has no ATOM or HETATM records".into());
    }
    fs::create_dir_all(output_dir).map_err(|error| error.to_string())?;
    let receptor_text = fs::read_to_string(receptor_path).map_err(|error| error.to_string())?;
    let receptor_centroid = centroid(&receptor);
    let ligand_centroid = centroid(&ligand);
    let mut written = Vec::with_capacity(samples);
    for index in 0..samples {
        let placed = place_ligand(&ligand, ligand_centroid, receptor_centroid, index, samples);
        let mut body = receptor_body(&receptor_text);
        for atom in &placed {
            body.push(as_hetatm(rewrite_coords(
                &atom.line, atom.x, atom.y, atom.z,
            )));
        }
        body.push("END".into());
        let path = output_dir.join(format!("pose-{:03}.pdb", index + 1));
        fs::write(&path, body.join("\n") + "\n").map_err(|error| error.to_string())?;
        written.push(path);
    }
    Ok(written)
}

fn place_ligand(
    ligand: &[Atom],
    ligand_centroid: [f64; 3],
    receptor_centroid: [f64; 3],
    index: usize,
    samples: usize,
) -> Vec<Atom> {
    let progress = if samples <= 1 {
        1.0
    } else {
        index as f64 / (samples - 1) as f64
    };
    let radius = 14.0 - 10.0 * progress;
    let angle = index as f64 * 2.399963229728653;
    let offset = [
        radius * angle.cos(),
        radius * 0.35 * angle.sin(),
        radius * 0.15 * (index as f64 * 0.7).sin(),
    ];
    let spin = progress * std::f64::consts::PI;
    ligand
        .iter()
        .map(|atom| {
            let relative = [
                atom.x - ligand_centroid[0],
                atom.y - ligand_centroid[1],
                atom.z - ligand_centroid[2],
            ];
            let rotated = rotate_z(relative, spin);
            Atom {
                line: atom.line.clone(),
                x: receptor_centroid[0] + offset[0] + rotated[0],
                y: receptor_centroid[1] + offset[1] + rotated[1],
                z: receptor_centroid[2] + offset[2] + rotated[2],
            }
        })
        .collect()
}

fn rotate_z(point: [f64; 3], angle: f64) -> [f64; 3] {
    let (sin, cos) = angle.sin_cos();
    [
        point[0] * cos - point[1] * sin,
        point[0] * sin + point[1] * cos,
        point[2],
    ]
}

fn receptor_body(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.starts_with("END"))
        .map(str::to_string)
        .collect()
}

fn read_pdb(path: &Path) -> Result<Vec<Atom>, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let atoms = text.lines().filter_map(parse_atom).collect();
    Ok(atoms)
}

fn parse_atom(line: &str) -> Option<Atom> {
    if !(line.starts_with("ATOM") || line.starts_with("HETATM")) {
        return None;
    }
    Some(Atom {
        line: line.to_string(),
        x: coord(line, 30)?,
        y: coord(line, 38)?,
        z: coord(line, 46)?,
    })
}

fn coord(line: &str, start: usize) -> Option<f64> {
    line.get(start..start + 8)?.trim().parse().ok()
}

fn centroid(atoms: &[Atom]) -> [f64; 3] {
    let count = atoms.len() as f64;
    let sum = atoms.iter().fold([0.0, 0.0, 0.0], |acc, atom| {
        [acc[0] + atom.x, acc[1] + atom.y, acc[2] + atom.z]
    });
    [sum[0] / count, sum[1] / count, sum[2] / count]
}

fn rewrite_coords(line: &str, x: f64, y: f64, z: f64) -> String {
    let mut chars: Vec<char> = format!("{line:<54}").chars().collect();
    let coords: Vec<char> = format!("{x:8.3}{y:8.3}{z:8.3}").chars().collect();
    if chars.len() < 54 {
        chars.resize(54, ' ');
    }
    chars.splice(30..54, coords);
    let mut rewritten: String = chars.into_iter().collect();
    if line.chars().count() > 54 {
        rewritten.push_str(&line.chars().skip(54).collect::<String>());
    }
    rewritten
}

fn as_hetatm(line: String) -> String {
    if line.starts_with("ATOM") {
        let mut chars: Vec<char> = line.chars().collect();
        for (index, ch) in "HETATM".chars().enumerate() {
            if index < chars.len() {
                chars[index] = ch;
            }
        }
        chars.into_iter().collect()
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(x: f64, y: f64, z: f64) -> String {
        rewrite_coords(
            "ATOM      1  CA  ALA A   1       0.000   0.000   0.000  1.00  0.00           C",
            x,
            y,
            z,
        )
    }

    #[test]
    fn each_sample_is_a_new_pose_and_the_ligand_moves_closer() {
        let root = std::env::temp_dir().join(format!("wisp-dock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let receptor = root.join("receptor.pdb");
        let ligand = root.join("ligand.pdb");
        fs::write(&receptor, atom(0.0, 0.0, 0.0) + "\nEND\n").unwrap();
        fs::write(&ligand, atom(30.0, 0.0, 0.0) + "\nEND\n").unwrap();
        let frames = write_docking_frames(&receptor, &ligand, &root.join("poses"), 4).unwrap();
        assert_eq!(frames.len(), 4);
        let first = fs::read_to_string(&frames[0]).unwrap();
        let last = fs::read_to_string(&frames[3]).unwrap();
        assert!(first.contains("HETATM"));
        assert_ne!(
            first.lines().find(|line| line.starts_with("HETATM")),
            last.lines().find(|line| line.starts_with("HETATM"))
        );
        let _ = fs::remove_dir_all(&root);
    }
}
