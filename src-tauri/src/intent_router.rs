//! Conservative pre-turn routing for project research Agents.
//!
//! The router does not read Skill bodies and does not execute tools. It only
//! decides whether a request is specific enough to attach a validated saved
//! Workflow or to ask the existing Skill portfolio planner for a bounded DAG.

use crate::specialists;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomaticRoute {
    SavedWorkflow(&'static str),
    SkillPortfolio,
}

const DEPMAP_TOPIC_WORKFLOW_ID: &str = "depmap_gene_to_cancer_topics";

pub(crate) fn automatic_route(
    message: &str,
    specialist_id: Option<&str>,
) -> Option<AutomaticRoute> {
    if specialist_id != Some(specialists::DEPMAP_SPECIALIST_ID) {
        return None;
    }
    let request = message.trim();
    if request.is_empty() || is_meta_request(request) {
        return None;
    }

    let has_gene = gene_symbols(request).next().is_some();
    let has_cancer = contains_any(
        request,
        &[
            "癌",
            "肿瘤",
            "肉瘤",
            "白血病",
            "淋巴瘤",
            "cancer",
            "tumor",
            "tumour",
            "carcinoma",
            "sarcoma",
            "leukemia",
            "lymphoma",
            "melanoma",
            "nsclc",
            "sclc",
            "luad",
            "lusc",
            "pdac",
            "paad",
            "gbm",
            "hcc",
            "crc",
            "aml",
            "cml",
            "cll",
            "skcm",
        ],
    ) || cancer_scope_tokens(request).next().is_some();
    let asks_for_topics = contains_any(
        request,
        &[
            "课题",
            "选题",
            "研究方向",
            "创新性",
            "可行性",
            "临床转化",
            "值得做",
            "能做什么",
            "研究价值",
            "topic",
            "hypothesis",
            "novelty",
            "feasibility",
            "translation",
            "research direction",
        ],
    );
    let asks_for_analysis = contains_any(
        request,
        &[
            "分析",
            "研究",
            "论证",
            "筛选",
            "寻找",
            "推荐",
            "帮我",
            "看看",
            "analyze",
            "analyse",
            "research",
            "evaluate",
            "identify",
            "find",
            "recommend",
        ],
    );

    if has_gene && (has_cancer || asks_for_topics || asks_for_analysis || is_bare_gene(request)) {
        return Some(AutomaticRoute::SavedWorkflow(DEPMAP_TOPIC_WORKFLOW_ID));
    }

    if has_cancer && (asks_for_topics || asks_for_analysis || request.chars().count() <= 30) {
        return Some(AutomaticRoute::SkillPortfolio);
    }
    None
}

fn is_meta_request(message: &str) -> bool {
    contains_any(
        message,
        &[
            "有没有实现",
            "是否实现",
            "如何调用",
            "怎么调用",
            "什么是",
            "介绍一下",
            "解释一下",
            "能不能自动",
            "是否自动",
            "how to use",
            "how does",
            "what is",
            "implemented",
        ],
    )
}

fn is_bare_gene(message: &str) -> bool {
    let trimmed = message.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-');
    !trimmed.is_empty()
        && trimmed.len() <= 20
        && trimmed.chars().any(|ch| ch.is_ascii_alphabetic())
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '-')
}

fn gene_symbols(message: &str) -> impl Iterator<Item = &str> {
    message
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .filter(|token| {
            (2..=20).contains(&token.len())
                && token.chars().any(|ch| ch.is_ascii_alphabetic())
                && token
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '-')
                && !matches!(
                    *token,
                    "DEPMap"
                        | "DEPMAP"
                        | "RNA"
                        | "DNA"
                        | "API"
                        | "FDR"
                        | "SQL"
                        | "MCQ"
                        | "NSCLC"
                        | "SCLC"
                        | "LUAD"
                        | "LUSC"
                        | "PDAC"
                        | "PAAD"
                        | "GBM"
                        | "HCC"
                        | "CRC"
                        | "AML"
                        | "ALL"
                        | "CML"
                        | "CLL"
                        | "SKCM"
                )
        })
}

fn cancer_scope_tokens(message: &str) -> impl Iterator<Item = &str> {
    message
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| {
            matches!(
                *token,
                "NSCLC"
                    | "SCLC"
                    | "LUAD"
                    | "LUSC"
                    | "PDAC"
                    | "PAAD"
                    | "GBM"
                    | "HCC"
                    | "CRC"
                    | "AML"
                    | "ALL"
                    | "CML"
                    | "CLL"
                    | "SKCM"
            )
        })
}

fn contains_any(message: &str, needles: &[&str]) -> bool {
    let normalized = message.to_lowercase();
    needles.iter().any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(message: &str) -> Option<AutomaticRoute> {
        automatic_route(message, Some(specialists::DEPMAP_SPECIALIST_ID))
    }

    #[test]
    fn routes_gene_and_gene_cancer_requests_to_the_validated_topic_workflow() {
        for request in ["KRAS", "帮我分析 KRAS 在肺癌里能做什么课题", "KRAS 肺癌"] {
            assert_eq!(
                route(request),
                Some(AutomaticRoute::SavedWorkflow(DEPMAP_TOPIC_WORKFLOW_ID))
            );
        }
    }

    #[test]
    fn routes_cancer_first_discovery_through_the_skill_portfolio() {
        for request in [
            "我研究胰腺癌，帮我筛选值得做的靶点和课题",
            "胰腺癌",
            "NSCLC",
            "ALL",
            "我研究的是乳腺癌，我想调用当前的 Agent 进行 true love gene 基因的挖掘。",
        ] {
            assert_eq!(route(request), Some(AutomaticRoute::SkillPortfolio));
        }
    }

    #[test]
    fn avoids_meta_questions_other_specialists_and_ordinary_followups() {
        assert_eq!(route("现在有没有实现自动分析 KRAS？"), None);
        assert_eq!(automatic_route("分析 KRAS", None), None);
        assert_eq!(route("这个结果为什么不显著？"), None);
    }
}
