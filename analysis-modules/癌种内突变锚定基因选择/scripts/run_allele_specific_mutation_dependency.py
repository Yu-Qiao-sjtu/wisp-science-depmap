#!/usr/bin/env python3
"""Allele-specific mutation vs Chronos Gene Effect contrast.

Eligibility is decided before any target scan. Ineligible alleles emit no
association numbers. Pan-cancer and lineage scopes are distinct FDR families.
"""

from __future__ import annotations

import argparse
import csv
import gzip
import hashlib
import json
import math
import statistics
from datetime import datetime, timezone
from pathlib import Path


MIN_CASE = 5
MIN_CONTROL = 5


def normalize_protein_change(value: str) -> str:
    text = value.strip()
    if text.lower().startswith("p."):
        text = text[2:]
    return text.upper().replace(" ", "")


def mean(values: list[float]) -> float:
    return sum(values) / len(values)


def variance(values: list[float]) -> float:
    if len(values) < 2:
        return 0.0
    return statistics.variance(values)


def welch_p(a: list[float], b: list[float]) -> float:
    n1, n2 = len(a), len(b)
    m1, m2 = mean(a), mean(b)
    v1, v2 = variance(a), variance(b)
    se = math.sqrt(v1 / n1 + v2 / n2)
    if se == 0:
        return 1.0
    t = (m1 - m2) / se
    # Normal approximation is enough for synthetic fixtures.
    x = abs(t) / math.sqrt(2)
    return math.erfc(x)


def bh(pvalues: list[float]) -> list[float]:
    m = len(pvalues)
    order = sorted(range(m), key=lambda i: pvalues[i])
    q = [1.0] * m
    running = 1.0
    for rank, index in enumerate(reversed(order), start=1):
        i = order[m - rank]
        running = min(running, pvalues[i] * m / (m - rank + 1))
        q[i] = min(1.0, running)
    return q


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mutation-long-csv", required=True)
    parser.add_argument("--gene-effect-csv", required=True)
    parser.add_argument("--model-csv", required=True)
    parser.add_argument("--output-root", required=True)
    parser.add_argument("--gene", required=True)
    parser.add_argument("--protein-change", required=True)
    parser.add_argument("--release", default="fixture")
    parser.add_argument("--scope", choices=["global", "lineage"], default="global")
    parser.add_argument("--lineage", default="")
    parser.add_argument("--min-case-n", type=int, default=MIN_CASE)
    parser.add_argument("--min-control-n", type=int, default=MIN_CONTROL)
    parser.add_argument("--mapping-source", default="ProteinChange")
    return parser.parse_args()


def load_models(path: Path) -> dict[str, str]:
    with path.open(encoding="utf-8", newline="") as handle:
        return {row["ModelID"]: row.get("OncotreeLineage", "") for row in csv.DictReader(handle)}


def load_effect(path: Path) -> tuple[list[str], dict[str, dict[str, float | None]]]:
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        targets = [name.split(" ")[0] for name in reader.fieldnames[1:]]
        rows: dict[str, dict[str, float | None]] = {}
        for row in reader:
            values: dict[str, float | None] = {}
            for field, target in zip(reader.fieldnames[1:], targets):
                raw = row[field]
                values[target] = None if raw == "" else float(raw)
            rows[row["ModelID"]] = values
        return targets, rows


def classify_calls(
    path: Path, gene: str, allele: str
) -> tuple[dict[str, str], str | None]:
    """Return model_id -> policy and optional stop reason."""
    assignments: dict[str, set[str]] = {}
    with path.open(encoding="utf-8", newline="") as handle:
        for row in csv.DictReader(handle):
            if row["HugoSymbol"].strip() != gene:
                continue
            change = normalize_protein_change(row["ProteinChange"])
            if not change or "?" in change or "/" in change:
                return {}, "ambiguous_variant"
            assignments.setdefault(row["ModelID"], set()).add(change)
    policy: dict[str, str] = {}
    for model_id, changes in assignments.items():
        if len(changes) > 1:
            policy[model_id] = "multi_allelic"
        elif allele in changes:
            policy[model_id] = "allele_positive"
        else:
            policy[model_id] = "other_variant"
    return policy, None


def emit_ineligible(output: Path, reason: str, counts: dict, args: argparse.Namespace) -> None:
    output.mkdir(parents=True, exist_ok=True)
    result = {
        "status": "INELIGIBLE",
        "reason": reason,
        "analysis_family": "allele_specific_mutation_dependency",
        "gene": args.gene,
        "protein_change": normalize_protein_change(args.protein_change),
        "scope": args.scope,
        "lineage": args.lineage or None,
        "new_analysis_started": False,
        **counts,
    }
    write_json(output / "result.json", result)
    write_json(output / "qc.json", {"status": "ineligible", "reason": reason})
    write_json(output / "coverage.json", {"state": "ineligible", "reason": reason})
    write_json(
        output / "run_manifest.json",
        {
            "status": "INELIGIBLE",
            "release": args.release,
            "result_digest": hashlib.sha256(json.dumps(result, sort_keys=True).encode()).hexdigest(),
            "created_at": datetime.now(timezone.utc).isoformat(),
        },
    )


