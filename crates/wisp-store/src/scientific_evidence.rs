use crate::{canonical_json, canonical_json_sha256, Store};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sqlx::Row;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScientificEvidenceRecord {
    pub id: String,
    pub project_id: String,
    pub frame_id: String,
    pub evidence_id: String,
    pub provider: String,
    pub provider_version: Option<String>,
    pub tool_name: String,
    pub canonical_arguments_json: String,
    pub evidence_state: String,
    pub semantics_json: String,
    pub provenance_json: String,
    pub compact_payload_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug)]
pub struct NewScientificEvidence<'a> {
    pub project_id: &'a str,
    pub frame_id: &'a str,
    pub provider: &'a str,
    pub provider_version: Option<&'a str>,
    pub tool_name: &'a str,
    pub arguments: &'a Value,
    pub evidence_state: &'a str,
    pub semantics: &'a Value,
    pub provenance: &'a Value,
    pub compact_payload: &'a Value,
}

fn required(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{name} is required");
    }
    Ok(())
}

impl Store {
    pub async fn upsert_scientific_evidence(
        &self,
        evidence: NewScientificEvidence<'_>,
    ) -> Result<ScientificEvidenceRecord> {
        required(evidence.project_id, "project_id")?;
        required(evidence.frame_id, "frame_id")?;
        required(evidence.provider, "provider")?;
        required(evidence.tool_name, "tool_name")?;
        required(evidence.evidence_state, "evidence_state")?;

        let canonical_arguments_json = canonical_json(evidence.arguments);
        let identity = json!({
            "provider": evidence.provider,
            "provider_version": evidence.provider_version,
            "tool_name": evidence.tool_name,
            "arguments": evidence.arguments
        });
        let evidence_id = canonical_json_sha256(&identity).1;
        let id = format!(
            "scientific-evidence-{}",
            canonical_json_sha256(&json!({
                "project_id": evidence.project_id,
                "frame_id": evidence.frame_id,
                "evidence_id": evidence_id
            }))
            .1
        );
        let semantics_json = canonical_json(evidence.semantics);
        let provenance_json = canonical_json(evidence.provenance);
        let compact_payload_json = canonical_json(evidence.compact_payload);
        let now = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            "INSERT INTO scientific_evidence_ledger(\
               id,project_id,frame_id,evidence_id,provider,provider_version,tool_name,\
               canonical_arguments_json,evidence_state,semantics_json,provenance_json,\
               compact_payload_json,created_at,updated_at) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
             ON CONFLICT(project_id,frame_id,evidence_id) DO UPDATE SET \
               evidence_state=excluded.evidence_state,semantics_json=excluded.semantics_json,\
               provenance_json=excluded.provenance_json,\
               compact_payload_json=excluded.compact_payload_json,updated_at=excluded.updated_at",
        )
        .bind(&id)
        .bind(evidence.project_id)
        .bind(evidence.frame_id)
        .bind(&evidence_id)
        .bind(evidence.provider)
        .bind(evidence.provider_version)
        .bind(evidence.tool_name)
        .bind(&canonical_arguments_json)
        .bind(evidence.evidence_state)
        .bind(&semantics_json)
        .bind(&provenance_json)
        .bind(&compact_payload_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("persist scientific evidence")?;

        self.get_scientific_evidence(evidence.project_id, evidence.frame_id, &evidence_id)
            .await?
            .context("persisted scientific evidence was not found")
    }

    pub async fn get_scientific_evidence(
        &self,
        project_id: &str,
        frame_id: &str,
        evidence_id: &str,
    ) -> Result<Option<ScientificEvidenceRecord>> {
        let row = sqlx::query(
            "SELECT id,project_id,frame_id,evidence_id,provider,provider_version,tool_name,\
                    canonical_arguments_json,evidence_state,semantics_json,provenance_json,\
                    compact_payload_json,created_at,updated_at \
             FROM scientific_evidence_ledger \
             WHERE project_id=? AND frame_id=? AND evidence_id=?",
        )
        .bind(project_id)
        .bind(frame_id)
        .bind(evidence_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(scientific_evidence_from_row).transpose()
    }

    pub async fn list_scientific_evidence(
        &self,
        project_id: &str,
        frame_id: &str,
        limit: u32,
    ) -> Result<Vec<ScientificEvidenceRecord>> {
        let limit = limit.clamp(1, 100);
        let rows = sqlx::query(
            "SELECT id,project_id,frame_id,evidence_id,provider,provider_version,tool_name,\
                    canonical_arguments_json,evidence_state,semantics_json,provenance_json,\
                    compact_payload_json,created_at,updated_at \
             FROM scientific_evidence_ledger \
             WHERE project_id=? AND frame_id=? ORDER BY updated_at DESC,id DESC LIMIT ?",
        )
        .bind(project_id)
        .bind(frame_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(scientific_evidence_from_row).collect()
    }
}

fn scientific_evidence_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ScientificEvidenceRecord> {
    Ok(ScientificEvidenceRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        frame_id: row.try_get("frame_id")?,
        evidence_id: row.try_get("evidence_id")?,
        provider: row.try_get("provider")?,
        provider_version: row.try_get("provider_version")?,
        tool_name: row.try_get("tool_name")?,
        canonical_arguments_json: row.try_get("canonical_arguments_json")?,
        evidence_state: row.try_get("evidence_state")?,
        semantics_json: row.try_get("semantics_json")?,
        provenance_json: row.try_get("provenance_json")?,
        compact_payload_json: row.try_get("compact_payload_json")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn evidence_identity_is_stable_and_upsert_refreshes_payload() {
        let root =
            std::env::temp_dir().join(format!("wisp-scientific-evidence-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(&root.join("store.sqlite")).await.unwrap();
        store.create_project("p", "Project", ".").await.unwrap();
        store
            .create_frame("f", "p", "Evidence", "test-model")
            .await
            .unwrap();
        let args = json!({"mode":"core","gene":"KRAS"});
        let first = store
            .upsert_scientific_evidence(NewScientificEvidence {
                project_id: "p",
                frame_id: "f",
                provider: "local",
                provider_version: Some("26Q1"),
                tool_name: "depmap_query",
                arguments: &args,
                evidence_state: "precomputed_query",
                semantics: &json!({"metric":"gene_effect"}),
                provenance: &json!({"manifest":"m1"}),
                compact_payload: &json!({"value":1}),
            })
            .await
            .unwrap();
        let second = store
            .upsert_scientific_evidence(NewScientificEvidence {
                project_id: "p",
                frame_id: "f",
                provider: "local",
                provider_version: Some("26Q1"),
                tool_name: "depmap_query",
                arguments: &json!({"gene":"KRAS","mode":"core"}),
                evidence_state: "precomputed_query",
                semantics: &json!({"metric":"gene_effect"}),
                provenance: &json!({"manifest":"m1"}),
                compact_payload: &json!({"value":2}),
            })
            .await
            .unwrap();
        assert_eq!(first.evidence_id, second.evidence_id);
        assert_eq!(first.id, second.id);
        let rows = store.list_scientific_evidence("p", "f", 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(&rows[0].compact_payload_json).unwrap()["value"],
            2
        );
        store.close().await;
        std::fs::remove_dir_all(root).ok();
    }
}
