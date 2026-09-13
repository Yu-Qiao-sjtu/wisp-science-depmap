CREATE TABLE IF NOT EXISTS research_journal_entries (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    exploration_id TEXT REFERENCES explorations(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    category TEXT NOT NULL CHECK(category IN ('progress','finding','decision','next')),
    occurred_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_research_journal_project_date
    ON research_journal_entries(project_id, occurred_at DESC);
