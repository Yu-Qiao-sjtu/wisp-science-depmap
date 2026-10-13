//! Conformation representation: rigid-body transform plus torsion tree.
//!
//! A ligand conformation is seven rigid degrees of freedom (position of the
//! reference centroid, orientation quaternion) plus one angle per rotatable
//! bond. The [`TorsionTree`] is built once per ligand from the rotatable
//! bonds: bonds are removed from the connectivity graph, the largest
//! remaining fragment becomes the rigid root, and each removed edge defines
//! a movable subtree. Applying the conformation applies torsions first
//! (breadth-first, so a parent rotation carries its subtree before the
//! child dihedral is measured), then the rigid transform about the
//! reference centroid.

use crate::docking::prep::atom_type::Bond;
use crate::docking::prep::PreparedLigand;
use std::collections::{HashSet, VecDeque};

/// Unit quaternion for orientation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaternion {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Quaternion {
    pub fn identity() -> Quaternion {
        Quaternion {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// Rotation of `angle` radians around a unit axis.
    pub fn from_axis_angle(axis: [f64; 3], angle: f64) -> Quaternion {
        let norm = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        if norm < 1e-12 {
            return Quaternion::identity();
        }
        let half = angle / 2.0;
        let (sin, cos) = half.sin_cos();
        let scale = sin / norm;
        Quaternion {
            w: cos,
            x: axis[0] * scale,
            y: axis[1] * scale,
            z: axis[2] * scale,
        }
    }

    /// Exponential map: rotation vector (axis * angle) to quaternion.
    pub fn from_rotation_vector(v: [f64; 3]) -> Quaternion {
        let angle = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if angle < 1e-12 {
            return Quaternion::identity();
        }
        Quaternion::from_axis_angle(v, angle)
    }

    pub fn multiply(self, other: Quaternion) -> Quaternion {
        Quaternion {
            w: self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
            x: self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            y: self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            z: self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
        }
    }

    pub fn normalized(self) -> Quaternion {
        let norm = (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if norm < 1e-12 {
            return Quaternion::identity();
        }
        Quaternion {
            w: self.w / norm,
            x: self.x / norm,
            y: self.y / norm,
            z: self.z / norm,
        }
    }

    /// Rotate a vector (active rotation): v' = v + w*t + q_vec x t with
    /// t = 2 (q_vec x v), the standard quadratic form.
    pub fn rotate(self, v: [f64; 3]) -> [f64; 3] {
        let q = [self.x, self.y, self.z];
        let cross_qv = [
            q[1] * v[2] - q[2] * v[1],
            q[2] * v[0] - q[0] * v[2],
            q[0] * v[1] - q[1] * v[0],
        ];
        let t = [2.0 * cross_qv[0], 2.0 * cross_qv[1], 2.0 * cross_qv[2]];
        let cross_qt = [
            q[1] * t[2] - q[2] * t[1],
            q[2] * t[0] - q[0] * t[2],
            q[0] * t[1] - q[1] * t[0],
        ];
        [
            v[0] + self.w * t[0] + cross_qt[0],
            v[1] + self.w * t[1] + cross_qt[1],
            v[2] + self.w * t[2] + cross_qt[2],
        ]
    }
}

/// Signed dihedral p0-p1-p2-p3 in radians, IUPAC convention (-pi, pi].
pub fn dihedral(p0: [f64; 3], p1: [f64; 3], p2: [f64; 3], p3: [f64; 3]) -> f64 {
    let b1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let b2 = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
    let b3 = [p3[0] - p2[0], p3[1] - p2[1], p3[2] - p2[2]];
    let n1 = [
        b1[1] * b2[2] - b1[2] * b2[1],
        b1[2] * b2[0] - b1[0] * b2[2],
        b1[0] * b2[1] - b1[1] * b2[0],
    ];
    let n2 = [
        b2[1] * b3[2] - b2[2] * b3[1],
        b2[2] * b3[0] - b2[0] * b3[2],
        b2[0] * b3[1] - b2[1] * b3[0],
    ];
    let b2_norm = (b2[0] * b2[0] + b2[1] * b2[1] + b2[2] * b2[2]).sqrt();
    if b2_norm < 1e-12 {
        return 0.0;
    }
    let m1 = [
        n1[1] * b2[2] - n1[2] * b2[1],
        n1[2] * b2[0] - n1[0] * b2[2],
        n1[0] * b2[1] - n1[1] * b2[0],
    ];
    let x = n1[0] * n2[0] + n1[1] * n2[1] + n1[2] * n2[2];
    let y = m1[0] * n2[0] + m1[1] * n2[1] + m1[2] * n2[2];
    y.atan2(x)
}

/// The rotatable-bond tree of one ligand: parent-side references and the
/// movable subtree of every torsion, in breadth-first order from the root
/// fragment.
#[derive(Debug, Clone, PartialEq)]
pub struct TorsionTree {
    /// Rotatable bond (parent atom, child atom), BFS order.
    pub bonds: Vec<Bond>,
    /// Reference atom on the parent side for the dihedral measurement
    /// (never inside any subtree).
    pub parent_refs: Vec<usize>,
    /// Reference atom inside the child subtree.
    pub child_refs: Vec<usize>,
    /// Atom indices that move when torsion `i` turns (the child side of
    /// bond `i` and everything hanging below it).
    pub subtrees: Vec<Vec<usize>>,
    /// Indices of the root (rigid) fragment atoms.
    pub root: Vec<usize>,
}

impl TorsionTree {
    /// Build the tree from ligand connectivity. `rotatable` comes from the
    /// prep layer; ring bonds are never rotatable so removal cannot split a
    /// ring fragment.
    pub fn build(ligand: &PreparedLigand) -> TorsionTree {
        let count = ligand.atoms.len();
        let rotatable: HashSet<(usize, usize)> = ligand
            .rotatable
            .iter()
            .map(|bond| (bond.a.min(bond.b), bond.a.max(bond.b)))
            .collect();
        // Adjacency without the rotatable edges defines the fragments.
        let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); count];
        for bond in &ligand.bonds {
            let key = (bond.a.min(bond.b), bond.a.max(bond.b));
            if !rotatable.contains(&key) {
                adjacency[bond.a].push(bond.b);
                adjacency[bond.b].push(bond.a);
            }
        }
        // Fragments by flood fill; the largest is the root.
        let mut fragment_of: Vec<usize> = vec![usize::MAX; count];
        let mut fragments: Vec<Vec<usize>> = Vec::new();
        for start in 0..count {
            if fragment_of[start] != usize::MAX {
                continue;
            }
            let fragment = fragments.len();
            let mut members = Vec::new();
            let mut queue = VecDeque::from([start]);
            fragment_of[start] = fragment;
            while let Some(atom) = queue.pop_front() {
                members.push(atom);
                for &next in &adjacency[atom] {
                    if fragment_of[next] == usize::MAX {
                        fragment_of[next] = fragment;
                        queue.push_back(next);
                    }
                }
            }
            members.sort_unstable();
            fragments.push(members);
        }
        let root_fragment = fragments
            .iter()
            .enumerate()
            .max_by_key(|(index, members)| (members.len(), usize::MAX - *index))
            .map(|(index, _)| index)
            .unwrap_or(0);

        // Breadth-first across the rotatable edges, root fragment first.
        let mut bonds: Vec<Bond> = Vec::new();
        let mut subtrees: Vec<Vec<usize>> = Vec::new();
        let mut parent_refs: Vec<usize> = Vec::new();
        let mut child_refs: Vec<usize> = Vec::new();
        let mut assigned: HashSet<usize> = fragments[root_fragment].iter().copied().collect();
        let mut frontier: VecDeque<usize> = fragments[root_fragment].iter().copied().collect();
        while frontier.pop_front().is_some() {
            for bond in &ligand.rotatable {
                let (parent, child) = if assigned.contains(&bond.a) {
                    (bond.a, bond.b)
                } else if assigned.contains(&bond.b) {
                    (bond.b, bond.a)
                } else {
                    continue;
                };
                if assigned.contains(&child) {
                    continue; // already reached through another branch
                }
                // Child side: flood from child without crossing rotatable edges.
                let mut members = Vec::new();
                let mut queue = VecDeque::from([child]);
                let mut seen: HashSet<usize> = HashSet::from([child]);
                while let Some(current) = queue.pop_front() {
                    members.push(current);
                    for &next in &adjacency[current] {
                        if seen.insert(next) {
                            queue.push_back(next);
                        }
                    }
                }
                members.sort_unstable();
                let parent_ref = adjacency[parent]
                    .iter()
                    .copied()
                    .filter(|next| !members.contains(next))
                    .min()
                    .unwrap_or(parent);
                let child_ref = adjacency[child]
                    .iter()
                    .copied()
                    .filter(|next| members.contains(next))
                    .min()
                    .unwrap_or(child);
                for &member in &members {
                    assigned.insert(member);
                    frontier.push_back(member);
                }
                bonds.push(Bond {
                    a: parent,
                    b: child,
                });
                parent_refs.push(parent_ref);
                child_refs.push(child_ref);
                subtrees.push(members);
            }
        }
        TorsionTree {
            bonds,
            parent_refs,
            child_refs,
            subtrees,
            root: fragments[root_fragment].clone(),
        }
    }

    pub fn len(&self) -> usize {
        self.bonds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bonds.is_empty()
    }
}

