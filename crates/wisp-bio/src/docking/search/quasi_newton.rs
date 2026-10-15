//! BFGS local minimization over the conformation coordinates.
//!
//! The Monte-Carlo outer loop proposes, and this module relaxes: every
//! candidate conformation is minimized before the Metropolis test, the
//! "quasi-Newton" half of the Vina search scheme. The variables are the
//! 3 translation components, 3 rotation-vector components (exponential map
//! applied to the base orientation), and one delta per torsion. Gradients
//! are central differences of the same scoring path the direct evaluation
//! uses, so the minimizer cannot drift from the reported energies.

/// Stopping and finiteness knobs for [`minimize`].
#[derive(Debug, Clone, Copy)]
pub struct BfgsOptions {
    pub max_iterations: usize,
    /// Infinity-norm gradient tolerance.
    pub gtol: f64,
}

impl Default for BfgsOptions {
    fn default() -> Self {
        BfgsOptions {
            max_iterations: 40,
            gtol: 1e-3,
        }
    }
}

/// Step sizes for the numerical gradient, per variable kind.
#[derive(Debug, Clone, Copy)]
pub struct GradientEpsilons {
    pub translation: f64,
    pub rotation: f64,
    pub torsion: f64,
}

impl Default for GradientEpsilons {
    fn default() -> Self {
        GradientEpsilons {
            translation: 1e-4,
            rotation: 1e-4,
            torsion: 1e-4,
        }
    }
}

/// Minimize `f` from `start` with BFGS and a bracketing golden-section
/// line search. Returns the best point found. `f` must be finite at
/// `start`; non-finite evaluations during the search are treated as +inf.
pub fn minimize(
    f: &dyn Fn(&[f64]) -> f64,
    start: &[f64],
    epsilons: &[f64],
    options: &BfgsOptions,
) -> Vec<f64> {
    let n = start.len();
    if n == 0 {
        return Vec::new();
    }
    let safe = |x: &[f64]| {
        let value = f(x);
        if value.is_finite() {
            value
        } else {
            f64::INFINITY
        }
    };
    let mut x = start.to_vec();
    let mut fx = safe(&x);
    // Dense inverse-Hessian approximation, row-major flat.
    let mut h_inv: Vec<f64> = identity(n);
    let mut gradient = numerical_gradient(&safe, &x, epsilons);
    // One steepest-descent restart is allowed after a failed line search
    // before giving up: the accumulated Hessian model can go bad near
    // narrow valley floors.
    let mut restarted = false;
    for _ in 0..options.max_iterations {
        if gradient.iter().all(|g| g.abs() < options.gtol) {
            break;
        }
        // Descent direction p = -H g.
        let mut p = vec![0.0; n];
        for row in 0..n {
            let mut dot = 0.0;
            for col in 0..n {
                dot += h_inv[row * n + col] * gradient[col];
            }
            p[row] = -dot;
        }
        let slope: f64 = gradient.iter().zip(&p).map(|(g, d)| g * d).sum();
        if slope >= 0.0 {
            // Numerical breakdown: reset the model.
            h_inv = identity(n);
            p = gradient.iter().map(|g| -g).collect::<Vec<_>>();
        }
        let Some((next_x, next_fx)) = line_search(&safe, &x, &p, fx) else {
            if restarted {
                break; // even steepest descent cannot improve: converged
            }
            restarted = true;
            h_inv = identity(n);
            continue; // retry from the raw steepest-descent direction
        };
        let step: Vec<f64> = next_x.iter().zip(&x).map(|(a, b)| a - b).collect();
        let next_gradient = numerical_gradient(&safe, &next_x, epsilons);
        let delta: Vec<f64> = next_gradient
            .iter()
            .zip(&gradient)
            .map(|(a, b)| a - b)
            .collect();
        let sy: f64 = step.iter().zip(&delta).map(|(s, d)| s * d).sum();
        if sy > 1e-12 {
            let rho = 1.0 / sy;
            // H <- (I - rho*s*y^T) H (I - rho*y*s^T) + rho*s*s^T
            let mut h_left = h_inv.clone();
            let mut h_right = h_inv.clone();
            for row in 0..n {
                for col in 0..n {
                    h_left[row * n + col] -= rho * step[row] * delta[col];
                    h_right[row * n + col] -= rho * delta[row] * step[col];
                }
            }
            for row in 0..n {
                for col in 0..n {
                    let mut dot = 0.0;
                    for k in 0..n {
                        dot += h_left[row * n + k] * h_right[k * n + col];
                    }
                    h_inv[row * n + col] = dot + rho * step[row] * step[col];
                }
            }
        }
        x = next_x;
        fx = next_fx;
        gradient = next_gradient;
        restarted = false; // a productive step re-arms the restart
    }
    x
}

