import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

from services.depmap_api.app import Settings
from services.depmap_mcp.server import DepMapEvidenceService


class DepMapMcpTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "depmap-26q1-core").mkdir()
        (self.root / "depmap-26q1-full").mkdir()
        (self.root / "depmap-26q1-module-catalog.csv").write_text(
            "module\n", encoding="utf-8"
        )
        (self.root / "depmap-26q1-qa.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "release": "26Q1",
                    "qa_status": "PASS",
                    "module_count": 26,
                }
            ),
            encoding="utf-8",
        )
        self.script = self.root / "query_depmap_kb.R"
        self.script.write_text("# fixture\n", encoding="utf-8")
        self.settings = Settings(
            knowledge_root=self.root,
            query_script=self.script,
            api_token="local-test-token",
            max_concurrency=2,
        )
        self.queries: list[dict] = []

        async def runner(_settings, query):
            self.queries.append(query)
            return {
                "mode": query["mode"],
                "status": "FOUND",
                "provenance": [
                    str(self.root / "depmap-26q1-full" / "fixture.parquet")
                ],
                "rows": [{"query_mode": query["mode"]}],
            }

        self.service = DepMapEvidenceService(self.settings, runner)

    def tearDown(self):
        self.temp.cleanup()

    async def test_gene_evidence_is_bounded_and_portable(self):
        result = await self.service.gene_evidence("esr1", "Breast Cancer", limit=3)
        self.assertEqual(result["request"]["gene"], "ESR1")
        self.assertEqual(result["evidence"]["query_count"], 11)
        self.assertTrue(result["evidence_id"].startswith("depmap-26q1-"))
        self.assertEqual(self.queries[1]["lineage"], "Breast")
        provenance = result["evidence"]["items"][0]["result"]["provenance"][0]
        self.assertEqual(
            provenance, "depmap://26Q1/depmap-26q1-full/fixture.parquet"
        )
        tcga = result["evidence"]["items"][-1]
        self.assertEqual(
            tcga["query"],
            {
                "mode": "tcga_expression_survival",
                "gene": "ESR1",
                "lineage": "Breast",
                "endpoint": "OS",
                "limit": 3,
            },
        )
        self.assertEqual(
            tcga["metric_semantics"]["metric"],
            "tcga_expression_and_survival_association",
        )

    async def test_gene_without_lineage_queries_tcga_across_projects(self):
        result = await self.service.gene_evidence("tp53", limit=4)
        tcga = result["evidence"]["items"][-1]
        self.assertEqual(
            tcga["query"],
            {
                "mode": "tcga_expression_survival",
                "gene": "TP53",
                "endpoint": "OS",
                "limit": 4,
            },
        )

    async def test_dedicated_tcga_tool_keeps_patient_evidence_separate(self):
        result = await self.service.tcga_expression_survival(
            "kras", lineage="Bowel", endpoint="PFI", limit=6
        )
        self.assertEqual(result["request"]["gene"], "KRAS")
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "tcga_expression_survival",
                "gene": "KRAS",
                "lineage": "Bowel",
                "endpoint": "PFI",
                "limit": 7,
            },
        )
        self.assertIn("patient_evidence", result["evidence"])
        self.assertIn("never", result["evidence"]["integration_rule"])

    async def test_tcga_project_and_lineage_are_mutually_exclusive(self):
        with self.assertRaisesRegex(ValueError, "alternative cohort selectors"):
            await self.service.tcga_expression_survival(
                "ESR1", project="TCGA-BRCA", lineage="Breast"
            )

    async def test_same_evidence_has_same_id(self):
        first = await self.service.drug_evidence("olaparib", "BRCA1", "Breast")
        second = await self.service.drug_evidence("olaparib", "BRCA1", "Breast")
        self.assertEqual(first["evidence_id"], second["evidence_id"])

    async def test_cancer_only_direction_discovery_needs_no_anchor_gene(self):
        result = await self.service.lineage_directions("结肠癌", 20)
        self.assertEqual(
            result["request"],
            {"lineage": "结肠癌", "limit": 20, "focus": "all"},
        )
        self.assertEqual(
            self.queries[0],
            {
                "mode": "lineage_directions",
                "lineage": "Bowel",
                "limit": 21,
                "focus": "all",
            },
        )
        self.assertNotIn("gene", result["request"])

    async def test_cancer_direction_focus_is_forwarded_to_the_provider(self):
        result = await self.service.lineage_directions(
            "肝癌", 8, "transcription_factor"
        )
        self.assertEqual(result["request"]["focus"], "transcription_factor")
        self.assertEqual(self.queries[0]["lineage"], "Liver")
        self.assertEqual(self.queries[0]["focus"], "transcription_factor")

    async def test_topic_plan_preserves_phenotype_and_molecular_focus(self):
        result = await self.service.topic_plan(
            "肝癌",
            phenotypes=["tumor_cell_stemness"],
            molecular_focus=["transcription_factor"],
            evidence_sources=["depmap"],
            requested_outputs=["candidate_topics", "feasibility"],
        )
        self.assertEqual(
            self.queries[0],
            {
                "mode": "topic_plan",
                "lineage": "Liver",
                "phenotypes": ["tumor_cell_stemness"],
                "molecular_focus": ["transcription_factor"],
                "mechanisms": [],
                "evidence_sources": ["depmap"],
                "requested_outputs": ["candidate_topics", "feasibility"],
                "execution_policy": "precomputed_only",
                "unresolved_concepts": [],
                "limit": 21,
            },
        )
        self.assertEqual(result["request"]["phenotypes"], ["tumor_cell_stemness"])
        self.assertFalse(result["new_analysis_started"])

    async def test_cancer_dependency_ranking_is_one_bounded_precomputed_query(self):
        result = await self.service.lineage_dependencies("乳腺癌", "selective", 10)
        self.assertEqual(
            self.queries[0],
            {
                "mode": "lineage_dependency",
                "lineage": "Breast",
                "ranking": "selective",
                "limit": 11,
            },
        )
        self.assertEqual(
            result["request"],
            {"lineage": "Breast", "ranking": "selective", "limit": 10},
        )
        self.assertEqual(
            result["evidence"]["metric_semantics"]["metric"],
            "gene_effect_lineage_vs_rest",
        )
        self.assertIn("not logFC", result["evidence"]["metric_semantics"]["interpretation"])
        self.assertFalse(result["new_analysis_started"])

        breast_cancer = await self.service.lineage_dependencies(
            "Breast Cancer", "selective", 10
        )
        short_alias = await self.service.lineage_dependencies("乳癌", "selective", 10)
        self.assertEqual(result["evidence_id"], breast_cancer["evidence_id"])
        self.assertEqual(result["evidence_id"], short_alias["evidence_id"])

    async def test_mcp_page_cursor_returns_the_next_window(self):
        async def paged_runner(_settings, query):
            rows = [{"rank": rank} for rank in range(1, min(query["limit"], 5) + 1)]
            return {
                "mode": "lineage_dependency",
                "status": "FOUND",
                "rows": rows,
                "summary": {"total_retained_rows": 5, "returned_count": len(rows)},
            }

        service = DepMapEvidenceService(self.settings, paged_runner)
        first = await service.lineage_dependencies("Liver", limit=2)
        first_result = first["evidence"]["result"]
        cursor = first_result["page_info"]["next_cursor"]
        second = await service.lineage_dependencies("Liver", limit=2, cursor=cursor)
        second_result = second["evidence"]["result"]

        self.assertEqual([row["rank"] for row in first_result["rows"]], [1, 2])
        self.assertEqual([row["rank"] for row in second_result["rows"]], [3, 4])
        self.assertEqual(second_result["page_info"]["total_retained_rows"], 5)
        self.assertTrue(second_result["page_info"]["has_more"])

    async def test_lineage_resolution_requires_confirmation_for_ambiguity(self):
        exact = await self.service.resolve_lineage("乳腺癌")
        self.assertEqual(exact["evidence"]["status"], "RESOLVED")
        self.assertEqual(exact["evidence"]["selected_lineage"], "Breast")

        ambiguous = await self.service.resolve_lineage("白血病")
        self.assertEqual(ambiguous["evidence"]["status"], "AMBIGUOUS")
        self.assertEqual(
            ambiguous["evidence"]["candidates"], ["Myeloid", "Lymphoid"]
        )
        self.assertTrue(ambiguous["evidence"]["requires_user_confirmation"])

        proposed = await self.service.resolve_lineage(
            "胃食管交界部恶性肿瘤", ["Esophagus Stomach"]
        )
        self.assertEqual(proposed["evidence"]["status"], "PROPOSED")
        self.assertIsNone(proposed["evidence"]["selected_lineage"])

    async def test_generic_entity_resolution_uses_one_typed_contract(self):
        result = await self.service.resolve_entity(
            "molecular_focus", "转录因子"
        )
        evidence = result["evidence"]
        self.assertEqual(evidence["schema_version"], "wisp.entity-resolution.v1")
        self.assertEqual(evidence["status"], "RESOLVED")
        self.assertEqual(evidence["selected"]["label"], "transcription_factor")
        self.assertFalse(evidence["is_scientific_evidence"])

    async def test_pair_semantics_distinguish_correlation_and_group_difference(self):
        result = await self.service.pair_evidence("KRAS", "RAF1")
        items = result["evidence"]["items"]
        semantics = {
            item["query"]["module"]: item["metric_semantics"]["metric"]
            for item in items
        }
        self.assertEqual(semantics["effect_correlation"], "correlation")
        self.assertEqual(
            semantics["damaging_mutation_dependency"], "mean_difference"
        )

    async def test_subtype_tool_is_one_bounded_query_with_canonical_lineage(self):
        result = await self.service.subtype_evidence(
            gene="wrn", lineage="结肠癌", contrast_id="FEATURE__BOWEL__MSI", limit=7
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "subtype",
                "gene": "WRN",
                "lineage": "Bowel",
                "contrast": "FEATURE__BOWEL__MSI",
                "limit": 8,
            },
        )
        self.assertEqual(result["request"]["lineage"], "Bowel")
        self.assertEqual(
            result["evidence"]["metric_semantics"]["metric"],
            "within_lineage_subtype_gene_effect_difference",
        )

    async def test_coamplification_tool_normalizes_genes_and_preserves_layer(self):
        result = await self.service.coamplification_evidence(
            "cttn", "rnf121", "tfec", "lineage_adjusted", 9
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "coamplification",
                "source": "CTTN",
                "partner": "RNF121",
                "target": "TFEC",
                "layer": "lineage_adjusted",
                "limit": 10,
            },
        )
        self.assertEqual(result["request"]["source"], "CTTN")
        self.assertEqual(
            result["evidence"]["metric_semantics"]["metric"],
            "coamplification_dependency_difference",
        )

    async def test_true_love_and_synthetic_lethal_tools_use_explicit_noncausal_contracts(self):
        true_love = await self.service.true_love_evidence("kras", "nras", 8)
        self.assertEqual(
            self.queries[-1],
            {"mode": "true_love", "gene": "KRAS", "partner": "NRAS", "limit": 9},
        )
        self.assertIn("not proof", true_love["evidence"]["metric_semantics"]["interpretation"])
        synthetic = await self.service.synthetic_lethal_evidence(
            "arid1a", "arid1b", "damaging_mutation", 6
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "synthetic_lethal",
                "source": "ARID1A",
                "target": "ARID1B",
                "event": "damaging_mutation",
                "limit": 7,
            },
        )
        self.assertIn("not causal", synthetic["evidence"]["metric_semantics"]["interpretation"])

    async def test_three_d_tool_preserves_family_and_cohort_selectors(self):
        result = await self.service.three_d_evidence(
            "dependency_profiles", gene="kras", cohort="three_d_all", limit=4
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "three_d",
                "family": "dependency_profiles",
                "gene": "KRAS",
                "cohort": "three_d_all",
                "limit": 5,
            },
        )
        self.assertEqual(result["request"]["family"], "dependency_profiles")
        self.assertFalse(result["new_analysis_started"])

    def test_expression_dependency_uses_its_real_target_gene_order(self):
        query_script = (
            Path(__file__).resolve().parents[3]
            / "skills"
            / "depmap-knowledge-query"
            / "scripts"
            / "query_depmap_kb.R"
        ).read_text(encoding="utf-8")
        self.assertIn(
            'expression_dependency=list(source="expression_gene_order.csv",target="dependency_gene_order.csv"',
            query_script,
        )

    async def test_stdio_protocol_lists_read_only_tools_and_calls_status(self):
        env = {
            **os.environ,
            "DEPMAP_KNOWLEDGE_ROOT": str(self.root),
            "DEPMAP_QUERY_SCRIPT": str(self.script),
            "DEPMAP_RELEASE": "26Q1",
        }
        params = StdioServerParameters(
            command=sys.executable,
            args=["-m", "services.depmap_mcp", "--transport", "stdio"],
            env=env,
            cwd=str(Path(__file__).resolve().parents[3]),
        )
        async with stdio_client(params) as (read, write):
            async with ClientSession(read, write) as session:
                await session.initialize()
                tools = await session.list_tools()
                names = {tool.name for tool in tools.tools}
                self.assertEqual(
                    names,
                    {
                        "depmap_status",
                        "depmap_describe_capabilities",
                        "depmap_resolve_lineage",
                        "depmap_resolve_entity",
                        "depmap_lineage_catalog",
                        "depmap_lineage_dependencies",
                        "depmap_lineage_direction_discovery",
                        "depmap_topic_plan",
                        "depmap_topic_plan",
                        "depmap_gene_evidence",
                        "tcga_gene_expression_survival",
                        "depmap_pair_evidence",
                        "depmap_drug_evidence",
                        "depmap_subtype_evidence",
                        "depmap_coamplification_evidence",
                        "depmap_true_love_evidence",
                        "depmap_synthetic_lethal_evidence",
                        "depmap_3d_evidence",
                    },
                )
                self.assertTrue(
                    all(tool.annotations.readOnlyHint for tool in tools.tools)
                )
                dependency_tool = next(
                    tool for tool in tools.tools
                    if tool.name == "depmap_lineage_dependencies"
                )
                self.assertIn("cursor", dependency_tool.inputSchema["properties"])
                called = await session.call_tool("depmap_status", {})
                self.assertFalse(called.isError)
                self.assertEqual(called.structuredContent["release"], "26Q1")
                self.assertFalse(
                    called.structuredContent["evidence"]["data_sources"]["tcga"][
                        "installed"
                    ]
                )


if __name__ == "__main__":
    unittest.main()
