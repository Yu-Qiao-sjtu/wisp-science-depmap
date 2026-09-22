use crate::Store;
use anyhow::{Context, Result};
use sqlx::Row;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimRecordRow {
    pub claim_id: String,
    pub project_id: String,
    pub frame_id: String,
    pub schema_version: i64,
    pub payload_json: String,
    pub source_versions: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Store {
    pub async fn persist_claim_record(
        &self,
        claim_id: &str,
        project_id: &str,
        frame_id: &str,
        schema_version: i64,
        payload_json: &str,
        source_versions: &str,
    ) -> Result<ClaimRecordRow> {
        if claim_id.trim().is_empty() {
            anyhow::bail!("claim_id is required");
        }
        if payload_json.len() > 64 * 1024 {
            anyhow::bail!("claim payload exceeds the 64KiB bound");
        }
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO claim_records(\
               claim_id,project_id,frame_id,schema_version,payload_json,source_versions,\
               created_at,updated_at) \
             VALUES(?,?,?,?,?,?,?,?) \
             ON CONFLICT(claim_id) DO UPDATE SET \
               payload_json=excluded.payload_json,\
               source_versions=excluded.source_versions,\
               schema_version=excluded.schema_version,\
               updated_at=excluded.updated_at",
        )
        .bind(claim_id)
        .bind(project_id)
        .bind(frame_id)
        .bind(schema_version)
        .bind(payload_json)
        .bind(source_versions)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("persist claim record")?;
        self.get_claim_record(claim_id)
            .await?
            .context("persisted claim record was not found")
    }

    pub async fn get_claim_record(&self, claim_id: &str) -> Result<Option<ClaimRecordRow>> {
        let row = sqlx::query(
            "SELECT claim_id,project_id,frame_id,schema_version,payload_json,source_versions,\
                    created_at,updated_at \
             FROM claim_records WHERE claim_id=?",
        )
        .bind(claim_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| ClaimRecordRow {
            claim_id: row.get(0),
            project_id: row.get(1),
            frame_id: row.get(2),
            schema_version: row.get(3),
            payload_json: row.get(4),
            source_versions: row.get(5),
            created_at: row.get(6),
            updated_at: row.get(7),
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;

    #[tokio::test]
    async fn persist_keeps_source_versions_across_reload() {
        let tmp =
            std::env::temp_dir().join(format!("wisp_claim_record_{}.sqlite", uuid::Uuid::new_v4()));
        let store = Store::open(&tmp).await.unwrap();
        store.create_project("p", "proj", "").await.unwrap();
        let row = store
            .persist_claim_record(
                "c1",
                "p",
                "f",
                1,
                r#"{"claim_id":"c1"}"#,
                r#"{"ev-1":"digest-a"}"#,
            )
            .await
            .unwrap();
        assert_eq!(row.source_versions, r#"{"ev-1":"digest-a"}"#);
        let reloaded = store.get_claim_record("c1").await.unwrap().unwrap();
        assert_eq!(reloaded.source_versions, row.source_versions);
    }
}
