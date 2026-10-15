//! Monte-Carlo search with quasi-Newton relaxation: the conformational
//! search half of the Vina scheme.
//!
//! Each of the `exhaustiveness` independent runs owns one deterministic
//! RNG stream (`Rng::from_stream(seed, run_index)`, the documented layout
//! of #274). A run samples a random start inside the search box, relaxes
//! it with BFGS, then alternates random perturbation (translation,
//! rotation, torsions) with relaxation, accepting or rejecting by the
//! Metropolis criterion at a fixed temperature (RT at 300 K). Run bests
//! are pooled, deduplicated by pose RMSD, and returned best-first.
//!
//! Runs execute on the available CPU threads (one stream each); results
//! are collected by run index, so thread scheduling never changes the
//! outcome.

use super::conformation::{wrap_angle, Conformation, LigandFrame, Quaternion, TorsionTree};
use super::quasi_newton::{minimize, BfgsOptions, GradientEpsilons};
use super::rng::Rng;
use crate::docking::engine::{ScoreTerms, ScoredPose, SearchOptions};
use crate::docking::prep::{PreparedLigand, PreparedReceptor};
use crate::docking::scoring::{evaluate_inter, evaluate_intra, VinaParams};

/// Axis-aligned search box for the ligand centroid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl SearchBox {
    /// Bounding box of the receptor atoms padded so a ligand whose centroid
    /// sits on the box surface can still reach the protein with its radius.
    pub fn from_receptor(receptor: &PreparedReceptor, ligand_radius: f64) -> SearchBox {
        let mut min = [f64::MAX; 3];
        let mut max = [f64::MIN; 3];
        for atom in &receptor.atoms {
            for axis in 0..3 {
                min[axis] = min[axis].min(atom.position[axis]);
                max[axis] = max[axis].max(atom.position[axis]);
            }
        }
        let padding = ligand_radius + 2.0;
        SearchBox {
            min: [min[0] - padding, min[1] - padding, min[2] - padding],
            max: [max[0] + padding, max[1] + padding, max[2] + padding],
        }
    }

    fn sample_center(&self, rng: &mut Rng) -> [f64; 3] {
        let mut center = [0.0; 3];
        for axis in 0..3 {
            center[axis] = self.min[axis] + rng.next_uniform() * (self.max[axis] - self.min[axis]);
        }
        center
    }

    fn clamp_center(&self, center: [f64; 3]) -> [f64; 3] {
        [
            center[0].clamp(self.min[0], self.max[0]),
            center[1].clamp(self.min[1], self.max[1]),
            center[2].clamp(self.min[2], self.max[2]),
        ]
    }
}

/// Perturbation and acceptance schedule. Amplitudes follow the published
/// magnitudes (about 2 A translation, 25 degrees rotation/torsion); the
/// temperature is RT at 300 K in kcal/mol.
#[derive(Debug, Clone, Copy)]
pub struct SearchSchedule {
    pub translation_amplitude: f64,
    pub rotation_amplitude: f64,
    pub torsion_amplitude: f64,
    /// Metropolis temperature k_t = k_B * T (0.593 kcal/mol at 300 K).
    pub k_t: f64,
    pub steps_per_run: usize,
}

impl SearchSchedule {
    pub fn for_degrees_of_freedom(degrees: usize) -> SearchSchedule {
        SearchSchedule {
            translation_amplitude: 2.0,
            rotation_amplitude: 25.0_f64.to_radians(),
            torsion_amplitude: 25.0_f64.to_radians(),
            k_t: 0.593,
            steps_per_run: 40 + 10 * degrees,
        }
    }
}

/// Hard cap on worker threads per search. Multi-start parallelism is part
/// of the design, but the search must not saturate the user's machine —
/// especially under `cargo test`, where several searches may run at once.
const MAX_SEARCH_THREADS: usize = 4;

