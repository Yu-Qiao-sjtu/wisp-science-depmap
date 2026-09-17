import json
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

from scripts.build_depmap_query_index import build, is_fresh


class QueryIndexTests(unittest.TestCase):
    def test_v3_catalog_relates_assets_and_detects_fresh_index(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = root / "analysis-modules" / "fixture" / "results" / "matrix"
            (unit / "blocks").mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "gene_order.csv").write_text("gene\nESR1\n", encoding="utf-8")
            (unit / "blocks" / "block_00001_00001.rds").write_bytes(b"fixture")
            (root / "public.csv").write_text("key,value\na,1\n", encoding="utf-8")
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
                    "analysis-modules/fixture/results/matrix/blocks/block_00001_00001.rds",
                )


if __name__ == "__main__":
    unittest.main()
