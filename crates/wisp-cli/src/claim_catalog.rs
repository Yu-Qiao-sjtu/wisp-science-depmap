use std::sync::Arc;
use wisp_core::{
    ClaimGroundingCatalog, ClaimRecord, ContextManager, GroundedArtifact, GroundedRun,
};
use wisp_store::Store;

pub async fn install_session_claim_grounding(
    ctx: &mut ContextManager,
    store: &Store,
    project_id: &str,
    frame_id: &str,
) {
    let mut catalog = ClaimGroundingCatalog::default();
    if let Ok(rows) = store
        .list_scientific_evidence(project_id, frame_id, 100)
        .await
    {
        for row in rows {
            catalog.push_ledger_evidence(
                row.evidence_id,
                row.evidence_state,
                row.provider_version,
                &row.semantics_json,
                &row.compact_payload_json,
            );
        }
    }
    if let Ok(runs) = store.list_runs_by_project(project_id).await {
        for run in runs {
            if run.frame_id.as_deref().is_none_or(|id| id == frame_id) {
                catalog.runs.push(GroundedRun {
                    run_id: run.id.clone(),
                    source_version: run.env_snapshot_json.clone(),
                    status: run.status.as_str().into(),
                    release: None,
                });
            }
        }
    }
    if let Ok(artifacts) = store.list_artifacts(frame_id).await {
        for (artifact_id, ..) in artifacts {
            catalog.artifacts.push(GroundedArtifact {
                artifact_id: artifact_id.clone(),
                source_version: artifact_id,
                producing_run_id: None,
            });
        }
    }
    ctx.set_claim_catalog(Some(catalog));
    let store = store.clone();
    let project_id = project_id.to_string();
    let frame_id = frame_id.to_string();
    ctx.set_claim_persist(Some(Arc::new(move |claims: &[ClaimRecord]| {
        let store = store.clone();
        let project_id = project_id.clone();
        let frame_id = frame_id.clone();
        let claims = claims.to_vec();
        tokio::spawn(async move {
            for claim in claims {
                let payload = serde_json::to_string(&claim).unwrap_or_default();
                let versions = serde_json::to_string(&claim.sources).unwrap_or_default();
                let _ = store
                    .persist_claim_record(
                        &claim.claim_id,
                        &project_id,
                        &frame_id,
                        claim.schema_version as i64,
                        &payload,
                        &versions,
                    )
                    .await;
            }
        });
    })));
}