/// Uniformly random orientation (Shoemake's method on SO(3)).
fn random_orientation(rng: &mut Rng) -> Quaternion {
    let u1 = rng.next_uniform();
    let u2 = rng.next_uniform();
    let u3 = rng.next_uniform();
    let (s2, c2) = (std::f64::consts::TAU * u2).sin_cos();
    let (s3, c3) = (std::f64::consts::TAU * u3).sin_cos();
    let sqrt1 = (1.0 - u1).sqrt();
    let sqrtu = u1.sqrt();
    Quaternion {
        w: sqrt1 * s2,
        x: sqrt1 * c2,
        y: sqrtu * s3,
        z: sqrtu * c3,
    }
    .normalized()
}

/// A random starting conformation inside the box.
pub fn random_conformation(rng: &mut Rng, box_: &SearchBox, torsions: usize) -> Conformation {
    Conformation {
        center: box_.sample_center(rng),
        orientation: random_orientation(rng),
        torsions: (0..torsions)
            .map(|_| wrap_angle(rng.next_uniform() * std::f64::consts::TAU - std::f64::consts::PI))
            .collect(),
    }
}

/// One MC perturbation of a conformation.
pub fn mutate(
    rng: &mut Rng,
    base: &Conformation,
    box_: &SearchBox,
    schedule: &SearchSchedule,
) -> Conformation {
    let center = box_.clamp_center([
        base.center[0] + rng.next_normal() * schedule.translation_amplitude,
        base.center[1] + rng.next_normal() * schedule.translation_amplitude,
        base.center[2] + rng.next_normal() * schedule.translation_amplitude,
    ]);
    let axis = [rng.next_normal(), rng.next_normal(), rng.next_normal()];
    let angle = rng.next_normal() * schedule.rotation_amplitude;
    let orientation = Quaternion::from_axis_angle(axis, angle)
        .multiply(base.orientation)
        .normalized();
    let torsions = base
        .torsions
        .iter()
        .map(|t| wrap_angle(t + rng.next_normal() * schedule.torsion_amplitude))
        .collect();
    Conformation {
        center,
        orientation,
        torsions,
    }
}

/// The relaxed score of one pose: inter + intra (the torsion penalty is
/// constant per ligand and irrelevant to the search).
fn pair_score(
    receptor: &PreparedReceptor,
    ligand: &PreparedLigand,
    positions: &[[f64; 3]],
    params: &VinaParams,
) -> f64 {
    evaluate_inter(receptor, ligand, positions, params).total()
        + evaluate_intra(ligand, positions, params).total()
}

/// Atom-position RMSD between two poses of the same ligand.
pub fn pose_rmsd(a: &[[f64; 3]], b: &[[f64; 3]]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return f64::INFINITY;
    }
    let sum: f64 = a
        .iter()
        .zip(b)
        .map(|(p, q)| {
            let dx = p[0] - q[0];
            let dy = p[1] - q[1];
            let dz = p[2] - q[2];
            dx * dx + dy * dy + dz * dz
        })
        .sum();
    (sum / a.len() as f64).sqrt()
}

/// One independent MC run: `stream_index` owns its RNG stream.
fn run_single(
    receptor: &PreparedReceptor,
    ligand: &PreparedLigand,
    tree: &TorsionTree,
    frame: &LigandFrame,
    params: &VinaParams,
    box_: &SearchBox,
    schedule: &SearchSchedule,
    rng: &mut Rng,
) -> (Vec<f64>, f64) {
    let score_x = |x: &[f64]| -> f64 {
        let conformation = x_to_conformation(x);
        let positions = frame.positions(tree, &conformation);
        pair_score(receptor, ligand, &positions, params)
    };
    let epsilons = gradient_epsilons(tree.len());
    let bfgs = BfgsOptions {
        max_iterations: 24,
        gtol: 1e-3,
    };
    let start = random_conformation(rng, box_, tree.len());
    let mut current = minimize(&score_x, &conformation_to_x(&start), &epsilons, &bfgs);
    let mut energy = score_x(&current);
    let mut best = current.clone();
    let mut best_energy = energy;
    for _ in 0..schedule.steps_per_run {
        let candidate = mutate(rng, &x_to_conformation(&current), box_, schedule);
        let relaxed = minimize(&score_x, &conformation_to_x(&candidate), &epsilons, &bfgs);
        let candidate_energy = score_x(&relaxed);
        let delta = candidate_energy - energy;
        if delta <= 0.0 || rng.next_uniform() < (-delta / schedule.k_t).exp() {
            current = relaxed;
            energy = candidate_energy;
            if candidate_energy < best_energy {
                best = current.clone();
                best_energy = candidate_energy;
            }
        }
    }
    (best, best_energy)
}