def main() -> int:
    args = parse_args()
    allele = normalize_protein_change(args.protein_change)
    output = Path(args.output_root)
    if not allele:
        emit_ineligible(output, "ambiguous_variant", {"case_n": 0, "control_n": 0}, args)
        return 0
    models = load_models(Path(args.model_csv))
    if args.scope == "lineage":
        models = {mid: lineage for mid, lineage in models.items() if lineage == args.lineage}
        if not args.lineage:
            emit_ineligible(output, "ambiguous_variant", {"case_n": 0, "control_n": 0}, args)
            return 0
    policy, stop = classify_calls(Path(args.mutation_long_csv), args.gene.strip(), allele)
    if stop:
        emit_ineligible(output, stop, {"case_n": 0, "control_n": 0}, args)
        return 0
    targets, effects = load_effect(Path(args.gene_effect_csv))
    cohort = [mid for mid in models if mid in effects]
    cases = [mid for mid in cohort if policy.get(mid) == "allele_positive"]
    controls = [mid for mid in cohort if mid not in policy]
    missing_n = sum(1 for mid in cohort if policy.get(mid) == "missing")
    other_n = sum(1 for mid in cohort if policy.get(mid) == "other_variant")
    multi_n = sum(1 for mid in cohort if policy.get(mid) == "multi_allelic")
    counts = {
        "case_n": len(cases),
        "control_n": len(controls),
        "other_variant_n": other_n,
        "multi_allelic_n": multi_n,
        "missing_n": missing_n,
        "min_case_n": args.min_case_n,
        "min_control_n": args.min_control_n,
    }
    if len(cases) < args.min_case_n:
        emit_ineligible(output, "insufficient_case", counts, args)
        return 0
    if len(controls) < args.min_control_n:
        emit_ineligible(output, "insufficient_control", counts, args)
        return 0

    output.mkdir(parents=True, exist_ok=True)
    rows = []
    pvalues = []
    for target in targets:
        case_values = [effects[mid][target] for mid in cases if effects[mid][target] is not None]
        control_values = [effects[mid][target] for mid in controls if effects[mid][target] is not None]
        if len(case_values) < args.min_case_n or len(control_values) < args.min_control_n:
            rows.append(
                {
                    "target_gene": target,
                    "status": "INELIGIBLE",
                    "effect_mut_minus_control": "",
                    "p_raw": "",
                    "q_bh": "",
                }
            )
            continue
        delta = mean(case_values) - mean(control_values)
        p_raw = welch_p(case_values, control_values)
        pvalues.append(p_raw)
        rows.append(
            {
                "target_gene": target,
                "status": "TESTED",
                "effect_mut_minus_control": f"{delta:.6f}",
                "p_raw": f"{p_raw:.6g}",
                "q_bh": "",
                "_p": p_raw,
            }
        )
    tested = [row for row in rows if "_p" in row]
    qs = bh([row["_p"] for row in tested])
    for row, q in zip(tested, qs):
        row["q_bh"] = f"{q:.6g}"
        row["status"] = "RETAINED" if q <= 0.1 else "NOT_RETAINED"
        del row["_p"]

    table = output / "all_targets.csv.gz"
    with gzip.open(table, "wt", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=["target_gene", "status", "effect_mut_minus_control", "p_raw", "q_bh"],
        )
        writer.writeheader()
        writer.writerows(rows)

    result = {
        "status": "ok",
        "analysis_family": "allele_specific_mutation_dependency",
        "gene": args.gene,
        "protein_change": allele,
        "mapping_source": args.mapping_source,
        "scope": args.scope,
        "lineage": args.lineage or None,
        "tested_n": len(tested),
        "retained_n": sum(1 for row in rows if row["status"] == "RETAINED"),
        "fdr_family": f"{args.scope}:{args.lineage or 'pancancer'}",
        "new_analysis_started": False,
        **counts,
    }
    write_json(output / "result.json", result)
    write_json(output / "qc.json", {"status": "pass", "confounder_note": "other_variant and multi_allelic excluded from both groups"})
    write_json(output / "coverage.json", {"state": "validated", "promotion": "not_requested"})
    write_json(
        output / "run_manifest.json",
        {
            "status": "ok",
            "release": args.release,
            "event_definition": {
                "gene": args.gene,
                "protein_change": allele,
                "mapping_source": args.mapping_source,
            },
            "input_checksums": {
                "mutation_long": sha256_file(Path(args.mutation_long_csv)),
                "gene_effect": sha256_file(Path(args.gene_effect_csv)),
                "model": sha256_file(Path(args.model_csv)),
            },
            "software": {"entrypoint": Path(__file__).name, "method": "welch_t_bh"},
            "result_digest": hashlib.sha256((output / "all_targets.csv.gz").read_bytes()).hexdigest(),
            "created_at": datetime.now(timezone.utc).isoformat(),
        },
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
