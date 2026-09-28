import json
import asyncio
import gzip
import shutil
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

from scripts.build_depmap_query_index import build, is_fresh
from services.depmap_mcp.catalog_readers import CatalogReaderRegistry
from services.depmap_mcp.artifact_integrity import (
    index_digest_path,
    index_publish_marker_path,
    write_index_digest,
)
from services.depmap_api.app import Settings, _run_analysis_catalog_query


def _valid_parquet(marker: bytes = b"x") -> bytes:
    return b"PAR1" + marker + len(marker).to_bytes(4, "little") + b"PAR1"


def _valid_rds(payload: bytes = b"x") -> bytes:
    return (
        b"X\n"
        + (3).to_bytes(4, "big")
        + (0x040500).to_bytes(4, "big")
        + (0x030500).to_bytes(4, "big")
        + payload
    )


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

    def test_index_provenance_is_revalidated_after_the_bounded_query(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = (
                root
                / "analysis-modules"
                / "通用共依赖分析"
                / "results"
                / "unit"
            )
            unit.mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "rows.csv").write_text(
                "source,target\nA,B\n", encoding="utf-8"
            )
            output = root / "depmap-26q1-query-index.sqlite"
            build(root, output)
            called = False

            async def runner(_settings, _query):
                nonlocal called
                called = True
                with closing(sqlite3.connect(output)) as db:
                    db.execute(
                        "UPDATE metadata SET value='changed' WHERE key='built_at'"
                    )
                    db.commit()
                return {"status": "FOUND", "provenance": [str(output)]}

            with self.assertLogs("depmap_mcp.catalog_readers", level="ERROR"):
                _resolution, result = asyncio.run(
                    CatalogReaderRegistry(root, "26Q1").read(
                        object(),
                        {"mode": "pair", "source": "A", "target": "B"},
                        runner,
                    )
                )

            self.assertTrue(called)
            self.assertEqual(result["status"], "MODULE_UNAVAILABLE")
            self.assertEqual(result["reason_code"], "CHECKSUM_MISMATCH")
            self.assertNotIn(str(root), json.dumps(result))

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

    def test_core_corruption_is_scoped_to_concrete_inputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            corrupt = root / "depmap-26q1-core" / "gene_core_summary.parquet"
            corrupt.parent.mkdir(parents=True)
            corrupt.write_bytes(b"PAR1PAR1")
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT integrity_state,integrity_reason_code "
                        "FROM artifact_catalog WHERE artifact_path=?",
                        ("depmap-26q1-core/gene_core_summary.parquet",),
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
                    1,
                )
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog "
                        "WHERE query_mode='drug'"
                    ).fetchone()[0],
                    1,
                )

    def test_model_gene_effect_ignores_unrelated_core_subtree_corruption(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            core = root / "depmap-26q1-core"
            core.mkdir(parents=True)
            (core / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (core / "model_gene_effect.parquet").write_bytes(_valid_parquet(b"e"))
            (core / "model_metadata.parquet").write_bytes(_valid_parquet(b"m"))
            (core / "model-gene-effect.parquet").write_bytes(b"PAR1PAR1")
            corrupt = core / "lineage_dependency_tests" / "broken.parquet"
            corrupt.parent.mkdir(parents=True)
            corrupt.write_bytes(b"PAR1PAR1")
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                coverage = dict(
                    db.execute(
                        "SELECT query_mode,coverage_state FROM reader_coverage "
                        "WHERE query_mode IN ('model_gene_effect','lineage_dependency')"
                    )
                )
                self.assertEqual(coverage["model_gene_effect"], "AVAILABLE")
                self.assertEqual(coverage["lineage_dependency"], "CORRUPT_ARTIFACT")
                self.assertEqual(
                    db.execute(
                        "SELECT COUNT(*) FROM capability_catalog "
                        "WHERE query_mode='model_gene_effect'"
                    ).fetchone()[0],
                    1,
                )

            resolution = CatalogReaderRegistry(root, "26Q1").resolve(
                {"mode": "model_gene_effect", "gene": "GENE_A"}
            )
            self.assertEqual(resolution.state, "RESOLVED")
            self.assertEqual(
                set(resolution.artifact_uris),
                {
                    "depmap-26q1-core/model_gene_effect.parquet",
                    "depmap-26q1-core/model_metadata.parquet",
                },
            )

    def test_core_root_assets_are_verified_before_runner_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            core = root / "depmap-26q1-core"
            blocks = core / "lineage_blocks"
            blocks.mkdir(parents=True)
            (core / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            summary = core / "gene_core_summary.parquet"
            summary.write_bytes(_valid_parquet(b"summary-a"))
            (blocks / "lineage_a.parquet").write_bytes(_valid_parquet(b"lineage"))
            sibling = core / "lineage-blocks" / "broken.parquet"
            sibling.parent.mkdir(parents=True)
            sibling.write_bytes(b"PAR1PAR1")
            descendant = core / "lineage_dependency_tests" / "unit"
            descendant.mkdir(parents=True)
            (descendant / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            output = root / "depmap-26q1-query-index.sqlite"
            build(root, output)
            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT rc.coverage_state,a.analysis_unit "
                        "FROM reader_coverage rc JOIN analysis_catalog a "
                        "ON a.analysis_id=rc.analysis_id WHERE rc.query_mode='core'"
                    ).fetchone(),
                    ("AVAILABLE", "depmap-26q1-core"),
                )
            summary.write_bytes(_valid_parquet(b"summary-b"))
            called = False

            async def runner(_settings, _query):
                nonlocal called
                called = True
                return {"status": "not_testable"}

            _resolution, result = asyncio.run(
                CatalogReaderRegistry(root, "26Q1").read(
                    object(),
                    {"mode": "core", "gene": "GENE_A"},
                    runner,
                )
            )

            self.assertFalse(called)
            self.assertEqual(result["status"], "MODULE_UNAVAILABLE")
            self.assertEqual(result["reason_code"], "CHECKSUM_MISMATCH")

    def test_reader_module_patterns_treat_underscores_as_literals(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            sibling = (
                root
                / "depmap-26q1-full"
                / "lineage-sparse-networks"
                / "unit"
            )
            sibling.mkdir(parents=True)
            (sibling / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (sibling / "rows.csv").write_text(
                "label,value\nA,1\n", encoding="utf-8"
            )
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT coverage_state FROM reader_coverage "
                        "WHERE query_mode='lineage_network'"
                    ).fetchone()[0],
                    "NOT_COMPUTED",
                )
            self.assertEqual(
                CatalogReaderRegistry(root, "26Q1").resolve(
                    {"mode": "lineage_network"}
                ).state,
                "NOT_INDEXED",
            )

    def test_archived_corruption_does_not_disable_complete_reader(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            module = root / "depmap-26q1-full" / "lineage_sparse_networks"
            complete = module / "effect_correlation" / "Lineage"
            complete.mkdir(parents=True)
            (complete / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (complete / "rows.csv").write_text(
                "label,value\nA,1\n", encoding="utf-8"
            )
            archived = module / "archive-copy"
            archived.mkdir(parents=True)
            (archived / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (archived / "broken.csv.gz").write_bytes(
                gzip.compress(b"label,value\nA,1\n")[:-4]
            )
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT completion_state FROM analysis_catalog "
                        "WHERE analysis_unit LIKE '%archive-copy'"
                    ).fetchone()[0],
                    "QUARANTINED",
                )
                self.assertEqual(
                    db.execute(
                        "SELECT coverage_state FROM reader_coverage "
                        "WHERE query_mode='lineage_network'"
                    ).fetchone()[0],
                    "AVAILABLE",
                )

    def test_lineage_catalog_tracks_manifests_not_network_blocks(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = (
                root
                / "depmap-26q1-full"
                / "lineage_sparse_networks"
                / "effect_correlation"
                / "Lineage"
            )
            unit.mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "broken.parquet").write_bytes(b"PAR1PAR1")
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                coverage = dict(
                    db.execute(
                        "SELECT query_mode,coverage_state FROM reader_coverage "
                        "WHERE query_mode IN ('lineage_catalog','lineage_network')"
                    )
                )
            self.assertEqual(coverage["lineage_catalog"], "AVAILABLE")
            self.assertEqual(coverage["lineage_network"], "CORRUPT_ARTIFACT")

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
                "drug": "prism_auc_effect_correlation",
                "enrichment": "lineage_gene_enrichment",
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
                        "WHERE query_mode IN "
                        "('lineage_network','lineage_cnv','lineage_drug','drug','enrichment','core')"
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

    def test_r_serialization_magic_without_payload_is_quarantined(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            unit = root / "analysis-modules" / "generic" / "results" / "unit"
            unit.mkdir(parents=True)
            (unit / "manifest.json").write_text(
                json.dumps({"status": "complete", "release": "26Q1"}),
                encoding="utf-8",
            )
            (unit / "binary-header-only.rds").write_bytes(b"X\n")
            (unit / "ascii-header-only.rds").write_bytes(b"A\n")
            (unit / "workspace-header-only.rdata").write_bytes(b"RDX3\n")
            (unit / "valid.rds").write_bytes(_valid_rds())
            output = root / "depmap-26q1-query-index.sqlite"

            build(root, output)

            with closing(sqlite3.connect(output)) as db:
                rows = db.execute(
                    "SELECT artifact_path,integrity_state,integrity_reason_code "
                    "FROM artifact_catalog WHERE artifact_path LIKE '%.rds' "
                    "OR artifact_path LIKE '%.rdata' ORDER BY artifact_path"
                ).fetchall()
            self.assertEqual(
                rows,
                [
                    (
                        "analysis-modules/generic/results/unit/ascii-header-only.rds",
                        "QUARANTINED",
                        "INVALID_R_OBJECT",
                    ),
                    (
                        "analysis-modules/generic/results/unit/binary-header-only.rds",
                        "QUARANTINED",
                        "INVALID_R_OBJECT",
                    ),
                    (
                        "analysis-modules/generic/results/unit/valid.rds",
                        "VERIFIED",
                        None,
                    ),
                    (
                        "analysis-modules/generic/results/unit/workspace-header-only.rdata",
                        "QUARANTINED",
                        "INVALID_R_OBJECT",
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
            (unit / "blocks" / "block_00001_00001.rds").write_bytes(
                _valid_rds(b"fixture")
            )
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
                _valid_rds(b"modified")
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
            write_index_digest(output)
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

    def test_readers_keep_the_previous_verified_pair_until_publish_completes(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            roots = {name: base / name for name in ("old", "new")}
            for name, root in roots.items():
                unit = root / "analysis-modules" / "generic" / "results" / name
                unit.mkdir(parents=True)
                (unit / "manifest.json").write_text(
                    json.dumps({"status": "complete", "release": "26Q1"}),
                    encoding="utf-8",
                )
                build(root, root / "depmap-26q1-query-index.sqlite")

            root = roots["old"]
            output = root / "depmap-26q1-query-index.sqlite"
            replacement = roots["new"] / output.name
            old_digest = index_digest_path(output).read_text(encoding="ascii").strip()
            previous = output.with_name(f"{output.name}.previous-{old_digest}")
            shutil.copyfile(output, previous)
            write_index_digest(previous, old_digest)
            index_publish_marker_path(output).write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "previous_name": previous.name,
                        "previous_sha256": old_digest,
                    }
                ),
                encoding="utf-8",
            )
            shutil.copyfile(replacement, output)

            settings = Settings(
                knowledge_root=root,
                query_script=root / "query_depmap_kb.R",
                api_token="test-token",
            )

            def analysis_units() -> list[str]:
                result = _run_analysis_catalog_query(settings, {})
                self.assertEqual(result["status"], "FOUND")
                return [row["analysis_unit"] for row in result["rows"]]

            self.assertTrue(any(value.endswith("/old") for value in analysis_units()))
            shutil.copyfile(
                index_digest_path(replacement), index_digest_path(output)
            )
            self.assertTrue(any(value.endswith("/old") for value in analysis_units()))
            index_publish_marker_path(output).unlink()
            units = analysis_units()
            self.assertTrue(any(value.endswith("/new") for value in units))
            self.assertFalse(any(value.endswith("/old") for value in units))

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
