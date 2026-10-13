//! Receptor cell-list and intramolecular pair precalculation (#273).
//!
//! The direct evaluation walks every receptor-ligand pair. This module
//! keeps the same arithmetic but accelerates the enumeration:
//!
//! * [`ReceptorGrid`] buckets receptor atoms into 8 A cells (the pair
//!   cutoff), so each ligand atom only meets the atoms that can interact.
//! * [`IntraPairs`] resolves the 1-2/1-3 exclusions once per ligand
//!   instead of once per energy evaluation.
//!
//! Both paths reproduce the direct evaluation bit-exactly: candidate
//! pairs are re-sorted into the direct `(receptor, ligand)` and `(i, j)`
//! enumeration order before the same `pair_energy` calls accumulate.

use crate::docking::prep::{DockAtom, PreparedLigand, PreparedReceptor};
use crate::docking::scoring::vina::{pair_energy, PairBreakdown, VinaParams, INTER_CUTOFF};

/// Cell edge length: equal to the pair cutoff, so a pair closer than the
/// cutoff always sits in adjacent cells.
const CELL_SIZE: f64 = INTER_CUTOFF;

/// Receptor atoms bucketed into a uniform grid.
#[derive(Debug, Clone)]
pub struct ReceptorGrid {
    atoms: Vec<DockAtom>,
    origin: [f64; 3],
    dims: [usize; 3],
    /// Flat `dims[0] * dims[1] * dims[2]` array of receptor atom indices.
    cells: Vec<Vec<usize>>,
}

impl ReceptorGrid {
    pub fn build(receptor: &PreparedReceptor) -> ReceptorGrid {
        let mut min = [f64::MAX; 3];
        let mut max = [f64::MIN; 3];
        for atom in &receptor.atoms {
            for axis in 0..3 {
                min[axis] = min[axis].min(atom.position[axis]);
                max[axis] = max[axis].max(atom.position[axis]);
            }
        }
        // Guard the empty receptor.
        if receptor.atoms.is_empty() {
            return ReceptorGrid {
                atoms: Vec::new(),
                origin: [0.0; 3],
                dims: [1, 1, 1],
                cells: vec![Vec::new()],
            };
        }
        let origin = [min[0] - 1.0, min[1] - 1.0, min[2] - 1.0];
        let dims = [
            ((max[0] - origin[0]) / CELL_SIZE).ceil() as usize + 1,
            ((max[1] - origin[1]) / CELL_SIZE).ceil() as usize + 1,
            ((max[2] - origin[2]) / CELL_SIZE).ceil() as usize + 1,
        ];
        let mut cells: Vec<Vec<usize>> = vec![Vec::new(); dims[0] * dims[1] * dims[2]];
        for (index, atom) in receptor.atoms.iter().enumerate() {
            let cell = cell_of(&origin, &dims, atom.position);
            cells[flat(&dims, cell[0], cell[1], cell[2])].push(index);
        }
        ReceptorGrid {
            atoms: receptor.atoms.clone(),
            origin,
            dims,
            cells,
        }
    }

    /// Intermolecular energy identical to the direct evaluation.
    pub fn evaluate_inter(
        &self,
        ligand: &PreparedLigand,
        ligand_positions: &[[f64; 3]],
        params: &VinaParams,
    ) -> PairBreakdown {
        let mut candidates: Vec<(usize, usize)> = Vec::new();
        for (ligand_index, position) in ligand_positions.iter().enumerate() {
            let [cx, cy, cz] = cell_of(&self.origin, &self.dims, *position);
            for x in cx.saturating_sub(1)..=(cx + 1).min(self.dims[0] - 1) {
                for y in cy.saturating_sub(1)..=(cy + 1).min(self.dims[1] - 1) {
                    for z in cz.saturating_sub(1)..=(cz + 1).min(self.dims[2] - 1) {
                        for &receptor_index in &self.cells[flat(&self.dims, x, y, z)] {
                            candidates.push((receptor_index, ligand_index));
                        }
                    }
                }
            }
        }
        // Restore the direct enumeration order (receptor-major, then ligand)
        // so the float sums are bit-identical.
        candidates.sort_unstable();
        let mut total = PairBreakdown::default();
        for (receptor_index, ligand_index) in candidates {
            let receptor_atom = &self.atoms[receptor_index];
            let ligand_atom = &ligand.atoms[ligand_index];
            let dx = receptor_atom.position[0] - ligand_positions[ligand_index][0];
            let dy = receptor_atom.position[1] - ligand_positions[ligand_index][1];
            let dz = receptor_atom.position[2] - ligand_positions[ligand_index][2];
            let distance = (dx * dx + dy * dy + dz * dz).sqrt();
            total.absorb(pair_energy(receptor_atom, ligand_atom, distance, params));
        }
        total
    }
}

fn cell_of(origin: &[f64; 3], dims: &[usize; 3], position: [f64; 3]) -> [usize; 3] {
    let clamp_axis = |axis: usize| {
        (((position[axis] - origin[axis]) / CELL_SIZE).floor() as usize).min(dims[axis] - 1)
    };
    [clamp_axis(0), clamp_axis(1), clamp_axis(2)]
}

fn flat(dims: &[usize; 3], x: usize, y: usize, z: usize) -> usize {
    (x * dims[1] + y) * dims[2] + z
}