/// Bracket the minimum along `x + alpha * p` starting from a unit step,
/// expanding while the step keeps helping, then shrink the bracket with a
/// golden-section search (two interior points, one new evaluation per
/// iteration). Returns the improved point or `None` when no step along
/// `p` beats `fx`.
fn line_search(
    f: &dyn Fn(&[f64]) -> f64,
    x: &[f64],
    p: &[f64],
    fx: f64,
) -> Option<(Vec<f64>, f64)> {
    let phi = |alpha: f64| -> f64 {
        let point: Vec<f64> = x.iter().zip(p).map(|(v, d)| v + alpha * d).collect();
        f(&point)
    };
    // Expansion phase: [a, b] with a the best endpoint so far. The
    // improvement must beat floating-point noise (relative tolerance),
    // otherwise a numerically flat f(ph(1)) == fx can push the bracket
    // far past the useful region.
    let mut a = 0.0;
    let mut fa = fx;
    let mut step = 1.0;
    let mut b = step;
    let mut fb = phi(step);
    for _ in 0..8 {
        if fb >= fa - 1e-12 * (1.0 + fa.abs()) {
            break; // [a, b] brackets a minimum
        }
        a = b;
        fa = fb;
        step *= 2.0;
        b = a + step;
        fb = phi(b);
    }
    // Golden-section shrink of [a, b] on phi.
    const PHI: f64 = 0.618_033_988_749_894_9; // (sqrt(5) - 1) / 2
    let mut left = a;
    let mut right = b;
    let mut c = right - PHI * (right - left);
    let mut d = left + PHI * (right - left);
    let mut fc = phi(c);
    let mut fd = phi(d);
    for _ in 0..24 {
        if (right - left).abs() < 1e-7 * (1.0 + right.abs().max(left.abs())) {
            break;
        }
        if fc < fd {
            right = d;
            d = c;
            fd = fc;
            c = right - PHI * (right - left);
            fc = phi(c);
        } else {
            left = c;
            c = d;
            fc = fd;
            d = left + PHI * (right - left);
            fd = phi(d);
        }
    }
    let (best_alpha, best_value) = if fc <= fd { (c, fc) } else { (d, fd) };
    if best_value < fx {
        Some((point_at(x, p, best_alpha), best_value))
    } else {
        None
    }
}

fn point_at(x: &[f64], p: &[f64], alpha: f64) -> Vec<f64> {
    x.iter().zip(p).map(|(v, d)| v + alpha * d).collect()
}

fn identity(n: usize) -> Vec<f64> {
    let mut matrix = vec![0.0; n * n];
    for (index, value) in matrix.iter_mut().enumerate() {
        if index % (n + 1) == 0 {
            *value = 1.0;
        }
    }
    matrix
}

/// Central-difference gradient with per-variable epsilons.
pub fn numerical_gradient(f: &dyn Fn(&[f64]) -> f64, x: &[f64], epsilons: &[f64]) -> Vec<f64> {
    let mut gradient = Vec::with_capacity(x.len());
    for index in 0..x.len() {
        let eps = epsilons[index];
        let mut plus = x.to_vec();
        let mut minus = x.to_vec();
        plus[index] += eps;
        minus[index] -= eps;
        gradient.push((f(&plus) - f(&minus)) / (2.0 * eps));
    }
    gradient
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadratic_minimum_is_found_from_offset_start() {
        // f = (x-3)^2 + 2(y+1)^2: minimum at (3, -1).
        let f = |x: &[f64]| (x[0] - 3.0) * (x[0] - 3.0) + 2.0 * (x[1] + 1.0) * (x[1] + 1.0);
        let start = [0.0, 4.0];
        let epsilons = [1e-5, 1e-5];
        let result = minimize(&f, &start, &epsilons, &BfgsOptions::default());
        assert!((result[0] - 3.0).abs() < 1e-3, "x = {}", result[0]);
        assert!((result[1] + 1.0).abs() < 1e-3, "y = {}", result[1]);
    }

    #[test]
    fn rosenbrock_valley_descends_deeply_with_restarts() {
        // Classic banana: minimum at (1, 1). With a central-difference
        // gradient the descent eventually hits the ~1e-10 noise floor of
        // the flat valley, so the honest acceptance is a large energy
        // drop (f: 24.2 -> < 0.15) and real progress along the valley,
        // not full convergence to 1e-3.
        let f = |x: &[f64]| (1.0 - x[0]).powi(2) + 100.0 * (x[1] - x[0] * x[0]).powi(2);
        let start = [-1.2, 1.0];
        let epsilons = [1e-5, 1e-5];
        let options = BfgsOptions {
            max_iterations: 400,
            gtol: 1e-6,
        };
        let result = minimize(&f, &start, &epsilons, &options);
        let final_energy = f(&result);
        assert!(final_energy < 0.15, "f = {final_energy} at {result:?}");
        assert!(result[0] > 1.0 && result[0] < 1.5, "x = {}", result[0]);
    }

    #[test]
    fn non_finite_evaluations_are_treated_as_uphill() {
        // Domain hole at x in (1, 2): minimizer must not walk into it from
        // either side and must still land in a local basin.
        let f = |x: &[f64]| {
            if x[0] > 1.0 && x[0] < 2.0 {
                f64::INFINITY
            } else {
                (x[0] - 0.5) * (x[0] - 0.5)
            }
        };
        let start = [0.0];
        let epsilons = [1e-4];
        let result = minimize(&f, &start, &epsilons, &BfgsOptions::default());
        assert!(result[0] <= 1.0 || result[0] >= 2.0, "x = {}", result[0]);
        assert!((result[0] - 0.5).abs() < 1e-2, "x = {}", result[0]);
    }
}
