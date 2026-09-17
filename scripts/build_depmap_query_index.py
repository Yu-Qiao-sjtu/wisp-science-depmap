#!/usr/bin/env python3
"""Build an atomic SQLite lookup index over retained DepMap query products."""

from __future__ import annotations

import argparse
import csv
import gzip
import hashlib
import json
import os
import sqlite3
from pathlib import Path
from typing import Any


CATALOG_SCHEMA_VERSION = "2"


def _relative(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def _module_name(relative: Path) -> str:
    parts = relative.parts
    if len(parts) >= 2 and parts[0] == "analysis-modules":
        return parts[1]
    return parts[0] if parts else "."


def _completion(manifest: dict[str, Any], directory: Path) -> tuple[str, str]:
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
    return "UNVERIFIED", "no_terminal_status"


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
          FOREIGN KEY(analysis_id) REFERENCES analysis_catalog(analysis_id)
        );
        CREATE TABLE capability_catalog (
          query_mode TEXT PRIMARY KEY, intent TEXT NOT NULL,
          module_pattern TEXT NOT NULL, indexed_content INTEGER NOT NULL
        );
        """
    )
    manifests: list[tuple[Path, str, str]] = []
    complete = 0
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
        complete += state == "COMPLETE"
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
        manifests.append((path.parent, analysis_id, state))

    # Assign each file to its nearest manifest ancestor. Paths are always
    # knowledge-root relative, so the catalog reveals no host/server layout.
    owner = {directory: analysis_id for directory, analysis_id, _ in manifests}
    artifact_count = 0
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path == output or path.name.endswith(".tmp"):
            continue
        directory = path.parent
        analysis_id = None
        while directory == root or root in directory.parents:
            if directory in owner:
                analysis_id = owner[directory]
                break
            if directory == root:
                break
            directory = directory.parent
        stat = path.stat()
        db.execute(
            "INSERT INTO artifact_catalog VALUES (?,?,?,?,?,?)",
            (_relative(path, root), analysis_id, _artifact_kind(path), path.suffix.lower(), stat.st_size, stat.st_mtime_ns),
        )
        artifact_count += 1

    capabilities = [
        ("core", "gene_evidence", "depmap-26q1-core", 0),
        ("lineage_catalog", "cancer_inventory", "depmap-26q1-full", 0),
        ("lineage_dependency", "cancer_dependency_ranking", "depmap-26q1-full", 0),
        ("lineage_directions", "cancer_direction_discovery", "analysis-modules", 0),
        ("pair", "gene_pair_evidence", "*相关性分析*", 0),
        ("enrichment", "pathway_enrichment", "*富集*", 0),
        ("mutation_anchor", "mutation_anchor_discovery", "*突变锚定基因选择*", 0),
        ("mutation_to_dependency", "mutation_to_dependency", "*突变锚定基因选择*", 0),
        ("dependency_to_mutation", "dependency_to_mutation", "*突变锚定基因选择*", 0),
        ("tf_dependency", "tf_activity_to_dependency", "*转录因子活性*", 1),
        ("biomarker_target", "expression_biomarker_model", "*表达基因-CRISPR*", 1),
        ("true_love", "true_love_gene_catalog", "*共依赖分析*", 1),
        ("subtype", "subtype_evidence", "depmap-26q1-full", 0),
        ("coamplification", "coamplification_evidence", "depmap-26q1-full", 0),
        ("synthetic_lethal", "synthetic_lethal_evidence", "*突变锚定基因选择*", 0),
        ("three_d", "three_d_evidence", "depmap-26q1-3d", 0),
        ("tcga_expression_survival", "tcga_survival_evidence", "depmap-26q1-tcga", 0),
    ]
    db.executemany("INSERT INTO capability_catalog VALUES (?,?,?,?)", capabilities)
    db.executescript(
        """
        CREATE INDEX idx_analysis_module_state ON analysis_catalog(module, completion_state);
        CREATE INDEX idx_analysis_unit ON analysis_catalog(analysis_unit);
        CREATE INDEX idx_artifact_analysis ON artifact_catalog(analysis_id, artifact_kind);
        CREATE INDEX idx_artifact_kind ON artifact_catalog(artifact_kind, extension);
        CREATE INDEX idx_capability_intent ON capability_catalog(intent);
        """
    )
    return {
        "analysis_units": len(manifests),
        "completed_analysis_units": complete,
        "artifacts": artifact_count,
        "capabilities": len(capabilities),
    }


def records(path: Path):
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt", encoding="utf-8-sig", newline="") as handle:
        yield from csv.DictReader(handle)


def build(root: Path, output: Path) -> dict:
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

    tlg = root / "depmap-26q1-full" / "true_love_gene"
    sources = [
        ("stable_negative_rank1", "all", tlg / "high_confidence_stability" / "final_high_confidence_true_love_genes.csv.gz"),
        ("negative_r_lt_minus_0_3", "all", tlg / "tm00_derived_catalogs_26Q1" / "negative_codependency_r_lt_minus_0.3_legacy.csv.gz"),
        ("negative_r_lt_minus_0_3", "quality", tlg / "tm00_derived_catalogs_26Q1" / "negative_codependency_r_lt_minus_0.3_n500.csv.gz"),
        ("positive_reciprocal_top20", "all", tlg / "tm00_derived_catalogs_26Q1" / "positive_reciprocal_top20_legacy.csv.gz"),
        ("positive_reciprocal_top20", "quality", tlg / "tm00_derived_catalogs_26Q1" / "positive_reciprocal_top20_n500.csv.gz"),
    ]
    for catalog, coverage, path in sources:
        if not path.is_file():
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
    if tf_path.is_file():
        rows = []
        for row in records(tf_path):
            rows.append((str(row.get("TF") or "").upper(), str(row.get("target_gene") or "").upper(), row.get("direction"), int(float(row.get("rank") or 0)), json.dumps(row, ensure_ascii=False, separators=(",", ":"))))
        db.executemany("INSERT INTO tf_dependency VALUES (?,?,?,?,?)", rows)
        counts["tf_dependency"] = len(rows)

    eligibility = root / "analysis-modules" / "表达基因-CRISPR基因依赖相关性分析" / "results" / "predictive_biomarker" / "target_eligibility_26Q1" / "target_eligibility_catalog.csv"
    if eligibility.is_file():
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
    catalog_counts = build_directory_catalog(db, root, output)
    counts.update(catalog_counts)
    db.execute("INSERT INTO metadata VALUES (?,?)", ("schema_version", CATALOG_SCHEMA_VERSION))
    db.execute("INSERT INTO metadata VALUES (?,?)", ("counts", json.dumps(counts, sort_keys=True)))
    db.execute("ANALYZE")
    integrity = db.execute("PRAGMA integrity_check").fetchone()[0]
    if integrity != "ok":
        raise RuntimeError(f"SQLite integrity check failed: {integrity}")
    db.commit(); db.close()
    output.parent.mkdir(parents=True, exist_ok=True)
    os.replace(temporary, output)
    return counts


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--knowledge-root", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    output = args.output or args.knowledge_root / "depmap-26q1-query-index.sqlite"
    counts = build(args.knowledge_root, output)
    print(json.dumps({"status": "PASS", "output": str(output), "counts": counts}, ensure_ascii=False))


if __name__ == "__main__":
    main()
