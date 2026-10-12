//! Gasteiger partial equalization of orbital electronegativity (PEOE).
//!
//! Iterative charge equalization with the classic element parameter table
//! and geometric damping. The Vina scoring functions never read partial
//! charges — these exist so prepared ligands round-trip through PDBQT files
//! that AD4-style tools and the file format itself expect.

use super::atom_type::{Bond, Element};

const CYCLES: usize = 8;

/// Electronegativity polynomial chi(q) = a + b*q + c*q^2 per element.
fn parameters(element: Element) -> Option<(f64, f64, f64)> {
    match element {
        Element::H => Some((7.17, 6.24, -0.56)),
        Element::C => Some((7.98, 9.18, 1.88)),
        Element::N => Some((11.54, 10.82, 1.36)),
        Element::O => Some((14.02, 10.95, 0.73)),
        Element::F => Some((14.66, 11.99, 0.35)),
        Element::P => Some((8.90, 6.70, 0.84)),
        Element::S => Some((10.14, 9.13, 1.38)),
        Element::Cl => Some((11.00, 9.69, 1.35)),
        Element::Br => Some((10.08, 8.47, 1.16)),
        Element::I => Some((9.90, 7.96, 0.96)),
        _ => None,
    }
}

/// Gasteiger PEOE charges, normalized to `total_charge`.
///
/// Atoms without parameters (metals, unusual elements) keep zero charge and
/// simply do not participate in the equalization.
pub fn gasteiger_charges(elements: &[Element], bonds: &[Bond], total_charge: f64) -> Vec<f64> {
    let mut charges = vec![0.0; elements.len()];
    let typed: Vec<Option<(f64, f64, f64)>> = elements
        .iter()
        .map(|element| parameters(*element))
        .collect();
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); elements.len()];
    for bond in bonds {
        neighbors[bond.a].push(bond.b);
        neighbors[bond.b].push(bond.a);
    }
    for cycle in 0..CYCLES {
        let damping = 0.5_f64.powi(cycle as i32);
        let mut increments = vec![0.0; elements.len()];
        for bond in bonds {
            let (Some(params_a), Some(params_b)) = (typed[bond.a], typed[bond.b]) else {
                continue;
            };
            let chi_a = params_a.0
                + params_a.1 * charges[bond.a]
                + params_a.2 * charges[bond.a] * charges[bond.a];
            let chi_b = params_b.0
                + params_b.1 * charges[bond.b]
                + params_b.2 * charges[bond.b] * charges[bond.b];
            // Charge flows from the less to the more electronegative atom,
            // damped each cycle so the equalization converges.
            let transfer = damping * (chi_b - chi_a) / (params_a.1 + params_b.1);
            increments[bond.a] += transfer;
            increments[bond.b] -= transfer;
        }
        for (charge, increment) in charges.iter_mut().zip(increments) {
            *charge += increment;
        }
    }
    let sum: f64 = charges.iter().sum();
    let shift = (total_charge - sum) / charges.len() as f64;
    for charge in &mut charges {
        *charge += shift;
    }
    charges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_polarizes_oxygen_negative_and_sums_to_zero() {
        let elements = vec![Element::O, Element::H, Element::H];
        let bonds = vec![Bond { a: 0, b: 1 }, Bond { a: 0, b: 2 }];
        let charges = gasteiger_charges(&elements, &bonds, 0.0);
        assert!(charges[0] < 0.0);
        assert!(charges[1] > 0.0 && charges[2] > 0.0);
        assert!((charges.iter().sum::<f64>()).abs() < 1e-9);
    }

    #[test]
    fn fluorine_pulls_charge_off_the_carbon_and_hydrogens() {
        // CH3-F: F most negative, C positive, H positive.
        let elements = vec![Element::C, Element::F, Element::H, Element::H, Element::H];
        let bonds = vec![
            Bond { a: 0, b: 1 },
            Bond { a: 0, b: 2 },
            Bond { a: 0, b: 3 },
            Bond { a: 0, b: 4 },
        ];
        let charges = gasteiger_charges(&elements, &bonds, 0.0);
        assert!(charges[1] < 0.0);
        assert!(charges[1] < charges[0]);
        assert!(charges[0] > 0.0);
        assert!(charges[2] > 0.0);
        assert!((charges.iter().sum::<f64>()).abs() < 1e-9);
    }

    #[test]
    fn formal_total_charge_is_preserved() {
        let elements = vec![Element::C, Element::C];
        let bonds = vec![Bond { a: 0, b: 1 }];
        let charges = gasteiger_charges(&elements, &bonds, -1.0);
        assert!((charges.iter().sum::<f64>() - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn metals_stay_neutral_without_parameters() {
        let elements = vec![Element::Zn, Element::O];
        let bonds = vec![Bond { a: 0, b: 1 }];
        let charges = gasteiger_charges(&elements, &bonds, 0.0);
        assert_eq!(charges[0], 0.0);
        assert!((charges.iter().sum::<f64>()).abs() < 1e-9);
    }
}
