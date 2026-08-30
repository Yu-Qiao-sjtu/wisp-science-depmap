use crate::Store;
use anyhow::Result;
use sqlx::Row;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpAppSnapshot {
    pub frame_id: String,
    pub presentation_id: String,
    pub app_kind: String,
    pub snapshot_json: String,
    pub updated_at: i64,
}

impl Store {
    pub async fn save_mcp_app_snapshot(
        &self,
        frame_id: &str,
        presentation_id: &str,
        app_kind: &str,
        snapshot_json: &str,
    ) -> Result<()> {
        let updated_at = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO mcp_app_snapshots(\
               frame_id,presentation_id,app_kind,snapshot_json,updated_at) \
             VALUES(?,?,?,?,?) \
             ON CONFLICT(frame_id,presentation_id) DO UPDATE SET \
               app_kind=excluded.app_kind,snapshot_json=excluded.snapshot_json,\
               updated_at=excluded.updated_at",
        )
        .bind(frame_id)
        .bind(presentation_id)
        .bind(app_kind)
        .bind(snapshot_json)
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_mcp_app_snapshot(
        &self,
        frame_id: &str,
        presentation_id: &str,
    ) -> Result<Option<McpAppSnapshot>> {
        let row = sqlx::query(
            "SELECT frame_id,presentation_id,app_kind,snapshot_json,updated_at \
             FROM mcp_app_snapshots WHERE frame_id=? AND presentation_id=?",
        )
        .bind(frame_id)
        .bind(presentation_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(McpAppSnapshot {
                frame_id: row.try_get("frame_id")?,
                presentation_id: row.try_get("presentation_id")?,
                app_kind: row.try_get("app_kind")?,
                snapshot_json: row.try_get("snapshot_json")?,
                updated_at: row.try_get("updated_at")?,
            })
        })
        .transpose()
    }
}
