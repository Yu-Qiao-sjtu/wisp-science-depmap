use crate::Store;
use anyhow::{Context, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeCheckpointRecord {
    pub checkpoint_id: String,
    pub operation_id: String,
    pub project_id: String,
    pub session_id: String,
    pub state: String,
    pub schema_version: i64,
    pub payload_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Store {
    pub async fn persist_bridge_checkpoint(
        &self,
        checkpoint_id: &str,
        operation_id: &str,
        project_id: &str,
        session_id: &str,
        state: &str,
        schema_version: i64,
        payload_json: &str,
    ) -> Result<BridgeCheckpointRecord> {
        if checkpoint_id.trim().is_empty() || operation_id.trim().is_empty() {
            anyhow::bail!("checkpoint_id and operation_id are required");
        }
        if payload_json.len() > 64 * 1024 {
            anyhow::bail!("bridge checkpoint payload exceeds the 64KiB bound");
        }
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO bridge_checkpoints(\
               checkpoint_id,operation_id,project_id,session_id,state,schema_version,\
               payload_json,created_at,updated_at) \
             VALUES(?,?,?,?,?,?,?,?,?) \
             ON CONFLICT(operation_id) DO NOTHING",
        )
        .bind(checkpoint_id)
        .bind(operation_id)
        .bind(project_id)
        .bind(session_id)
        .bind(state)
        .bind(schema_version)
        .bind(payload_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.get_bridge_checkpoint_by_operation(operation_id)
            .await?
            .context("bridge checkpoint was not stored")
    }

    pub async fn update_bridge_checkpoint(
        &self,
        checkpoint_id: &str,
        state: &str,
        payload_json: &str,
    ) -> Result<bool> {
        if payload_json.len() > 64 * 1024 {
            anyhow::bail!("bridge checkpoint payload exceeds the 64KiB bound");
        }
        let now = chrono::Utc::now().timestamp();
        let updated = sqlx::query(
            "UPDATE bridge_checkpoints SET state=?, payload_json=?, updated_at=? \
             WHERE checkpoint_id=?",
        )
        .bind(state)
        .bind(payload_json)
        .bind(now)
        .bind(checkpoint_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(updated > 0)
    }

    pub async fn get_bridge_checkpoint(
        &self,
        checkpoint_id: &str,
    ) -> Result<Option<BridgeCheckpointRecord>> {
        Ok(sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                i64,
                String,
                i64,
                i64,
            ),
        >(
            "SELECT checkpoint_id,operation_id,project_id,session_id,state,schema_version,\
                    payload_json,created_at,updated_at \
             FROM bridge_checkpoints WHERE checkpoint_id=?",
        )
        .bind(checkpoint_id)
        .fetch_optional(&self.pool)
        .await?
        .map(record_from_row))
    }

    pub async fn get_bridge_checkpoint_by_operation(
        &self,
        operation_id: &str,
    ) -> Result<Option<BridgeCheckpointRecord>> {
        Ok(sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                i64,
                String,
                i64,
                i64,
            ),
        >(
            "SELECT checkpoint_id,operation_id,project_id,session_id,state,schema_version,\
                    payload_json,created_at,updated_at \
             FROM bridge_checkpoints WHERE operation_id=?",
        )
        .bind(operation_id)
        .fetch_optional(&self.pool)
        .await?
        .map(record_from_row))
    }

    pub async fn pending_bridge_checkpoints(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> Result<Vec<BridgeCheckpointRecord>> {
        Ok(sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                i64,
                String,
                i64,
                i64,
            ),
        >(
            "SELECT checkpoint_id,operation_id,project_id,session_id,state,schema_version,\
                    payload_json,created_at,updated_at \
             FROM bridge_checkpoints \
             WHERE project_id=? AND session_id=? \
               AND state NOT IN ('cancelled','completed') \
             ORDER BY updated_at DESC, rowid DESC",
        )
        .bind(project_id)
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(record_from_row)
        .collect())
    }
}

fn record_from_row(
    row: (
        String,
        String,
        String,
        String,
        String,
        i64,
        String,
        i64,
        i64,
    ),
) -> BridgeCheckpointRecord {
    BridgeCheckpointRecord {
        checkpoint_id: row.0,
        operation_id: row.1,
        project_id: row.2,
        session_id: row.3,
        state: row.4,
        schema_version: row.5,
        payload_json: row.6,
        created_at: row.7,
        updated_at: row.8,
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;

    #[tokio::test]
    async fn persist_is_idempotent_on_operation_id_and_survives_reopen() {
        let tmp = std::env::temp_dir().join(format!(
            "wisp_bridge_checkpoint_{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let store = Store::open(&tmp).await.unwrap();
        store.create_project("p1", "proj", "").await.unwrap();
        let first = store
            .persist_bridge_checkpoint(
                "cp-1",
                "op-1",
                "p1",
                "session-a",
                "coverage_gap",
                1,
                r#"{"capability_id":"codependency_query","release":"25Q2"}"#,
            )
            .await
            .unwrap();
        let second = store
            .persist_bridge_checkpoint(
                "cp-2",
                "op-1",
                "p1",
                "session-a",
                "coverage_gap",
                1,
                r#"{"capability_id":"other"}"#,
            )
            .await
            .unwrap();
        assert_eq!(first.checkpoint_id, second.checkpoint_id);
        assert!(second.payload_json.contains("codependency_query"));
        store
            .update_bridge_checkpoint(
                "cp-1",
                "new_analysis_proposed",
                r#"{"capability_id":"codependency_query","proposal_id":"prop-1"}"#,
            )
            .await
            .unwrap();
        store.pool.close().await;
        let store = Store::open(&tmp).await.unwrap();
        let pending = store
            .pending_bridge_checkpoints("p1", "session-a")
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].state, "new_analysis_proposed");
        assert!(pending[0].payload_json.contains("prop-1"));
        let _ = std::fs::remove_file(tmp);
    }
}
