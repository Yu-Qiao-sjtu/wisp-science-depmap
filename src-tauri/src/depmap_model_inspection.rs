//! Restricted per-model inspection: server may traverse the full cohort;
//! the client never receives ModelIDs, pagination, or exportable row dumps.

use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const MAX_INSPECTION_ROWS: usize = 20;
pub const MIN_GROUP_N: usize = 5;
pub const DISCLOSURE_BUDGET: usize = 40;

const ALLOWED_FIELDS: &[&str] = &["gene_effect", "mutation_group", "influence"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionPurpose {
    OutlierValidation,
    MissingnessCheck,
    GroupAssignment,
}

#[derive(Debug, Clone)]
pub struct InspectionRequest {
    pub purpose: InspectionPurpose,
    pub max_rows: usize,
    pub offset: Option<usize>,
    pub export_allowed: bool,
    pub fields: Vec<String>,
    pub query_id: String,
}

#[derive(Debug, Clone)]
pub struct ModelRow {
    pub model_id: String,
    pub gene_effect: f64,
    pub mutation_group: String,
    pub influence: f64,
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InspectionError {
    PaginationForbidden,
    ExportForbidden,
    PurposeRequired,
    GroupTooSmall,
    FieldNotAllowed(String),
    BudgetExceeded,
    OverlappingReconstruction,
}

impl InspectionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::PaginationForbidden => "pagination_forbidden",
            Self::ExportForbidden => "export_forbidden",
            Self::PurposeRequired => "purpose_required",
            Self::GroupTooSmall => "group_too_small",
            Self::FieldNotAllowed(_) => "field_not_allowed",
            Self::BudgetExceeded => "disclosure_budget_exceeded",
            Self::OverlappingReconstruction => "overlapping_query_reconstruction",
        }
    }
}

#[derive(Debug, Default)]
pub struct DisclosureLedger {
    /// True ModelIDs already shown under any query (internal only).
    disclosed_models: HashSet<String>,
    /// Query-scoped pseudonyms already issued.
    issued_pseudonyms: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct AggregateDiagnostics {
    pub cohort_n: usize,
    pub non_missing_n: usize,
    pub mean: f64,
    pub median: f64,
    pub sd: f64,
    pub outlier_count: usize,
    pub model_ids_returned: bool,
}

#[derive(Debug, Clone)]
pub struct RestrictedRow {
    pub pseudonym: String,
    pub values: Vec<(String, String)>,
}

pub fn aggregate_diagnostics(rows: &[ModelRow]) -> AggregateDiagnostics {
    let values: Vec<f64> = rows
        .iter()
        .filter(|row| !row.missing)
        .map(|row| row.gene_effect)
        .collect();
    let n = values.len();
    let mean = if n == 0 {
        0.0
    } else {
        values.iter().sum::<f64>() / n as f64
    };
    let mut sorted = values.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if n == 0 {
        0.0
    } else if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    };
    let sd = if n < 2 {
        0.0
    } else {
        let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        var.sqrt()
    };
    let outlier_count = values
        .iter()
        .filter(|v| sd > 0.0 && (**v - mean).abs() > 2.0 * sd)
        .count();
    AggregateDiagnostics {
        cohort_n: rows.len(),
        non_missing_n: n,
        mean,
        median,
        sd,
        outlier_count,
        model_ids_returned: false,
    }
}

