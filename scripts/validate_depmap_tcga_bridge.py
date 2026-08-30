"""Validate a published DepMap–TCGA expression-survival bridge."""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path

import pyarrow.compute as compute
import pyarrow.parquet as parquet


ENDPOINTS = ("os", "dss", "dfi", "pfi")
BASE_COLUMNS = {
    "symbol",
    "tcga_project",
    "depmap_lineage",
    "expression_available",
    "expression_mapping_basis",
    "expression_n",
    "expression_median_log2_tpm",
}


def validate(root: Path) -> dict[str, object]:
    qa_path = root / "qa.json"
    catalog_path = root / "project_catalog.csv"
    qa = json.loads(qa_path.read_text(encoding="utf-8-sig"))
    with catalog_path.open(encoding="utf-8-sig", newline="") as handle:
        catalog = list(csv.DictReader(handle))
    expected_rows = int(qa["target_gene_count"])
    if qa.get("status") != "PASS":
        raise ValueError("qa.json status is not PASS")
    if len(catalog) != int(qa["project_count"]):
        raise ValueError("project catalog count does not match qa.json")

    required_columns = set(BASE_COLUMNS)
    for endpoint in ENDPOINTS:
        required_columns.update(
            {
                f"expression_{endpoint}_n",
                f"expression_{endpoint}_events",
                f"expression_{endpoint}_score_z",
                f"expression_{endpoint}_p_value",
                f"expression_{endpoint}_fdr",
            }
        )

    matched_gene_counts: list[int] = []
    primary_sample_counts: list[int] = []
    for item in catalog:
        project = item["tcga_project"]
        project_root = root / "projects" / project
        manifest = json.loads(
            (project_root / "manifest.json").read_text(encoding="utf-8-sig")
        )
        if manifest.get("status") != "complete":
            raise ValueError(f"{project}: manifest is not complete")
        path = project_root / "gene_associations.parquet"
        file = parquet.ParquetFile(path)
        if file.metadata.num_rows != expected_rows:
            raise ValueError(
                f"{project}: expected {expected_rows} rows, found {file.metadata.num_rows}"
            )
        missing = required_columns - set(file.schema.names)
        if missing:
            raise ValueError(f"{project}: missing columns {sorted(missing)}")
        identity = parquet.read_table(path, columns=["symbol", "tcga_project"])
        if compute.count_distinct(identity["symbol"]).as_py() != expected_rows:
            raise ValueError(f"{project}: symbols are not unique")
        projects = set(identity["tcga_project"].to_pylist())
        if projects != {project}:
            raise ValueError(f"{project}: project column contains {sorted(projects)}")
        matched_gene_counts.append(int(manifest["expression_matched_gene_count"]))
        primary_sample_counts.append(int(manifest["expression_primary_tumour_n"]))

    return {
        "status": "PASS",
        "release": qa.get("release"),
        "project_count": len(catalog),
        "target_gene_count": expected_rows,
        "total_project_gene_rows": len(catalog) * expected_rows,
        "min_expression_matched_gene_count": min(matched_gene_counts),
        "max_expression_matched_gene_count": max(matched_gene_counts),
        "min_primary_sample_count": min(primary_sample_counts),
        "max_primary_sample_count": max(primary_sample_counts),
        "endpoints": [endpoint.upper() for endpoint in ENDPOINTS],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    print(json.dumps(validate(args.root), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
