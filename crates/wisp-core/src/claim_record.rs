//! Versioned scientific claims checked against durable records before presentation.
//!
//! Models may still write prose. Only `ClaimRecord`s are hard-checked. Interpretations
//! and hypotheses stay unlabeled prose unless they masquerade as measured values.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CLAIM_RECORD_CONTRACT: &str = "wisp.claim-record.v1";
pub const CLAIM_RECORD_SCHEMA_VERSION: u32 = 1;

const VALUE_TOLERANCE: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    MeasuredFact,
    DerivedResult,
    LiteratureStatement,
    Interpretation,
    Hypothesis,
}

impl ClaimKind {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimSourceKind {
    Evidence,
    Run,
    Artifact,
    Paper,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimSourceRef {
    pub kind: ClaimSourceKind,
    pub id: String,
    /// Digest of the referenced record at claim time. Resume/compaction must
    /// still match the catalog's current digest.
    pub source_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimRecord {
    pub contract: String,
    pub schema_version: u32,
    pub claim_id: String,
    pub kind: ClaimKind,
    pub subject: Option<String>,
    pub predicate: Option<String>,
    pub scope: Option<String>,
    pub metric: Option<String>,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub direction: Option<String>,
    pub p_value: Option<f64>,
    pub release: Option<String>,
    pub sample_count: Option<u64>,
    pub coverage_status: Option<String>,
    pub sources: Vec<ClaimSourceRef>,
    #[serde(default)]
    pub text: Option<String>,
}

impl ClaimRecord {
    pub fn new(claim_id: impl Into<String>, kind: ClaimKind) -> Self {
        Self {
            contract: CLAIM_RECORD_CONTRACT.into(),
            schema_version: CLAIM_RECORD_SCHEMA_VERSION,
            claim_id: claim_id.into(),
            kind,
            subject: None,
            predicate: None,
            scope: None,
            metric: None,
            value: None,
            unit: None,
            direction: None,
            p_value: None,
            release: None,
            sample_count: None,
            coverage_status: None,
            sources: Vec::new(),
            text: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundedEvidence {
    pub evidence_id: String,
    pub evidence_state: String,
    pub source_version: String,
    pub provider_version: Option<String>,
    pub subject: Option<String>,
    pub scope: Option<String>,
    pub metric: Option<String>,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub direction: Option<String>,
    pub p_value: Option<f64>,
    pub sample_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundedRun {
    pub run_id: String,
    pub source_version: String,
    pub status: String,
    pub release: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundedArtifact {
    pub artifact_id: String,
    pub source_version: String,
    pub producing_run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundedPaper {
    pub paper_id: String,
    pub source_version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClaimGroundingCatalog {
    pub evidence: Vec<GroundedEvidence>,
    pub runs: Vec<GroundedRun>,
    pub artifacts: Vec<GroundedArtifact>,
    pub papers: Vec<GroundedPaper>,
}

pub type ClaimPersistHook = std::sync::Arc<dyn Fn(&[ClaimRecord]) + Send + Sync>;

impl ClaimGroundingCatalog {
    pub fn evidence(&self, id: &str) -> Option<&GroundedEvidence> {
        self.evidence.iter().find(|row| row.evidence_id == id)
    }

    pub fn run(&self, id: &str) -> Option<&GroundedRun> {
        self.runs.iter().find(|row| row.run_id == id)
    }

    pub fn artifact(&self, id: &str) -> Option<&GroundedArtifact> {
        self.artifacts.iter().find(|row| row.artifact_id == id)
    }

    pub fn paper(&self, id: &str) -> Option<&GroundedPaper> {
        self.papers.iter().find(|row| row.paper_id == id)
    }

    pub fn push_ledger_evidence(
        &mut self,
        evidence_id: impl Into<String>,
        evidence_state: impl Into<String>,
        provider_version: Option<String>,
        semantics_json: &str,
        compact_payload_json: &str,
    ) {
        let semantics: Value = serde_json::from_str(semantics_json).unwrap_or(Value::Null);
        self.evidence.push(GroundedEvidence {
            evidence_id: evidence_id.into(),
            evidence_state: evidence_state.into(),
            source_version: source_digest(compact_payload_json),
            provider_version,
            subject: string_field(&semantics, &["subject", "entity", "gene"]),
            scope: string_field(&semantics, &["scope", "lineage"]),
            metric: string_field(&semantics, &["metric"]),
            value: number_field(&semantics, &["value", "effect"]),
            unit: string_field(&semantics, &["unit"]),
            direction: string_field(&semantics, &["direction"]),
            p_value: number_field(&semantics, &["p_value", "pvalue"]),
            sample_count: number_field(&semantics, &["sample_count", "n"])
                .and_then(|n| (n >= 0.0).then_some(n as u64)),
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimCheckCode {
    Ok,
    MissingSource,
    SourceMissing,
    SourceVersionDetached,
    ValueMismatch,
    DirectionMismatch,
    StaleRelease,
    SampleCountMismatch,
    CoverageStatusMisread,
    KindMasquerade,
    EntityMismatch,
    MetricMismatch,
    EvidenceNotFound,
    RunNotValidated,
}

impl ClaimCheckCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::MissingSource => "missing_source",
            Self::SourceMissing => "source_missing",
            Self::SourceVersionDetached => "source_version_detached",
            Self::ValueMismatch => "value_mismatch",
            Self::DirectionMismatch => "direction_mismatch",
            Self::StaleRelease => "stale_release",
            Self::SampleCountMismatch => "sample_count_mismatch",
            Self::CoverageStatusMisread => "coverage_status_misread",
            Self::KindMasquerade => "kind_masquerade",
            Self::EntityMismatch => "entity_mismatch",
            Self::MetricMismatch => "metric_mismatch",
            Self::EvidenceNotFound => "evidence_not_found",
            Self::RunNotValidated => "run_not_validated",
        }
    }

    pub fn is_hard_failure(self) -> bool {
        self != Self::Ok
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimCheck {
    pub claim_id: String,
    pub code: ClaimCheckCode,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimValidationReport {
    pub contract: String,
    pub checks: Vec<ClaimCheck>,
}

impl ClaimValidationReport {
    pub fn hard_failures(&self) -> impl Iterator<Item = &ClaimCheck> {
        self.checks
            .iter()
            .filter(|check| check.code.is_hard_failure())
    }

    pub fn typed_reason(&self) -> Option<String> {
        let failures: Vec<String> = self
            .hard_failures()
            .map(|check| format!("{}:{}", check.claim_id, check.code.as_str()))
            .collect();
        if failures.is_empty() {
            None
        } else {
            Some(format!("claim grounding failed: {}", failures.join(",")))
        }
    }
}

pub fn claims_from_output(output: &Value) -> Result<Vec<ClaimRecord>, String> {
    let Some(claims) = output.get("claims") else {
        return Ok(Vec::new());
    };
    let Some(array) = claims.as_array() else {
        return Err("claim grounding failed: claims_not_array".into());
    };
    let mut out = Vec::with_capacity(array.len());
    for (index, value) in array.iter().enumerate() {
        match serde_json::from_value::<ClaimRecord>(value.clone()) {
            Ok(claim) => out.push(claim),
            Err(error) => {
                return Err(format!(
                    "claim grounding failed: malformed_claim:{index}:{error}"
                ));
            }
        }
    }
    Ok(out)
}

pub fn validate_claims(
    claims: &[ClaimRecord],
    catalog: &ClaimGroundingCatalog,
) -> ClaimValidationReport {
    let checks = claims
        .iter()
        .map(|claim| validate_one(claim, catalog))
        .collect();
    ClaimValidationReport {
        contract: CLAIM_RECORD_CONTRACT.into(),
        checks,
    }
}

pub fn rewrite_unsupported_claims(
    claims: &[ClaimRecord],
    report: &ClaimValidationReport,
) -> Vec<ClaimRecord> {
    claims
        .iter()
        .map(|claim| {
            let failed = report
                .checks
                .iter()
                .find(|check| check.claim_id == claim.claim_id && check.code.is_hard_failure());
            match failed {
                Some(check) => {
                    let mut rewritten = claim.clone();
                    rewritten.kind = ClaimKind::Interpretation;
                    rewritten.value = None;
                    rewritten.p_value = None;
                    rewritten.direction = None;
                    rewritten.text = Some(format!(
                        "Unsupported claim {} withheld ({})",
                        claim.claim_id,
                        check.code.as_str()
                    ));
                    rewritten
                }
                None => claim.clone(),
            }
        })
        .collect()
}

fn validate_one(claim: &ClaimRecord, catalog: &ClaimGroundingCatalog) -> ClaimCheck {
    if claim.kind == ClaimKind::Interpretation || claim.kind == ClaimKind::Hypothesis {
        if claim.value.is_some() || claim.p_value.is_some() {
            return fail(
                claim,
                ClaimCheckCode::KindMasquerade,
                "interpretation or hypothesis must not carry a measured value",
            );
        }
        return ok(claim);
    }

    if claim.kind == ClaimKind::LiteratureStatement {
        return validate_literature(claim, catalog);
    }

    if claim.sources.is_empty() {
        return fail(
            claim,
            ClaimCheckCode::MissingSource,
            "measured or derived claims must cite Evidence, Run, or Artifact records",
        );
    }

    for source in &claim.sources {
        match source.kind {
            ClaimSourceKind::Evidence => {
                let Some(row) = catalog.evidence(&source.id) else {
                    return fail(
                        claim,
                        ClaimCheckCode::EvidenceNotFound,
                        "referenced evidence is absent from the local ledger",
                    );
                };
                if row.source_version != source.source_version {
                    return fail(
                        claim,
                        ClaimCheckCode::SourceVersionDetached,
                        "claim source_version does not match the current evidence digest",
                    );
                }
                if let Some(check) = compare_identity(claim, row) {
                    return check;
                }
                if let Some(check) = compare_numerics(claim, row) {
                    return check;
                }
                if let Some(check) = compare_coverage(claim, row) {
                    return check;
                }
            }
            ClaimSourceKind::Run => {
                let Some(row) = catalog.run(&source.id) else {
                    return fail(
                        claim,
                        ClaimCheckCode::SourceMissing,
                        "referenced run is absent",
                    );
                };
                if row.source_version != source.source_version {
                    return fail(
                        claim,
                        ClaimCheckCode::SourceVersionDetached,
                        "claim source_version does not match the current run digest",
                    );
                }
                if !run_is_validated(&row.status) {
                    return fail(
                        claim,
                        ClaimCheckCode::RunNotValidated,
                        "claim cites a run that is not in a validated terminal state",
                    );
                }
                if let (Some(claimed), Some(actual)) =
                    (claim.release.as_ref(), row.release.as_ref())
                {
                    if claimed != actual {
                        return fail(
                            claim,
                            ClaimCheckCode::StaleRelease,
                            "claim release does not match the run release",
                        );
                    }
                }
            }
            ClaimSourceKind::Artifact => {
                let Some(row) = catalog.artifact(&source.id) else {
                    return fail(
                        claim,
                        ClaimCheckCode::SourceMissing,
                        "referenced artifact is absent",
                    );
                };
                if row.source_version != source.source_version {
                    return fail(
                        claim,
                        ClaimCheckCode::SourceVersionDetached,
                        "claim source_version does not match the current artifact digest",
                    );
                }
            }
            ClaimSourceKind::Paper => {
                return fail(
                    claim,
                    ClaimCheckCode::MissingSource,
                    "measured or derived claims cannot cite Paper records",
                );
            }
        }
    }
    ok(claim)
}

fn validate_literature(claim: &ClaimRecord, catalog: &ClaimGroundingCatalog) -> ClaimCheck {
    if claim.sources.is_empty() {
        return fail(
            claim,
            ClaimCheckCode::MissingSource,
            "literature statements must cite a Paper record",
        );
    }
    for source in &claim.sources {
        if source.kind != ClaimSourceKind::Paper {
            return fail(
                claim,
                ClaimCheckCode::MissingSource,
                "literature statements must cite a Paper record",
            );
        }
        let Some(row) = catalog.paper(&source.id) else {
            return fail(
                claim,
                ClaimCheckCode::SourceMissing,
                "referenced paper is absent",
            );
        };
        if row.source_version != source.source_version {
            return fail(
                claim,
                ClaimCheckCode::SourceVersionDetached,
                "claim source_version does not match the current paper digest",
            );
        }
    }
    ok(claim)
}

fn compare_identity(claim: &ClaimRecord, row: &GroundedEvidence) -> Option<ClaimCheck> {
    if let (Some(claimed), Some(actual)) = (claim.subject.as_ref(), row.subject.as_ref()) {
        if !eq_ignore_ascii(claimed, actual) {
            return Some(fail(
                claim,
                ClaimCheckCode::EntityMismatch,
                "claim subject does not match the evidence entity",
            ));
        }
    }
    if let (Some(claimed), Some(actual)) = (claim.scope.as_ref(), row.scope.as_ref()) {
        if !eq_ignore_ascii(claimed, actual) {
            return Some(fail(
                claim,
                ClaimCheckCode::EntityMismatch,
                "claim scope does not match the evidence scope",
            ));
        }
    }
    if let (Some(claimed), Some(actual)) = (claim.metric.as_ref(), row.metric.as_ref()) {
        if !eq_ignore_ascii(claimed, actual) {
            return Some(fail(
                claim,
                ClaimCheckCode::MetricMismatch,
                "claim metric does not match the evidence metric",
            ));
        }
    }
    if let (Some(claimed), Some(actual)) = (claim.release.as_ref(), row.provider_version.as_ref()) {
        if claimed != actual {
            return Some(fail(
                claim,
                ClaimCheckCode::StaleRelease,
                "claim release does not match the evidence provider version",
            ));
        }
    }
    None
}

fn compare_numerics(claim: &ClaimRecord, row: &GroundedEvidence) -> Option<ClaimCheck> {
    if let Some(claimed) = claim.value {
        match row.value {
            Some(actual) if approx_eq(claimed, actual) => {}
            Some(_) => {
                return Some(fail(
                    claim,
                    ClaimCheckCode::ValueMismatch,
                    "claim value does not match the referenced record",
                ));
            }
            None => {
                return Some(fail(
                    claim,
                    ClaimCheckCode::ValueMismatch,
                    "referenced evidence has no value for the claimed numeric field",
                ));
            }
        }
    }
    if let Some(claimed) = claim.p_value {
        match row.p_value {
            Some(actual) if approx_eq(claimed, actual) => {}
            Some(_) => {
                return Some(fail(
                    claim,
                    ClaimCheckCode::ValueMismatch,
                    "claim p-value does not match the referenced record",
                ));
            }
            None => {
                return Some(fail(
                    claim,
                    ClaimCheckCode::ValueMismatch,
                    "referenced evidence has no p-value for the claimed numeric field",
                ));
            }
        }
    }
    if let Some(claimed) = claim.direction.as_ref() {
        match row.direction.as_ref() {
            Some(actual) if normalize_direction(claimed) == normalize_direction(actual) => {}
            Some(_) => {
                return Some(fail(
                    claim,
                    ClaimCheckCode::DirectionMismatch,
                    "claim direction does not match the referenced record",
                ));
            }
            None => {
                return Some(fail(
                    claim,
                    ClaimCheckCode::DirectionMismatch,
                    "referenced evidence has no direction for the claimed field",
                ));
            }
        }
    }
    if let Some(claimed) = claim.sample_count {
        match row.sample_count {
            Some(actual) if claimed == actual => {}
            Some(_) => {
                return Some(fail(
                    claim,
                    ClaimCheckCode::SampleCountMismatch,
                    "claim sample_count does not match the referenced record",
                ));
            }
            None => {
                return Some(fail(
                    claim,
                    ClaimCheckCode::SampleCountMismatch,
                    "referenced evidence has no sample_count for the claimed field",
                ));
            }
        }
    }
    if let (Some(claimed), Some(actual)) = (claim.unit.as_ref(), row.unit.as_ref()) {
        if !eq_ignore_ascii(claimed, actual) {
            return Some(fail(
                claim,
                ClaimCheckCode::MetricMismatch,
                "claim unit does not match the referenced record",
            ));
        }
    }
    None
}

fn compare_coverage(claim: &ClaimRecord, row: &GroundedEvidence) -> Option<ClaimCheck> {
    let status = claim
        .coverage_status
        .as_deref()
        .unwrap_or(row.evidence_state.as_str());
    if status != row.evidence_state {
        return Some(fail(
            claim,
            ClaimCheckCode::CoverageStatusMisread,
            "claim coverage status does not match the evidence state",
        ));
    }
    if asserts_negative_biology(claim) && !is_retained_positive(status) {
        return Some(fail(
            claim,
            ClaimCheckCode::CoverageStatusMisread,
            "non-retained or coverage statuses are not biological negatives",
        ));
    }
    None
}

fn asserts_negative_biology(claim: &ClaimRecord) -> bool {
    let pred = claim.predicate.as_deref().unwrap_or("");
    let dir = claim.direction.as_deref().unwrap_or("");
    let text = claim.text.as_deref().unwrap_or("");
    let haystack = format!("{pred} {dir} {text}").to_ascii_lowercase();
    haystack.contains("negative")
        || haystack.contains("deplet")
        || haystack.contains("dependenc")
        || haystack.contains("essential")
        || normalize_direction(dir) == "down"
}

fn is_retained_positive(status: &str) -> bool {
    matches!(status, "FOUND" | "observed" | "validated")
}

fn run_is_validated(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "success" | "completed" | "validated" | "harvested"
    )
}

fn normalize_direction(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "up" | "positive" | "enrichment" | "greater" => "up".into(),
        "down" | "negative" | "depletion" | "less" => "down".into(),
        other => other.to_string(),
    }
}

fn eq_ignore_ascii(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn approx_eq(left: f64, right: f64) -> bool {
    if left == right {
        return true;
    }
    (left - right).abs() <= VALUE_TOLERANCE * (1.0 + right.abs())
}

fn ok(claim: &ClaimRecord) -> ClaimCheck {
    ClaimCheck {
        claim_id: claim.claim_id.clone(),
        code: ClaimCheckCode::Ok,
        reason: String::new(),
    }
}

fn fail(claim: &ClaimRecord, code: ClaimCheckCode, reason: &str) -> ClaimCheck {
    ClaimCheck {
        claim_id: claim.claim_id.clone(),
        code,
        reason: reason.into(),
    }
}

fn source_digest(payload: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(|text| text.to_string())
    })
}

fn number_field(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> ClaimGroundingCatalog {
        ClaimGroundingCatalog {
            evidence: vec![GroundedEvidence {
                evidence_id: "ev-1".into(),
                evidence_state: "FOUND".into(),
                source_version: "digest-a".into(),
                provider_version: Some("26Q1".into()),
                subject: Some("GENE_A".into()),
                scope: Some("LineageX".into()),
                metric: Some("chronos".into()),
                value: Some(-0.42),
                unit: Some("chronos".into()),
                direction: Some("depletion".into()),
                p_value: Some(0.001),
                sample_count: Some(12),
            }],
            runs: vec![GroundedRun {
                run_id: "run-1".into(),
                source_version: "run-digest".into(),
                status: "succeeded".into(),
                release: Some("26Q1".into()),
            }],
            artifacts: vec![GroundedArtifact {
                artifact_id: "art-1".into(),
                source_version: "art-digest".into(),
                producing_run_id: Some("run-1".into()),
            }],
            papers: vec![GroundedPaper {
                paper_id: "paper-1".into(),
                source_version: "paper-digest".into(),
            }],
        }
    }

    fn measured() -> ClaimRecord {
        let mut claim = ClaimRecord::new("c1", ClaimKind::MeasuredFact);
        claim.subject = Some("GENE_A".into());
        claim.scope = Some("LineageX".into());
        claim.metric = Some("chronos".into());
        claim.value = Some(-0.42);
        claim.p_value = Some(0.001);
        claim.direction = Some("depletion".into());
        claim.release = Some("26Q1".into());
        claim.sample_count = Some(12);
        claim.coverage_status = Some("FOUND".into());
        claim.sources = vec![ClaimSourceRef {
            kind: ClaimSourceKind::Evidence,
            id: "ev-1".into(),
            source_version: "digest-a".into(),
        }];
        claim
    }

    #[test]
    fn correct_claims_against_local_records_pass() {
        let report = validate_claims(&[measured()], &catalog());
        assert!(report.typed_reason().is_none());
        assert_eq!(report.checks[0].code, ClaimCheckCode::Ok);
    }

    #[test]
    fn changed_p_value_fails_before_presentation() {
        let mut claim = measured();
        claim.p_value = Some(0.04);
        let report = validate_claims(&[claim], &catalog());
        assert!(report.typed_reason().unwrap().contains("value_mismatch"));
    }

    #[test]
    fn wrong_direction_fails() {
        let mut claim = measured();
        claim.direction = Some("up".into());
        let report = validate_claims(&[claim], &catalog());
        assert!(report
            .typed_reason()
            .unwrap()
            .contains("direction_mismatch"));
    }

    #[test]
    fn stale_release_fails() {
        let mut claim = measured();
        claim.release = Some("25Q2".into());
        let report = validate_claims(&[claim], &catalog());
        assert!(report.typed_reason().unwrap().contains("stale_release"));
    }

    #[test]
    fn not_retained_is_not_a_biological_negative() {
        let mut rows = catalog();
        rows.evidence[0].evidence_state = "NOT_RETAINED".into();
        rows.evidence[0].value = None;
        rows.evidence[0].p_value = None;
        rows.evidence[0].direction = None;
        let mut claim = measured();
        claim.value = None;
        claim.p_value = None;
        claim.coverage_status = Some("NOT_RETAINED".into());
        claim.predicate = Some("is_dependency".into());
        claim.direction = None;
        claim.text = Some("this is a negative essential hit".into());
        let report = validate_claims(&[claim], &rows);
        assert!(report
            .typed_reason()
            .unwrap()
            .contains("coverage_status_misread"));
    }

    #[test]
    fn interpretation_is_not_forced_into_a_numeric_schema() {
        let mut claim = ClaimRecord::new("c-interp", ClaimKind::Interpretation);
        claim.text = Some("this may suggest a follow-up experiment".into());
        let report = validate_claims(&[claim], &catalog());
        assert!(report.typed_reason().is_none());
    }

    #[test]
    fn interpretation_cannot_masquerade_as_a_measured_value() {
        let mut claim = ClaimRecord::new("c-bad", ClaimKind::Interpretation);
        claim.p_value = Some(0.001);
        let report = validate_claims(&[claim], &catalog());
        assert!(report.typed_reason().unwrap().contains("kind_masquerade"));
    }

    #[test]
    fn detached_source_version_fails_closed() {
        let mut claim = measured();
        claim.sources[0].source_version = "stale-digest".into();
        let report = validate_claims(&[claim], &catalog());
        assert!(report
            .typed_reason()
            .unwrap()
            .contains("source_version_detached"));
    }

    #[test]
    fn rewrite_drops_only_the_unsupported_claim_numerics() {
        let mut bad = measured();
        bad.claim_id = "c-bad".into();
        bad.p_value = Some(0.9);
        let good = measured();
        let claims = vec![good, bad];
        let report = validate_claims(&claims, &catalog());
        let rewritten = rewrite_unsupported_claims(&claims, &report);
        assert_eq!(rewritten[0].kind, ClaimKind::MeasuredFact);
        assert_eq!(rewritten[0].p_value, Some(0.001));
        assert_eq!(rewritten[1].kind, ClaimKind::Interpretation);
        assert!(rewritten[1].p_value.is_none());
    }

    #[test]
    fn prose_without_claims_is_not_checked() {
        let output = serde_json::json!({"answer": "hello"});
        assert!(claims_from_output(&output).unwrap().is_empty());
    }

    #[test]
    fn malformed_claim_entries_are_rejected() {
        let output = serde_json::json!({"claims": [{"kind": "measured_fact"}]});
        let err = claims_from_output(&output).unwrap_err();
        assert!(err.contains("malformed_claim"), "{err}");
    }

    #[test]
    fn claimed_numeric_without_evidence_field_fails() {
        let mut rows = catalog();
        rows.evidence[0].p_value = None;
        let report = validate_claims(&[measured()], &rows);
        assert!(report.typed_reason().unwrap().contains("value_mismatch"));
    }

    #[test]
    fn measured_claims_cannot_cite_only_a_paper() {
        let mut claim = ClaimRecord::new("c-paper", ClaimKind::MeasuredFact);
        claim.p_value = Some(0.001);
        claim.sources = vec![ClaimSourceRef {
            kind: ClaimSourceKind::Paper,
            id: "paper-1".into(),
            source_version: "paper-digest".into(),
        }];
        let report = validate_claims(&[claim], &catalog());
        assert!(report.typed_reason().unwrap().contains("missing_source"));
    }
}
