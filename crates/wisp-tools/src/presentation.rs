//! Query-only presentation: no unsolicited project artifacts.

use serde_json::{json, Value};

/// How a scientific turn may materialize project files.
///
/// `Unrestricted` is the ordinary coding-agent default. Scientific turns start
/// at `ChatOnly` until the user names an artifact or answers the presentation card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactPresentation {
    Unrestricted,
    ChatOnly,
    Table,
    Figure,
    Report,
}

impl ArtifactPresentation {
    pub fn allows_project_writes(self) -> bool {
        !matches!(self, Self::ChatOnly)
    }

    pub fn allows_plotting_skill(self) -> bool {
        matches!(self, Self::Unrestricted | Self::Figure)
    }
}

pub const ARTIFACT_CARD_PURPOSE: &str = "artifact_presentation";

/// Explicit artifact language in the user message. Absent means the turn stays chat-only.
pub fn explicit_artifact_presentation(text: &str) -> Option<ArtifactPresentation> {
    let folded = text.to_ascii_lowercase();
    let figure = folded.contains("图表")
        || folded.contains("热图")
        || folded.contains("figure")
        || folded.contains("plot")
        || folded.contains("chart")
        || folded.contains("可视化");
    let report = folded.contains("报告") || folded.contains("report");
    let table = folded.contains("表格")
        || folded.contains("csv")
        || folded.contains("tsv")
        || folded.contains("table")
        || folded.contains("导出");
    match (figure, report, table) {
        (false, false, false) => None,
        (true, false, false) => Some(ArtifactPresentation::Figure),
        (false, true, false) => Some(ArtifactPresentation::Report),
        (false, false, true) => Some(ArtifactPresentation::Table),
        _ => None,
    }
}

/// Map a presentation-card answer. Dismissal and unknown text stay chat-only.
pub fn presentation_from_answer(text: &str) -> ArtifactPresentation {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ArtifactPresentation::ChatOnly;
    }
    explicit_artifact_presentation(trimmed).unwrap_or_else(|| {
        let folded = trimmed.to_ascii_lowercase();
        if folded.contains("仅保留")
            || folded.contains("仅聊天")
            || folded.contains("chat")
            || folded.contains("dismiss")
        {
            ArtifactPresentation::ChatOnly
        } else {
            ArtifactPresentation::ChatOnly
        }
    })
}

pub fn depmap_turn_presentation(message: &str) -> ArtifactPresentation {
    explicit_artifact_presentation(message).unwrap_or(ArtifactPresentation::ChatOnly)
}

/// Arguments for `ask_user` after a query-only scientific answer.
pub fn artifact_presentation_card() -> Value {
    json!({
        "purpose": ARTIFACT_CARD_PURPOSE,
        "question": "科学结论已经在聊天中给出。希望如何呈现这次结果？",
        "allow_freeform": true,
        "options": [
            {"label": "仅保留聊天结论", "description": "不写项目文件，也不运行绑图流程。"},
            {"label": "生成表格", "description": "只导出选定的表格。"},
            {"label": "生成图表", "description": "只生成选定的图。"},
            {"label": "生成报告", "description": "只写报告，不自动附带图。"}
        ]
    })
}

pub fn plotting_skill_request(name_or_query: &str) -> bool {
    let folded = name_or_query.to_ascii_lowercase();
    [
        "figure",
        "plot",
        "chart",
        "ggplot",
        "matplotlib",
        "绑图",
        "可视化",
        "heatmap",
    ]
    .iter()
    .any(|needle| folded.contains(needle))
}

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

pub fn presentation_forbids_path(presentation: ArtifactPresentation, path: &str) -> bool {
    match presentation {
        ArtifactPresentation::Unrestricted => false,
        ArtifactPresentation::ChatOnly => query_only_forbids_path(path),
        ArtifactPresentation::Table => {
            let normalized = path.replace('\\', "/").to_ascii_lowercase();
            normalized.contains("results/reports") || is_figure_path(&normalized)
        }
        ArtifactPresentation::Figure => {
            let normalized = path.replace('\\', "/").to_ascii_lowercase();
            normalized.contains("results/reports")
                || ((normalized.ends_with(".csv") || normalized.ends_with(".tsv"))
                    && !normalized.contains("analysis/"))
        }
        ArtifactPresentation::Report => {
            let normalized = path.replace('\\', "/").to_ascii_lowercase();
            is_figure_path(&normalized)
                || ((normalized.ends_with(".csv") || normalized.ends_with(".tsv"))
                    && !normalized.contains("analysis/"))
        }
    }
}

fn is_figure_path(normalized: &str) -> bool {
    normalized.ends_with(".png")
        || normalized.ends_with(".svg")
        || normalized.ends_with(".pdf")
        || normalized.ends_with(".jpg")
}

pub fn presentation_forbidden_target(
    presentation: ArtifactPresentation,
    text: &str,
) -> Option<String> {
    if presentation == ArtifactPresentation::Unrestricted {
        return None;
    }
    if presentation == ArtifactPresentation::ChatOnly {
        return query_only_forbidden_target(text);
    }
    let normalized = text.replace('\\', "/");
    if presentation_forbids_path(presentation, "results/reports")
        && normalized.to_ascii_lowercase().contains("results/reports")
    {
        return Some("results/reports".to_string());
    }
    for token in normalized.split(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ',' | ';' | '(' | ')' | '>' | '<')
    }) {
        let token = token.trim_matches(|c: char| matches!(c, '=' | ':'));
        if !token.is_empty() && presentation_forbids_path(presentation, token) {
            return Some(token.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{query_only_forbidden_target, query_only_forbids_path, ArtifactPresentation};

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

    #[test]
    fn kidney_query_stays_chat_only_until_the_user_names_an_artifact() {
        let prompt = "使用 depmap MCP：帮我分析哪些基因是肾癌的特异性的，肿瘤脆性的基因？";
        assert_eq!(
            super::depmap_turn_presentation(prompt),
            ArtifactPresentation::ChatOnly
        );
        assert_eq!(
            super::explicit_artifact_presentation("请生成一张肾癌脆性热图"),
            Some(ArtifactPresentation::Figure)
        );
        assert_eq!(
            super::presentation_from_answer("仅保留聊天结论"),
            ArtifactPresentation::ChatOnly
        );
        assert_eq!(
            super::presentation_from_answer("生成表格"),
            ArtifactPresentation::Table
        );
        assert_eq!(
            super::presentation_from_answer(""),
            ArtifactPresentation::ChatOnly
        );
        let card = super::artifact_presentation_card();
        assert_eq!(card["purpose"], super::ARTIFACT_CARD_PURPOSE);
        assert!(super::presentation_forbids_path(
            ArtifactPresentation::Table,
            "results/reports/note.md"
        ));
        assert!(!super::presentation_forbids_path(
            ArtifactPresentation::Table,
            "results/kidney.csv"
        ));
        assert!(super::presentation_forbids_path(
            ArtifactPresentation::Figure,
            "results/reports/note.md"
        ));
        assert!(super::presentation_forbids_path(
            ArtifactPresentation::Report,
            "figures/kidney.png"
        ));
        assert!(super::plotting_skill_request("figure-style"));
        assert!(!super::plotting_skill_request("depmap-knowledge-query"));
    }
}
