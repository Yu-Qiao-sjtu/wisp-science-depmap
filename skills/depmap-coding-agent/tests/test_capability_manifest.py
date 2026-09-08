import json
import unittest
from pathlib import Path


class ExpressionDependencyCapabilityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.repo_root = Path(__file__).resolve().parents[3]

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


if __name__ == "__main__":
    unittest.main()
