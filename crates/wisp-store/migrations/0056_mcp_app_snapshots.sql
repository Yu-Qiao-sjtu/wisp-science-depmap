CREATE TABLE IF NOT EXISTS mcp_app_snapshots (
    frame_id        TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    presentation_id TEXT NOT NULL,
    app_kind        TEXT NOT NULL,
    snapshot_json   TEXT NOT NULL,
    updated_at      INTEGER NOT NULL,
    PRIMARY KEY(frame_id, presentation_id)
);

CREATE INDEX IF NOT EXISTS ix_mcp_app_snapshots_frame_updated
    ON mcp_app_snapshots(frame_id, updated_at DESC);
