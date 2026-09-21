#!/usr/bin/env python3
"""Allele-specific mutation vs Chronos Gene Effect contrast.

Eligibility is decided before any target scan. Ineligible alleles emit no
association numbers. Pan-cancer and lineage scopes are distinct FDR families.
Controls are mutation-profiled gene-negative models only.
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


def unusable_protein_change(change: str) -> bool:
    return not change or "?" in change or "/" in change


def truthy(value: str | None) -> bool:
    if value is None or value == "":
        return True
    return value.strip().lower() in {"1", "true", "t", "yes", "y"}


def mean(values: list[float]) -> float:
    return sum(values) / len(values)


def variance(values: list[float]) -> float:
    if len(values) < 2:
        return 0.0
    return statistics.variance(values)


def _betacf(a: float, b: float, x: float, max_iter: int = 200, eps: float = 3e-12) -> float:
    qab, qap, qam = a + b, a + 1.0, a - 1.0
    am = bm = az = 1.0
    bz = 1.0 - qab * x / qap
    for m in range(1, max_iter + 1):
        em = float(m)
        tem = em + em
        d = em * (b - em) * x / ((qam + tem) * (a + tem))
        ap = az + d * am
        bp = bz + d * bm
        d = -(a + em) * (qab + em) * x / ((a + tem) * (qap + tem))
        app = ap + d * az
        bpp = bp + d * bz
        aold = az
        am, bm, az, bz = ap / bpp, bp / bpp, app / bpp, 1.0
        if abs(az - aold) < eps * abs(az):
            return az
    return az


def _betai(a: float, b: float, x: float) -> float:
    if x <= 0.0:
        return 0.0
    if x >= 1.0:
        return 1.0
    lbeta = math.lgamma(a) + math.lgamma(b) - math.lgamma(a + b)
    front = math.exp(a * math.log(x) + b * math.log(1.0 - x) - lbeta)
    if x < (a + 1.0) / (a + b + 2.0):
        return front * _betacf(a, b, x) / a
    return 1.0 - front * _betacf(b, a, 1.0 - x) / b


def pooled_t_p(a: list[float], b: list[float]) -> float:
    n1, n2 = len(a), len(b)
    df = n1 + n2 - 2
    if df <= 0:
        return 1.0
    m1, m2 = mean(a), mean(b)
    v1, v2 = variance(a), variance(b)
    pooled = ((n1 - 1) * v1 + (n2 - 1) * v2) / df
    se = math.sqrt(pooled * (1.0 / n1 + 1.0 / n2)) if pooled > 0 else 0.0
    delta = m1 - m2
    if se == 0:
        return 1.0 if delta == 0 else 0.0
    t_stat = delta / se
    return _betai(df / 2.0, 0.5, df / (df + t_stat * t_stat))


def bh(pvalues: list[float]) -> list[float]:
    m = len(pvalues)
    order = sorted(range(m), key=lambda i: pvalues[i])
    q = [1.0] * m
    running = 1.0
    for rank in range(m, 0, -1):
        i = order[rank - 1]
        running = min(running, pvalues[i] * m / rank)
        q[i] = min(1.0, running)
    return q


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mutation-long-csv", required=True)
    parser.add_argument("--gene-effect-csv", required=True)
    parser.add_argument("--model-csv", required=True)
    parser.add_argument("--coverage-matrix-csv", required=True)
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


def load_coverage(path: Path) -> set[str]:
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        key = reader.fieldnames[0]
        return {row[key] for row in reader}


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


def classify_calls(path: Path, gene: str, allele: str) -> dict[str, str]:
    assignments: dict[str, set[str]] = {}
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        for row in reader:
            if row["HugoSymbol"].strip() != gene:
                continue
            if "IsDefaultEntryForModel" in row and not truthy(row["IsDefaultEntryForModel"]):
                continue
            change = normalize_protein_change(row.get("ProteinChange", ""))
            if unusable_protein_change(change):
                continue
            assignments.setdefault(row["ModelID"], set()).add(change)
    policy: dict[str, str] = {}
    for model_id, changes in assignments.items():
        if len(changes) > 1:
            policy[model_id] = "multi_allelic"
        elif allele in changes:
            policy[model_id] = "allele_positive"
        else:
            policy[model_id] = "other_variant"
    return policy


def contract_result(status: str, args: argparse.Namespace, extra: dict) -> dict:
    allele = normalize_protein_change(args.protein_change)
    payload = {
        "schema_version": 1,
        "status": status,
        "question": "Which CRISPR Gene Effect targets differ between one protein-change allele and gene-mutation-negative models?",
        "targets": [args.gene, allele],
        "cohort": {
            "requested": args.lineage or args.scope,
            "n_before": extra.get("profiled_n", 0),
            "n_after": extra.get("case_n", 0) + extra.get("control_n", 0),
        },
        "methods": [
            "two-sided pooled-variance independent t-test",
            "BH within scope family",
        ],
        "observations": extra.get("observations", []),
        "tables": extra.get("tables", []),
        "figures": [],
        "warnings": extra.get("warnings", []),
        "analysis_family": "allele_specific_mutation_dependency",
        "gene": args.gene,
        "protein_change": allele,
        "scope": args.scope,
        "lineage": args.lineage or None,
        "new_analysis_started": False,
    }
    payload.update({key: value for key, value in extra.items() if key not in payload})
    return payload


def emit_ineligible(output: Path, reason: str, counts: dict, args: argparse.Namespace) -> None:
    output.mkdir(parents=True, exist_ok=True)
    result = contract_result(
        "INELIGIBLE",
        args,
        {**counts, "reason": reason, "warnings": [reason]},
    )
    write_json(output / "result.json", result)
    write_json(
        output / "qc.json",
        {
            "schema_version": 1,
            "status": "ineligible",
            "checks": [{"name": "eligibility", "status": "fail", "detail": reason}],
            "blocking_failures": [reason],
            "warnings": [],
        },
    )
    write_json(output / "coverage.json", {"state": "ineligible", "reason": reason})
    write_json(
        output / "run_manifest.json",
        {
            "schema_version": 1,
            "analysis_id": "allele_specific_mutation_dependency",
            "status": "INELIGIBLE",
            "dataset_release": args.release,
            "language": "Python",
            "entrypoint": Path(__file__).name,
            "created_at": datetime.now(timezone.utc).isoformat(),
            "result_digest": hashlib.sha256(json.dumps(result, sort_keys=True).encode()).hexdigest(),
            "inputs": [],
            "parameters": {"gene": args.gene, "protein_change": args.protein_change},
            "software": {"method": "pooled_t_bh"},
            "outputs": ["result.json", "qc.json", "coverage.json"],
        },
    )


def main() -> int:
    args = parse_args()
    allele = normalize_protein_change(args.protein_change)
    output = Path(args.output_root)
    if unusable_protein_change(allele):
        emit_ineligible(output, "ambiguous_variant", {"case_n": 0, "control_n": 0}, args)
        return 0
    models = load_models(Path(args.model_csv))
    if args.scope == "lineage":
        if not args.lineage:
            emit_ineligible(output, "ambiguous_variant", {"case_n": 0, "control_n": 0}, args)
            return 0
        models = {mid: lineage for mid, lineage in models.items() if lineage == args.lineage}
    policy = classify_calls(Path(args.mutation_long_csv), args.gene.strip(), allele)
    targets, effects = load_effect(Path(args.gene_effect_csv))
    profiled = load_coverage(Path(args.coverage_matrix_csv))
    cohort = [mid for mid in models if mid in effects]
    cases = [mid for mid in cohort if mid in profiled and policy.get(mid) == "allele_positive"]
    controls = [mid for mid in cohort if mid in profiled and mid not in policy]
    missing_n = sum(1 for mid in cohort if mid not in profiled)
    other_n = sum(1 for mid in cohort if policy.get(mid) == "other_variant")
    multi_n = sum(1 for mid in cohort if policy.get(mid) == "multi_allelic")
    counts = {
        "case_n": len(cases),
        "control_n": len(controls),
        "other_variant_n": other_n,
        "multi_allelic_n": multi_n,
        "missing_n": missing_n,
        "profiled_n": sum(1 for mid in cohort if mid in profiled),
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
        p_raw = pooled_t_p(case_values, control_values)
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

    result = contract_result(
        "ok",
        args,
        {
            **counts,
            "mapping_source": args.mapping_source,
            "tested_n": len(tested),
            "retained_n": sum(1 for row in rows if row["status"] == "RETAINED"),
            "fdr_family": f"{args.scope}:{args.lineage or 'pancancer'}",
            "tables": [{"path": "all_targets.csv.gz", "role": "full_ranked_universe"}],
            "observations": [
                {
                    "comparison": "allele_positive_minus_gene_mutation_negative",
                    "test": "two-sided pooled-variance t-test",
                    "direction": "negative delta means stronger Chronos dependency in allele-positive models",
                }
            ],
        },
    )
    write_json(output / "result.json", result)
    write_json(
        output / "qc.json",
        {
            "schema_version": 1,
            "status": "pass",
            "checks": [
                {"name": "release_identified", "status": "pass", "detail": args.release},
                {"name": "effect_direction_declared", "status": "pass", "detail": "mut_minus_control Chronos"},
                {"name": "coverage_controls", "status": "pass", "detail": "unprofiled models excluded from WT"},
            ],
            "blocking_failures": [],
            "warnings": [],
        },
    )
    write_json(output / "coverage.json", {"state": "validated", "promotion": "not_requested"})
    write_json(
        output / "run_manifest.json",
        {
            "schema_version": 1,
            "analysis_id": "allele_specific_mutation_dependency",
            "status": "ok",
            "dataset_release": args.release,
            "language": "Python",
            "entrypoint": Path(__file__).name,
            "created_at": datetime.now(timezone.utc).isoformat(),
            "event_definition": {
                "gene": args.gene,
                "protein_change": allele,
                "mapping_source": args.mapping_source,
            },
            "inputs": [
                {"path": str(args.mutation_long_csv), "role": "somatic_mutations", "sha256": sha256_file(Path(args.mutation_long_csv))},
                {"path": str(args.gene_effect_csv), "role": "gene_effect", "sha256": sha256_file(Path(args.gene_effect_csv))},
                {"path": str(args.model_csv), "role": "model", "sha256": sha256_file(Path(args.model_csv))},
                {"path": str(args.coverage_matrix_csv), "role": "mutation_coverage", "sha256": sha256_file(Path(args.coverage_matrix_csv))},
            ],
            "parameters": {"scope": args.scope, "min_case_n": args.min_case_n, "min_control_n": args.min_control_n},
            "software": {"method": "pooled_t_bh"},
            "outputs": ["result.json", "qc.json", "coverage.json", "all_targets.csv.gz"],
            "result_digest": sha256_file(table),
        },
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
