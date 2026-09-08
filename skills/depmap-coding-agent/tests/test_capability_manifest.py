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


if __name__ == "__main__":
    unittest.main()
