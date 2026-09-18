//! Query-only presentation: no unsolicited project artifacts.

/// When `artifact_requested` is false, these paths must not be materialized.
pub fn query_only_forbids_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if normalized.contains("results/reports") {
        return true;
    }
    let csv_like = normalized.ends_with(".csv") || normalized.ends_with(".tsv");
    csv_like && !normalized.contains("analysis/")
}

pub fn query_only_write_error(path: &str) -> String {
    format!(
        "query-only turn forbids writing `{path}` (artifact_requested=false); answer from the bounded evidence envelope in chat"
    )
}

#[cfg(test)]
mod tests {
    use super::query_only_forbids_path;

    #[test]
    fn forbids_reports_and_unsolicited_csv() {
        assert!(query_only_forbids_path("results/reports/depmap.md"));
        assert!(query_only_forbids_path(r"results\reports\x.csv"));
        assert!(query_only_forbids_path("pairs.csv"));
        assert!(!query_only_forbids_path("analysis/depmap-agent/out.csv"));
        assert!(!query_only_forbids_path("notes.md"));
    }
}