pub fn restricted_inspect(
    rows: &[ModelRow],
    request: &InspectionRequest,
    ledger: &mut DisclosureLedger,
) -> Result<Vec<RestrictedRow>, InspectionError> {
    if request.query_id.trim().is_empty() {
        return Err(InspectionError::PurposeRequired);
    }
    if request.offset.is_some() {
        return Err(InspectionError::PaginationForbidden);
    }
    if request.export_allowed {
        return Err(InspectionError::ExportForbidden);
    }
    if rows.len() < MIN_GROUP_N {
        return Err(InspectionError::GroupTooSmall);
    }
    for field in &request.fields {
        if !ALLOWED_FIELDS.contains(&field.as_str()) {
            return Err(InspectionError::FieldNotAllowed(field.clone()));
        }
    }
    let cap = request.max_rows.min(MAX_INSPECTION_ROWS);
    let mut ranked: Vec<&ModelRow> = rows.iter().filter(|row| !row.missing).collect();
    match request.purpose {
        InspectionPurpose::OutlierValidation => {
            ranked.sort_by(|a, b| {
                b.influence
                    .partial_cmp(&a.influence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        InspectionPurpose::MissingnessCheck => {
            ranked = rows.iter().filter(|row| row.missing).collect();
        }
        InspectionPurpose::GroupAssignment => {
            ranked.sort_by(|a, b| a.mutation_group.cmp(&b.mutation_group));
        }
    }
    let selected: Vec<&ModelRow> = ranked.into_iter().take(cap).collect();
    if ledger.disclosed_models.len() + selected.len() > DISCLOSURE_BUDGET {
        return Err(InspectionError::BudgetExceeded);
    }
    let overlap = selected
        .iter()
        .filter(|row| ledger.disclosed_models.contains(&row.model_id))
        .count();
    if overlap > 0 && overlap * 2 >= selected.len() {
        return Err(InspectionError::OverlappingReconstruction);
    }
    let mut out = Vec::new();
    for row in selected {
        ledger.disclosed_models.insert(row.model_id.clone());
        let pseudonym = query_scoped_pseudonym(&request.query_id, &row.model_id);
        if !ledger.issued_pseudonyms.insert(pseudonym.clone()) {
            return Err(InspectionError::OverlappingReconstruction);
        }
        let mut values = Vec::new();
        for field in &request.fields {
            let value = match field.as_str() {
                "gene_effect" => format!("{:.6}", row.gene_effect),
                "mutation_group" => row.mutation_group.clone(),
                "influence" => format!("{:.6}", row.influence),
                _ => continue,
            };
            values.push((field.clone(), value));
        }
        out.push(RestrictedRow {
            pseudonym,
            values,
        });
    }
    Ok(out)
}

fn query_scoped_pseudonym(query_id: &str, model_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(query_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(model_id.as_bytes());
    let digest = hasher.finalize();
    format!("m{:x}", u32::from_be_bytes(digest[0..4].try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cohort(n: usize) -> Vec<ModelRow> {
        (0..n)
            .map(|i| ModelRow {
                model_id: format!("ACH-{i:06}"),
                gene_effect: -0.2 - (i as f64) * 0.01,
                mutation_group: if i % 5 == 0 { "mut" } else { "wt" }.into(),
                influence: i as f64,
                missing: i == 3,
            })
            .collect()
    }

    fn request(query: &str) -> InspectionRequest {
        InspectionRequest {
            purpose: InspectionPurpose::OutlierValidation,
            max_rows: 20,
            offset: None,
            export_allowed: false,
            fields: vec!["gene_effect".into(), "mutation_group".into()],
            query_id: query.into(),
        }
    }

    #[test]
    fn aggregates_cover_the_full_cohort_without_model_ids() {
        let stats = aggregate_diagnostics(&cohort(1208));
        assert_eq!(stats.cohort_n, 1208);
        assert_eq!(stats.non_missing_n, 1207);
        assert!(!stats.model_ids_returned);
    }

    #[test]
    fn restricted_inspection_caps_rows_and_pseudonymizes() {
        let mut ledger = DisclosureLedger::default();
        let rows = restricted_inspect(&cohort(80), &request("q1"), &mut ledger).unwrap();
        assert!(rows.len() <= MAX_INSPECTION_ROWS);
        assert!(rows.iter().all(|row| row.pseudonym.starts_with('m')));
        assert!(rows
            .iter()
            .all(|row| !row.pseudonym.contains("ACH-") && !row.values.iter().any(|(_, v)| v.contains("ACH-"))));
    }

    #[test]
    fn pagination_and_export_are_blocked() {
        let mut ledger = DisclosureLedger::default();
        let mut paged = request("q1");
        paged.offset = Some(20);
        assert_eq!(
            restricted_inspect(&cohort(80), &paged, &mut ledger).unwrap_err().code(),
            "pagination_forbidden"
        );
        let mut export = request("q2");
        export.export_allowed = true;
        assert_eq!(
            restricted_inspect(&cohort(80), &export, &mut ledger).unwrap_err().code(),
            "export_forbidden"
        );
    }

    #[test]
    fn identifiers_do_not_join_across_queries() {
        let mut ledger = DisclosureLedger::default();
        let a = restricted_inspect(&cohort(80), &request("alpha"), &mut ledger).unwrap();
        let mut ledger_b = DisclosureLedger::default();
        let b = restricted_inspect(&cohort(80), &request("beta"), &mut ledger_b).unwrap();
        assert_ne!(a[0].pseudonym, b[0].pseudonym);
    }

    #[test]
    fn overlapping_followup_cannot_enumerate_the_cohort() {
        let mut ledger = DisclosureLedger::default();
        let first = restricted_inspect(&cohort(80), &request("q1"), &mut ledger).unwrap();
        assert!(!first.is_empty());
        let err = restricted_inspect(&cohort(80), &request("q1-repeat"), &mut ledger).unwrap_err();
        assert_eq!(err.code(), "overlapping_query_reconstruction");
    }
}
