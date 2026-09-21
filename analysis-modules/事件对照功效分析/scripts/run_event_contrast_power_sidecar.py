#!/usr/bin/env python3
"""Event-contrast power sidecar.

Attaches to a validated event-contrast artifact. It never reruns the biological
contrast. Prospective/design MDE is reported separately from any observed
effect size copied from upstream.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
from datetime import datetime, timezone
from pathlib import Path


SCHEMA_VERSION = 1
CAPABILITY_ID = "event_contrast_power"
OPERATION_ID = "run_event_contrast_power_sidecar"


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


def two_sided_t_p(t_stat: float, df: float) -> float:
    if df <= 0:
        return 1.0
    return _betai(df / 2.0, 0.5, df / (df + t_stat * t_stat))


def t_quantile(probability: float, df: float) -> float:
    """Central Student-t quantile via bisection on the two-sided p mapping."""
    if abs(probability - 0.5) < 1e-15:
        return 0.0
    if probability < 0.5:
        return -t_quantile(1.0 - probability, df)
    target_two_sided = 2.0 * (1.0 - probability)
    lo, hi = 0.0, 1.0
    while two_sided_t_p(hi, df) > target_two_sided:
        hi *= 2.0
        if hi > 1e6:
            break
    for _ in range(80):
        mid = 0.5 * (lo + hi)
        if two_sided_t_p(mid, df) > target_two_sided:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def canonical_digest(payload: dict) -> str:
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def write_tsv(path: Path, rows: list[dict], fieldnames: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, delimiter="\t")
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def emit_run(
    output_dir: Path,
    result: dict,
    *,
    config_path: Path | None = None,
    upstream_path: Path | None = None,
    extra_outputs: list[str] | None = None,
) -> None:
    created = result.get("created_at") or datetime.now(timezone.utc).isoformat()
    result.setdefault("created_at", created)
    write_json(output_dir / "result.json", result)
    failed = bool(result.get("blocked"))
    write_json(
        output_dir / "qc.json",
        {
            "schema_version": SCHEMA_VERSION,
            "status": "fail" if failed else "pass",
            "checks": [
                {
                    "name": "typed_status",
                    "status": "fail" if failed else "pass",
                    "detail": result.get("status"),
                }
            ],
            "blocking_failures": [result.get("status")] if failed else [],
            "warnings": result.get("warnings") or [],
        },
    )
    outputs = ["run_manifest.json", "result.json", "qc.json"]
    if extra_outputs:
        outputs.extend(extra_outputs)
    write_json(
        output_dir / "run_manifest.json",
        {
            "schema_version": SCHEMA_VERSION,
            "analysis_id": CAPABILITY_ID,
            "created_at": created,
            "language": "Python",
            "entrypoint": "analysis-modules/事件对照功效分析/scripts/run_event_contrast_power_sidecar.py",
            "reference_scripts": [],
            "inputs": [
                item
                for item in [
                    {"path": str(upstream_path), "role": "upstream_event_contrast"}
                    if upstream_path
                    else None,
                    {"path": str(config_path), "role": "config"} if config_path else None,
                ]
                if item
            ],
            "parameters": {
                "operation_id": OPERATION_ID,
                "status": result.get("status"),
            },
            "software": {"python": "stdlib"},
            "outputs": outputs,
        },
    )


def blocked(status: str, reason: str, extra: dict | None = None) -> dict:
    payload = {
        "schema_version": SCHEMA_VERSION,
        "capability_id": CAPABILITY_ID,
        "operation_id": OPERATION_ID,
        "status": status,
        "blocked": True,
        "reason": reason,
        "biological_significance": False,
        "evidence_of_no_effect": False,
    }
    if extra:
        payload.update(extra)
    return payload


def mde(n_case: int, n_control: int, sigma: float, alpha: float, power: float) -> dict:
    df = n_case + n_control - 2
    se = sigma * math.sqrt(1.0 / n_case + 1.0 / n_control)
    t_crit = t_quantile(1.0 - alpha / 2.0, df)
    t_pow = t_quantile(power, df)
    delta = (t_crit + t_pow) * se
    return {
        "df": df,
        "se": se,
        "t_critical": t_crit,
        "t_power": t_pow,
        "minimum_detectable_effect": delta,
        "approximation": "central_student_t_sum_of_quantiles",
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--upstream-artifact", required=True)
    parser.add_argument("--config-json", required=True)
    parser.add_argument("--output-dir", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    table_path = output_dir / "tables" / "minimum_detectable_effect.tsv"
    config_path = Path(args.config_json)
    upstream_path = Path(args.upstream_artifact)

    def fail(payload: dict) -> int:
        emit_run(output_dir, payload, config_path=config_path if config_path.is_file() else None, upstream_path=upstream_path if upstream_path.is_file() else None)
        return 2

    if not config_path.is_file():
        return fail(blocked("CONFIG_MISSING", "config json is required"))
    config = load_json(config_path)

    if not upstream_path.is_file():
        return fail(
            blocked(
                "UPSTREAM_MISSING",
                "event-contrast artifact is required; sidecar does not run the biological contrast",
            )
        )

    upstream = load_json(upstream_path)
    expected_digest = config.get("upstream_digest")
    actual_digest = sha256_file(upstream_path)
    extra = {"upstream_path": str(upstream_path), "upstream_digest": actual_digest}

    if expected_digest and expected_digest != actual_digest:
        return fail(
            blocked(
                "STALE_UPSTREAM_DIGEST",
                "pinned upstream digest does not match the supplied artifact",
                extra,
            )
        )

    upstream_status = str(upstream.get("status", "")).upper()
    if upstream_status not in {"COMPLETE", "OK", "SUCCESS"}:
        return fail(
            blocked(
                "UPSTREAM_INELIGIBLE",
                "upstream contrast is not a completed eligible event-contrast artifact",
                extra | {"upstream_status": upstream_status or "MISSING"},
            )
        )

    cohort = upstream.get("cohort") or {}
    methods = upstream.get("methods") or {}
    n_case = int(cohort.get("n_case") or 0)
    n_control = int(cohort.get("n_control") or 0)
    tested_target_count = int(
        methods.get("tested_target_count") or config.get("tested_target_count") or 0
    )
    variance_model = str(methods.get("variance_model") or config.get("variance_model") or "pooled")
    sigma = float(
        methods.get("empirical_sigma")
        or config.get("empirical_sigma")
        or 0.0
    )
    if "alpha" in config:
        alpha = float(config["alpha"])
    else:
        alpha = float(methods.get("alpha") or 0.05)
    if "desired_power" in config:
        desired_power = float(config["desired_power"])
    else:
        desired_power = 0.8
    fdr_policy = str(config.get("fdr_policy") or methods.get("fdr_policy") or "BH")
    effect_scale = str(config.get("effect_scale") or "gene_effect_delta")
    direction = str(config.get("direction") or methods.get("direction") or "two_sided")

    if direction != "two_sided":
        return fail(
            blocked(
                "UNSUPPORTED_DIRECTION",
                "sidecar computes two-sided pooled-t MDE only",
                extra | {"direction": direction},
            )
        )
    if variance_model.lower() not in {"pooled", "pooled-variance", "equal_variance"}:
        return fail(
            blocked(
                "UNEQUAL_VARIANCE_POLICY_UNSUPPORTED",
                "sidecar computes pooled-variance design MDE only",
                extra | {"variance_model": variance_model},
            )
        )
    if n_case < 2 or n_control < 2:
        return fail(
            blocked(
                "TINY_GROUPS",
                "case and control counts must each be at least 2 for a two-sample t design",
                extra | {"n_case": n_case, "n_control": n_control},
            )
        )
    if not (0.0 < alpha < 1.0):
        return fail(blocked("INVALID_ALPHA", "alpha must be in (0, 1)", extra))
    if not (0.0 < desired_power < 1.0):
        return fail(blocked("INVALID_POWER", "desired_power must be in (0, 1)", extra))
    if sigma <= 0.0:
        return fail(
            blocked(
                "ZERO_VARIANCE",
                "an empirical or declared positive sigma is required; zero variance cannot yield an MDE",
                extra,
            )
        )
    if tested_target_count < 1:
        return fail(
            blocked(
                "INVALID_TARGET_COUNT",
                "tested_target_count must be a positive integer",
                extra,
            )
        )

    unadjusted = mde(n_case, n_control, sigma, alpha, desired_power)
    bonferroni_alpha = alpha / tested_target_count
    bonferroni = mde(n_case, n_control, sigma, bonferroni_alpha, desired_power)

    observed = (upstream.get("observations") or {}).get("observed_effect")
    rows = [
        {
            "scenario": "unadjusted_alpha",
            "kind": "prospective_design",
            "alpha": f"{alpha:.12g}",
            "power": f"{desired_power:.12g}",
            "mde": f"{unadjusted['minimum_detectable_effect']:.12g}",
            "df": unadjusted["df"],
            "multiplicity": "none",
        },
        {
            "scenario": "bonferroni",
            "kind": "prospective_design",
            "alpha": f"{bonferroni_alpha:.12g}",
            "power": f"{desired_power:.12g}",
            "mde": f"{bonferroni['minimum_detectable_effect']:.12g}",
            "df": bonferroni["df"],
            "multiplicity": "Bonferroni",
        },
    ]
    if observed is not None:
        rows.append(
            {
                "scenario": "observed_effect_description",
                "kind": "post_hoc_description",
                "alpha": "",
                "power": "",
                "mde": str(observed),
                "df": unadjusted["df"],
                "multiplicity": "not_a_power_claim",
            }
        )

    write_tsv(
        table_path,
        rows,
        ["scenario", "kind", "alpha", "power", "mde", "df", "multiplicity"],
    )

    result = {
        "schema_version": SCHEMA_VERSION,
        "capability_id": CAPABILITY_ID,
        "operation_id": OPERATION_ID,
        "status": "COMPLETE",
        "blocked": False,
        "question": "What gene-effect delta is detectable at the declared power for this completed event contrast?",
        "targets": [],
        "cohort": {
            "n_case": n_case,
            "n_control": n_control,
            "tested_target_count": tested_target_count,
        },
        "methods": {
            "variance_model": "pooled",
            "degrees_of_freedom": unadjusted["df"],
            "alpha": alpha,
            "desired_power": desired_power,
            "upstream_fdr_policy": fdr_policy,
            "multiplicity_for_mde": "Bonferroni for family-wide design; BH has no fixed per-target alpha",
            "effect_scale": effect_scale,
            "direction": "two_sided",
            "approximation": unadjusted["approximation"],
            "empirical_sigma": sigma,
        },
        "observations": {
            "unadjusted_mde": unadjusted["minimum_detectable_effect"],
            "bonferroni_mde": bonferroni["minimum_detectable_effect"],
            "observed_effect": observed,
            "observed_effect_is_not_power": True,
        },
        "tables": [{"path": "tables/minimum_detectable_effect.tsv"}],
        "figures": [],
        "warnings": [
            "This sidecar is not biological significance and is not evidence of no effect.",
            "BH does not yield a single per-target design alpha; Bonferroni is the multiplicity-adjusted MDE scenario.",
        ],
        "upstream_digest": actual_digest,
        "biological_significance": False,
        "evidence_of_no_effect": False,
        "created_at": datetime.now(timezone.utc).isoformat(),
    }
    result["digest"] = canonical_digest(
        {
            "cohort": result["cohort"],
            "methods": result["methods"],
            "observations": {
                k: result["observations"][k]
                for k in ("unadjusted_mde", "bonferroni_mde")
            },
            "upstream_digest": actual_digest,
        }
    )
    emit_run(
        output_dir,
        result,
        config_path=config_path,
        upstream_path=upstream_path,
        extra_outputs=["tables/minimum_detectable_effect.tsv"],
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
