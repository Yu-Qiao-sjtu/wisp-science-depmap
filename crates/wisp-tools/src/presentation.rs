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

/// Scan a shell or runtime program for a path a query-only turn must not materialize.
pub fn query_only_forbidden_target(text: &str) -> Option<String> {
    let normalized = text.replace('\\', "/");
    if normalized.to_ascii_lowercase().contains("results/reports") {
        return Some("results/reports".to_string());
    }
    for token in normalized.split(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ',' | ';' | '(' | ')' | '>' | '<')
    }) {
        let token = token.trim_matches(|c: char| matches!(c, '=' | ':'));
        if !token.is_empty() && query_only_forbids_path(token) {
            return Some(token.to_string());
        }
    }
    None
}

pub fn query_only_write_error(path: &str) -> String {
    format!(
        "query-only turn forbids writing `{path}` (artifact_requested=false); answer from the bounded evidence envelope in chat"
    )
}

#[cfg(test)]
mod tests {
    use super::{query_only_forbidden_target, query_only_forbids_path};

    #[test]
    fn forbids_reports_and_unsolicited_csv() {
        assert!(query_only_forbids_path("results/reports/depmap.md"));
        assert!(query_only_forbids_path(r"results\reports\x.csv"));
        assert!(query_only_forbids_path("pairs.csv"));
        assert!(!query_only_forbids_path("analysis/depmap-agent/out.csv"));
        assert!(!query_only_forbids_path("notes.md"));
    }

    #[test]
    fn shell_and_python_cannot_smuggle_a_report_or_csv() {
        assert_eq!(
            query_only_forbidden_target("python plot.py > results/reports/kidney.md"),
            Some("results/reports".into())
        );
        assert_eq!(
            query_only_forbidden_target("open('pairs.csv','w')"),
            Some("pairs.csv".into())
        );
        assert!(query_only_forbidden_target("python -c \"print(1)\"").is_none());
        assert!(query_only_forbidden_target("analysis/depmap-agent/out.csv").is_none());
    }
}
