CREATE TABLE IF NOT EXISTS scientific_evidence_ledger (
    id                      TEXT PRIMARY KEY,
    project_id              TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    frame_id                TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
    evidence_id             TEXT NOT NULL,
    provider                TEXT NOT NULL,
    provider_version        TEXT,
    tool_name               TEXT NOT NULL,
    canonical_arguments_json TEXT NOT NULL,
    evidence_state          TEXT NOT NULL,
    semantics_json          TEXT NOT NULL DEFAULT '{}',
    provenance_json         TEXT NOT NULL DEFAULT '{}',
    compact_payload_json    TEXT NOT NULL,
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL,
    UNIQUE(project_id, frame_id, evidence_id)
);

CREATE INDEX IF NOT EXISTS ix_scientific_evidence_project_updated
    ON scientific_evidence_ledger(project_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS ix_scientific_evidence_frame_updated
    ON scientific_evidence_ledger(frame_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS ix_scientific_evidence_identity
    ON scientific_evidence_ledger(project_id, evidence_id);
