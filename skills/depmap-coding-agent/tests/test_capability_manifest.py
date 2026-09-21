import json
import unittest
from pathlib import Path


class ExpressionDependencyCapabilityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.repo_root = Path(__file__).resolve().parents[3]

    def test_public_release_fallback_uses_the_same_26q1_raw_file_dictionary(self):
        capability_path = (
            self.repo_root
            / "skills"
            / "depmap-coding-agent"
            / "references"
            / "capability-manifest.json"
        )
        fallback_path = (
            self.repo_root
            / "skills"
            / "depmap-knowledge-query"
            / "references"
            / "public-release-fallback.json"
        )
        capability = json.loads(capability_path.read_text(encoding="utf-8"))
        fallback = json.loads(fallback_path.read_text(encoding="utf-8"))
        expected = {
            (item["id"], item["path"])
            for item in capability["datasets"]
            if item["kind"] == "raw"
        }
        actual = {(item["id"], item["path"]) for item in fallback["files"]}
        self.assertEqual(fallback["release"], "26Q1")
        self.assertEqual(actual, expected)
        self.assertFalse(
            fallback["provider_policy"]["live_portal_api_is_query_provider"]
        )

    def test_tm00_expression_dependency_capabilities_have_modality_roles(self):
        path = (
            self.repo_root
            / "skills"
            / "depmap-coding-agent"
            / "references"
            / "capability-manifest.json"
        )
        manifest = json.loads(path.read_text(encoding="utf-8"))
        capabilities = {item["id"]: item for item in manifest["capabilities"]}

        for capability_id in ("predictive_biomarkers", "gene_to_dependency"):
            capability = capabilities[capability_id]
            self.assertEqual(capability["module_id"], "expression_dependency")
            self.assertEqual(
                capability["data_modality"], "expression_vs_crispr_gene_effect"
            )
            self.assertEqual(
                capability["entity_roles"],
                {
                    "source": "expression_gene",
                    "target": "gene_effect_gene",
                    "symmetric": False,
                },
            )

    def test_analysis_module_declares_asymmetric_query_contract(self):
        path = (
            self.repo_root
            / "analysis-modules"
            / "表达基因-CRISPR基因依赖相关性分析"
            / "module.intent.json"
        )
        intent = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(intent["module_id"], "expression_dependency")
        self.assertFalse(intent["entity_roles"]["symmetric"])
        self.assertEqual(
            intent["query_contract"]["global_pair"],
            {"mode": "pair", "module": "expression_dependency"},
        )

    def test_semantic_routing_examples_have_valid_context_and_actions(self):
        path = self.repo_root / "analysis-modules" / "表达基因-CRISPR基因依赖相关性分析" / "module.intent.json"
        routing = json.loads(path.read_text(encoding="utf-8"))["semantic_routing"]
        allowed = {
            "global_gsea", "clarify", "do_not_execute", "explain_only",
            "query_precomputed_correlations", "route_drug_analysis",
            "separate_ora_method", "requested_gene_sets_required",
            "complete_lineage_ranking_required",
            "train_predictive_biomarker_model",
            "clarify_dependency_target_gene",
        }
        examples = routing["examples"]
        self.assertEqual({x["expected"] for x in examples}, allowed)
        for example in examples:
            self.assertTrue(example["text"].strip())
            self.assertIn(example["expected"], allowed)
            if example["expected"] == "global_gsea" and "context" in example:
                context = example["context"]
                self.assertEqual(context["module_id"], "expression_dependency")
                self.assertTrue(context["source_gene"])
                self.assertEqual(context["scope"], "global")
        for aliases in routing["synonyms"].values():
            self.assertEqual(len(aliases), len(set(aliases)))
            self.assertTrue(all(alias.strip() for alias in aliases))

    def test_predictive_biomarker_operation_requires_target_and_nested_validation(self):
        manifest_path = self.repo_root / "skills" / "depmap-coding-agent" / "references" / "capability-manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        capability = next(item for item in manifest["capabilities"] if item["id"] == "predictive_biomarkers")
        operation = next(item for item in capability["operations"] if item["id"] == "train_expression_dependency_model")
        self.assertEqual(operation["execution_mode"], "on_demand_cached")
        self.assertEqual(operation["required_entities"], ["dependency_target_gene"])
        self.assertEqual(operation["validation"], ["nested_cross_validation", "leave_one_lineage_out"])

        intent_path = self.repo_root / "analysis-modules" / "表达基因-CRISPR基因依赖相关性分析" / "module.intent.json"
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        query = intent["query_contract"]["predictive_biomarker_model"]
        self.assertEqual(query["operation_id"], "train_expression_dependency_model")
        self.assertEqual(query["required_entities"], ["dependency_target_gene"])

    def test_on_demand_gsea_capability_has_reviewed_entrypoint_and_defaults(self):
        manifest_path = (
            self.repo_root
            / "skills"
            / "depmap-coding-agent"
            / "references"
            / "capability-manifest.json"
        )
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        capabilities = {item["id"]: item for item in manifest["capabilities"]}
        capability = capabilities["gene_to_dependency"]
        operation = capability["operations"]["pathway_enrichment"]

        self.assertEqual(operation["operation"], "gsea")
        self.assertEqual(operation["execution_mode"], "on_demand_cached")
        self.assertEqual(operation["supported_scopes"], ["global"])
        self.assertEqual(
            operation["executable_entrypoint"],
            "analysis-modules/表达基因-CRISPR基因依赖相关性分析/scripts/run_expression_dependency_gsea.R",
        )
        self.assertEqual(
            operation["defaults"],
            {
                "collection": "hallmark",
                "rank_metric": "negative_signed_t",
                "min_pair_fraction": 0.8,
            },
        )

        intent_path = (
            self.repo_root
            / "analysis-modules"
            / "表达基因-CRISPR基因依赖相关性分析"
            / "module.intent.json"
        )
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        route = intent["operation_routing"]["pathway_enrichment"]
        self.assertEqual(route["action"], "execute_expression_dependency_gsea")
        self.assertEqual(route["capability_id"], "gene_to_dependency")
        self.assertEqual(route["operation_id"], "pathway_enrichment")
        self.assertEqual(
            route["trigger_requires"],
            ["expression_source_gene", "pathway_or_enrichment_language"],
        )
        lineage = intent["query_contract"]["lineage_pathway_context"]
        self.assertEqual(lineage["method"], "rank_sum")
        self.assertFalse(lineage["gsea_equivalent"])

    def test_expression_threshold_contrast_is_a_distinct_authorized_operation(self):
        manifest_path = (
            self.repo_root
            / "skills"
            / "depmap-coding-agent"
            / "references"
            / "capability-manifest.json"
        )
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        capability = next(
            item
            for item in manifest["capabilities"]
            if item["id"] == "expression_threshold_dependency"
        )
        self.assertEqual(
            capability["data_modality"],
            "expression_group_vs_crispr_gene_effect",
        )
        operation = capability["operations"]["run_expression_threshold_dependency_contrast"]
        self.assertEqual(operation["execution_mode"], "authorized_on_demand_cached")
        self.assertEqual(
            operation["executable_entrypoint"],
            "analysis-modules/表达基因-CRISPR基因依赖相关性分析/scripts/run_expression_threshold_dependency_contrast.R",
        )
        self.assertEqual(
            operation["supported_scopes"],
            ["global", "lineage"],
        )
        self.assertEqual(
            operation["defaults"]["threshold_policy"],
            "prespecified_quantile_tails",
        )

        intent_path = (
            self.repo_root
            / "analysis-modules"
            / "表达基因-CRISPR基因依赖相关性分析"
            / "module.intent.json"
        )
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        query = intent["query_contract"]["expression_threshold_dependency_contrast"]
        self.assertEqual(query["capability_id"], "expression_threshold_dependency")
        self.assertEqual(query["required_entities"], ["expression_source_gene"])
        route = intent["operation_routing"]["expression_threshold_dependency_contrast"]
        self.assertIn("analysis_authorization", route["trigger_requires"])

    def test_allele_specific_mutation_is_a_distinct_authorized_operation(self):
        manifest_path = (
            self.repo_root
            / "skills"
            / "depmap-coding-agent"
            / "references"
            / "capability-manifest.json"
        )
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        capability = next(
            item
            for item in manifest["capabilities"]
            if item["id"] == "allele_specific_mutation_dependency"
        )
        self.assertEqual(
            capability["data_modality"],
            "protein_change_vs_crispr_gene_effect",
        )
        operation = capability["operations"]["run_allele_specific_mutation_dependency"]
        self.assertEqual(operation["execution_mode"], "authorized_on_demand_cached")
        self.assertEqual(operation["defaults"]["min_case_n"], 5)

        intent_path = (
            self.repo_root
            / "analysis-modules"
            / "癌种内突变锚定基因选择"
            / "module.intent.json"
        )
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        query = intent["query_contract"]["allele_specific_mutation_dependency"]
        self.assertEqual(query["capability_id"], "allele_specific_mutation_dependency")
        self.assertEqual(query["required_entities"], ["mutation_gene", "protein_change"])


if __name__ == "__main__":
    unittest.main()
