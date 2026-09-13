#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import gzip
import json
from datetime import datetime, timezone
from pathlib import Path


EXPECTED = {
    "lineage_selective_dependency": ["manifest.json", "lineage_catalog.csv"],
    "progeny_prism_associations": ["manifest.json", "pathway_drug_associations.csv.gz"],
    "observational_synthetic_lethal_candidates": ["manifest.json", "pair_evidence_summary.csv.gz"],
    "coamplification_dependency": [],
    "bipolar_dependency_candidates": ["manifest.json", "pair_summary.csv.gz"],
    "true_love_gene": ["manifest.json", "strict_mutual_rank1_pairs.csv.gz", "tm00_example_validation.csv"],
    "lineage_selective_enrichment": ["manifest.json", "lineage_catalog.csv"],
}


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def csv_header(path: Path) -> list[str]:
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt", encoding="utf-8-sig", newline="") as handle:
        return next(csv.reader(handle))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--knowledge-root", required=True)
    args = parser.parse_args()
    root = Path(args.knowledge_root).resolve()
    full = root / "depmap-26q1-full"
    mutation_root = (
        root
        / "analysis-modules"
        / "癌种内突变锚定基因选择"
        / "cancer_anchor_catalog_v2"
        / "downstream_dependency"
        / "05_precomputed_gene_effect_matrices"
    )

    def module_root_for(name: str) -> Path:
        relocated = mutation_root / name
        return relocated if relocated.is_dir() else full / name

    failures: list[str] = []
    modules: list[dict] = []

    for name, files in EXPECTED.items():
        module_root = module_root_for(name)
        if not module_root.is_dir():
            failures.append(f"{name}: module directory is missing")
            continue
        for relative in files:
            path = module_root / relative
            if not path.is_file() or path.stat().st_size == 0:
                failures.append(f"{name}: required output is missing or empty: {relative}")
        manifest_path = module_root / "manifest.json"
        if manifest_path.is_file():
            manifest = load_json(manifest_path)
            if manifest.get("status") != "complete":
                failures.append(f"{name}: manifest status is not complete")
            if manifest.get("release") != "26Q1":
                failures.append(f"{name}: release is not 26Q1")
        else:
            manifests = sorted(module_root.rglob("manifest.json"))
            complete = [p for p in manifests if load_json(p).get("status") == "complete"]
            manifest = {
                "family": name,
                "status": "complete" if complete else "missing",
                "completed_run_count": len(complete),
            }
            if not complete:
                failures.append(f"{name}: no completed Run manifest exists")
        modules.append({
            "module": name,
            "status": manifest.get("status"),
            "path": str(module_root),
            "manifest": manifest,
        })

    lineage_root = full / "lineage_selective_dependency"
    if (lineage_root / "lineage_catalog.csv").is_file():
        with (lineage_root / "lineage_catalog.csv").open(encoding="utf-8-sig", newline="") as handle:
            rows = list(csv.DictReader(handle))
        if len(rows) != 24 or any(row.get("status") != "complete" for row in rows):
            failures.append("lineage_selective_dependency: expected 24 complete lineages")
        for row in rows:
            lineage_dir = lineage_root / row["lineage_key"]
            for name in ("manifest.json", "all_genes.csv.gz", "selective_hits.csv.gz"):
                if not (lineage_dir / name).is_file():
                    failures.append(f"lineage_selective_dependency/{row['lineage_key']}: missing {name}")

    progeny = full / "progeny_prism_associations" / "pathway_drug_associations.csv.gz"
    if progeny.is_file() and csv_header(progeny) != [
        "pathway", "drug_id", "drug_name", "n", "pearson_r", "p_value", "fdr"
    ]:
        failures.append("progeny_prism_associations: unexpected table schema")

    bipolar = full / "bipolar_dependency_candidates" / "pair_summary.csv.gz"
    if bipolar.is_file() and "reciprocal_top_k" not in csv_header(bipolar):
        failures.append("bipolar_dependency_candidates: reciprocal flag is missing")

    true_love_root = full / "true_love_gene"
    true_love_manifest = true_love_root / "manifest.json"
    if true_love_manifest.is_file():
        manifest = load_json(true_love_manifest)
        if manifest.get("min_pair_n", 0) < 500:
            failures.append("true_love_gene: minimum shared-model coverage is below 500")
        if manifest.get("strict_mutual_rank1_pair_count", 0) <= 0:
            failures.append("true_love_gene: no strict reciprocal rank-1 pairs")
    true_love_pairs = true_love_root / "strict_mutual_rank1_pairs.csv.gz"
    if true_love_pairs.is_file() and "both_directions_fdr_pass" not in csv_header(true_love_pairs):
        failures.append("true_love_gene: bidirectional FDR flag is missing")

    synth = module_root_for("observational_synthetic_lethal_candidates") / "pair_evidence_summary.csv.gz"
    if synth.is_file() and "evidence_family_count" not in csv_header(synth):
        failures.append("observational_synthetic_lethal_candidates: evidence count is missing")

    coamp = full / "coamplification_dependency" / "MYCN__DDX1" / "ALL" / "manifest.json"
    if coamp.is_file():
        manifest = load_json(coamp)
        if manifest.get("coamplified_n") != 14 or manifest.get("source_only_n") != 13:
            failures.append("coamplification_dependency: MYCN/DDX1 smoke groups do not match 14/13")
    else:
        failures.append("coamplification_dependency: MYCN/DDX1 smoke manifest is missing")

    coamp_root = full / "coamplification_dependency"
    pair_manifest_path = coamp_root / "pair_catalog" / "manifest.json"
    screen_manifest_path = coamp_root / "exhaustive_high_confidence" / "manifest.json"
    shard_catalog_path = coamp_root / "exhaustive_high_confidence" / "shard_catalog.csv"
    for path in (
        coamp_root / "pair_catalog" / "directional_pair_catalog.csv.gz",
        coamp_root / "exhaustive_high_confidence" / "screen_pair_catalog.csv.gz",
        coamp_root / "exhaustive_high_confidence" / "significant_hits.csv.gz",
    ):
        if not path.is_file() or path.stat().st_size == 0:
            failures.append(f"coamplification_dependency: missing or empty {path.relative_to(coamp_root)}")
    if pair_manifest_path.is_file():
        pair_manifest = load_json(pair_manifest_path)
        if pair_manifest.get("status") != "complete":
            failures.append("coamplification_dependency: pair catalog is not complete")
        if pair_manifest.get("directional_pair_count") != 20_988_320:
            failures.append("coamplification_dependency: unexpected directional pair catalog size")
    else:
        failures.append("coamplification_dependency: pair catalog manifest is missing")
    if screen_manifest_path.is_file() and shard_catalog_path.is_file():
        screen_manifest = load_json(screen_manifest_path)
        with shard_catalog_path.open(encoding="utf-8-sig", newline="") as handle:
            shards = list(csv.DictReader(handle))
        shard_pairs = sum(int(row["end"]) - int(row["start"]) + 1 for row in shards)
        shard_hits = sum(int(row["retained_hit_count"]) for row in shards)
        if screen_manifest.get("status") != "complete":
            failures.append("coamplification_dependency: exhaustive screen is not complete")
        if len(shards) != 61 or any(row.get("status") not in {"complete", "skipped_existing"} for row in shards):
            failures.append("coamplification_dependency: expected 61 complete screen shards")
        if shard_pairs != screen_manifest.get("directional_pair_count"):
            failures.append("coamplification_dependency: shard pair counts do not reconcile")
        if shard_hits != screen_manifest.get("retained_significant_hit_count"):
            failures.append("coamplification_dependency: shard hit counts do not reconcile")
        expected_tests = screen_manifest.get("directional_pair_count", 0) * screen_manifest.get("target_gene_count", 0)
        if screen_manifest.get("nominal_test_count") != expected_tests:
            failures.append("coamplification_dependency: nominal test count does not reconcile")
    else:
        failures.append("coamplification_dependency: exhaustive screen manifest or shard catalog is missing")

    qa = {
        "schema_version": 1,
        "release": "26Q1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "qa_status": "PASS" if not failures else "FAIL",
        "module_count": len(modules),
        "modules": modules,
        "failures": failures,
        "scope": "TM00-derived extensions; excludes WGCNA and predictive models",
    }
    (root / "depmap-26q1-tm00-extension-catalog.json").write_text(
        json.dumps(qa, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    with (root / "depmap-26q1-tm00-extension-catalog.csv").open(
        "w", encoding="utf-8", newline=""
    ) as handle:
        writer = csv.DictWriter(handle, fieldnames=["module", "status", "path"])
        writer.writeheader()
        writer.writerows({key: row[key] for key in writer.fieldnames} for row in modules)
    print(json.dumps({"qa_status": qa["qa_status"], "module_count": len(modules), "failures": failures}, ensure_ascii=False))
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