/// One ligand conformation: rigid placement plus torsion angles (radians).
#[derive(Debug, Clone, PartialEq)]
pub struct Conformation {
    /// Position of the reference centroid.
    pub center: [f64; 3],
    pub orientation: Quaternion,
    /// Absolute dihedral per torsion-tree bond, in tree order.
    pub torsions: Vec<f64>,
}

/// The ligand geometry the search starts from: reference positions plus
/// their centroid. Cheap to clone per MC run.
#[derive(Debug, Clone)]
pub struct LigandFrame {
    pub reference: Vec<[f64; 3]>,
    pub centroid: [f64; 3],
}

impl LigandFrame {
    pub fn from_positions(reference: Vec<[f64; 3]>) -> LigandFrame {
        let mut centroid = [0.0; 3];
        for position in &reference {
            centroid[0] += position[0];
            centroid[1] += position[1];
            centroid[2] += position[2];
        }
        let n = reference.len().max(1) as f64;
        centroid[0] /= n;
        centroid[1] /= n;
        centroid[2] /= n;
        LigandFrame {
            reference,
            centroid,
        }
    }

    /// Materialize atom positions for a conformation.
    pub fn positions(&self, tree: &TorsionTree, conformation: &Conformation) -> Vec<[f64; 3]> {
        let mut positions = self.reference.clone();
        // Torsions in tree order: each dihedral is measured against the
        // current geometry, so parent rotations carry child subtrees.
        for (index, bond) in tree.bonds.iter().enumerate() {
            let current = dihedral(
                positions[tree.parent_refs[index]],
                positions[bond.a],
                positions[bond.b],
                positions[tree.child_refs[index]],
            );
            let delta = conformation.torsions[index] - current;
            if delta.abs() < 1e-12 {
                continue;
            }
            let axis = [
                positions[bond.b][0] - positions[bond.a][0],
                positions[bond.b][1] - positions[bond.a][1],
                positions[bond.b][2] - positions[bond.a][2],
            ];
            let rotation = Quaternion::from_axis_angle(axis, delta);
            let pivot = positions[bond.b];
            for &atom in &tree.subtrees[index] {
                let mut p = positions[atom];
                p[0] -= pivot[0];
                p[1] -= pivot[1];
                p[2] -= pivot[2];
                let rotated = rotation.rotate(p);
                positions[atom] = [
                    rotated[0] + pivot[0],
                    rotated[1] + pivot[1],
                    rotated[2] + pivot[2],
                ];
            }
        }
        // Rigid transform about the reference centroid.
        for position in positions.iter_mut() {
            let mut p = [
                position[0] - self.centroid[0],
                position[1] - self.centroid[1],
                position[2] - self.centroid[2],
            ];
            p = conformation.orientation.rotate(p);
            *position = [
                p[0] + conformation.center[0],
                p[1] + conformation.center[1],
                p[2] + conformation.center[2],
            ];
        }
        positions
    }
}

