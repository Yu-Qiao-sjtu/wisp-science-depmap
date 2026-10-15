//! Geometry-only connectivity: covalent bonds, rings, aromatic rings.
//!
//! PDB coordinates carry no bond table, so connectivity is perceived from
//! distances against Cordero-style covalent radii. Rings come from shortest
//! alternate paths around each bond, and aromaticity from ring size, member
//! elements, and geometric planarity — no external chemistry toolkit.

use super::atom_type::{Bond, Element};
use std::collections::HashSet;

/// Slack added to summed covalent radii before two atoms count as bonded.
const BOND_TOLERANCE: f64 = 0.45;
/// Rings larger than this are never treated as aromatic docking rings.
const MAX_AROMATIC_RING_SIZE: usize = 6;
/// Maximum out-of-plane deviation for an aromatic ring.
const RING_PLANARITY_TOLERANCE: f64 = 0.15;

/// Covalent radius in Angstrom (Cordero et al. 2008, docking-relevant subset).
pub fn covalent_radius(element: Element) -> f64 {
    match element {
        Element::H => 0.31,
        Element::B => 0.84,
        Element::C => 0.76,
        Element::N => 0.71,
        Element::O => 0.66,
        Element::F => 0.57,
        Element::Si => 1.11,
        Element::P => 1.07,
        Element::S => 1.05,
        Element::Cl => 1.02,
        Element::Br => 1.20,
        Element::I => 1.39,
        Element::Mg => 1.41,
        Element::Mn => 1.39,
        Element::Zn => 1.22,
        Element::Ca => 1.76,
        Element::Fe => 1.32,
        Element::Other => 1.50,
    }
}

/// Perceive covalent bonds from interatomic distances.
pub fn perceive_bonds(elements: &[Element], positions: &[[f64; 3]]) -> Vec<Bond> {
    let mut bonds = Vec::new();
    for i in 0..elements.len() {
        for j in (i + 1)..elements.len() {
            let limit =
                covalent_radius(elements[i]) + covalent_radius(elements[j]) + BOND_TOLERANCE;
            if distance(positions[i], positions[j]) <= limit {
                bonds.push(Bond { a: i, b: j });
            }
        }
    }
    bonds
}

/// Atom indices that belong to a ring of at most [`MAX_AROMATIC_RING_SIZE`]
/// plus one members (5- and 6-rings for the accepted value).
pub fn ring_atoms(count: usize, bonds: &[Bond]) -> HashSet<usize> {
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); count];
    for bond in bonds {
        neighbors[bond.a].push(bond.b);
        neighbors[bond.b].push(bond.a);
    }
    let mut rings = HashSet::new();
    for bond in bonds {
        // Removing the bond must leave an alternate path to close a ring.
        if let Some(path) = shortest_path(bond.a, bond.b, &neighbors, *bond) {
            if path.len() + 1 <= MAX_AROMATIC_RING_SIZE + 1 {
                rings.insert(bond.a);
                rings.insert(bond.b);
                for node in path {
                    rings.insert(node);
                }
            }
        }
    }
    rings
}

/// BFS shortest path from `from` to `to`, forbidden to use `skip`.
/// Returns the intermediate nodes (empty when directly adjacent).
fn shortest_path(
    from: usize,
    to: usize,
    neighbors: &[Vec<usize>],
    skip: Bond,
) -> Option<Vec<usize>> {
    let mut previous: Vec<Option<usize>> = vec![None; neighbors.len()];
    let mut visited = vec![false; neighbors.len()];
    visited[from] = true;
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(from);
    while let Some(current) = queue.pop_front() {
        for &next in &neighbors[current] {
            let forbidden =
                (current == skip.a && next == skip.b) || (current == skip.b && next == skip.a);
            if forbidden || visited[next] {
                continue;
            }
            visited[next] = true;
            previous[next] = Some(current);
            if next == to {
                let mut path = Vec::new();
                let mut walker = to;
                while walker != from {
                    path.push(walker);
                    walker = previous[walker]?;
                }
                path.reverse();
                path.pop(); // drop `to` itself; keep the intermediate ring members
                return Some(path);
            }
            queue.push_back(next);
        }
    }
    None
}

