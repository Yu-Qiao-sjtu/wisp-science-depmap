#!/usr/bin/env python3
"""Build an atomic SQLite lookup index over retained DepMap query products."""

from __future__ import annotations

import argparse
import csv
import gzip
import json
import os
import sqlite3
from pathlib import Path


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
    db.execute("INSERT INTO metadata VALUES (?,?)", ("schema_version", "1"))
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