/// Wrap an angle into (-pi, pi].
pub fn wrap_angle(angle: f64) -> f64 {
    let mut wrapped = angle.rem_euclid(std::f64::consts::TAU);
    if wrapped > std::f64::consts::PI {
        wrapped -= std::f64::consts::TAU;
    }
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quaternion_rotates_axes_and_round_trips() {
        let quarter = Quaternion::from_axis_angle([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2);
        let rotated = quarter.rotate([1.0, 0.0, 0.0]);
        assert!((rotated[0] - 0.0).abs() < 1e-12);
        assert!((rotated[1] - 1.0).abs() < 1e-12);
        assert!((rotated[2] - 0.0).abs() < 1e-12);
        // Applying the inverse returns the original vector.
        let inverse = Quaternion::from_axis_angle([0.0, 0.0, 1.0], -std::f64::consts::FRAC_PI_2);
        let restored = inverse.rotate(rotated);
        assert!((restored[0] - 1.0).abs() < 1e-12);
        assert!((restored[1] - 0.0).abs() < 1e-12);
        // Rotation vector exponential map matches axis-angle.
        let exp = Quaternion::from_rotation_vector([0.0, 0.0, std::f64::consts::FRAC_PI_2]);
        assert!((exp.w - quarter.w).abs() < 1e-12 && (exp.x - quarter.x).abs() < 1e-12);
    }

    #[test]
    fn dihedral_matches_known_geometries() {
        let zero = dihedral(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        );
        assert!(zero.abs() < 1e-12);
        let trans = dihedral(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        );
        assert!((trans - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn wrap_angle_stays_in_range() {
        assert!((wrap_angle(std::f64::consts::TAU + 0.25) - 0.25).abs() < 1e-12);
        assert!((wrap_angle(-std::f64::consts::TAU - 0.25) + 0.25).abs() < 1e-12);
        assert!((wrap_angle(std::f64::consts::PI) - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn frame_identity_conformation_reproduces_reference() {
        let frame = LigandFrame::from_positions(vec![[0.0, 0.0, 0.0], [1.5, 0.0, 0.0]]);
        let tree = TorsionTree {
            bonds: Vec::new(),
            parent_refs: Vec::new(),
            child_refs: Vec::new(),
            subtrees: Vec::new(),
            root: vec![0, 1],
        };
        let conformation = Conformation {
            center: frame.centroid,
            orientation: Quaternion::identity(),
            torsions: Vec::new(),
        };
        let positions = frame.positions(&tree, &conformation);
        assert_eq!(positions, frame.reference);
    }

    #[test]
    fn frame_rigid_rotation_moves_the_centroid_to_target() {
        let frame = LigandFrame::from_positions(vec![[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]]);
        let tree = TorsionTree {
            bonds: Vec::new(),
            parent_refs: Vec::new(),
            child_refs: Vec::new(),
            subtrees: Vec::new(),
            root: vec![0, 1],
        };
        let conformation = Conformation {
            center: [5.0, 7.0, 9.0],
            orientation: Quaternion::from_axis_angle([0.0, 1.0, 0.0], std::f64::consts::PI),
            torsions: Vec::new(),
        };
        let positions = frame.positions(&tree, &conformation);
        // Centroid lands on the target, bond length preserved, orientation flipped.
        let centroid = [
            (positions[0][0] + positions[1][0]) / 2.0,
            (positions[0][1] + positions[1][1]) / 2.0,
            (positions[0][2] + positions[1][2]) / 2.0,
        ];
        assert!((centroid[0] - 5.0).abs() < 1e-12);
        assert!((centroid[1] - 7.0).abs() < 1e-12);
        assert!((centroid[2] - 9.0).abs() < 1e-12);
        // Half a turn around y flips the bond along x: [1,0,0] -> [-1,0,0]
        // before the translation, so the atoms land at x = 4 and x = 6.
        assert!(
            (positions[0][0] - 4.0).abs() < 1e-12,
            "x0 = {}",
            positions[0][0]
        );
        assert!(
            (positions[1][0] - 6.0).abs() < 1e-12,
            "x1 = {}",
            positions[1][0]
        );
        assert!((positions[0][2] - 9.0).abs() < 1e-12);
        assert!((positions[1][2] - 9.0).abs() < 1e-12);
    }
}
