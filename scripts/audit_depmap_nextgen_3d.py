#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path

import pandas as pd


REQUIRED = {
    "screen_metadata.csv": (1592, 17),
    "model_metadata.csv": (1999, 16),
    "screen_gene_effect.csv": (1534, 18436),
    "screen_gene_dependency.csv": (1534, 18436),
    "crispr_naive_gene_score.csv": (1534, 18436),
    "next_gen_expression.csv": (309, 19216),
    "next_gen_copy_number.csv": (291, 19956),
    "next_gen_damaging.csv": (311, 19617),
    "next_gen_hotspot.csv": (311, 538),
}


def shape(path: Path) -> tuple[int, int]:
    with path.open("r", encoding="utf-8-sig", errors="replace", newline="") as handle:
        reader = csv.reader(handle)
        header = next(reader)
        return sum(1 for _ in reader), len(header)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--data-root", required=True)
    parser.add_argument("--output-root", required=True)
    args = parser.parse_args()
    root = Path(args.data_root).resolve()
    output = Path(args.output_root).resolve()
    output.mkdir(parents=True, exist_ok=True)
    failures: list[str] = []
    files: list[dict] = []
    for name, expected in REQUIRED.items():
        path = root / name
        if not path.is_file():
            failures.append(f"missing {name}")
            continue
        observed = shape(path)
        if observed != expected:
            failures.append(f"{name}: expected {expected}, observed {observed}")
        files.append({"name": name, "bytes": path.stat().st_size,
                      "rows": observed[0], "columns": observed[1], "sha256": sha256(path)})

    metadata = pd.read_csv(root / "screen_metadata.csv")
    passed = metadata[metadata["PassesQC"].eq(True)].copy()
    counts = passed["ScreenType"].value_counts().to_dict()
    expected_counts = {"2DS": 1387, "2DO": 14, "3DO": 94, "2DN": 25, "3DN": 14}
    if counts != expected_counts:
        failures.append(f"unexpected QC-passing screen counts: {counts}")
    with (root / "screen_gene_effect.csv").open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.reader(handle)
        next(reader)
        effect_ids = {row[0] for row in reader}
    if effect_ids != set(passed["ScreenID"].astype(str)):
        failures.append("screen_gene_effect row IDs do not exactly match QC-passing metadata")

    catalog = {
        "schema_version": 1,
        "release": "NextGen Model Manuscript 2026",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "status": "complete" if not failures else "failed",
        "qa_status": "PASS" if not failures else "FAIL",
        "screen_type_counts": counts,
        "traditional_2d_screen_count": counts.get("2DS", 0),
        "nextgen_2d_screen_count": counts.get("2DO", 0) + counts.get("2DN", 0),
        "nextgen_3d_screen_count": counts.get("3DO", 0) + counts.get("3DN", 0),
        "screen_gene_count": REQUIRED["screen_gene_effect.csv"][1] - 1,
        "files": files,
        "failures": failures,
        "design_boundary": "CRISPR 2D and 3D ScreenTypes do not share ModelIDs; use covariate-adjusted unpaired comparisons. CNS expression includes nine explicit 2D/3D pairs.",
    }
    (output / "catalog.json").write_text(json.dumps(catalog, indent=2), encoding="utf-8")
    pd.DataFrame(files).to_csv(output / "file_catalog.csv", index=False)
    print(json.dumps({"qa_status": catalog["qa_status"], "failures": failures}))
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
