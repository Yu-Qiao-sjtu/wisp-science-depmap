import json
import asyncio
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

from scripts.build_depmap_query_index import build, is_fresh
from services.depmap_mcp.catalog_readers import CatalogReaderRegistry
from services.depmap_api.app import Settings, _run_analysis_catalog_query


class QueryIndexTests(unittest.TestCase):
    def test_v3_catalog_relates_assets_and_detects_fresh_index(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = root / "analysis-modules" / "CRISPR基因-基因共依赖分析" / "results" / "matrix"
            (unit / "blocks").mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "gene_order.csv").write_text("gene_index,symbol\n1,ESR1\n", encoding="utf-8")
            (unit / "blocks" / "block_00001_00001.rds").write_bytes(b"fixture")
            (root / "public.csv").write_text("key,value\na,1\n", encoding="utf-8")
            for family in ("subtype_dependency", "coamplification_dependency"):
                family_root = root / "depmap-26q1-full" / family
                family_root.mkdir(parents=True)
                (family_root / "manifest.json").write_text(
                    json.dumps({"status": "complete", "release": "26Q1"}),
                    encoding="utf-8",
                )
            output = root / "depmap-26q1-query-index.sqlite"

            counts = build(root, output)

            self.assertEqual(counts["capabilities"], 19)
            self.assertEqual(counts["matrix_gene_blocks"], 1)
            self.assertTrue(is_fresh(root, output))
            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute("SELECT COUNT(*) FROM artifact_catalog WHERE analysis_id IS NULL").fetchone()[0],
                    0,
                )
                self.assertEqual(
                    db.execute("SELECT COUNT(*) FROM analysis_catalog WHERE completion_state='UNVERIFIED'").fetchone()[0],
                    0,
                )
                self.assertEqual(
                    db.execute("SELECT block_path FROM matrix_block_index WHERE gene='ESR1'").fetchone()[0],
                    "analysis-modules/CRISPR基因-基因共依赖分析/results/matrix/blocks/block_00001_00001.rds",
                )
            resolution = CatalogReaderRegistry(root, "26Q1").resolve(
                {"mode": "pair", "source": "ESR1", "target": "FOXA1"}
            )
            self.assertEqual(resolution.state, "RESOLVED")
            self.assertEqual(resolution.reader_id, "pair_adapter")
            self.assertIn(
                "analysis-modules/CRISPR基因-基因共依赖分析/results/matrix/blocks/block_00001_00001.rds",
                resolution.artifact_uris,
            )
            self.assertIn(
                "analysis-modules/CRISPR基因-基因共依赖分析/results/matrix/blocks/block_00001_00001.rds",
                resolution.matrix_blocks,
            )
            seen = {}

            async def runner(_settings, query):
                seen.update(query)
                return {
                    "status": "FOUND",
                    "provenance": str(
                        unit / "blocks" / "block_00001_00001.rds"
                    ),
                }

            bound, result = asyncio.run(
                CatalogReaderRegistry(root, "26Q1").read(
                    object(),
                    {"mode": "pair", "source": "ESR1", "target": "FOXA1"},
                    runner,
                )
            )
            self.assertEqual(result["status"], "FOUND")
            self.assertEqual(bound.validated_provenance_count, 1)
            self.assertEqual(bound.artifact_uris, (
                "analysis-modules/CRISPR基因-基因共依赖分析/results/matrix/blocks/block_00001_00001.rds",
            ))
            self.assertEqual(seen["_catalog_reader_id"], "pair_adapter")
            self.assertEqual(len(seen["_catalog_matrix_blocks"]), 1)

            settings = Settings(
                knowledge_root=root,
                query_script=root / "query_depmap_kb.R",
                api_token="test-token",
            )
            exact = _run_analysis_catalog_query(
                settings,
                {"mode": "analysis_catalog", "module": "CRISPR基因-基因共依赖分析"},
            )
            alias = _run_analysis_catalog_query(
                settings,
                {"mode": "analysis_catalog", "module": "true_love"},
            )
            unmatched = _run_analysis_catalog_query(
                settings,
                {"mode": "analysis_catalog", "module": "not-a-real-module"},
            )
            subtype = _run_analysis_catalog_query(
                settings,
                {"mode": "analysis_catalog", "module": "subtype"},
            )
            self.assertEqual(exact["status"], "FOUND")
            self.assertEqual(alias["status"], "FOUND")
            self.assertEqual(unmatched["status"], "NOT_RETAINED")
            self.assertEqual(unmatched["rows"], [])
            self.assertEqual(subtype["returned_count"], 1)
            self.assertIn("subtype_dependency", subtype["rows"][0]["analysis_unit"])


if __name__ == "__main__":
    unittest.main()
