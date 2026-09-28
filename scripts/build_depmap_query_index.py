#!/usr/bin/env python3
"""Build an atomic SQLite lookup index over retained DepMap query products."""

from __future__ import annotations

import argparse
import csv
import gzip
import hashlib
import json
import logging
import os
import sqlite3
import sys
from datetime import datetime, timezone
from contextlib import closing
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from services.depmap_mcp.artifact_integrity import (
    QUARANTINED,
    VERIFIED,
    index_digest_path,
    inspect_artifact,
    reader_artifact_pattern,
    sqlite_like_pattern,
    verify_index_artifact,
    verify_declared_checksum,
    write_index_digest,
)


CATALOG_SCHEMA_VERSION = "6"
LOGGER = logging.getLogger("depmap_query_index")


def _relative(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def _module_name(relative: Path) -> str:
    parts = relative.parts
    if len(parts) >= 2 and parts[0] == "analysis-modules":
        return parts[1]
    return parts[0] if parts else "."


def _completion(manifest: dict[str, Any], directory: Path) -> tuple[str, str]:
    if "backup" in directory.as_posix().lower() or "archive" in directory.as_posix().lower():
        return "ARCHIVED", "path.archive_or_backup"
    status = str(manifest.get("status") or "").lower()
    qa = str(manifest.get("qa_status") or "").lower()
    if status in {"complete", "completed", "pass", "success"}:
        return "COMPLETE", "manifest.status"
    if qa in {"pass", "complete", "success"}:
        return "COMPLETE", "manifest.qa_status"
    block_count = manifest.get("block_count")
    if isinstance(block_count, int) and block_count > 0:
        blocks = directory / "blocks"
        if blocks.is_dir() and sum(1 for path in blocks.iterdir() if path.is_file()) >= block_count:
            return "COMPLETE", "declared.block_count"
    runs = manifest.get("runs")
    if isinstance(runs, list) and runs:
        accepted = {"complete", "completed", "success", "pass", "skipped_existing"}
        if all(str(run.get("status") or "").lower() in accepted for run in runs if isinstance(run, dict)):
            return "COMPLETE", "manifest.runs"
    declared = manifest.get("outputs")
    if isinstance(declared, dict) and declared:
        present = 0
        for value in declared.values():
            if not isinstance(value, str):
                continue
            if "*" in value:
                present += any(directory.glob(value))
            else:
                present += (directory / value).exists()
        if present:
            return "COMPLETE", "declared.outputs"
    if any(path.is_file() and path.name != "manifest.json" for path in directory.iterdir()):
        return "COMPLETE", "structural.sibling_artifact"
    return "INCOMPLETE", "no_terminal_evidence"


def _declared_artifact_checksums(manifest: dict[str, Any]) -> dict[str, str]:
    """Read output checksums without treating external input hashes as outputs."""
    declared: dict[str, str] = {}
    for field in ("artifact_checksums", "output_checksums", "checksums"):
        value = manifest.get(field)
        if isinstance(value, dict):
            for raw_path, raw_checksum in value.items():
                checksum = (
                    raw_checksum.get("sha256") or raw_checksum.get("checksum")
                    if isinstance(raw_checksum, dict)
                    else raw_checksum
                )
                if isinstance(raw_path, str) and isinstance(checksum, str):
                    declared[raw_path.replace("\\", "/").removeprefix("./")] = checksum
        elif isinstance(value, list):
            for item in value:
                if not isinstance(item, dict):
                    continue
                raw_path = (
                    item.get("path")
                    or item.get("artifact_path")
                    or item.get("file")
                    or item.get("name")
                )
                checksum = item.get("sha256") or item.get("checksum")
                if isinstance(raw_path, str) and isinstance(checksum, str):
                    declared[raw_path.replace("\\", "/").removeprefix("./")] = checksum
    return declared


def _artifact_kind(path: Path) -> str:
    name = path.name.lower()
    if name == "manifest.json":
        return "manifest"
    if name.endswith(".csv.gz") or name.endswith(".tsv.gz"):
        return "compressed_table"
    if path.suffix.lower() in {".csv", ".tsv", ".parquet"}:
        return "table"
    if path.suffix.lower() in {".rds", ".rdata"}:
        return "r_object"
    if path.suffix.lower() in {".sqlite", ".db"}:
        return "database"
    if path.suffix.lower() == ".json":
        return "metadata"
    return "file"


def _source_max_mtime_ns(root: Path, output: Path) -> int:
    excluded = {output, index_digest_path(output)}
    return max(
        (
            path.stat().st_mtime_ns
            for path in root.rglob("*")
            if path.is_file()
            and path not in excluded
            and not path.name.endswith(".tmp")
        ),
        default=root.stat().st_mtime_ns,
    )


def is_fresh(root: Path, output: Path) -> bool:
    if not output.is_file():
        return False
    if verify_index_artifact(output).state != VERIFIED:
        return False
    try:
        with closing(sqlite3.connect(f"file:{output.as_posix()}?mode=ro&immutable=1", uri=True)) as db:
            metadata = dict(db.execute("SELECT key,value FROM metadata"))
        return (
            metadata.get("schema_version") == CATALOG_SCHEMA_VERSION
            and int(metadata.get("source_max_mtime_ns", "0")) >= _source_max_mtime_ns(root, output)
        )
    except (OSError, ValueError, sqlite3.Error):
        return False


def build_directory_catalog(db: sqlite3.Connection, root: Path, output: Path) -> dict[str, int]:
    db.executescript(
        """
        CREATE TABLE analysis_catalog (
          analysis_id TEXT PRIMARY KEY, module TEXT NOT NULL,
          analysis_unit TEXT NOT NULL, completion_state TEXT NOT NULL,
          completion_basis TEXT NOT NULL, release TEXT, family TEXT,
          dataset TEXT, method TEXT, manifest_path TEXT NOT NULL UNIQUE,
          manifest_mtime_ns INTEGER NOT NULL
        );
        CREATE TABLE artifact_catalog (
          artifact_path TEXT PRIMARY KEY, analysis_id TEXT,
          artifact_kind TEXT NOT NULL, extension TEXT NOT NULL,
          size_bytes INTEGER NOT NULL, mtime_ns INTEGER NOT NULL,
          integrity_method TEXT NOT NULL, integrity_value TEXT NOT NULL,
          integrity_state TEXT NOT NULL, integrity_reason_code TEXT,
          FOREIGN KEY(analysis_id) REFERENCES analysis_catalog(analysis_id)
        );
        CREATE TABLE capability_catalog (
          query_mode TEXT NOT NULL, intent TEXT PRIMARY KEY,
          module_pattern TEXT NOT NULL, indexed_content INTEGER NOT NULL,
          mcp_tool TEXT NOT NULL, payload_json TEXT NOT NULL
        );
        CREATE TABLE reader_registry (
          query_mode TEXT PRIMARY KEY, adapter TEXT NOT NULL,
          module_pattern TEXT NOT NULL, supported_formats TEXT NOT NULL
        );
        CREATE TABLE coverage_registry (
          analysis_id TEXT PRIMARY KEY, module TEXT NOT NULL, release TEXT,
          scope TEXT, lineage TEXT, modality TEXT, model_count INTEGER,
          model_set_fingerprint TEXT, tested_gene_count INTEGER,
          retained_gene_count INTEGER, gene_universe TEXT,
          cohort_definition TEXT, intersection_policy TEXT,
          event_definition TEXT, mutation_policy TEXT, threshold_definition TEXT,
          source_asset_fingerprint TEXT, storage_completeness TEXT NOT NULL,
          qa_state TEXT NOT NULL, generated_at TEXT, payload_json TEXT NOT NULL,
          FOREIGN KEY(analysis_id) REFERENCES analysis_catalog(analysis_id)
        );
        CREATE TABLE matrix_block_index (
          analysis_id TEXT NOT NULL, gene TEXT NOT NULL, gene_index INTEGER NOT NULL,
          block_path TEXT NOT NULL, PRIMARY KEY(analysis_id,gene)
        );
        CREATE TABLE analysis_relation (
          analysis_id TEXT NOT NULL, artifact_path TEXT NOT NULL,
          role TEXT NOT NULL, PRIMARY KEY(analysis_id,artifact_path)
        );
        CREATE TABLE reader_coverage (
          query_mode TEXT PRIMARY KEY, analysis_id TEXT,
          coverage_state TEXT NOT NULL,
          FOREIGN KEY(query_mode) REFERENCES reader_registry(query_mode),
          FOREIGN KEY(analysis_id) REFERENCES coverage_registry(analysis_id)
        );
        """
    )
    root_id = hashlib.sha256(b"_knowledge_root").hexdigest()[:24]
    db.execute(
        "INSERT INTO analysis_catalog VALUES (?,?,?,?,?,?,?,?,?,?,?)",
        (root_id, "_knowledge_root", ".", "COMPLETE", "catalog.root", None, "knowledge_root_assets", None, None, ".catalog", root.stat().st_mtime_ns),
    )
    manifests: list[tuple[Path, str, str]] = []
    declared_checksums: dict[Path, dict[str, str]] = {}
    for path in sorted(root.rglob("manifest.json")):
        if path == output or output in path.parents:
            continue
        try:
            manifest = json.loads(path.read_text(encoding="utf-8-sig"))
            if not isinstance(manifest, dict):
                continue
        except (OSError, UnicodeError, json.JSONDecodeError):
            continue
        relative = path.relative_to(root)
        unit = relative.parent.as_posix()
        analysis_id = hashlib.sha256(unit.encode("utf-8")).hexdigest()[:24]
        state, basis = _completion(manifest, path.parent)
        stat = path.stat()
        db.execute(
            "INSERT INTO analysis_catalog VALUES (?,?,?,?,?,?,?,?,?,?,?)",
            (
                analysis_id, _module_name(relative), unit, state, basis,
                str(manifest.get("release") or "") or None,
                str(manifest.get("family") or "") or None,
                str(manifest.get("dataset") or "") or None,
                str(manifest.get("method") or "") or None,
                relative.as_posix(), stat.st_mtime_ns,
            ),
        )
        def first(*keys):
            for key in keys:
                value = manifest.get(key)
                if value not in (None, "", [], {}):
                    return value
            return None
        cohort_definition = first("cohort_definition", "cohort")
        intersection_policy = first("intersection_policy", "sample_intersection")
        event_definition = first("event_definition", "mutation_definition", "event")
        mutation_policy = first("mutation_policy", "mut_wt_policy", "missing_policy")
        threshold_definition = first("threshold_definition", "thresholds", "cutoffs")
        source_checksums = first("source_checksums", "input_checksums", "checksums")
        source_asset_fingerprint = (
            hashlib.sha256(json.dumps(source_checksums, sort_keys=True, default=str).encode()).hexdigest()
            if source_checksums is not None else None
        )
        coverage = {
            "cohort_definition": cohort_definition,
            "intersection_policy": intersection_policy,
            "event_definition": event_definition,
            "mutation_policy": mutation_policy,
            "threshold_definition": threshold_definition,
        }
        db.execute(
            "INSERT INTO coverage_registry VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (
                analysis_id, _module_name(relative), str(manifest.get("release") or "") or None,
                str(first("scope", "analysis_scope") or "") or None,
                str(first("lineage", "oncotree_lineage") or "") or None,
                str(first("modality", "data_modality") or "") or None,
                first("model_count", "n_models", "cell_line_count"),
                str(first("model_set_fingerprint", "cohort_fingerprint") or "") or None,
                first("tested_gene_count", "n_tested_genes", "gene_count"),
                first("retained_gene_count", "n_retained_genes"),
                str(first("gene_universe", "gene_universe_id") or "") or None,
                json.dumps(cohort_definition, ensure_ascii=False) if cohort_definition is not None else None,
                json.dumps(intersection_policy, ensure_ascii=False) if intersection_policy is not None else None,
                json.dumps(event_definition, ensure_ascii=False) if event_definition is not None else None,
                json.dumps(mutation_policy, ensure_ascii=False) if mutation_policy is not None else None,
                json.dumps(threshold_definition, ensure_ascii=False) if threshold_definition is not None else None,
                source_asset_fingerprint,
                str(first("storage_completeness") or ("full" if state == "COMPLETE" else "partial")),
                str(first("qa_status", "qa_state") or state),
                str(first("generated_at", "created_at") or "") or None,
                json.dumps(coverage, ensure_ascii=False, separators=(",", ":")),
            ),
        )
        manifests.append((path.parent, analysis_id, state))
        declared_checksums[path.parent] = _declared_artifact_checksums(manifest)

    # Assign each file to its nearest manifest ancestor. Paths are always
    # knowledge-root relative, so the catalog reveals no host/server layout.
    owner = {directory: analysis_id for directory, analysis_id, _ in manifests}
    artifact_count = 0
    quarantined_artifact_count = 0
    quarantined_analysis_ids: set[str] = set()
    excluded = {output, index_digest_path(output)}
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path in excluded or path.name.endswith(".tmp"):
            continue
        directory = path.parent
        owner_directory: Path | None = None
        analysis_id = None
        while directory == root or root in directory.parents:
            if directory in owner:
                analysis_id = owner[directory]
                owner_directory = directory
                break
            if directory == root:
                break
            directory = directory.parent
        stat = path.stat()
        analysis_id = analysis_id or root_id
        relative_path = _relative(path, root)
        artifact_kind = _artifact_kind(path)
        integrity = inspect_artifact(path, artifact_kind)
        if owner_directory is not None and path.name != "manifest.json":
            relative_to_owner = path.relative_to(owner_directory).as_posix()
            checksums = declared_checksums.get(owner_directory, {})
            expected_checksum = checksums.get(relative_to_owner)
            if expected_checksum is None:
                expected_checksum = checksums.get(path.name)
            integrity = verify_declared_checksum(integrity, expected_checksum)
        db.execute(
            "INSERT INTO artifact_catalog VALUES (?,?,?,?,?,?,?,?,?,?)",
            (
                relative_path,
                analysis_id,
                artifact_kind,
                path.suffix.lower(),
                stat.st_size,
                stat.st_mtime_ns,
                integrity.method,
                integrity.value,
                integrity.state,
                integrity.reason_code,
            ),
        )
        role = "script" if "/scripts/" in f"/{relative_path}" or path.suffix.lower() in {".r", ".py", ".sh", ".ps1"} else "manifest" if path.name == "manifest.json" else "result" if "/results/" in f"/{relative_path}" else "data" if "/data/" in f"/{relative_path}" else "asset"
        db.execute("INSERT INTO analysis_relation VALUES (?,?,?)", (analysis_id, relative_path, role))
        if integrity.state == QUARANTINED:
            quarantined_artifact_count += 1
            quarantined_analysis_ids.add(analysis_id)
            LOGGER.warning(
                "quarantined indexed artifact path=%s reason=%s diagnostic=%s",
                relative_path,
                integrity.reason_code,
                integrity.diagnostic,
            )
        artifact_count += 1

    quarantined_analysis_ids.discard(root_id)
    if quarantined_analysis_ids:
        placeholders = ",".join("?" for _ in quarantined_analysis_ids)
        db.execute(
            f"UPDATE analysis_catalog SET completion_state='QUARANTINED', "
            f"completion_basis='artifact.integrity' WHERE analysis_id IN ({placeholders})",
            tuple(sorted(quarantined_analysis_ids)),
        )
        db.execute(
            f"UPDATE coverage_registry SET storage_completeness='corrupt', "
            f"qa_state='CORRUPT_ARTIFACT' WHERE analysis_id IN ({placeholders})",
            tuple(sorted(quarantined_analysis_ids)),
        )

    reader_rows = [
        ("core", "gene_evidence", "depmap-26q1-core/%", 0),
        ("model_gene_effect", "model_gene_effect_slice", "depmap-26q1-core/%", 0),
        ("cross_platform_validation", "cross_platform_dependency_validation", "depmap-26q1-full/cross_platform_validation/%", 0),
        ("lineage_catalog", "cancer_inventory", "depmap-26q1-full/lineage_sparse_networks/%|depmap-26q1-full/lineage_cnv_amplification_dependency/%|depmap-26q1-full/lineage_prism_associations/%|depmap-26q1-full/lineage_gene_enrichment/%|depmap-26q1-full/subtype_dependency/%|depmap-26q1-tcga/%", 0),
        ("lineage_dependency", "cancer_dependency_ranking", "depmap-26q1-core/lineage_dependency_tests/%", 0),
        ("pan_cancer_dependency", "pan_cancer_dependency_summary", "depmap-26q1-core/lineage_dependency_tests/%", 0),
        ("lineage_directions", "cancer_direction_discovery", "depmap-26q1-full/lineage_sparse_networks/%|depmap-26q1-full/lineage_cnv_amplification_dependency/%|depmap-26q1-full/lineage_prism_associations/%|depmap-26q1-full/lineage_gene_enrichment/%", 0),
        ("lineage_network", "lineage_network", "depmap-26q1-full/lineage_sparse_networks/%", 0),
        ("lineage_cnv", "lineage_cnv", "depmap-26q1-full/lineage_cnv_amplification_dependency/%", 0),
        ("lineage_drug", "lineage_drug", "depmap-26q1-full/lineage_prism_associations/%", 0),
        ("top", "top", "depmap-26q1-full/effect_correlation/%|depmap-26q1-full/expression_correlation/%|depmap-26q1-full/expression_dependency/%|depmap-26q1-full/damaging_mutation_dependency/%|depmap-26q1-full/custom_missense_mutation_dependency/%|depmap-26q1-full/hotspot_mutation_dependency/%|depmap-26q1-full/cnv_amplification_dependency/%|analysis-modules/%/downstream_dependency/%", 0),
        ("lineage", "lineage", "depmap-26q1-full/lineage_%_mutation_dependency/%|analysis-modules/%突变锚定基因选择%/cancer_anchor_catalog_v2/downstream_dependency/lineage_%_mutation_dependency/%", 0),
        ("pathway", "pathway", "depmap-26q1-full/progeny_dependency/%", 0),
        ("drug", "drug_gene_evidence", "depmap-26q1-full/prism_auc_%_correlation/%", 0),
        ("pair", "gene_pair_evidence", "*相关性分析*|*共依赖分析*|depmap-26q1-full/effect_correlation/%|depmap-26q1-full/expression_correlation/%|depmap-26q1-full/expression_dependency/%|depmap-26q1-full/damaging_mutation_dependency/%|depmap-26q1-full/custom_missense_mutation_dependency/%|depmap-26q1-full/hotspot_mutation_dependency/%|depmap-26q1-full/cnv_amplification_dependency/%", 0),
        ("enrichment", "pathway_enrichment", "*富集*|depmap-26q1-full/lineage_gene_enrichment/%", 0),
        ("mutation_anchor", "mutation_anchor_discovery", "*突变锚定基因选择*", 0),
        ("lineage_mutation_dependency", "mutation_to_dependency", "*突变锚定基因选择*", 0),
        ("mutation_to_dependency", "mutation_to_dependency", "*突变锚定基因选择*", 0),
        ("dependency_to_mutation", "dependency_to_mutation", "*突变锚定基因选择*", 0),
        ("codependency", "codependency_evidence", "*共依赖分析*", 0),
        ("tf_dependency", "tf_activity_to_dependency", "*转录因子活性*", 1),
        ("biomarker_target", "expression_biomarker_model", "*表达基因-CRISPR*", 1),
        ("true_love", "true_love_gene_catalog", "*共依赖分析*|depmap-26q1-full/true_love_gene/%", 1),
        ("subtype", "subtype_evidence", "depmap-26q1-full/subtype_dependency/%", 0),
        ("coamplification", "coamplification_evidence", "depmap-26q1-full/coamplification_dependency/%", 0),
        ("synthetic_lethal", "synthetic_lethal_evidence", "*突变锚定基因选择*|depmap-26q1-full/observational_synthetic_lethal_candidates/%", 0),
        ("three_d", "three_d_evidence", "depmap-26q1-3d/%", 0),
        ("tcga_expression_survival", "tcga_survival_evidence", "depmap-26q1-tcga/%", 0),
    ]
    try:
        from services.depmap_mcp.capability_catalog import INTENT_CAPABILITIES
    except ModuleNotFoundError:
        import importlib.util
        capability_path = Path(__file__).with_name("capability_catalog.py")
        spec = importlib.util.spec_from_file_location("depmap_capability_catalog", capability_path)
        if spec is None or spec.loader is None:
            raise RuntimeError(f"cannot load capability catalog: {capability_path}")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        INTENT_CAPABILITIES = module.INTENT_CAPABILITIES
    cap_by_mode = {row[0]: row for row in reader_rows}
    capabilities = []
    for capability in INTENT_CAPABILITIES:
        intent = capability["intent"]
        mode = {
            "provider_status":"status", "lineage_resolution":"resolve_lineage",
            "cancer_inventory":"lineage_catalog", "cancer_direction_discovery":"lineage_directions",
            "analysis_inventory":"analysis_catalog", "mutation_anchor_discovery":"mutation_anchor",
            "mutation_to_dependency":"synthetic_lethal", "dependency_to_mutation":"synthetic_lethal",
            "codependency_evidence":"codependency",
            "gene_pair_evidence":"pair", "cancer_dependency_ranking":"lineage_dependency",
            "pan_cancer_dependency_summary":"pan_cancer_dependency",
            "model_gene_effect_slice":"model_gene_effect",
            "cross_platform_dependency_validation":"cross_platform_validation",
            "tf_activity_to_dependency":"tf_dependency", "expression_biomarker_model":"biomarker_target",
            "true_love_gene_catalog":"true_love", "gene_evidence":"enrichment",
            "tcga_expression_survival":"tcga_expression_survival", "drug_gene_evidence":"drug",
            "subtype_evidence":"subtype", "coamplification_evidence":"coamplification",
            "three_d_evidence":"three_d",
        }[intent]
        base = cap_by_mode.get(mode, (mode, intent, "analysis-modules", 0))
        capabilities.append((mode, intent, base[2], base[3], capability["mcp_tool"], json.dumps(capability, ensure_ascii=False, separators=(",", ":"))))
    db.executemany("INSERT INTO capability_catalog VALUES (?,?,?,?,?,?)", capabilities)
    readers = []
    for mode in sorted({row[0] for row in capabilities} | {row[0] for row in reader_rows}):
        base = cap_by_mode.get(mode, (mode, mode, "analysis-modules", 0))
        readers.append((mode, f"{mode}_adapter", base[2], "csv,csv.gz,parquet,rds,json"))
    db.executemany("INSERT INTO reader_registry VALUES (?,?,?,?)", readers)
    for mode, _adapter, pattern, _formats in readers:
        likes = []
        for part in pattern.split("|"):
            like = part.strip().replace("*", "%")
            if not like:
                continue
            likes.append(like)
            if like.endswith("/%"):
                likes.append(like[:-2])
        predicates = " OR ".join("module LIKE ? OR analysis_unit LIKE ?" for _ in likes)
        parameters = tuple(value for like in likes for value in (like, like))
        current = db.execute(
            f"SELECT analysis_id FROM analysis_catalog WHERE completion_state='COMPLETE' "
            f"AND ({predicates}) ORDER BY manifest_mtime_ns DESC LIMIT 1",
            parameters,
        ).fetchone() if likes else None
        artifact_likes = [
            sqlite_like_pattern(value)
            for value in reader_artifact_pattern(mode, pattern).split("|")
            if value.strip()
        ]
        artifact_predicates = " OR ".join(
            "artifact_path LIKE ? ESCAPE '\\'" for _ in artifact_likes
        )
        quarantined = (
            db.execute(
                f"SELECT analysis_id FROM artifact_catalog "
                f"WHERE integrity_state='QUARANTINED' AND ({artifact_predicates}) "
                f"ORDER BY artifact_path LIMIT 1",
                tuple(artifact_likes),
            ).fetchone()
            if artifact_likes
            else None
        )
        if current is None and artifact_likes and quarantined is None:
            current = db.execute(
                f"SELECT f.analysis_id FROM artifact_catalog f "
                f"JOIN analysis_catalog a ON a.analysis_id=f.analysis_id "
                f"WHERE f.integrity_state='VERIFIED' "
                f"AND a.completion_state='QUARANTINED' "
                f"AND ({artifact_predicates.replace('artifact_path', 'f.artifact_path')}) "
                f"ORDER BY f.artifact_path LIMIT 1",
                tuple(artifact_likes),
            ).fetchone()
        db.execute(
            "INSERT INTO reader_coverage VALUES (?,?,?)",
            (
                mode,
                quarantined[0] if quarantined else current[0] if current else None,
                "CORRUPT_ARTIFACT" if quarantined else "AVAILABLE" if current else "NOT_COMPUTED",
            ),
        )
    db.execute(
        "DELETE FROM capability_catalog WHERE query_mode IN ("
        "SELECT query_mode FROM reader_coverage WHERE coverage_state='CORRUPT_ARTIFACT') "
        "OR (query_mode='enrichment' AND NOT EXISTS ("
        "SELECT 1 FROM reader_coverage WHERE query_mode='enrichment' "
        "AND coverage_state='AVAILABLE' AND analysis_id IS NOT NULL))"
    )

    # Map every ordered gene to its declared matrix block without opening RDS files.
    for directory, analysis_id, state in manifests:
        order = directory / "gene_order.csv"
        blocks = directory / "blocks"
        if (
            state != "COMPLETE"
            or analysis_id in quarantined_analysis_ids
            or not order.is_file()
            or not blocks.is_dir()
        ):
            continue
        genes = []
        for row in records(order):
            value = (
                row.get("gene")
                or row.get("Gene")
                or row.get("symbol")
                or row.get("gene_symbol")
                or row.get("target_gene")
                or row.get("source_gene")
                or next((item for key, item in row.items() if "index" not in key.lower()), "")
            )
            genes.append(str(value).upper())
        block_files = sorted(path for path in blocks.iterdir() if path.is_file())
        for index, gene in enumerate(genes, 1):
            match = next((path for path in block_files if f"_{index:05d}_" in path.name or path.name.startswith(f"block_{index:05d}_")), None)
            if match is None:
                match = next((path for path in block_files if path.name.startswith("block_") and int(path.stem.split("_")[1]) <= index <= int(path.stem.split("_")[2])), None)
            if match:
                db.execute("INSERT OR REPLACE INTO matrix_block_index VALUES (?,?,?,?)", (analysis_id, gene, index, _relative(match, root)))
    db.executescript(
        """
        CREATE INDEX idx_analysis_module_state ON analysis_catalog(module, completion_state);
        CREATE INDEX idx_analysis_unit ON analysis_catalog(analysis_unit);
        CREATE INDEX idx_coverage_dimensions ON coverage_registry(release,module,scope,lineage,modality);
        CREATE INDEX idx_artifact_analysis ON artifact_catalog(analysis_id, artifact_kind);
        CREATE INDEX idx_artifact_kind ON artifact_catalog(artifact_kind, extension);
        CREATE INDEX idx_artifact_path ON artifact_catalog(artifact_path);
        CREATE INDEX idx_capability_intent ON capability_catalog(intent);
        CREATE INDEX idx_capability_mode ON capability_catalog(query_mode);
        CREATE INDEX idx_relation_role ON analysis_relation(analysis_id,role);
        CREATE INDEX idx_matrix_gene ON matrix_block_index(gene);
        CREATE INDEX idx_reader_coverage_analysis ON reader_coverage(analysis_id);
        """
    )
    return {
        "analysis_units": len(manifests),
        "completed_analysis_units": db.execute(
            "SELECT COUNT(*) FROM analysis_catalog "
            "WHERE analysis_id<>? AND completion_state='COMPLETE'",
            (root_id,),
        ).fetchone()[0],
        "artifacts": artifact_count,
        "quarantined_artifacts": quarantined_artifact_count,
        "capabilities": db.execute("SELECT COUNT(*) FROM capability_catalog").fetchone()[0],
        "readers": len(readers),
        "coverage_records": db.execute("SELECT COUNT(*) FROM coverage_registry").fetchone()[0],
        "reader_coverage_records": db.execute("SELECT COUNT(*) FROM reader_coverage").fetchone()[0],
        "matrix_gene_blocks": db.execute("SELECT COUNT(*) FROM matrix_block_index").fetchone()[0],
    }


