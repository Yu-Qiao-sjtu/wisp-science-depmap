import json
import gzip
import hashlib
import os
import shutil
import sqlite3
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path
from typing import get_args

from jsonschema.validators import validator_for
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

from services.depmap_api.app import Settings
from services.depmap_api.app import QueryRequest
from services.depmap_mcp.catalog_readers import CatalogResolution
from services.depmap_mcp.catalog_readers import MODE_ALIASES
from services.depmap_mcp.artifact_integrity import (
    index_digest_path,
    index_publish_marker_path,
    write_index_digest,
)
from services.depmap_mcp.portable_refs import PortableReferences
from services.depmap_mcp.server import DepMapEvidenceService
from services.depmap_mcp.server import MAX_MODEL_EVIDENCE_BYTES
from services.depmap_mcp.server import _bounded_model_projection


class DepMapMcpTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        # Windows runners may expose the temporary directory through an 8.3
        # alias while child paths resolve to the long form. Keep the fixture's
        # configured knowledge root canonical so the path-boundary assertion
        # compares equivalent paths on every platform.
        self.root = Path(self.temp.name).resolve()
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
                "(artifact_path TEXT PRIMARY KEY, artifact_kind TEXT, size_bytes INTEGER, "
                "integrity_method TEXT, integrity_value TEXT, integrity_state TEXT, "
                "integrity_reason_code TEXT)"
            )
            db.execute(
                "INSERT OR REPLACE INTO artifact_catalog VALUES (?,?,?,?,?,?,?)",
                (
                    relative,
                    "manifest",
                    path.stat().st_size,
                    "sha256",
                    hashlib.sha256(path.read_bytes()).hexdigest(),
                    "VERIFIED",
                    None,
                ),
            )
            db.commit()
        write_index_digest(index)
        return f"depmap://26Q1/{relative}"

    def index_bytes(self, relative: str, content: bytes) -> str:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        index = self.root / "depmap-26q1-query-index.sqlite"
        with closing(sqlite3.connect(index)) as db:
            db.execute(
                "CREATE TABLE IF NOT EXISTS artifact_catalog "
                "(artifact_path TEXT PRIMARY KEY, artifact_kind TEXT, size_bytes INTEGER, "
                "integrity_method TEXT, integrity_value TEXT, integrity_state TEXT, "
                "integrity_reason_code TEXT)"
            )
            db.execute(
                "INSERT OR REPLACE INTO artifact_catalog VALUES (?,?,?,?,?,?,?)",
                (
                    relative,
                    "result",
                    path.stat().st_size,
                    "sha256",
                    hashlib.sha256(path.read_bytes()).hexdigest(),
                    "VERIFIED",
                    None,
                ),
            )
            db.commit()
        write_index_digest(index)
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

    async def test_wide_csv_projection_drops_duplicate_content_before_rows(self):
        relative = "analysis-modules/wide/table.csv"
        payload = "gene,detail\n" + "".join(
            f"GENE{index},{'x' * 700}\n" for index in range(110)
        )
        uri = self.index_resource(relative, payload)

        page = await self.service.read_resource(uri, max_rows=100)
        evidence = page["evidence"]

        self.assertTrue(page["model_projection"]["is_bounded_projection"])
        self.assertLessEqual(
            page["model_projection"]["projected_bytes"], MAX_MODEL_EVIDENCE_BYTES
        )
        self.assertNotIn("content", evidence)
        self.assertEqual(len(evidence["rows"]), 100)
        self.assertEqual(evidence["returned_count"], 100)
        self.assertNotIn("returned_count_before_projection", evidence)
        self.assertEqual(evidence["total_row_count"], 110)
        self.assertEqual(evidence["next_cursor"], 100)
        self.assertEqual(evidence["uri"], uri)

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
        self.assertEqual(broken["evidence"]["status"], "MODULE_UNAVAILABLE")
        self.assertEqual(
            broken["evidence"]["integrity_reason_code"],
            "TRUNCATED_COMPRESSED_ARTIFACT",
        )
        self.assertEqual(broken["evidence"]["returned_count"], 0)

    async def test_checksum_mismatch_fails_closed_without_leaking_storage_details(self):
        relative = "depmap-26q1-full/results/checksum.csv.gz"
        original = gzip.compress(b"label,value\nA,1\n")
        uri = self.index_bytes(relative, original)
        (self.root / relative).write_bytes(gzip.compress(b"label,value\nB,2\n"))

        result = await self.service.read_resource(uri, max_rows=20)
        evidence = result["evidence"]

        self.assertEqual(evidence["status"], "MODULE_UNAVAILABLE")
        self.assertEqual(evidence["integrity_reason_code"], "CHECKSUM_MISMATCH")
        self.assertEqual(evidence["rows"], [])
        serialized = json.dumps(result)
        self.assertNotIn("EOFError", serialized)
        self.assertNotIn(str(self.root), serialized)

    async def test_gene_evidence_is_bounded_and_portable(self):
        result = await self.service.gene_evidence("esr1", "Breast Cancer", limit=3)
        self.assertEqual(result["request"]["gene"], "ESR1")
        self.assertEqual(result["evidence"]["query_count"], 11)
        self.assertTrue(result["evidence_id"].startswith("depmap-26q1-"))
        self.assertEqual(self.queries[1]["lineage"], "Breast")
        item = result["evidence"]["items"][0]["result"]
        self.assertNotIn("provenance", item)
        self.assertNotIn("manifest", item)

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

    def test_default_model_evidence_omits_provenance_and_server_locations(self):
        shared = {
            "status": "FOUND",
            "rows": [
                {
                    "symbol": "ESR1",
                    "artifact_path": "depmap-26q1-full/results/esr1.csv",
                },
                {
                    "symbol": "TP53",
                    "artifact_path": r"D:\srv\private.tsv",
                },
            ],
        }
        first = self.service._envelope(
            tool="depmap_status",
            request={"mode": "status"},
            evidence={**shared, "provenance": ["source-alpha"], "index_path": r"D:\srv\a"},
        )
        second = self.service._envelope(
            tool="depmap_status",
            request={"mode": "status"},
            evidence={**shared, "provenance": ["source-beta"], "index_path": r"D:\srv\b"},
        )
        self.assertEqual(
            first["evidence"]["rows"][0]["artifact_path"],
            "depmap-26q1-full/results/esr1.csv",
        )
        self.assertNotIn("artifact_path", first["evidence"]["rows"][1])
        self.assertNotIn("provenance", first["evidence"])
        self.assertNotIn("index_path", first["evidence"])
        self.assertNotEqual(first["evidence_id"], second["evidence_id"])

    async def test_status_publishes_deployment_contract_identity(self):
        first = await self.service.status()
        second = await self.service.status()
        evidence = first["evidence"]

        self.assertEqual(evidence["query_contract_version"], 13)
        self.assertEqual(evidence["server_build_identity"], "wisp-depmap-mcp-contract-13")
        self.assertTrue(evidence["capability_catalog_digest"].startswith("sha256:"))
        self.assertEqual(evidence["catalog_build_identity"], "catalog-missing")
        self.assertEqual(
            evidence["capability_catalog_digest"],
            second["evidence"]["capability_catalog_digest"],
        )

    async def test_read_resource_recursively_makes_locations_portable(self):
        inside = self.root / "analysis-modules" / "分析 α" / "part-001.rds"
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
            "depmap://26Q1/analysis-modules/%E5%88%86%E6%9E%90%20%CE%B1/part-001.rds",
        )
        opaque = {
            "reference_type": "opaque_location",
            "state": "OMITTED",
            "reason": "absolute path outside the public knowledge root",
        }
        self.assertEqual(content["inputs"][1:], [opaque] * 5)
        self.assertEqual(content["nested"]["safe_url"], "https://example.org/reference")
        self.assertEqual(content["[private location omitted]"], "key is sanitized too")
        self.assertEqual(
            content["[private location omitted]#2"], "second key survives"
        )
        serialized = json.dumps(result)
        self.assertNotIn(str(self.root), serialized)
        self.assertNotIn("/home/private", serialized)
        self.assertNotIn("fileserver", serialized)
        self.assertNotIn("-secrets", serialized)
        self.assertNotIn("<redacted:absolute-path>", serialized)

    async def test_read_resource_sanitizes_csv_fields_and_text_previews(self):
        csv_uri = self.index_resource(
            "depmap-26q1-full/fixture.csv",
            "gene,input,note\nESR1,/srv/private/input.rds,safe\n",
        )
        csv_result = await self.service.read_resource(csv_uri, max_rows=10)
        self.assertEqual(
            csv_result["evidence"]["content"][0]["input"],
            {
                "reference_type": "opaque_location",
                "state": "OMITTED",
                "reason": "absolute path outside the public knowledge root",
            },
        )

        text_uri = self.index_resource(
            "depmap-26q1-full/fixture.yaml",
            "input: /srv/private/input.rds\nwindows: D:\\private\\input.rds\n",
        )
        text_result = await self.service.read_resource(text_uri, max_rows=10)
        preview = text_result["evidence"]["content"]
        self.assertNotIn("/srv/private", preview)
        self.assertNotIn(r"D:\private", preview)
        self.assertEqual(preview.count("[private location omitted]"), 2)
        self.assertNotIn("<redacted:absolute-path>", preview)

        exact_text_uri = self.index_resource(
            "depmap-26q1-full/exact-path.txt", "/srv/private/input.rds"
        )
        exact_text = await self.service.read_resource(exact_text_uri, max_rows=10)
        self.assertEqual(
            exact_text["evidence"]["content"], "[private location omitted]"
        )
        self.assertIsInstance(exact_text["evidence"]["content"], str)

        malformed_uri_text = self.index_resource(
            "depmap-26q1-full/malformed-uri.txt",
            r"spill: depmap://26Q1/D:\private\spill.json",
        )
        malformed_uri = await self.service.read_resource(
            malformed_uri_text, max_rows=10
        )
        self.assertEqual(
            malformed_uri["evidence"]["content"],
            "spill: [private location omitted]",
        )
        self.assertNotIn("depmap://", malformed_uri["evidence"]["content"])

    async def test_catalog_references_are_public_uris_or_typed_opaque_records(self):
        relative = "analysis-modules/分析 α/manifest.json"
        uri = self.index_resource(relative, '{"state":"complete"}')
        inside = self.root / relative
        collision = str(self.root) + "-private/manifest.json"
        resolution = CatalogResolution(
            state="RESOLVED",
            query_mode="core",
            artifact_uris=(
                relative,
                str(inside),
                "/srv/private/manifest.json",
                r"D:\private\manifest.json",
                r"\\server\share\manifest.json",
                collision,
                r"depmap://26Q1/C:\private\manifest.json",
            ),
            matrix_blocks=(relative,),
        )

        evidence = resolution.evidence(self.settings.release, self.root)
        expected_uri = (
            "depmap://26Q1/analysis-modules/"
            "%E5%88%86%E6%9E%90%20%CE%B1/manifest.json"
        )
        self.assertEqual(uri, "depmap://26Q1/analysis-modules/分析 α/manifest.json")
        self.assertEqual(evidence["artifact_uris"][:2], [expected_uri, expected_uri])
        self.assertEqual(evidence["matrix_blocks"], [expected_uri])
        for item in evidence["artifact_uris"][2:]:
            self.assertEqual(item["reference_type"], "opaque_location")
            self.assertEqual(item["state"], "OMITTED")

        references = PortableReferences(self.root, self.settings.release)
        public_uris = [
            item for item in evidence["artifact_uris"] if isinstance(item, str)
        ] + evidence["matrix_blocks"]
        self.assertTrue(public_uris)
        for public_uri in public_uris:
            self.assertEqual(references.parse_public_uri(public_uri), relative)
        read_back = await self.service.read_resource(expected_uri)
        self.assertEqual(read_back["evidence"]["content"]["state"], "complete")

        serialized = json.dumps(evidence)
        for secret in ("/srv/private", r"D:\private", "server", "-private"):
            self.assertNotIn(secret, serialized)
        self.assertNotIn("<redacted:absolute-path>", serialized)

    def test_portable_boundary_covers_nested_overflow_and_spill_hints(self):
        payload = {
            "overflow": {
                "spill_hint": "read /var/private/spill.json before D:\\private\\next.json",
                "artifact": str(
                    self.root / "analysis-modules" / "分析 α" / "summary.yaml"
                ),
            },
            "provenance": (r"\\server\share\source.tsv",),
        }
        portable = self.service._portable(payload)
        serialized = json.dumps(portable)
        self.assertEqual(
            portable["overflow"]["artifact"],
            "depmap://26Q1/analysis-modules/%E5%88%86%E6%9E%90%20%CE%B1/summary.yaml",
        )
        self.assertEqual(portable["overflow"]["spill_hint"].count("[private location omitted]"), 2)
        self.assertEqual(portable["provenance"][0]["reference_type"], "opaque_location")
        for secret in ("/var/private", r"D:\private", "server"):
            self.assertNotIn(secret, serialized)

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
                "codependency_evidence",
                "gene_pair_evidence",
                "cancer_dependency_ranking",
                "model_gene_effect_slice",
                "cross_platform_dependency_validation",
                "pan_cancer_dependency_summary",
                "tf_activity_to_dependency",
                "expression_biomarker_model",
                "true_love_gene_catalog",
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
        self.assertIn("cursor", intents["cancer_dependency_ranking"]["optional"])
        self.assertIn("cursor", intents["pan_cancer_dependency_summary"]["optional"])
        self.assertEqual(
            intents["mutation_anchor_discovery"]["mcp_tool"],
            "depmap_mutation_anchor_evidence",
        )
        self.assertEqual(
            intents["mutation_to_dependency"]["lineage_mcp_tool"],
            "depmap_lineage_mutation_dependency",
        )

    async def test_capability_catalog_skips_nullable_or_malformed_records(self):
        index = self.root / "depmap-26q1-query-index.sqlite"
        valid = {
            "intent": "provider_status",
            "description": "Describe provider status.",
            "required": [],
            "optional": [],
            "examples_zh": ["检查状态"],
            "precise_prompt_template_zh": "检查服务状态。",
            "confusable_with": [],
            "mcp_tool": "depmap_status",
        }
        with closing(sqlite3.connect(index)) as db:
            db.execute(
                "CREATE TABLE capability_catalog "
                "(intent TEXT,mcp_tool TEXT,payload_json TEXT)"
            )
            db.executemany(
                "INSERT INTO capability_catalog VALUES (?,?,?)",
                [
                    ("provider_status", "depmap_status", json.dumps(valid)),
                    ("wrong_intent", "wrong_tool", json.dumps(valid)),
                    ("missing", "missing", json.dumps({})),
                    ("null", "null", "null"),
                    ("broken", "broken", "{broken"),
                ],
            )
            db.commit()
        write_index_digest(index)

        result = await self.service.capabilities()
        self.assertEqual(result["catalog_source"], "sqlite_capability_catalog")
        self.assertEqual(result["catalog_status"], "PARTIAL")
        self.assertEqual(result["invalid_record_count"], 4)
        self.assertEqual(result["capabilities"], [valid])

    async def test_enrichment_is_advertised_only_with_an_executable_reader(self):
        index = self.root / "depmap-26q1-query-index.sqlite"
        payload = {
            "intent": "gene_evidence",
            "description": "pathway enrichment",
            "required": ["gene"],
            "optional": [],
            "examples_zh": ["富集"],
            "precise_prompt_template_zh": "查询富集。",
            "confusable_with": [],
            "mcp_tool": "depmap_gene_evidence",
        }
        with closing(sqlite3.connect(index)) as db:
            db.executescript(
                """
                CREATE TABLE capability_catalog (
                  intent TEXT, mcp_tool TEXT, payload_json TEXT
                );
                CREATE TABLE reader_registry (
                  query_mode TEXT PRIMARY KEY, adapter TEXT, module_pattern TEXT,
                  supported_formats TEXT
                );
                CREATE TABLE reader_coverage (
                  query_mode TEXT PRIMARY KEY, analysis_id TEXT, coverage_state TEXT
                );
                CREATE TABLE analysis_catalog (
                  analysis_id TEXT PRIMARY KEY, release TEXT, completion_state TEXT
                );
                """
            )
            db.execute(
                "INSERT INTO capability_catalog VALUES (?,?,?)",
                ("gene_evidence", "depmap_gene_evidence", json.dumps(payload)),
            )
            db.execute(
                "INSERT INTO reader_registry VALUES ('enrichment','enrichment_adapter','*富集*','parquet')"
            )
            db.execute(
                "INSERT INTO reader_coverage VALUES ('enrichment', NULL, 'NOT_COMPUTED')"
            )
            db.execute(
                "INSERT INTO analysis_catalog VALUES ('analysis-1', '25Q4', 'COMPLETE')"
            )
            db.commit()
        write_index_digest(index)
        hidden = await self.service.capabilities()
        self.assertNotIn(
            "gene_evidence",
            {item["intent"] for item in hidden["capabilities"]},
        )

        with closing(sqlite3.connect(index)) as db:
            db.execute(
                "UPDATE reader_coverage SET analysis_id='analysis-1', coverage_state='AVAILABLE' "
                "WHERE query_mode='enrichment'"
            )
            db.commit()
        write_index_digest(index)
        other_release = await self.service.capabilities()
        self.assertNotIn(
            "gene_evidence",
            {item["intent"] for item in other_release["capabilities"]},
        )

        with closing(sqlite3.connect(index)) as db:
            db.execute(
                "UPDATE analysis_catalog SET release=? WHERE analysis_id='analysis-1'",
                (self.settings.release,),
            )
            db.commit()
        write_index_digest(index)
        shown = await self.service.capabilities()
        self.assertEqual(shown["capabilities"], [payload])

    async def test_empty_indexed_capability_catalog_does_not_restore_static_capabilities(self):
        index = self.root / "depmap-26q1-query-index.sqlite"
        with closing(sqlite3.connect(index)) as db:
            db.execute(
                "CREATE TABLE capability_catalog "
                "(intent TEXT,mcp_tool TEXT,payload_json TEXT)"
            )
            db.commit()
        write_index_digest(index)

        result = await self.service.capabilities()

        self.assertEqual(result["catalog_source"], "sqlite_capability_catalog")
        self.assertEqual(result["catalog_status"], "FOUND")
        self.assertEqual(result["capabilities"], [])

    async def test_capabilities_use_previous_pair_while_index_publish_is_in_progress(self):
        index = self.root / "depmap-26q1-query-index.sqlite"

        def capability(name: str) -> dict:
            return {
                "intent": name,
                "description": f"Describe {name}.",
                "required": [],
                "optional": [],
                "examples_zh": [name],
                "precise_prompt_template_zh": name,
                "confusable_with": [],
                "mcp_tool": "depmap_status",
            }

        def create_catalog(path: Path, name: str) -> None:
            with closing(sqlite3.connect(path)) as db:
                db.execute(
                    "CREATE TABLE capability_catalog "
                    "(intent TEXT,mcp_tool TEXT,payload_json TEXT)"
                )
                db.execute(
                    "INSERT INTO capability_catalog VALUES (?,?,?)",
                    (name, "depmap_status", json.dumps(capability(name))),
                )
                db.commit()
            write_index_digest(path)

        create_catalog(index, "old_catalog")
        old_digest = index_digest_path(index).read_text(encoding="ascii").strip()
        previous = index.with_name(f"{index.name}.previous-{old_digest}")
        shutil.copyfile(index, previous)
        write_index_digest(previous, old_digest)
        replacement = self.root / "replacement.sqlite"
        create_catalog(replacement, "new_catalog")
        index_publish_marker_path(index).write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "previous_name": previous.name,
                    "previous_sha256": old_digest,
                }
            ),
            encoding="utf-8",
        )
        shutil.copyfile(replacement, index)

        during_database_switch = await self.service.capabilities()
        self.assertEqual(
            during_database_switch["capabilities"][0]["intent"], "old_catalog"
        )
        shutil.copyfile(index_digest_path(replacement), index_digest_path(index))
        during_digest_switch = await self.service.capabilities()
        self.assertEqual(
            during_digest_switch["capabilities"][0]["intent"], "old_catalog"
        )
        index_publish_marker_path(index).unlink()
        after_publish = await self.service.capabilities()
        self.assertEqual(after_publish["capabilities"][0]["intent"], "new_catalog")

    async def test_unreadable_indexed_capability_catalog_fails_closed(self):
        index = self.root / "depmap-26q1-query-index.sqlite"
        index.write_bytes(b"not sqlite")

        with self.assertLogs("depmap_mcp", level="ERROR"):
            result = await self.service.capabilities()

        self.assertEqual(result["catalog_source"], "sqlite_capability_catalog")
        self.assertEqual(result["catalog_status"], "UNAVAILABLE")
        self.assertEqual(result["invalid_record_count"], 0)
        self.assertEqual(result["capabilities"], [])

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
        write_index_digest(index)

        result = await self.service.data_coverage(lineage="Liver", limit=1000)
        self.assertEqual(result["evidence"]["status"], "FOUND")
        self.assertEqual(result["evidence"]["returned_count"], 1)
        self.assertEqual(result["evidence"]["rows"][0]["model_count"], 25)
        serialized = json.dumps(result)
        self.assertNotIn("private_path", serialized)
        self.assertNotIn("/srv/secret", serialized)

    async def test_data_coverage_failure_uses_stable_path_free_integrity_reason(self):
        index = self.root / "depmap-26q1-query-index.sqlite"
        index.write_bytes(b"not sqlite")

        with self.assertLogs("depmap_mcp", level="ERROR"):
            result = await self.service.data_coverage()

        evidence = result["evidence"]
        self.assertEqual(evidence["status"], "MODULE_UNAVAILABLE")
        self.assertEqual(
            evidence["reason_code"], "INTEGRITY_CATALOG_UNAVAILABLE"
        )
        serialized = json.dumps(result)
        self.assertNotIn("DatabaseError", serialized)
        self.assertNotIn(str(self.root), serialized)

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
        self.assertFalse(result["presentation_contract"]["artifact_requested"])
        self.assertEqual(
            result["presentation_contract"]["disclosure"]["default"],
            ["status_sentence", "bounded_top_rows", "filter_truncation_flags"],
        )
        self.assertEqual(
            result["presentation_contract"]["disclosure"]["expanded"],
            ["manifest", "evidence_id", "provenance"],
        )
        self.assertTrue(
            result["presentation_contract"]["literature_is_separate_evidence_class"]
        )

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

    async def test_module_unavailable_counts_as_aggregate_query_failure(self):
        async def failing_runner(_settings, _query):
            raise RuntimeError("synthetic reader failure")

        service = DepMapEvidenceService(self.settings, failing_runner)
        with self.assertLogs("depmap_mcp", level="ERROR"):
            gene = await service.gene_evidence(
                "GENERIC", sections=["core"], limit=3
            )
            pair = await service.pair_evidence("GENERIC_A", "GENERIC_B")
            codependency = await service.codependency_evidence("GENERIC", limit=3)
            drug = await service.drug_evidence("generic-drug", "GENERIC", limit=3)

        self.assertEqual(gene["evidence"]["query_error_count"], 1)
        self.assertFalse(gene["evidence"]["complete"])
        for result in (pair, codependency, drug):
            self.assertEqual(
                result["evidence"]["query_error_count"],
                result["evidence"]["query_count"],
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

    async def test_projection_keeps_an_under_budget_fifty_row_page_intact(self):
        for lineage in ("Kidney", "Liver", "Breast", "Bowel", "Lung"):
            with self.subTest(lineage=lineage):
                rows = [
                    {
                        "rank": index + 1,
                        "symbol": f"GENE{index:02d}",
                        "detail": lineage + "x" * 700,
                    }
                    for index in range(50)
                ]

                evidence, projection = _bounded_model_projection(
                    {
                        "status": "FOUND",
                        "lineage": lineage,
                        "rows": rows,
                        "returned_count": 50,
                        "matched_row_count": 73,
                    }
                )

                self.assertFalse(projection["is_bounded_projection"])
                self.assertEqual(len(evidence["rows"]), 50)
                self.assertEqual(evidence["returned_count"], 50)
                self.assertEqual(evidence["matched_row_count"], 73)
                self.assertNotIn("returned_count_before_projection", evidence)

    async def test_projection_shrinks_auxiliary_lists_before_scientific_rows(self):
        rows = [
            {"rank": index + 1, "symbol": f"GENE{index:02d}", "detail": "x" * 700}
            for index in range(50)
        ]

        evidence, projection = _bounded_model_projection(
            {
                "status": "FOUND",
                "rows": rows,
                "returned_count": 50,
                "matched_row_count": 81,
                "provenance": [
                    f"depmap://26Q1/source/{index}/" + "p" * 5000
                    for index in range(40)
                ],
            }
        )

        self.assertLessEqual(
            projection["projected_bytes"], MAX_MODEL_EVIDENCE_BYTES
        )
        self.assertEqual(len(evidence["rows"]), 50)
        self.assertEqual(evidence["returned_count"], 50)
        self.assertEqual(evidence["matched_row_count"], 81)
        self.assertLess(len(evidence["provenance"]), 40)

    async def test_projection_uses_one_scientific_page_width_across_lineages(self):
        labels = ["Kidney", "Liver", "Breast", "Bowel", "Lung"]
        lineages = []
        for lineage_index in range(30):
            rows = [
                {
                    "rank": row_index + 1,
                    "symbol": f"GENE{row_index:02d}",
                    "detail": labels[lineage_index % len(labels)] + "x" * 650,
                }
                for row_index in range(10)
            ]
            lineages.append(
                {
                    "lineage": f"{labels[lineage_index % len(labels)]}-{lineage_index:02d}",
                    "rows": rows,
                    "returned_count": 10,
                    "matched_row_count": 25 + lineage_index,
                }
            )

        evidence, _projection = _bounded_model_projection(
            {
                "status": "FOUND",
                "lineages": lineages,
                "lineage_count": 30,
            }
        )
        widths = {len(section["rows"]) for section in evidence["lineages"]}

        self.assertEqual(len(evidence["lineages"]), 30)
        self.assertEqual(evidence["lineage_count"], 30)
        self.assertNotIn("lineage_count_before_projection", evidence)
        self.assertEqual(len(widths), 1)
        retained_width = widths.pop()
        self.assertGreater(retained_width, 0)
        self.assertLess(retained_width, 10)
        for index, section in enumerate(evidence["lineages"]):
            self.assertEqual(section["returned_count"], retained_width)
            self.assertEqual(section["returned_count_before_projection"], 10)
            self.assertEqual(section["matched_row_count"], 25 + index)

    async def test_projection_drops_lineages_only_after_pages_reach_one_row(self):
        lineages = [
            {
                "lineage": f"Lineage-{index:02d}",
                "rows": [
                    {
                        "rank": 1,
                        "symbol": f"GENE{index:02d}",
                        "detail": "x" * 6000,
                    }
                ],
                "returned_count": 1,
                "matched_row_count": 20,
            }
            for index in range(30)
        ]

        evidence, _projection = _bounded_model_projection(
            {
                "status": "FOUND",
                "lineages": lineages,
                "lineage_count": 30,
            }
        )

        self.assertGreater(len(evidence["lineages"]), 0)
        self.assertLess(len(evidence["lineages"]), 30)
        self.assertTrue(
            all(len(section["rows"]) == 1 for section in evidence["lineages"])
        )
        self.assertEqual(evidence["lineage_count"], len(evidence["lineages"]))
        self.assertEqual(evidence["lineage_count_before_projection"], 30)

    async def test_projection_keeps_the_longest_fitting_prefix_not_a_fixed_forty(self):
        rows = [
            {"rank": index + 1, "symbol": f"GENE{index:03d}", "detail": "x" * 1100}
            for index in range(100)
        ]

        evidence, _projection = _bounded_model_projection(
            {
                "status": "FOUND",
                "rows": rows,
                "returned_count": 100,
                "matched_row_count": 144,
            }
        )
        retained_count = len(evidence["rows"])

        self.assertGreater(retained_count, 40)
        self.assertLess(retained_count, 100)
        self.assertEqual(evidence["returned_count"], retained_count)
        self.assertEqual(evidence["returned_count_before_projection"], 100)
        self.assertEqual(evidence["matched_row_count"], 144)

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
        self.assertNotIn("provenance", result["evidence"]["result"])

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
            "chronos_gene_effect_lineage_vs_rest_mean_difference",
        )
        semantics = result["evidence"]["metric_semantics"]
        self.assertEqual(semantics["units"], "Chronos Gene Effect score difference")
        self.assertEqual(
            semantics["direction"],
            "more_negative_is_stronger_lineage_dependency",
        )
        self.assertEqual(semantics["descriptive_cutoff"], "not_computed")
        self.assertEqual(
            semantics["common_essential_role"],
            "independent_sidecar_annotation",
        )
        self.assertIn("not logFC", semantics["interpretation"])
        self.assertIn("dependency probability", semantics["interpretation"])
        self.assertFalse(result["new_analysis_started"])

        descriptive = await self.service.lineage_dependencies(
            "乳腺癌", "mean_dependency", 10
        )
        descriptive_semantics = descriptive["evidence"]["metric_semantics"]
        self.assertEqual(
            descriptive_semantics["metric"],
            "chronos_gene_effect_lineage_mean",
        )
        self.assertEqual(
            descriptive_semantics["selection_policy"],
            "descriptive_ordering_only",
        )
        self.assertIn("not a dependency probability", descriptive_semantics["interpretation"])

        filtered = await self.service.lineage_dependencies(
            "乳腺癌", "selective", 10, exclude_common_essential=True
        )
        self.assertTrue(self.queries[-1]["exclude_common_essential"])
        self.assertEqual(self.queries[-1]["common_essential_source"], "depmap_26q1")

        paged = await self.service.lineage_dependencies(
            "乳腺癌", "selective", 10, cursor=20
        )
        self.assertEqual(self.queries[-1]["cursor"], 20)
        self.assertEqual(paged["request"]["cursor"], 20)

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

    async def test_model_gene_effect_normalizes_modelid_and_declares_threshold(self):
        result = await self.service.model_gene_effect(
            "gpx4", "髓系", "ach-000001", -0.5, 10
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "model_gene_effect",
                "gene": "GPX4",
                "lineage": "Myeloid",
                "model_id": "ACH-000001",
                "gene_effect_at_or_below": -0.5,
                "limit": 10,
            },
        )
        self.assertEqual(result["request"]["model_id"], "ACH-000001")
        semantics = result["evidence"]["metric_semantics"]
        self.assertEqual(semantics["metric"], "chronos_gene_effect_model_score")
        self.assertEqual(semantics["entity_key"], "canonical_model_id")

    async def test_cross_platform_validation_keeps_scope_and_metrics_distinct(self):
        result = await self.service.cross_platform_validation(
            "gpx4", "lineage", "髓系"
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "cross_platform_validation",
                "gene": "GPX4",
                "scope": "lineage",
                "lineage": "Myeloid",
            },
        )
        semantics = result["evidence"]["metric_semantics"]
        self.assertEqual(semantics["metric"], "platform_specific_gene_level_correlation")
        self.assertIn("DEMETER2 RNAi", semantics["comparisons"][1])

    async def test_pan_cancer_dependency_forwards_exact_gene(self):
        result = await self.service.pan_cancer_dependencies("selective", 5, gene="fbxo7")
        self.assertEqual(self.queries[-1]["gene"], "FBXO7")
        self.assertEqual(result["request"]["gene"], "FBXO7")

    async def test_pan_cancer_limit_rejection_preserves_normalized_request(self):
        filtered = await self.service.pan_cancer_dependencies(
            "selective", 25, exclude_common_essential=True, gene="fbxo7"
        )
        unfiltered = await self.service.pan_cancer_dependencies(
            "selective", 25, exclude_common_essential=False, gene="mdm2"
        )

        self.assertEqual(
            filtered["request"],
            {
                "mode": "pan_cancer_dependency",
                "ranking": "selective",
                "exclude_common_essential": True,
                "common_essential_source": "depmap_26q1",
                "limit": 25,
                "gene": "FBXO7",
            },
        )
        self.assertTrue(filtered["evidence"]["schema_error"])
        self.assertNotEqual(filtered["evidence_id"], unfiltered["evidence_id"])
        self.assertEqual(self.queries, [])

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
            "chronos_gene_effect_lineage_vs_rest_mean_difference",
        )

        paged = await self.service.pan_cancer_dependencies(
            "selective", 5, exclude_common_essential=True, cursor=5
        )
        self.assertEqual(self.queries[-1]["cursor"], 5)
        self.assertEqual(paged["request"]["cursor"], 5)

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
        result = await self.service.codependency_evidence(
            "kras", lineage="Lung", direction="positive", limit=7
        )
        self.assertEqual(
            self.queries,
            [
                {"mode": "true_love", "catalog": "positive_reciprocal_top20", "gene": "KRAS", "scope": "pancancer", "limit": 7, "coverage": "quality"},
                {"mode": "lineage_network", "family": "effect_correlation", "lineage": "Lung", "source": "KRAS", "reciprocal": True, "direction": "positive", "limit": 7},
            ],
        )
        self.assertEqual([section["scope"] for section in result["evidence"]["sections"]], ["global", "lineage"])
        self.assertIn("not synthetic lethality", result["evidence"]["interpretation"])
        self.queries.clear()

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

    async def test_first_class_lineage_mutation_dependency_tool_preserves_exact_filters(self):
        result = await self.service.lineage_mutation_dependency_evidence(
            "肝癌", "tp53", "gpx4", "damaging", 7
        )
        self.assertEqual(
            self.queries[-1],
            {
                "mode": "lineage_mutation_dependency",
                "lineage": "Liver",
                "source": "TP53",
                "target": "GPX4",
                "event": "damaging",
                "limit": 7,
            },
        )
        self.assertNotIn("tool", result)
        self.assertEqual(result["request"]["source"], "TP53")
        self.assertEqual(result["request"]["target"], "GPX4")
        self.assertEqual(
            result["request"]["provider"], "lineage_official_gene_effect_v2"
        )
        self.assertIn(
            "not causal synthetic lethality",
            result["evidence"]["metric_semantics"]["interpretation"],
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
                        "depmap_model_gene_effect",
                        "depmap_cross_platform_validation",
                        "depmap_pan_cancer_dependencies",
                        "depmap_lineage_direction_discovery",
                        "depmap_gene_evidence",
                        "tcga_gene_expression_survival",
                        "depmap_pair_evidence",
                        "depmap_codependency_evidence",
                        "depmap_tf_dependency_evidence",
                        "depmap_biomarker_model_evidence",
                        "depmap_drug_evidence",
                        "depmap_subtype_evidence",
                        "depmap_coamplification_evidence",
                        "depmap_true_love_evidence",
                        "depmap_synthetic_lethal_evidence",
                        "depmap_lineage_mutation_dependency",
                        "depmap_3d_evidence",
                    },
                )
                self.assertTrue(
                    all(tool.annotations.readOnlyHint for tool in tools.tools)
                )
                schemas = {tool.name: tool.inputSchema for tool in tools.tools}
                capability_result = await session.call_tool("depmap_capabilities", {})
                self.assertFalse(capability_result.isError)
                for capability in capability_result.structuredContent["capabilities"]:
                    tool_name = capability["mcp_tool"]
                    self.assertIn(tool_name, schemas, capability["intent"])
                    schema = schemas[tool_name]
                    representative = {}
                    for field in schema.get("required", []):
                        field_schema = schema["properties"][field]
                        if field_schema.get("enum"):
                            representative[field] = field_schema["enum"][0]
                        elif field_schema.get("type") == "integer":
                            representative[field] = field_schema.get("minimum", 1)
                        elif field_schema.get("type") == "number":
                            representative[field] = field_schema.get("minimum", 1.0)
                        elif field_schema.get("type") == "boolean":
                            representative[field] = False
                        elif field_schema.get("type") == "array":
                            representative[field] = ["fixture"] * field_schema.get(
                                "minItems", 0
                            )
                        else:
                            representative[field] = "fixture"
                    self.assertFalse(
                        any(value is None for value in representative.values()),
                        capability["intent"],
                    )
                    self.assertTrue(
                        set(schema.get("required", [])).issubset(representative),
                        capability["intent"],
                    )
                    self.assertTrue(
                        set(representative).issubset(schema.get("properties", {})),
                        capability["intent"],
                    )
                    validator = validator_for(schema)
                    validator.check_schema(schema)
                    validator(schema).validate(representative)
                called = await session.call_tool("depmap_status", {})
                self.assertFalse(called.isError)
                self.assertEqual(called.structuredContent["release"], "26Q1")
                self.assertFalse(
                    called.structuredContent["evidence"]["data_sources"]["tcga"][
                        "installed"
                    ]
                )
                invalid = await session.call_tool(
                    "depmap_lineage_dependencies",
                    {"lineage": "Breast", "limit": 494},
                )
                self.assertFalse(invalid.isError)
                self.assertTrue(
                    invalid.structuredContent["evidence"]["schema_error"]
                )
                self.assertEqual(
                    invalid.structuredContent["evidence"]["limit_max"], 100
                )


if __name__ == "__main__":
    unittest.main()
