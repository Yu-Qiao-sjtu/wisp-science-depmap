CREATE TABLE IF NOT EXISTS bridge_checkpoints (
    checkpoint_id   TEXT PRIMARY KEY,
    operation_id    TEXT NOT NULL UNIQUE,
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    session_id      TEXT NOT NULL,
    state           TEXT NOT NULL,
    schema_version  INTEGER NOT NULL,
    payload_json    TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_bridge_checkpoints_project_session
    ON bridge_checkpoints(project_id, session_id, updated_at DESC);