/// Aromatic ring atoms among the perceived rings.
///
/// A ring is aromatic when every member is C/N/O/S, the heteroatom budget
/// stays small (at most 2 in a 5-ring, 3 in a 6-ring), and the ring is
/// geometrically planar. This accepts benzene, pyridine, imidazole, furan,
/// and thiazole while rejecting proline and sugars by shape.
pub fn aromatic_atoms(
    elements: &[Element],
    positions: &[[f64; 3]],
    bonds: &[Bond],
) -> HashSet<usize> {
    let rings = ring_atoms(elements.len(), bonds);
    if rings.is_empty() {
        return HashSet::new();
    }
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); elements.len()];
    for bond in bonds {
        neighbors[bond.a].push(bond.b);
        neighbors[bond.b].push(bond.a);
    }
    // Expand each ring atom into its full smallest ring and test that ring.
    let mut aromatic = HashSet::new();
    for &seed in &rings {
        if aromatic.contains(&seed) {
            continue;
        }
        for bond in bonds {
            if bond.a != seed && bond.b != seed {
                continue;
            }
            if let Some(path) = shortest_path(bond.a, bond.b, &neighbors, *bond) {
                let mut ring = vec![bond.a, bond.b];
                ring.extend(path.iter().copied());
                let size = ring.len();
                if size > MAX_AROMATIC_RING_SIZE + 1 {
                    continue;
                }
                let heteroatoms = ring
                    .iter()
                    .filter(|&&index| elements[index] != Element::C)
                    .count();
                let hetero_budget = if size <= 5 { 2 } else { 3 };
                let members_ok = ring.iter().all(|&index| {
                    matches!(
                        elements[index],
                        Element::C | Element::N | Element::O | Element::S
                    )
                });
                if members_ok && heteroatoms <= hetero_budget && ring_is_planar(&ring, positions) {
                    aromatic.extend(ring.iter().copied());
                }
            }
        }
    }
    aromatic
}

/// Newell-normal planarity test for a closed atom loop.
fn ring_is_planar(ring: &[usize], positions: &[[f64; 3]]) -> bool {
    let centroid = ring
        .iter()
        .fold([0.0, 0.0, 0.0], |acc, &index| {
            [
                acc[0] + positions[index][0],
                acc[1] + positions[index][1],
                acc[2] + positions[index][2],
            ]
        })
        .map(|value| value / ring.len() as f64);
    let mut normal = [0.0, 0.0, 0.0];
    for (slot, &index) in ring.iter().enumerate() {
        let next = ring[(slot + 1) % ring.len()];
        let a = positions[index];
        let b = positions[next];
        normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
        normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
        normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if length < 1e-6 {
        return false;
    }
    let normal = [normal[0] / length, normal[1] / length, normal[2] / length];
    ring.iter().all(|&index| {
        let delta = [
            positions[index][0] - centroid[0],
            positions[index][1] - centroid[1],
            positions[index][2] - centroid[2],
        ];
        (delta[0] * normal[0] + delta[1] * normal[1] + delta[2] * normal[2]).abs()
            <= RING_PLANARITY_TOLERANCE
    })
}

/// Rotatable bonds of a ligand, for the torsion-count term.
///
/// A heavy-heavy bond is rotatable when neither end is terminal, it is not
/// part of a ring, and it is not an amide C-N bond (kept rigid by the
/// classic preparation scripts).
pub fn rotatable_bonds(
    elements: &[Element],
    positions: &[[f64; 3]],
    bonds: &[Bond],
    aromatic: &HashSet<usize>,
) -> Vec<Bond> {
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); elements.len()];
    for bond in bonds {
        neighbors[bond.a].push(bond.b);
        neighbors[bond.b].push(bond.a);
    }
    let rings = ring_atoms(elements.len(), bonds);
    let is_carbonyl_carbon = |index: usize| -> bool {
        elements[index] == Element::C
            && neighbors[index].iter().any(|&other| {
                elements[other] == Element::O
                    && distance(positions[index], positions[other])
                        <= super::atom_type::CARBONYL_O_DISTANCE
            })
    };
    bonds
        .iter()
        .filter(|bond| {
            let heavy = |index: usize| elements[index] != Element::H;
            let terminal = |index: usize| {
                neighbors[index]
                    .iter()
                    .filter(|&&other| heavy(other))
                    .count()
                    <= 1
            };
            heavy(bond.a)
                && heavy(bond.b)
                && !terminal(bond.a)
                && !terminal(bond.b)
                && !rings.contains(&bond.a)
                && !rings.contains(&bond.b)
                && !aromatic.contains(&bond.a)
                && !aromatic.contains(&bond.b)
                && !((elements[bond.a] == Element::N && is_carbonyl_carbon(bond.b))
                    || (elements[bond.b] == Element::N && is_carbonyl_carbon(bond.a)))
        })
        .copied()
        .collect()
}

