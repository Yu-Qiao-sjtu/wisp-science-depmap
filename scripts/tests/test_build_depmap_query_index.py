import json
import asyncio
import gzip
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

from scripts.build_depmap_query_index import build, is_fresh
from services.depmap_mcp.catalog_readers import CatalogReaderRegistry
from services.depmap_api.app import Settings, _run_analysis_catalog_query


class QueryIndexTests(unittest.TestCase):
    def test_corrupt_artifact_is_quarantined_without_disabling_healthy_family(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            corrupt_unit = (
                root
                / "analysis-modules"
                / "通用富集分析"
                / "results"
                / "corrupt"
            )
            healthy_unit = (
                root
                / "analysis-modules"
                / "通用共依赖分析"
                / "results"
                / "healthy"
            )
            corrupt_unit.mkdir(parents=True)
            healthy_unit.mkdir(parents=True)
            for unit in (corrupt_unit, healthy_unit):
                (unit / "manifest.json").write_text(
                    json.dumps({"status": "complete", "release": "26Q1"}),
                    encoding="utf-8",
                )
            compressed = gzip.compress(b"label,value\nA,1\n")
            (corrupt_unit / "rows.csv.gz").write_bytes(compressed[:-4])
            (healthy_unit / "rows.csv.gz").write_bytes(compressed)
            output = root / "depmap-26q1-query-index.sqlite"

            counts = build(root, output)

            self.assertEqual(counts["quarantined_artifacts"], 1)
            with closing(sqlite3.connect(output)) as db:
                corrupt = db.execute(
                    "SELECT integrity_state,integrity_reason_code FROM artifact_catalog "
                    "WHERE artifact_path LIKE '%/corrupt/rows.csv.gz'"
                ).fetchone()
                self.assertEqual(
                    corrupt,
                    ("QUARANTINED", "TRUNCATED_COMPRESSED_ARTIFACT"),
                )
                self.assertEqual(
                    db.execute(
                        "SELECT completion_state FROM analysis_catalog "
                        "WHERE analysis_unit LIKE '%/corrupt'"
                    ).fetchone()[0],
                    "QUARANTINED",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT completion_state FROM analysis_catalog "
                        "WHERE analysis_unit LIKE '%/healthy'"
                    ).fetchone()[0],
                    "COMPLETE",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT storage_completeness,qa_state FROM coverage_registry "
                        "WHERE analysis_id=(SELECT analysis_id FROM analysis_catalog "
                        "WHERE analysis_unit LIKE '%/corrupt')"
                    ).fetchone(),
                    ("corrupt", "CORRUPT_ARTIFACT"),
                )
                self.assertEqual(
                    db.execute(
                        "SELECT coverage_state FROM reader_coverage "
                        "WHERE query_mode='enrichment'"
                    ).fetchone()[0],
                    "CORRUPT_ARTIFACT",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog "
                        "WHERE query_mode='enrichment'"
                    ).fetchone()[0],
                    0,
                )
                self.assertEqual(
                    db.execute(
                        "SELECT coverage_state FROM reader_coverage "
                        "WHERE query_mode='pair'"
                    ).fetchone()[0],
                    "AVAILABLE",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog "
                        "WHERE query_mode='pair'"
                    ).fetchone()[0],
                    1,
                )

            registry = CatalogReaderRegistry(root, "26Q1")
            corrupt_resolution = registry.resolve(
                {"mode": "enrichment", "source": "FEATURE", "lineage": "Lineage"}
            )
            healthy_resolution = registry.resolve(
                {"mode": "pair", "source": "FEATURE_A", "target": "FEATURE_B"}
            )
            self.assertEqual(corrupt_resolution.state, "CORRUPT_ARTIFACT")
            self.assertEqual(healthy_resolution.state, "RESOLVED")
            called = False

            async def runner(_settings, _query):
                nonlocal called
                called = True
                return {"status": "FOUND"}

            _resolution, result = asyncio.run(
                registry.read(
                    object(),
                    {
                        "mode": "enrichment",
                        "source": "FEATURE",
                        "lineage": "Lineage",
                    },
                    runner,
                )
            )
            self.assertFalse(called)
            self.assertEqual(result["status"], "MODULE_UNAVAILABLE")
            self.assertEqual(result["reason_code"], "CORRUPT_ARTIFACT")

            with closing(sqlite3.connect(output)) as db:
                first_identity = db.execute(
                    "SELECT value FROM metadata WHERE key='built_at'"
                ).fetchone()[0]
            (corrupt_unit / "rows.csv.gz").write_bytes(compressed)
            self.assertEqual(
                registry.resolve(
                    {
                        "mode": "enrichment",
                        "source": "FEATURE",
                        "lineage": "Lineage",
                    }
                ).state,
                "CORRUPT_ARTIFACT",
            )

            build(root, output)

            rebuilt = CatalogReaderRegistry(root, "26Q1").resolve(
                {"mode": "enrichment", "source": "FEATURE", "lineage": "Lineage"}
            )
            self.assertEqual(rebuilt.state, "RESOLVED")
            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT integrity_state FROM artifact_catalog "
                        "WHERE artifact_path LIKE '%/corrupt/rows.csv.gz'"
                    ).fetchone()[0],
                    "VERIFIED",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog "
                        "WHERE query_mode='enrichment'"
                    ).fetchone()[0],
                    1,
                )
                self.assertNotEqual(
                    db.execute(
                        "SELECT value FROM metadata WHERE key='built_at'"
                    ).fetchone()[0],
                    first_identity,
                )

    def test_duplicate_table_headers_fail_minimum_schema_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = root / "analysis-modules" / "通用富集分析" / "results" / "unit"
            unit.mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "rows.csv").write_text(
                "label, Label \nA,B\n", encoding="utf-8"
            )
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT integrity_state,integrity_reason_code "
                        "FROM artifact_catalog WHERE artifact_path LIKE '%/rows.csv'"
                    ).fetchone(),
                    ("QUARANTINED", "INVALID_TABLE_SCHEMA"),
                )

    def test_zero_column_table_header_fails_minimum_schema_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = root / "analysis-modules" / "generic" / "results" / "unit"
            unit.mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "rows.csv").write_text("\n", encoding="utf-8")
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT integrity_state,integrity_reason_code "
                        "FROM artifact_catalog WHERE artifact_path LIKE '%/rows.csv'"
                    ).fetchone(),
                    ("QUARANTINED", "INVALID_TABLE_SCHEMA"),
                )

    def test_declared_checksum_mismatch_is_quarantined_before_advertisement(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = root / "analysis-modules" / "通用富集分析" / "results" / "unit"
            unit.mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps(
                    {
                        "status": "complete",
                        "release": "26Q1",
                        "artifact_checksums": {"rows.csv.gz": "0" * 64},
                    }
                ),
                encoding="utf-8",
            )
            (unit / "rows.csv.gz").write_bytes(
                gzip.compress(b"label,value\nA,1\n")
            )
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT integrity_state,integrity_reason_code "
                        "FROM artifact_catalog WHERE artifact_path LIKE '%/rows.csv.gz'"
                    ).fetchone(),
                    ("QUARANTINED", "CHECKSUM_MISMATCH"),
                )
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog "
                        "WHERE query_mode='enrichment'"
                    ).fetchone()[0],
                    0,
                )

    def test_root_owned_corrupt_artifact_disables_matching_reader_family(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            corrupt = root / "analysis-modules" / "通用富集" / "rows.csv.gz"
            corrupt.parent.mkdir(parents=True)
            compressed = gzip.compress(b"label,value\nA,1\n")
            corrupt.write_bytes(compressed[:-4])
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT coverage_state FROM reader_coverage "
                        "WHERE query_mode='enrichment'"
                    ).fetchone()[0],
                    "CORRUPT_ARTIFACT",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog "
                        "WHERE query_mode='enrichment'"
                    ).fetchone()[0],
                    0,
                )

    def test_root_owned_core_corruption_matches_descendant_artifact_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            corrupt = root / "depmap-26q1-core" / "results" / "broken.parquet"
            corrupt.parent.mkdir(parents=True)
            corrupt.write_bytes(b"PAR1PAR1")
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT integrity_state,integrity_reason_code "
                        "FROM artifact_catalog WHERE artifact_path=?",
                        ("depmap-26q1-core/results/broken.parquet",),
                    ).fetchone(),
                    ("QUARANTINED", "INVALID_PARQUET"),
                )
                self.assertEqual(
                    db.execute(
                        "SELECT coverage_state FROM reader_coverage "
                        "WHERE query_mode='core'"
                    ).fetchone()[0],
                    "CORRUPT_ARTIFACT",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog "
                        "WHERE query_mode='model_gene_effect'"
                    ).fetchone()[0],
                    0,
                )
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog "
                        "WHERE query_mode='drug'"
                    ).fetchone()[0],
                    1,
                )

    def test_concrete_lineage_readers_track_their_full_artifact_roots(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            healthy = root / "depmap-26q1-core"
            healthy.mkdir(parents=True)
            (healthy / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (healthy / "rows.csv").write_text(
                "label,value\nA,1\n", encoding="utf-8"
            )
            roots = {
                "lineage_network": "lineage_sparse_networks",
                "lineage_cnv": "lineage_cnv_amplification_dependency",
                "lineage_drug": "lineage_prism_associations",
            }
            compressed = gzip.compress(b"label,value\nA,1\n")
            for directory in roots.values():
                path = root / "depmap-26q1-full" / directory / "broken.csv.gz"
                path.parent.mkdir(parents=True)
                path.write_bytes(compressed[:-4])
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                coverage = dict(
                    db.execute(
                        "SELECT query_mode,coverage_state FROM reader_coverage "
                        "WHERE query_mode IN ('lineage_network','lineage_cnv','lineage_drug','core')"
                    )
                )
            self.assertEqual(coverage["core"], "AVAILABLE")
            for mode in roots:
                self.assertEqual(coverage[mode], "CORRUPT_ARTIFACT")
                self.assertEqual(
                    CatalogReaderRegistry(root, "26Q1").resolve(
                        {"mode": mode}
                    ).state,
                    "CORRUPT_ARTIFACT",
                )

    def test_parquet_magic_without_metadata_is_quarantined(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = root / "analysis-modules" / "generic" / "results" / "unit"
            unit.mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "header-only.parquet").write_bytes(b"PAR1")
            (unit / "header-footer-only.parquet").write_bytes(b"PAR1PAR1")
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                rows = db.execute(
                    "SELECT artifact_path,integrity_state,integrity_reason_code "
                    "FROM artifact_catalog WHERE artifact_path LIKE '%.parquet' "
                    "ORDER BY artifact_path"
                ).fetchall()
            self.assertEqual(
                rows,
                [
                    (
                        "analysis-modules/generic/results/unit/header-footer-only.parquet",
                        "QUARANTINED",
                        "INVALID_PARQUET",
                    ),
                    (
                        "analysis-modules/generic/results/unit/header-only.parquet",
                        "QUARANTINED",
                        "INVALID_PARQUET",
                    ),
                ],
            )

    def test_v6_catalog_relates_assets_coverage_and_detects_fresh_index(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = root / "analysis-modules" / "CRISPR基因-基因共依赖分析" / "results" / "matrix"
            (unit / "blocks").mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({
                    "status": "complete", "release": "26Q1",
                    "scope": "pan_cancer", "modality": "CRISPRGeneEffect",
                    "model_count": 1208, "model_set_fingerprint": "models-26q1",
                    "tested_gene_count": 18531, "retained_gene_count": 17787,
                    "gene_universe": "DepMap 26Q1 CRISPR genes",
                }),
                encoding="utf-8",
            )
            (unit / "gene_order.csv").write_text("gene_index,symbol\n1,ESR1\n", encoding="utf-8")
            (unit / "blocks" / "block_00001_00001.rds").write_bytes(b"X\nfixture")
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

            self.assertEqual(counts["capabilities"], 22)
            self.assertEqual(counts["matrix_gene_blocks"], 1)
            self.assertEqual(counts["coverage_records"], 3)
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
                coverage = db.execute(
                    "SELECT release,scope,modality,model_count,model_set_fingerprint,"
                    "tested_gene_count,retained_gene_count,gene_universe "
                    "FROM coverage_registry WHERE module='CRISPR基因-基因共依赖分析'"
                ).fetchone()
                self.assertEqual(
                    coverage,
                    (
                        "26Q1", "pan_cancer", "CRISPRGeneEffect", 1208,
                        "models-26q1", 18531, 17787,
                        "DepMap 26Q1 CRISPR genes",
                    ),
                )
                self.assertEqual(
                    db.execute(
                        "SELECT adapter FROM reader_registry WHERE query_mode='enrichment'"
                    ).fetchone()[0],
                    "enrichment_adapter",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT adapter FROM reader_registry WHERE query_mode='model_gene_effect'"
                    ).fetchone()[0],
                    "model_gene_effect_adapter",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT adapter FROM reader_registry WHERE query_mode='cross_platform_validation'"
                    ).fetchone()[0],
                    "cross_platform_validation_adapter",
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

            (unit / "blocks" / "block_00001_00001.rds").write_bytes(
                b"X\nmodified"
            )
            with self.assertLogs("depmap_mcp.catalog_readers", level="ERROR"):
                _bound, changed = asyncio.run(
                    CatalogReaderRegistry(root, "26Q1").read(
                        object(),
                        {"mode": "pair", "source": "ESR1", "target": "FOXA1"},
                        runner,
                    )
                )
            self.assertEqual(changed["status"], "MODULE_UNAVAILABLE")
            self.assertEqual(changed["reason_code"], "CHECKSUM_MISMATCH")
            self.assertNotIn(str(root), json.dumps(changed))

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

            # Nullable legacy capability metadata must not break inventory queries.
            with closing(sqlite3.connect(output)) as db:
                db.execute(
                    "UPDATE capability_catalog SET payload_json='null' "
                    "WHERE intent='true_love_gene_catalog'"
                )
                db.commit()
            nullable = _run_analysis_catalog_query(
                settings,
                {"mode": "analysis_catalog", "module": "true_love"},
            )
            self.assertEqual(nullable["status"], "NOT_RETAINED")
            self.assertEqual(nullable["rows"], [])
            self.assertEqual(nullable["catalog_status"], "PARTIAL")
            self.assertEqual(nullable["invalid_record_count"], 1)

            # A changed source invalidates the release-scoped coverage snapshot.
            (unit / "new-result.csv").write_text("gene,value\nESR1,1\n", encoding="utf-8")
            self.assertFalse(is_fresh(root, output))

    def test_enrichment_reader_resolves_only_with_a_complete_registered_artifact(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = root / "analysis-modules" / "表达依赖富集分析" / "results" / "Breast"
            unit.mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "significant_enrichment.csv.gz").write_bytes(
                gzip.compress(b"term,p_value\nPATHWAY_A,0.01\n")
            )
            output = root / "depmap-26q1-query-index.sqlite"
            build(root, output)
            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog WHERE query_mode='enrichment'"
                    ).fetchone()[0],
                    1,
                )

            resolved = CatalogReaderRegistry(root, "26Q1").resolve(
                {"mode": "enrichment", "source": "GPX4", "lineage": "Breast"}
            )
            self.assertEqual(resolved.state, "RESOLVED")
            self.assertEqual(resolved.reader_id, "enrichment_adapter")
            unsupported = CatalogReaderRegistry(root, "25Q3").resolve(
                {"mode": "enrichment", "source": "GPX4", "lineage": "Breast"}
            )
            self.assertEqual(unsupported.state, "NOT_INDEXED")

            (unit / "manifest.json").write_text(
                json.dumps({"status": "incomplete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "significant_enrichment.csv.gz").unlink()
            build(root, output)
            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog WHERE query_mode='enrichment'"
                    ).fetchone()[0],
                    0,
                )
            missing = CatalogReaderRegistry(root, "26Q1").resolve(
                {"mode": "enrichment", "source": "GPX4", "lineage": "Breast"}
            )
            self.assertEqual(missing.state, "NOT_INDEXED")
            self.assertEqual(missing.reader_id, "enrichment_adapter")


if __name__ == "__main__":
    unittest.main()