/// The intramolecular pair list of one ligand with exclusions resolved.
#[derive(Debug, Clone)]
pub struct IntraPairs {
    pairs: Vec<(usize, usize)>,
}

impl IntraPairs {
    pub fn build(ligand: &PreparedLigand) -> IntraPairs {
        let excluded: std::collections::HashSet<(usize, usize)> =
            ligand.intra_excluded.iter().copied().collect();
        let mut pairs = Vec::new();
        for i in 0..ligand.atoms.len() {
            for j in (i + 1)..ligand.atoms.len() {
                if !excluded.contains(&(i, j)) {
                    pairs.push((i, j));
                }
            }
        }
        IntraPairs { pairs }
    }

    /// Intramolecular energy identical to the direct evaluation.
    pub fn evaluate(
        &self,
        ligand: &PreparedLigand,
        ligand_positions: &[[f64; 3]],
        params: &VinaParams,
    ) -> PairBreakdown {
        let mut total = PairBreakdown::default();
        for &(i, j) in &self.pairs {
            let dx = ligand_positions[i][0] - ligand_positions[j][0];
            let dy = ligand_positions[i][1] - ligand_positions[j][1];
            let dz = ligand_positions[i][2] - ligand_positions[j][2];
            let distance = (dx * dx + dy * dy + dz * dz).sqrt();
            total.absorb(pair_energy(
                &ligand.atoms[i],
                &ligand.atoms[j],
                distance,
                params,
            ));
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docking::prep::{prepare_ligand_pdb, prepare_receptor};
    use crate::docking::scoring::{evaluate_inter, evaluate_intra, VINA_1_1};
    use std::time::Instant;

    fn lattice_receptor(repeat: usize) -> PreparedReceptor {
        let mut body = String::new();
        let mut serial = 0;
        for x in 0..repeat {
            for y in 0..repeat {
                for z in 0..repeat {
                    serial += 1;
                    // Same column layout as the PDBQT writer: coordinates
                    // right-aligned in 8-character fields from column 31.
                    body.push_str(&format!(
                        "ATOM  {serial:5}  CA  GLY A   1    {x:8.3}{y:8.3}{z:8.3}  1.00  0.00           C\n",
                        x = x as f64 * 3.8,
                        y = y as f64 * 3.8,
                        z = z as f64 * 3.8,
                    ));
                }
            }
        }
        prepare_receptor(&body).unwrap()
    }

    #[test]
    fn grid_inter_matches_direct_evaluation_bit_exactly() {
        let receptor = lattice_receptor(5); // 125 atoms spread over 19 A
        let ligand = prepare_ligand_pdb(
            "HETATM    1  C   LIG L   1       7.100   7.700   3.900  1.00  0.00           C\n\
             HETATM    2  C   LIG L   2       8.500   7.700   3.900  1.00  0.00           C\n",
        )
        .unwrap();
        let positions: Vec<[f64; 3]> = ligand.atoms.iter().map(|a| a.position).collect();
        let grid = ReceptorGrid::build(&receptor);
        let direct = evaluate_inter(&receptor, &ligand, &positions, &VINA_1_1);
        let accelerated = grid.evaluate_inter(&ligand, &positions, &VINA_1_1);
        assert_eq!(direct, accelerated);
    }

    #[test]
    fn intra_pairs_match_direct_evaluation_bit_exactly() {
        let ligand = prepare_ligand_pdb(
            "HETATM    1  C   LIG L   1       0.000   0.000   0.000  1.00  0.00           C\n\
             HETATM    2  C   LIG L   2       1.500   0.000   0.000  1.00  0.00           C\n\
             HETATM    3  C   LIG L   3       3.000   0.000   0.000  1.00  0.00           C\n\
             HETATM    4  O   LIG L   4       4.100   1.000   0.000  1.00  0.00           O\n",
        )
        .unwrap();
        let positions: Vec<[f64; 3]> = ligand.atoms.iter().map(|a| a.position).collect();
        let pairs = IntraPairs::build(&ligand);
        let direct = evaluate_intra(&ligand, &positions, &VINA_1_1);
        let accelerated = pairs.evaluate(&ligand, &positions, &VINA_1_1);
        assert_eq!(direct, accelerated);
    }

    #[test]
    fn grid_beats_direct_walk_on_a_large_receptor() {
        // 8000 atoms, evaluated repeatedly: the cell list must amortize.
        let receptor = lattice_receptor(20);
        let ligand = prepare_ligand_pdb(
            "HETATM    1  C   LIG L   1      38.000  38.000  38.000  1.00  0.00           C\n",
        )
        .unwrap();
        let positions: Vec<[f64; 3]> = ligand.atoms.iter().map(|a| a.position).collect();
        let grid = ReceptorGrid::build(&receptor);
        let repetitions = 50;
        let start = Instant::now();
        for _ in 0..repetitions {
            let _ = evaluate_inter(&receptor, &ligand, &positions, &VINA_1_1);
        }
        let direct_elapsed = start.elapsed();
        let start = Instant::now();
        for _ in 0..repetitions {
            let _ = grid.evaluate_inter(&ligand, &positions, &VINA_1_1);
        }
        let grid_elapsed = start.elapsed();
        // Orders of magnitude on paper; require a clear win to keep the test
        // robust on noisy CI runners.
        assert!(
            grid_elapsed < direct_elapsed,
            "grid {grid_elapsed:?} not faster than direct {direct_elapsed:?}"
        );
    }
}