/// Atom pairs separated by at most three bonds (1-2, 1-3, and 1-4 pairs);
/// excluded from the intramolecular score just like in the reference engine.
pub fn bonded_exclusions(count: usize, bonds: &[Bond]) -> Vec<(usize, usize)> {
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); count];
    for bond in bonds {
        neighbors[bond.a].push(bond.b);
        neighbors[bond.b].push(bond.a);
    }
    let mut excluded = HashSet::new();
    for start in 0..count {
        let mut visited = vec![false; count];
        visited[start] = true;
        let mut frontier = vec![start];
        for _ in 0..3 {
            let mut next = Vec::new();
            for &current in &frontier {
                for &neighbor in &neighbors[current] {
                    if !visited[neighbor] {
                        visited[neighbor] = true;
                        next.push(neighbor);
                    }
                }
            }
            for &reached in &next {
                if reached > start {
                    excluded.insert((start, reached));
                }
            }
            frontier = next;
        }
    }
    let mut pairs: Vec<(usize, usize)> = excluded.into_iter().collect();
    pairs.sort_unstable();
    pairs
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

    /// Planar hexagon of atoms with the given element and bond length.
    fn hexagon(element: Element, bond_length: f64) -> (Vec<Element>, Vec<[f64; 3]>) {
        let positions = (0..6)
            .map(|index| {
                let angle = index as f64 * std::f64::consts::FRAC_PI_3;
                [bond_length * angle.cos(), bond_length * angle.sin(), 0.0]
            })
            .collect();
        (vec![element; 6], positions)
    }

    fn hexagon_bonds() -> Vec<Bond> {
        (0..6)
            .map(|index| Bond {
                a: index,
                b: (index + 1) % 6,
            })
            .collect()
    }

    #[test]
    fn benzene_perceives_six_bonds_one_ring_and_full_aromaticity() {
        let (elements, positions) = hexagon(Element::C, 1.39);
        let bonds = perceive_bonds(&elements, &positions);
        assert_eq!(bonds.len(), 6);
        let rings = ring_atoms(6, &bonds);
        assert_eq!(rings.len(), 6);
        let aromatic = aromatic_atoms(&elements, &positions, &bonds);
        assert_eq!(aromatic.len(), 6);
    }

    #[test]
    fn cyclohexane_chair_is_a_ring_but_not_aromatic() {
        let mut positions: Vec<[f64; 3]> = Vec::new();
        for index in 0..6 {
            let angle = index as f64 * std::f64::consts::FRAC_PI_3;
            let puckered = if index % 2 == 0 { 0.25 } else { -0.25 };
            positions.push([1.48 * angle.cos(), 1.48 * angle.sin(), puckered]);
        }
        let elements = vec![Element::C; 6];
        let bonds = hexagon_bonds();
        let rings = ring_atoms(6, &bonds);
        assert_eq!(rings.len(), 6);
        let aromatic = aromatic_atoms(&elements, &positions, &bonds);
        assert!(aromatic.is_empty());
    }

    #[test]
    fn distant_atoms_do_not_bond() {
        let elements = vec![Element::C, Element::C];
        let positions = [[0.0, 0.0, 0.0], [3.8, 0.0, 0.0]];
        assert!(perceive_bonds(&elements, &positions).is_empty());
    }

    #[test]
    fn methyl_terminal_bonds_are_not_rotatable() {
        // Ethane: every C-C and C-H endpoint is terminal, nothing rotates.
        let elements = vec![Element::C, Element::C, Element::H, Element::H, Element::H];
        let positions = [
            [0.0, 0.0, 0.0],
            [1.54, 0.0, 0.0],
            [-0.36, 0.55, 0.75],
            [-0.36, 0.55, -0.75],
            [-0.36, -1.03, 0.0],
        ];
        let bonds = perceive_bonds(&elements, &positions);
        let rotatable = rotatable_bonds(&elements, &positions, &bonds, &HashSet::new());
        assert!(rotatable.is_empty());
    }

    #[test]
    fn butane_middle_bond_is_rotatable() {
        // C-C-C-C zig-zag: only the middle bond has two non-terminal ends.
        let elements = vec![Element::C; 4];
        let positions = [
            [0.0, 0.0, 0.0],
            [1.54, 0.0, 0.0],
            [2.51, 0.89, 0.0],
            [3.98, 0.89, 0.0],
        ];
        let bonds = perceive_bonds(&elements, &positions);
        assert_eq!(bonds.len(), 3);
        let rotatable = rotatable_bonds(&elements, &positions, &bonds, &HashSet::new());
        assert_eq!(rotatable, vec![Bond { a: 1, b: 2 }]);
    }

    #[test]
    fn exclusions_cover_one_two_one_three_and_one_four_pairs() {
        // Linear chain C-C-C-C: every pair is at most three bonds apart.
        let bonds = vec![
            Bond { a: 0, b: 1 },
            Bond { a: 1, b: 2 },
            Bond { a: 2, b: 3 },
        ];
        let excluded = bonded_exclusions(4, &bonds);
        assert_eq!(
            excluded,
            vec![(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
        );
    }
}
