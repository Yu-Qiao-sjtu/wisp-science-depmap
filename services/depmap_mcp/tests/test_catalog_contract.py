import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

from mcp.server.fastmcp import FastMCP

from services.depmap_api.scientific_query import CatalogArtifactError
from services.depmap_mcp.artifact_integrity import write_index_digest
from services.depmap_mcp.catalog_readers import CatalogReaderRegistry
from services.depmap_mcp.server import _withhold_unregistered_catalog_tools


def _verified_index(root: Path, *, modes: tuple[str, ...]) -> None:
    index = root / "depmap-26q1-query-index.sqlite"
    with closing(sqlite3.connect(index)) as db:
        db.executescript(
            """
            CREATE TABLE reader_registry (
              query_mode TEXT, adapter TEXT, module_pattern TEXT
            );
            CREATE TABLE reader_coverage (
              query_mode TEXT, analysis_id TEXT, coverage_state TEXT
            );
            CREATE TABLE analysis_catalog (
              analysis_id TEXT, release TEXT, completion_state TEXT,
              module TEXT, analysis_unit TEXT, manifest_mtime_ns INTEGER
            );
            CREATE TABLE matrix_block_index (
              block_path TEXT, gene TEXT, analysis_id TEXT
            );
            CREATE TABLE artifact_catalog (
              artifact_path TEXT, analysis_id TEXT, artifact_kind TEXT,
              integrity_method TEXT, integrity_value TEXT, integrity_state TEXT
            );
            """
        )
        db.execute(
            "INSERT INTO analysis_catalog VALUES "
            "('analysis-1','26Q1','COMPLETE','coremod','coremod',1)"
        )
        for mode in modes:
            db.execute(
                "INSERT INTO reader_registry VALUES (?,?,?)",
                (mode, f"{mode}_adapter", "coremod"),
            )
            db.execute(
                "INSERT INTO reader_coverage VALUES (?,?,?)",
                (mode, "analysis-1", "AVAILABLE"),
            )
        db.commit()
    write_index_digest(index)


class CatalogContractTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()

    def tearDown(self):
        self.temp.cleanup()

    async def test_unbound_provenance_is_not_a_resolved_catalog(self):
        _verified_index(self.root, modes=("model_gene_effect",))
        registry = CatalogReaderRegistry(self.root, "26Q1")

        async def runner(_settings, _query):
            return {"provenance": "depmap://26Q1/missing.csv"}

        resolution, result = await registry.read(
            None, {"mode": "model_gene_effect", "gene": "GENE1"}, runner
        )
        self.assertEqual(resolution.state, "CATALOG_INCOMPLETE")
        self.assertNotEqual(resolution.state, "RESOLVED")
        self.assertEqual(resolution.artifact_uris, ("missing.csv",))
        self.assertEqual(resolution.validated_provenance_count, 0)
        self.assertEqual(result["reason_code"], "PROVENANCE_NOT_CATALOGED")
        self.assertEqual(result["status"], "MODULE_UNAVAILABLE")

    async def test_truncated_compressed_artifact_names_its_uri(self):
        _verified_index(self.root, modes=("three_d",))
        relative = "depmap-26q1-3d/lineage_dependency_enrichment/unit/significant_enrichment.csv.gz"
        path = self.root / relative
        path.parent.mkdir(parents=True)
        path.write_bytes(b"not a gzip member")
        registry = CatalogReaderRegistry(self.root, "26Q1")

        async def runner(_settings, _query):
            from services.depmap_api.app import _iter_csv_records

            list(_iter_csv_records(path))
            return {"rows": []}

        resolution, result = await registry.read(
            None, {"mode": "three_d", "family": "lineage_dependency_enrichment"}, runner
        )
        self.assertEqual(resolution.state, "CATALOG_INCOMPLETE")
        self.assertEqual(result["status"], "QUERY_ERROR")
        self.assertEqual(result["reason_code"], "TRUNCATED_COMPRESSED_ARTIFACT")
        self.assertEqual(result["artifact_uris"], [f"depmap://26Q1/{relative}"])
        self.assertEqual(resolution.artifact_uris, (relative,))

    def test_direct_truncated_open_keeps_the_path(self):
        path = self.root / "broken.csv.gz"
        path.write_bytes(b"not gzip")
        from services.depmap_api.app import _iter_csv_records

        with self.assertRaises(CatalogArtifactError) as raised:
            list(_iter_csv_records(path))
        self.assertEqual(raised.exception.path, path)

    def test_unregistered_query_mode_is_absent_from_the_tool_schema(self):
        _verified_index(self.root, modes=("lineage_dependency",))
        registry = CatalogReaderRegistry(self.root, "26Q1")
        mcp = FastMCP("catalog-contract")

        @mcp.tool()
        async def depmap_pan_cancer_dependencies() -> dict:
            return {}

        @mcp.tool()
        async def depmap_lineage_dependencies() -> dict:
            return {}

        _withhold_unregistered_catalog_tools(mcp, registry)
        self.assertIsNone(mcp._tool_manager.get_tool("depmap_pan_cancer_dependencies"))
        self.assertIsNotNone(mcp._tool_manager.get_tool("depmap_lineage_dependencies"))


if __name__ == "__main__":
    unittest.main()