/// Run the full search and return poses best-first, deduplicated. The
/// `score_of` callback adds the constant torsion penalty and produces the
/// reported per-term decomposition.
pub fn search_poses(
    receptor: &PreparedReceptor,
    ligand: &PreparedLigand,
    tree: &TorsionTree,
    frame: &LigandFrame,
    params: &VinaParams,
    options: &SearchOptions,
    score_of: &dyn Fn(&[[f64; 3]]) -> ScoreTerms,
) -> Vec<ScoredPose> {
    let ligand_radius = frame
        .reference
        .iter()
        .map(|p| {
            let dx = p[0] - frame.centroid[0];
            let dy = p[1] - frame.centroid[1];
            let dz = p[2] - frame.centroid[2];
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
        .fold(0.0_f64, f64::max);
    let box_ = SearchBox::from_receptor(receptor, ligand_radius);
    let schedule = SearchSchedule::for_degrees_of_freedom(6 + tree.len());
    let runs = options.exhaustiveness.max(1) as usize;

    let mut slots: Vec<Option<(Vec<f64>, f64)>> = (0..runs).map(|_| None).collect();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(runs)
        .min(MAX_SEARCH_THREADS);
    let chunk = runs.div_ceil(threads);
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for (thread_index, slot_chunk) in slots.chunks_mut(chunk).enumerate() {
            let first_run = thread_index * chunk;
            let handles = &mut handles;
            handles.push(scope.spawn(move || {
                for (offset, slot) in slot_chunk.iter_mut().enumerate() {
                    let run_index = (first_run + offset) as u64;
                    let mut rng = Rng::from_stream(options.seed, run_index);
                    *slot = Some(run_single(
                        receptor, ligand, tree, frame, params, &box_, &schedule, &mut rng,
                    ));
                }
            }));
        }
        for handle in handles {
            let _ = handle.join();
        }
    });

    // Rank by relaxed pair energy (run index as the deterministic
    // tiebreaker), deduplicate by pose RMSD, keep num_modes.
    let mut outcomes: Vec<(usize, Vec<f64>, f64)> = slots
        .into_iter()
        .enumerate()
        .filter_map(|(index, slot)| slot.map(|(x, energy)| (index, x, energy)))
        .collect();
    outcomes.sort_by(|a, b| {
        a.2.partial_cmp(&b.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let mut poses: Vec<ScoredPose> = Vec::new();
    for (_, x, _) in outcomes {
        if poses.len() >= options.num_modes {
            break;
        }
        let positions = frame.positions(tree, &x_to_conformation(&x));
        if poses
            .iter()
            .any(|pose| pose_rmsd(&pose.positions, &positions) < 1.5)
        {
            continue;
        }
        poses.push(ScoredPose {
            terms: score_of(&positions),
            positions,
        });
    }
    poses
}

/// Parameter vector of a conformation: [cx, cy, cz, rx, ry, rz, torsions].
fn conformation_to_x(conformation: &Conformation) -> Vec<f64> {
    let mut x = Vec::with_capacity(6 + conformation.torsions.len());
    x.extend_from_slice(&conformation.center);
    x.extend_from_slice(&rotation_vector_of(conformation.orientation));
    x.extend(conformation.torsions.iter().copied());
    x
}

fn x_to_conformation(x: &[f64]) -> Conformation {
    Conformation {
        center: [x[0], x[1], x[2]],
        orientation: Quaternion::from_rotation_vector([x[3], x[4], x[5]]),
        torsions: x[6..].iter().map(|t| wrap_angle(*t)).collect(),
    }
}

/// Log map of a unit quaternion to a rotation vector.
fn rotation_vector_of(q: Quaternion) -> [f64; 3] {
    let norm = (q.x * q.x + q.y * q.y + q.z * q.z).sqrt();
    if norm < 1e-12 {
        return [0.0, 0.0, 0.0];
    }
    let angle = 2.0 * q.w.clamp(-1.0, 1.0).acos();
    let scale = angle / norm;
    [q.x * scale, q.y * scale, q.z * scale]
}

fn gradient_epsilons(torsions: usize) -> Vec<f64> {
    let eps = GradientEpsilons::default();
    let mut e = vec![eps.translation; 3];
    e.extend(vec![eps.rotation; 3]);
    e.extend(vec![eps.torsion; torsions]);
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docking::prep::{prepare_ligand_pdb, prepare_receptor};
    use crate::docking::scoring::PairBreakdown;

    fn single_carbon_receptor() -> PreparedReceptor {
        // One carbon at the origin: the energy surface is spherically
        // symmetric, so every optimal pose sits on a shell at the ideal
        // contact distance regardless of approach direction.
        prepare_receptor(
            "ATOM      1  CA  GLY A   1       0.000   0.000   0.000  1.00  0.00           C\n",
        )
        .unwrap()
    }

    fn single_carbon_ligand() -> PreparedLigand {
        prepare_ligand_pdb(
            "HETATM    1  C   LIG L   1       8.000   8.000   8.000  1.00  0.00           C\n",
        )
        .unwrap()
    }

    #[test]
    fn search_places_the_ligand_at_the_optimal_contact_radius() {
        let receptor = single_carbon_receptor();
        let ligand = single_carbon_ligand();
        let tree = TorsionTree::build(&ligand);
        let frame = LigandFrame::from_positions(ligand.atoms.iter().map(|a| a.position).collect());
        let params = crate::docking::scoring::VINA_1_1;
        let options = SearchOptions {
            seed: 2024,
            exhaustiveness: 2,
            num_modes: 3,
        };
        let poses = search_poses(
            &receptor,
            &ligand,
            &tree,
            &frame,
            &params,
            &options,
            &|positions| {
                let inter = evaluate_inter(&receptor, &ligand, positions, &params);
                let intra = evaluate_intra(&ligand, positions, &params);
                ScoreTerms::from_breakdowns(inter, intra, 0.0)
            },
        );
        assert!(!poses.is_empty());
        let best = &poses[0];
        // The optimum is the attractive contact shell (~3.5-4.0 A surface
        // distance for C-C); the direction is degenerate by symmetry, so
        // only the radius is asserted.
        let [x, y, z] = best.positions[0];
        let radius = (x * x + y * y + z * z).sqrt();
        assert!(
            (3.3..=4.5).contains(&radius),
            "pose at {x},{y},{z} (r={radius})"
        );
        assert!(
            best.terms.total < 0.0,
            "no binding energy: {}",
            best.terms.total
        );
    }

    #[test]
    fn same_seed_reproduces_identical_poses_and_different_seed_differs() {
        let receptor = single_carbon_receptor();
        let ligand = single_carbon_ligand();
        let tree = TorsionTree::build(&ligand);
        let frame = LigandFrame::from_positions(ligand.atoms.iter().map(|a| a.position).collect());
        let params = crate::docking::scoring::VINA_1_1;
        let run = |seed: u64| {
            let options = SearchOptions {
                seed,
                exhaustiveness: 2,
                num_modes: 3,
            };
            search_poses(
                &receptor,
                &ligand,
                &tree,
                &frame,
                &params,
                &options,
                &|positions| {
                    let inter = evaluate_inter(&receptor, &ligand, positions, &params);
                    ScoreTerms::from_breakdowns(inter, PairBreakdown::default(), 0.0)
                },
            )
        };
        let a = run(7);
        let b = run(7);
        let c = run(8);
        assert_eq!(a, b, "same seed must reproduce identical output");
        assert_ne!(a, c, "different seed must differ somewhere");
    }

    #[test]
    fn pose_rmsd_measures_displacement() {
        let a = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let b = vec![[3.0, 0.0, 0.0], [4.0, 0.0, 0.0]];
        assert!((pose_rmsd(&a, &b) - 3.0).abs() < 1e-12);
        assert_eq!(pose_rmsd(&a, &a), 0.0);
    }
}
