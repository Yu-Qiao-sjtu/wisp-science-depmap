import json
import gzip
import os
import sqlite3
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path
from typing import get_args

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

from services.depmap_api.app import Settings
from services.depmap_api.app import QueryRequest
from services.depmap_mcp.catalog_readers import MODE_ALIASES
from services.depmap_mcp.server import DepMapEvidenceService
from services.depmap_mcp.server import MAX_MODEL_EVIDENCE_BYTES


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

    def index_resource(self, relative: str, content: str) -> str:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        index = self.root / "depmap-26q1-query-index.sqlite"
        with closing(sqlite3.connect(index)) as db:
            db.execute(
                "CREATE TABLE IF NOT EXISTS artifact_catalog "
                "(artifact_path TEXT PRIMARY KEY, artifact_kind TEXT, size_bytes INTEGER)"
            )
            db.execute(
                "INSERT OR REPLACE INTO artifact_catalog VALUES (?,?,?)",
                (relative, "manifest", path.stat().st_size),
            )
            db.commit()
        return f"depmap://26Q1/{relative}"

    def index_bytes(self, relative: str, content: bytes) -> str:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        index = self.root / "depmap-26q1-query-index.sqlite"
        with closing(sqlite3.connect(index)) as db:
            db.execute(
                "CREATE TABLE IF NOT EXISTS artifact_catalog "
                "(artifact_path TEXT PRIMARY KEY, artifact_kind TEXT, size_bytes INTEGER)"
            )
            db.execute(
                "INSERT OR REPLACE INTO artifact_catalog VALUES (?,?,?)",
                (relative, "result", path.stat().st_size),
            )
            db.commit()
        return f"depmap://26Q1/{relative}"

    async def test_compressed_table_reader_honors_bounds_counts_and_cursor(self):
        relative = "depmap-26q1-full/results/pairs.csv.gz"
        payload = "gene,value\n" + "".join(
            f"G{index},{index}\n" for index in range(110)
        )
        uri = self.index_bytes(relative, gzip.compress(payload.encode("utf-8")))

        first = await self.service.read_resource(uri, max_rows=100)
        self.assertEqual(first["evidence"]["returned_count"], 100)
        self.assertEqual(first["evidence"]["total_row_count"], 110)
        self.assertTrue(first["evidence"]["truncated"])
        self.assertEqual(first["evidence"]["next_cursor"], 100)
        self.assertEqual(first["evidence"]["rows"][0]["gene"], "G0")

        tail = await self.service.read_resource(uri, max_rows=100, cursor=100)
        self.assertEqual(tail["evidence"]["returned_count"], 10)
        self.assertEqual(tail["evidence"]["rows"][0]["gene"], "G100")
        self.assertFalse(tail["evidence"]["truncated"])
        self.assertIsNone(tail["evidence"]["next_cursor"])

    async def test_csv_resource_pages_honor_max_rows_greater_than_one(self):
        relative = "analysis-modules/tf/tf_order.csv"
        payload = "TF\n" + "".join(f"TF{index}\n" for index in range(8))
        uri = self.index_resource(relative, payload)
        page = await self.service.read_resource(uri, max_rows=5)
        self.assertEqual(page["evidence"]["returned_count"], 5)
        self.assertEqual(page["evidence"]["total_row_count"], 8)
        self.assertGreater(page["evidence"]["returned_count"], 1)

    async def test_compressed_table_reader_types_empty_and_malformed_inputs(self):
        empty_uri = self.index_bytes(
            "depmap-26q1-full/results/empty.csv.gz",
            gzip.compress(b"gene,value\n"),
        )
        empty = await self.service.read_resource(empty_uri, max_rows=20)
        self.assertEqual(empty["evidence"]["status"], "NOT_RETAINED")
        self.assertEqual(empty["evidence"]["total_row_count"], 0)
        self.assertEqual(empty["evidence"]["rows"], [])

        bad_uri = self.index_bytes(
            "depmap-26q1-full/results/broken.csv.gz", b"not gzip"
        )
        broken = await self.service.read_resource(bad_uri, max_rows=20)
        self.assertEqual(broken["evidence"]["status"], "ERROR")
        self.assertEqual(broken["evidence"]["returned_count"], 0)

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

    async def test_read_resource_recursively_redacts_absolute_paths(self):
        inside = self.root / "depmap-26q1-full" / "blocks" / "part-001.rds"
        uri = self.index_resource(
            "depmap-26q1-full/true_love_gene/manifest.json",
            json.dumps(
                {
                    "inputs": [
                        str(inside),
                        "/home/private/depmap/secret.csv",
                        r"C:\Users\analyst\secret.csv",
                        r"\\fileserver\team\secret.csv",
                        "/scratch",
                        str(self.root) + "-secrets/token.txt",
                    ],
                    "/private/path/as-key": "key is sanitized too",
                    "/another/private/key": "second key survives",
                    "nested": {"safe_url": "https://example.org/reference"},
                }
            ),
        )

        result = await self.service.read_resource(uri, max_rows=20)
        content = result["evidence"]["content"]
        self.assertEqual(
            content["inputs"][0],
            "depmap://26Q1/depmap-26q1-full/blocks/part-001.rds",
        )
        self.assertEqual(content["inputs"][1:], ["<redacted:absolute-path>"] * 5)
        self.assertEqual(content["nested"]["safe_url"], "https://example.org/reference")
        self.assertEqual(content["<redacted:absolute-path>"], "key is sanitized too")
        self.assertEqual(
            content["<redacted:absolute-path>#2"], "second key survives"
        )
        serialized = json.dumps(result)
        self.assertNotIn(str(self.root), serialized)
        self.assertNotIn("/home/private", serialized)
        self.assertNotIn("fileserver", serialized)
        self.assertNotIn("-secrets", serialized)

    async def test_read_resource_sanitizes_csv_fields_and_text_previews(self):
        csv_uri = self.index_resource(
            "depmap-26q1-full/fixture.csv",
            "gene,input,note\nESR1,/srv/private/input.rds,safe\n",
        )
        csv_result = await self.service.read_resource(csv_uri, max_rows=10)
        self.assertEqual(
            csv_result["evidence"]["content"][0]["input"],
            "<redacted:absolute-path>",
        )

        text_uri = self.index_resource(
            "depmap-26q1-full/fixture.yaml",
            "input: /srv/private/input.rds\nwindows: D:\\private\\input.rds\n",
        )
        text_result = await self.service.read_resource(text_uri, max_rows=10)
        preview = text_result["evidence"]["content"]
        self.assertNotIn("/srv/private", preview)
        self.assertNotIn(r"D:\private", preview)
        self.assertEqual(preview.count("<redacted:absolute-path>"), 2)

    def test_every_bounded_query_mode_has_a_catalog_reader_family(self):
        modes = set(get_args(QueryRequest.model_fields["mode"].annotation))
        self.assertEqual(modes - set(MODE_ALIASES), set())
        self.assertEqual(MODE_ALIASES["enrichment"], "enrichment")

    async def test_capability_catalog_is_lightweight_and_marks_direction_ambiguity(self):
        result = await self.service.capabilities()
        self.assertEqual(result["state"], "CAPABILITY_CATALOG")
        self.assertEqual(self.queries, [])
        intents = {item["intent"]: item for item in result["capabilities"]}
        self.assertEqual(
            set(intents),
            {
                "provider_status",
                "lineage_resolution",
                "cancer_inventory",
                "cancer_direction_discovery",
                "analysis_inventory",
                "mutation_anchor_discovery",
                "mutation_to_dependency",
                "dependency_to_mutation",
                "gene_pair_evidence",
                "cancer_dependency_ranking",
                "pan_cancer_dependency_summary",
                "tf_activity_to_dependency",
                "expression_biomarker_model",
                "true_love_gene_catalog",
                "gene_evidence",
                "tcga_expression_survival",
                "drug_gene_evidence",
                "subtype_evidence",
                "coamplification_evidence",
                "three_d_evidence",
            },
        )
        self.assertIn("mutation_to_dependency", intents)
        self.assertIn("dependency_to_mutation", intents)
        self.assertIn(
            "dependency_to_mutation",
            intents["mutation_to_dependency"]["confusable_with"],
        )
        self.assertIn(
            "{source_gene}",
            intents["mutation_to_dependency"]["precise_prompt_template_zh"],
        )
        self.assertEqual(
            result["routing_policy"]["critical_direction_ambiguity"],
            "clarify_before_query",
        )
        self.assertIn("lineage", intents["mutation_to_dependency"]["optional"])
        self.assertIn("lineage", intents["dependency_to_mutation"]["optional"])
        self.assertIn("gene", intents["mutation_anchor_discovery"]["optional"])
        self.assertEqual(
            intents["mutation_anchor_discovery"]["mcp_tool"],
            "depmap_mutation_anchor_evidence",
        )

    async def test_capability_catalog_skips_nullable_or_malformed_records(self):
        index = self.root / "depmap-26q1-query-index.sqlite"
        valid = {"intent": "provider_status", "mcp_tool": "depmap_status"}
        with closing(sqlite3.connect(index)) as db:
            db.execute("CREATE TABLE capability_catalog (payload_json TEXT)")
            db.executemany(
                "INSERT INTO capability_catalog VALUES (?)",
                [(json.dumps(valid),), ("null",), ("{broken",)],
            )
            db.commit()

        result = await self.service.capabilities()
        self.assertEqual(result["catalog_source"], "sqlite_capability_catalog")
        self.assertEqual(result["catalog_status"], "PARTIAL")
        self.assertEqual(result["invalid_record_count"], 2)
        self.assertEqual(result["capabilities"], [valid])

    async def test_data_coverage_is_bounded_and_omits_internal_paths(self):
        index = self.root / "depmap-26q1-query-index.sqlite"
        with closing(sqlite3.connect(index)) as db:
            db.execute(
                "CREATE TABLE coverage_registry (analysis_id TEXT,module TEXT,release TEXT,"
                "scope TEXT,lineage TEXT,modality TEXT,model_count INTEGER,"
                "model_set_fingerprint TEXT,tested_gene_count INTEGER,retained_gene_count INTEGER,"
                "gene_universe TEXT,cohort_definition TEXT,intersection_policy TEXT,"
                "event_definition TEXT,mutation_policy TEXT,threshold_definition TEXT,"
                "source_asset_fingerprint TEXT,storage_completeness TEXT,"
                "qa_state TEXT,generated_at TEXT,payload_json TEXT)"
            )
            db.execute(
                "CREATE TABLE reader_coverage (query_mode TEXT,analysis_id TEXT,coverage_state TEXT)"
            )
            db.execute(
                "INSERT INTO coverage_registry VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                (
                    "analysis-1", "dependency", "26Q1", "lineage", "Liver",
                    "CRISPRGeneEffect", 25, "cohort-hash", 18531, 17787,
                    "DepMap genes", '"Liver vs rest"', '"GeneEffect intersection"',
                    '"damaging > 0"', '"Mut/WT/missing"', '"FDR <= 0.05"',
                    "asset-hash", "full", "PASS",
                    "2026-09-18T00:00:00Z", '{"private_path":"/srv/secret"}',
                ),
            )
            db.execute(
                "INSERT INTO reader_coverage VALUES ('lineage_dependency','analysis-1','AVAILABLE')"
            )
            db.commit()

        result = await self.service.data_coverage(lineage="Liver", limit=1000)
        self.assertEqual(result["evidence"]["status"], "FOUND")
        self.assertEqual(result["evidence"]["returned_count"], 1)
        self.assertEqual(result["evidence"]["rows"][0]["model_count"], 25)
        serialized = json.dumps(result)
        self.assertNotIn("private_path", serialized)
        self.assertNotIn("/srv/secret", serialized)

    async def test_analysis_catalog_uses_completed_directory_index(self):
        result = await self.service.analysis_catalog("癌种内突变锚定基因选择", 25)
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "analysis_catalog",
                "completion_state": "COMPLETE",
                "module": "癌种内突变锚定基因选择",
                "limit": 25,
            },
        )
        self.assertEqual(result["request"]["mode"], "analysis_catalog")
        self.assertEqual(
            result["presentation_contract"]["answer_type"], "analysis_inventory"
        )
        self.assertTrue(result["presentation_contract"]["do_not_answer_with_paths_only"])

    async def test_analysis_catalog_normalizes_null_adapter_result(self):
        async def null_runner(_settings, _query):
            return None

        service = DepMapEvidenceService(self.settings, null_runner)
        result = await service.analysis_catalog("not-indexed", 10)

        self.assertEqual(result["evidence"]["status"], "NOT_RETAINED")
        self.assertEqual(result["evidence"]["result"]["rows"], [])
        self.assertEqual(result["evidence"]["result"]["returned_count"], 0)

    async def test_mutation_anchor_intent_queries_dedicated_result(self):
        result = await self.service.mutation_anchor_evidence(
            "Lung", "damaging", "priority", False, 12
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "mutation_anchor",
                "lineage": "Lung",
                "event": "damaging",
                "anchor_tier": "priority",
                "include_common_essential": False,
                "limit": 12,
            },
        )
        self.assertEqual(result["request"]["lineage"], "Lung")

    async def test_mutation_anchor_exact_gene_is_forwarded(self):
        result = await self.service.mutation_anchor_evidence(
            "肝癌", "damaging", "priority", False, 1, "ptk7"
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "mutation_anchor",
                "lineage": "Liver",
                "event": "damaging",
                "anchor_tier": "priority",
                "include_common_essential": False,
                "limit": 1,
                "gene": "PTK7",
            },
        )
        self.assertEqual(result["request"]["gene"], "PTK7")
        self.assertEqual(result["request"]["lineage"], "Liver")

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
                "limit": 6,
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

    async def test_large_evidence_is_projected_without_changing_evidence_id(self):
        async def large_runner(_settings, query):
            return {
                "mode": query["mode"],
                "status": "FOUND",
                "result": {
                    "rows": [
                        {"symbol": f"G{index}", "detail": "x" * 5000}
                        for index in range(200)
                    ]
                },
            }

        service = DepMapEvidenceService(self.settings, large_runner)
        first = await service.pan_cancer_dependencies(limit=5)
        second = await service.pan_cancer_dependencies(limit=5)
        self.assertEqual(first["evidence_id"], second["evidence_id"])
        self.assertTrue(first["model_projection"]["is_bounded_projection"])
        self.assertLessEqual(
            first["model_projection"]["projected_bytes"], MAX_MODEL_EVIDENCE_BYTES
        )
        self.assertIn("result", first["evidence"])
        self.assertGreater(
            len(first["evidence"]["result"]["result"]["rows"]), 0
        )
        self.assertGreater(first["model_projection"]["omitted_items"], 0)

    async def test_projection_rechecks_budget_for_many_long_provenance_strings(self):
        async def provenance_runner(_settings, query):
            return {
                "status": "FOUND",
                "result": {"rows": [{"symbol": "ESR1", "score": -0.42}]},
                "provenance": ["p" * 5000 for _ in range(40)],
            }

        service = DepMapEvidenceService(self.settings, provenance_runner)
        result = await service.pan_cancer_dependencies(limit=5)
        self.assertLessEqual(
            result["model_projection"]["projected_bytes"], MAX_MODEL_EVIDENCE_BYTES
        )
        self.assertEqual(
            result["evidence"]["result"]["result"]["rows"][0]["symbol"],
            "ESR1",
        )
        self.assertGreater(result["model_projection"]["truncated_strings"], 0)

    async def test_cancer_only_direction_discovery_needs_no_anchor_gene(self):
        result = await self.service.lineage_directions("结肠癌", 20)
        self.assertEqual(result["request"], {"lineage": "结肠癌", "limit": 20})
        self.assertEqual(self.queries[0], {"mode": "lineage_directions", "lineage": "Bowel", "limit": 20})
        self.assertNotIn("gene", result["request"])

    async def test_cancer_dependency_ranking_is_one_bounded_precomputed_query(self):
        result = await self.service.lineage_dependencies("乳腺癌", "selective", 10)
        self.assertEqual(
            self.queries[0],
            {
                "mode": "lineage_dependency",
                "lineage": "Breast",
                "ranking": "selective",
                "exclude_common_essential": False,
                "common_essential_source": "depmap_26q1",
                "limit": 10,
            },
        )
        self.assertEqual(
            result["request"],
            {
                "lineage": "Breast",
                "ranking": "selective",
                "exclude_common_essential": False,
                "common_essential_source": "depmap_26q1",
                "limit": 10,
            },
        )
        self.assertEqual(
            result["evidence"]["metric_semantics"]["metric"],
            "gene_effect_lineage_vs_rest",
        )
        self.assertIn("not logFC", result["evidence"]["metric_semantics"]["interpretation"])
        self.assertFalse(result["new_analysis_started"])

        filtered = await self.service.lineage_dependencies(
            "乳腺癌", "selective", 10, exclude_common_essential=True
        )
        self.assertTrue(self.queries[-1]["exclude_common_essential"])
        self.assertEqual(self.queries[-1]["common_essential_source"], "depmap_26q1")

        breast_cancer = await self.service.lineage_dependencies(
            "Breast Cancer", "selective", 10
        )
        short_alias = await self.service.lineage_dependencies("乳癌", "selective", 10)
        self.assertEqual(result["evidence_id"], breast_cancer["evidence_id"])
        self.assertEqual(result["evidence_id"], short_alias["evidence_id"])

    async def test_lineage_dependency_forwards_exact_gene(self):
        result = await self.service.lineage_dependencies(
            "肝癌", "selective", 5, gene="mdm2"
        )
        self.assertEqual(self.queries[-1]["gene"], "MDM2")
        self.assertEqual(self.queries[-1]["lineage"], "Liver")
        self.assertEqual(result["request"]["gene"], "MDM2")

    async def test_pan_cancer_dependency_forwards_exact_gene(self):
        result = await self.service.pan_cancer_dependencies("selective", 5, gene="fbxo7")
        self.assertEqual(self.queries[-1]["gene"], "FBXO7")
        self.assertEqual(result["request"]["gene"], "FBXO7")

    async def test_pan_cancer_dependency_summary_is_one_bounded_query(self):
        result = await self.service.pan_cancer_dependencies(
            "selective", 5, exclude_common_essential=True
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "pan_cancer_dependency",
                "ranking": "selective",
                "exclude_common_essential": True,
                "common_essential_source": "depmap_26q1",
                "limit": 5,
            },
        )
        self.assertEqual(result["request"], self.queries[-1])
        self.assertEqual(
            result["evidence"]["metric_semantics"]["metric"],
            "gene_effect_lineage_vs_rest",
        )

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

        correlation_semantics = {
            item["query"]["module"]: item["metric_semantics"]
            for item in items
            if item["query"]["module"] in {
                "effect_correlation", "expression_correlation", "expression_dependency"
            }
        }
        self.assertEqual(
            correlation_semantics["effect_correlation"]["analysis_label"],
            "gene_gene_codependency",
        )
        self.assertEqual(
            correlation_semantics["effect_correlation"]["data_modality"],
            "crispr_gene_effect",
        )
        self.assertEqual(
            correlation_semantics["expression_correlation"]["analysis_label"],
            "gene_gene_coexpression",
        )
        self.assertEqual(
            correlation_semantics["expression_correlation"]["data_modality"],
            "transcript_expression_log2_tpm_plus_1",
        )
        self.assertEqual(
            correlation_semantics["expression_dependency"]["relation_type"],
            "predictive_association",
        )

    async def test_tf_activity_dependency_query_preserves_direction_and_fdr_scope(self):
        result = await self.service.tf_dependency_evidence("stat3", "gpx4", 12)
        self.assertEqual(
            self.queries[-1],
            {"mode": "tf_dependency", "source": "STAT3", "target": "GPX4", "limit": 12},
        )
        self.assertEqual(result["request"]["transcription_factor"], "STAT3")
        semantics = result["evidence"]["metric_semantics"]
        self.assertEqual(semantics["analysis_label"], "inferred_tf_activity_to_crispr_dependency")
        self.assertIn("stronger dependency", semantics["interpretation"])
        self.assertFalse(result["new_analysis_started"])

    async def test_tf_universe_and_bulk_ranking_are_forwarded(self):
        universe = await self.service.tf_dependency_evidence(view="universe", limit=20)
        self.assertEqual(
            self.queries[-1],
            {"mode": "tf_dependency", "limit": 20, "view": "universe"},
        )
        self.assertEqual(universe["request"]["view"], "universe")
        bulk = await self.service.tf_dependency_evidence(limit=5)
        self.assertEqual(self.queries[-1], {"mode": "tf_dependency", "limit": 5})

    async def test_biomarker_model_intent_bridges_target_to_indexed_query(self):
        result = await self.service.biomarker_model_evidence("gpx4")
        self.assertEqual(
            self.queries[-1],
            {"mode": "biomarker_target", "target": "GPX4"},
        )
        self.assertEqual(result["request"]["target_gene"], "GPX4")
        semantics = result["evidence"]["metric_semantics"]
        self.assertEqual(semantics["analysis_label"], "expression_to_dependency_predictive_biomarker_model")
        self.assertIn("not evidence", semantics["interpretation"])

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
                "limit": 7,
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
                "limit": 9,
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
            {"mode": "true_love", "gene": "KRAS", "partner": "NRAS", "limit": 8, "catalog": "stable_negative_rank1", "scope": "pancancer"},
        )
        self.assertIn("not proof", true_love["evidence"]["metric_semantics"]["interpretation"])
        positive = await self.service.true_love_evidence(
            "kras", "raf1", 6, "positive_reciprocal_top20", "quality"
        )
        self.assertEqual(self.queries[-1]["catalog"], "positive_reciprocal_top20")
        self.assertEqual(self.queries[-1]["coverage"], "quality")
        self.assertIn("similar dependency profiles", positive["evidence"]["metric_semantics"]["interpretation"])
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
                "limit": 6,
            },
        )
        self.assertIn("not causal", synthetic["evidence"]["metric_semantics"]["interpretation"])
        lineage_scoped = await self.service.synthetic_lethal_evidence(
            "ptk7", None, "damaging_mutation", 5, "肝癌"
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "lineage_mutation_dependency",
                "lineage": "Liver",
                "source": "PTK7",
                "event": "damaging_mutation",
                "limit": 5,
            },
        )
        self.assertEqual(
            lineage_scoped["request"]["provider"],
            "lineage_official_gene_effect_v2",
        )
        self.assertIn(
            "not causal synthetic lethality",
            lineage_scoped["evidence"]["metric_semantics"]["interpretation"],
        )

    async def test_provider_schema_matches_runtime_and_returns_envelopes(self):
        catalog = await self.service.capabilities()
        tlg = next(
            item
            for item in catalog["capabilities"]
            if item["intent"] == "true_love_gene_catalog"
        )
        self.assertNotIn("coverage", tlg["optional"])
        self.assertEqual(tlg["limit_max"], 100)
        self.assertEqual(
            tlg["conditional_optional"]["coverage"]["when_catalog_in"],
            ["negative_r_lt_minus_0_3", "positive_reciprocal_top20"],
        )
        coverage = await self.service.true_love_evidence(
            None, None, 20, "stable_negative_rank1", "quality"
        )
        self.assertTrue(coverage["evidence"]["schema_error"])
        self.assertEqual(coverage["evidence"]["status"], "INELIGIBLE")
        self.assertEqual(self.queries, [])
        limit_60 = await self.service.lineage_directions("Liver", 60)
        self.assertTrue(limit_60["evidence"]["schema_error"])
        self.assertEqual(limit_60["evidence"]["limit_max"], 50)
        limit_120 = await self.service.true_love_evidence(None, None, 120)
        self.assertTrue(limit_120["evidence"]["schema_error"])
        limit_150 = await self.service.gene_evidence("KRAS", None, None, 150)
        self.assertTrue(limit_150["evidence"]["schema_error"])
        valid = await self.service.true_love_evidence(
            "kras", "nras", 8, "stable_negative_rank1", None
        )
        self.assertEqual(valid["evidence"]["status"], "FOUND")
        lineage = await self.service.true_love_evidence(
            None, None, 8, "stable_negative_rank1", None, "lineage", "Liver"
        )
        self.assertEqual(self.queries[-1]["scope"], "lineage")
        self.assertEqual(self.queries[-1]["lineage"], "Liver")
        mixed = await self.service.true_love_evidence(
            None, None, 8, "stable_negative_rank1", None, "pancancer", "Liver"
        )
        self.assertTrue(mixed["evidence"]["schema_error"])

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
                "limit": 4,
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

    def test_r_evidence_json_uses_lossless_numeric_serialization(self):
        query_script = (
            Path(__file__).resolve().parents[3]
            / "skills"
            / "depmap-knowledge-query"
            / "scripts"
            / "query_depmap_kb.R"
        ).read_text(encoding="utf-8")
        self.assertIn("digits=NA", query_script)
        self.assertNotIn("digits=4", query_script)

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
                        "depmap_analysis_catalog",
                        "depmap_artifact_catalog",
                        "depmap_data_coverage",
                        "depmap_read_resource",
                        "depmap_mutation_anchor_evidence",
                        "depmap_capabilities",
                        "depmap_status",
                        "depmap_resolve_lineage",
                        "depmap_lineage_catalog",
                        "depmap_lineage_dependencies",
                        "depmap_pan_cancer_dependencies",
                        "depmap_lineage_direction_discovery",
                        "depmap_gene_evidence",
                        "tcga_gene_expression_survival",
                        "depmap_pair_evidence",
                        "depmap_tf_dependency_evidence",
                        "depmap_biomarker_model_evidence",
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
