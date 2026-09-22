CREATE TABLE IF NOT EXISTS claim_records (
    claim_id         TEXT PRIMARY KEY,
    project_id       TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    frame_id         TEXT NOT NULL,
    schema_version   INTEGER NOT NULL,
    payload_json     TEXT NOT NULL,
    source_versions  TEXT NOT NULL,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_claim_records_project_frame
    ON claim_records(project_id, frame_id, updated_at DESC);
