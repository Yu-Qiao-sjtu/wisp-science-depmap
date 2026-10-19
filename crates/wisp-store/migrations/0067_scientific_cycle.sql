CREATE TABLE IF NOT EXISTS scientific_cycle_nodes (
    id TEXT PRIMARY KEY,
    cycle_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    frame_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    refs_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_scientific_cycle_nodes_cycle
    ON scientific_cycle_nodes(cycle_id, created_at, id);

CREATE TABLE IF NOT EXISTS scientific_cycle_edges (
    id TEXT PRIMARY KEY,
    cycle_id TEXT NOT NULL,
    from_id TEXT NOT NULL,
    to_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_scientific_cycle_edges_cycle
    ON scientific_cycle_edges(cycle_id, created_at, id);