def records(path: Path):
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt", encoding="utf-8-sig", newline="") as handle:
        yield from csv.DictReader(handle, strict=True)


def build(root: Path, output: Path) -> dict:
    source_max_mtime_ns = _source_max_mtime_ns(root, output)
    temporary = output.with_suffix(output.suffix + ".tmp")
    temporary.unlink(missing_ok=True)
    db = sqlite3.connect(temporary)
    db.execute("PRAGMA journal_mode=OFF")
    db.execute("PRAGMA synchronous=OFF")
    db.executescript(
        """
        CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE true_love (
          catalog TEXT NOT NULL, coverage TEXT NOT NULL,
          gene_a TEXT NOT NULL, gene_b TEXT NOT NULL,
          sort_1 REAL, sort_2 REAL, row_json TEXT NOT NULL
        );
        CREATE TABLE tf_dependency (
          tf TEXT NOT NULL, target_gene TEXT NOT NULL,
          direction TEXT, rank INTEGER, row_json TEXT NOT NULL
        );
        CREATE TABLE biomarker_target (
          target_gene TEXT PRIMARY KEY, eligible INTEGER NOT NULL,
          row_json TEXT NOT NULL
        );
        """
    )
    counts = {"true_love": 0, "tf_dependency": 0, "biomarker_target": 0}
    counts.update(build_directory_catalog(db, root, output))

    def cataloged_and_verified(path: Path) -> bool:
        if not path.is_file():
            return False
        row = db.execute(
            "SELECT integrity_state FROM artifact_catalog WHERE artifact_path=?",
            (_relative(path, root),),
        ).fetchone()
        return bool(row and row[0] == "VERIFIED")

    tlg = root / "depmap-26q1-full" / "true_love_gene"
    sources = [
        ("stable_negative_rank1", "all", tlg / "high_confidence_stability" / "final_high_confidence_true_love_genes.csv.gz"),
        ("negative_r_lt_minus_0_3", "all", tlg / "tm00_derived_catalogs_26Q1" / "negative_codependency_r_lt_minus_0.3_legacy.csv.gz"),
        ("negative_r_lt_minus_0_3", "quality", tlg / "tm00_derived_catalogs_26Q1" / "negative_codependency_r_lt_minus_0.3_n500.csv.gz"),
        ("positive_reciprocal_top20", "all", tlg / "tm00_derived_catalogs_26Q1" / "positive_reciprocal_top20_legacy.csv.gz"),
        ("positive_reciprocal_top20", "quality", tlg / "tm00_derived_catalogs_26Q1" / "positive_reciprocal_top20_n500.csv.gz"),
    ]
    for catalog, coverage, path in sources:
        if not cataloged_and_verified(path):
            continue
        batch = []
        for row in records(path):
            a = str(row.get("gene_a") or row.get("source_gene") or "").upper()
            b = str(row.get("gene_b") or row.get("target_gene") or "").upper()
            if catalog == "stable_negative_rank1":
                s1 = -float(row.get("bootstrap_reciprocal_stability") or 0)
                s2 = float(row.get("worst_direction_fdr") or 1)
            elif catalog == "negative_r_lt_minus_0_3":
                s1 = float(row.get("correlation") or 0)
                s2 = 0.0
            else:
                s1 = float(row.get("reciprocal_rank_sum") or 999)
                s2 = -float(row.get("correlation_a_to_b") or 0)
            batch.append((catalog, coverage, a, b, s1, s2, json.dumps(row, ensure_ascii=False, separators=(",", ":"))))
            if len(batch) >= 10_000:
                db.executemany("INSERT INTO true_love VALUES (?,?,?,?,?,?,?)", batch)
                counts["true_love"] += len(batch); batch.clear()
        db.executemany("INSERT INTO true_love VALUES (?,?,?,?,?,?,?)", batch)
        counts["true_love"] += len(batch)

    tf_path = root / "analysis-modules" / "转录因子活性-CRISPR基因依赖相关性分析" / "results" / "tf_activity_dependency_26Q1_v2" / "top_hits.csv.gz"
    if cataloged_and_verified(tf_path):
        rows = []
        for row in records(tf_path):
            rows.append((str(row.get("TF") or "").upper(), str(row.get("target_gene") or "").upper(), row.get("direction"), int(float(row.get("rank") or 0)), json.dumps(row, ensure_ascii=False, separators=(",", ":"))))
        db.executemany("INSERT INTO tf_dependency VALUES (?,?,?,?,?)", rows)
        counts["tf_dependency"] = len(rows)

    eligibility = root / "analysis-modules" / "表达基因-CRISPR基因依赖相关性分析" / "results" / "predictive_biomarker" / "target_eligibility_26Q1" / "target_eligibility_catalog.csv"
    if cataloged_and_verified(eligibility):
        rows = []
        for row in records(eligibility):
            eligible = str(row.get("eligible_for_nested_model", "")).upper() == "TRUE"
            rows.append((str(row["target_gene"]).upper(), int(eligible), json.dumps(row, ensure_ascii=False, separators=(",", ":"))))
        db.executemany("INSERT INTO biomarker_target VALUES (?,?,?)", rows)
        counts["biomarker_target"] = len(rows)

    db.executescript(
        """
        CREATE INDEX idx_tlg_a ON true_love(catalog, coverage, gene_a, sort_1, sort_2);
        CREATE INDEX idx_tlg_b ON true_love(catalog, coverage, gene_b, sort_1, sort_2);
        CREATE INDEX idx_tlg_order ON true_love(catalog, coverage, sort_1, sort_2);
        CREATE INDEX idx_tf_source_rank ON tf_dependency(tf, direction, rank);
        CREATE INDEX idx_tf_pair ON tf_dependency(tf, target_gene);
        CREATE INDEX idx_biomarker_eligible ON biomarker_target(eligible, target_gene);
        """
    )
    db.execute("INSERT INTO metadata VALUES (?,?)", ("schema_version", CATALOG_SCHEMA_VERSION))
    db.execute("INSERT INTO metadata VALUES (?,?)", ("counts", json.dumps(counts, sort_keys=True)))
    db.execute("INSERT INTO metadata VALUES (?,?)", ("source_max_mtime_ns", str(source_max_mtime_ns)))
    db.execute("INSERT INTO metadata VALUES (?,?)", ("built_at", datetime.now(timezone.utc).isoformat()))
    db.execute("ANALYZE")
    integrity = db.execute("PRAGMA integrity_check").fetchone()[0]
    if integrity != "ok":
        raise RuntimeError(f"SQLite integrity check failed: {integrity}")
    db.commit(); db.close()
    output.parent.mkdir(parents=True, exist_ok=True)
    os.replace(temporary, output)
    write_index_digest(output)
    return counts


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--knowledge-root", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--if-stale", action="store_true", help="skip an atomic rebuild when indexed sources have not changed")
    args = parser.parse_args()
    output = args.output or args.knowledge_root / "depmap-26q1-query-index.sqlite"
    if args.if_stale and is_fresh(args.knowledge_root, output):
        print(json.dumps({"status": "FRESH", "output": str(output)}, ensure_ascii=False))
        return
    counts = build(args.knowledge_root, output)
    print(json.dumps({"status": "PASS", "output": str(output), "counts": counts}, ensure_ascii=False))


if __name__ == "__main__":
    main()
