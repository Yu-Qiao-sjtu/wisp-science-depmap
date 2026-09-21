#!/usr/bin/env python3
"""Feature-conditioned pathway enrichment on a complete ranked universe.

Consumes a validated feature-to-dependency contrast artifact. Does not rebuild
the contrast, does not use PROGENy activity tables, and keeps stronger,
weaker, and unsigned hypotheses in separate FDR families.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import random
from datetime import datetime, timezone
from pathlib import Path


SCHEMA_VERSION = 1
CAPABILITY_ID = "feature_conditioned_pathway_enrichment"
OPERATION_ID = "run_feature_conditioned_pathway_enrichment"
HYPOTHESIS_SEED = {
    "stronger_dependency": 0,
    "weaker_dependency": 11,
    "unsigned_crosstalk": 23,
}


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
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def emit_run(output_dir: Path, result: dict, extra_outputs: list[str] | None = None) -> None:
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
            "entrypoint": "analysis-modules/特征条件通路富集/scripts/run_feature_conditioned_pathway_enrichment.py",
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
    }
    if extra:
        payload.update(extra)
    return payload


def load_ranked_universe(path: Path) -> list[tuple[str, float]]:
    rows: list[tuple[str, float]] = []
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        if reader.fieldnames is None:
            return rows
        fields = {name.lower(): name for name in reader.fieldnames}
        gene_key = fields.get("gene") or fields.get("symbol") or fields.get("id")
        stat_key = (
            fields.get("statistic")
            or fields.get("rank_statistic")
            or fields.get("delta")
            or fields.get("t")
        )
        if gene_key is None or stat_key is None:
            return rows
        for row in reader:
            gene = row[gene_key].strip()
            if not gene:
                continue
            rows.append((gene, float(row[stat_key])))
    return rows


def load_gmt(path: Path) -> dict[str, set[str]]:
    sets: dict[str, set[str]] = {}
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            parts = line.strip().split("\t")
            if len(parts) < 3:
                continue
            sets[parts[0]] = {gene for gene in parts[2:] if gene}
    return sets


def running_enrichment_score(ranked_genes: list[str], gene_set: set[str]) -> tuple[float, list[str]] | None:
    n = len(ranked_genes)
    hits = [1 if gene in gene_set else 0 for gene in ranked_genes]
    nh = sum(hits)
    if nh == 0 or nh == n:
        return None
    miss_weight = 1.0 / (n - nh)
    hit_weight = 1.0 / nh
    running = 0.0
    best = 0.0
    leading: list[str] = []
    current_lead: list[str] = []
    for i, gene in enumerate(ranked_genes):
        if hits[i]:
            running += hit_weight
            current_lead.append(gene)
        else:
            running -= miss_weight
        if running > best:
            best = running
            leading = list(current_lead)
    return best, leading


def permutation_p(observed: float, ranked_genes: list[str], gene_set: set[str], permutations: int, seed: int) -> float:
    rng = random.Random(seed)
    labels = list(ranked_genes)
    extreme = 0
    size = len(gene_set & set(ranked_genes))
    if size == 0 or size == len(ranked_genes):
        return 1.0
    for _ in range(permutations):
        rng.shuffle(labels)
        fake_set = set(labels[:size])
        scored = running_enrichment_score(ranked_genes, fake_set)
        if scored is None:
            continue
        score, _ = scored
        if score >= observed - 1e-15:
            extreme += 1
    return (extreme + 1) / (permutations + 1)


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


def order_genes(rows: list[tuple[str, float]], hypothesis: str) -> list[str]:
    if hypothesis == "stronger_dependency":
        keyed = sorted(rows, key=lambda item: item[1])
    elif hypothesis == "weaker_dependency":
        keyed = sorted(rows, key=lambda item: item[1], reverse=True)
    elif hypothesis == "unsigned_crosstalk":
        keyed = sorted(rows, key=lambda item: abs(item[1]), reverse=True)
    else:
        raise ValueError(hypothesis)
    return [gene for gene, _ in keyed]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--upstream-artifact", required=True)
    parser.add_argument("--ranked-universe-tsv", required=True)
    parser.add_argument("--gmt", required=True)
    parser.add_argument("--config-json", required=True)
    parser.add_argument("--output-dir", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    config = json.loads(Path(args.config_json).read_text(encoding="utf-8"))
    upstream_path = Path(args.upstream_artifact)
    universe_path = Path(args.ranked_universe_tsv)
    gmt_path = Path(args.gmt)

    if not upstream_path.is_file():
        emit_run(output_dir, blocked("UPSTREAM_MISSING", "feature-to-dependency artifact is required"))
        return 2
    upstream = json.loads(upstream_path.read_text(encoding="utf-8"))
    actual_digest = sha256_file(upstream_path)
    if config.get("upstream_digest") and config["upstream_digest"] != actual_digest:
        emit_run(
            output_dir,
            blocked("STALE_UPSTREAM_DIGEST", "pinned upstream digest does not match"),
        )
        return 2
    if str(upstream.get("status", "")).upper() not in {"COMPLETE", "OK", "SUCCESS"}:
        emit_run(
            output_dir,
            blocked("UPSTREAM_INELIGIBLE", "upstream contrast is not a completed artifact"),
        )
        return 2
    if upstream.get("retained_only") or config.get("retained_only"):
        emit_run(
            output_dir,
            blocked("RETAINED_ONLY_INPUT", "method requires a complete ranked universe, not retained hits"),
        )
        return 2

    rows = load_ranked_universe(universe_path)
    if not rows:
        emit_run(output_dir, blocked("WRONG_UNIVERSE", "ranked universe is empty or missing gene/statistic columns"))
        return 2
    genes = [gene for gene, _ in rows]
    if len(genes) != len(set(genes)):
        emit_run(output_dir, blocked("DUPLICATE_MAPPINGS", "ranked universe has duplicate gene identifiers"))
        return 2
    expected_n = config.get("universe_size")
    if expected_n is not None and int(expected_n) != len(rows):
        emit_run(output_dir, blocked("WRONG_UNIVERSE", "ranked universe size does not match the pinned universe"))
        return 2
    universe_digest = sha256_file(universe_path)
    config_digest = config.get("ranked_universe_digest")
    upstream_universe_digest = upstream.get("ranked_universe_digest")
    if config_digest and upstream_universe_digest and config_digest != upstream_universe_digest:
        emit_run(output_dir, blocked("UNIVERSE_DIGEST_MISMATCH", "config and upstream ranked-universe digests disagree"))
        return 2
    pinned_universe = config_digest or upstream_universe_digest
    if not pinned_universe:
        emit_run(
            output_dir,
            blocked("UNIVERSE_DIGEST_MISSING", "ranked universe must be pinned by digest to the upstream contrast"),
        )
        return 2
    if pinned_universe != universe_digest:
        emit_run(
            output_dir,
            blocked("UNIVERSE_DIGEST_MISMATCH", "ranked universe checksum does not match the pinned contrast"),
        )
        return 2

    min_size = int(config["min_set_size"]) if "min_set_size" in config else 15
    max_size = int(config["max_set_size"]) if "max_set_size" in config else 500
    permutations = int(config.get("permutations") or 200)
    seed = int(config.get("seed") or 1)
    collection = str(config.get("collection") or gmt_path.name)
    ranking_statistic = str(config.get("ranking_statistic") or "signed_delta")
    identifier_mapping = str(config.get("identifier_mapping") or "symbol")

    gene_sets = load_gmt(gmt_path)
    universe = set(genes)
    hypotheses = ["stronger_dependency", "weaker_dependency", "unsigned_crosstalk"]
    table_rows: list[dict] = []
    too_small = 0
    for hypothesis in hypotheses:
        ranked = order_genes(rows, hypothesis)
        family: list[dict] = []
        for name, members in gene_sets.items():
            overlap = members & universe
            if len(overlap) < min_size:
                too_small += 1
                continue
            if len(overlap) > max_size or len(overlap) == len(universe):
                continue
            scored = running_enrichment_score(ranked, overlap)
            if scored is None:
                continue
            score, leading = scored
            p_value = permutation_p(
                score,
                ranked,
                overlap,
                permutations,
                seed + HYPOTHESIS_SEED[hypothesis],
            )
            family.append(
                {
                    "pathway": name,
                    "hypothesis": hypothesis,
                    "size": len(members),
                    "overlap": len(overlap),
                    "score": score,
                    "p_value": p_value,
                    "leading_edge": ",".join(leading),
                }
            )
        qvalues = bh([item["p_value"] for item in family])
        for item, q in zip(family, qvalues):
            item["fdr"] = q
            table_rows.append(item)

    if not table_rows:
        emit_run(
            output_dir,
            blocked("SET_TOO_SMALL", "no gene sets survived min/max size filters against the universe"),
        )
        return 2

    table_path = output_dir / "tables" / "pathway_enrichment.tsv"
    table_path.parent.mkdir(parents=True, exist_ok=True)
    with table_path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=["pathway", "hypothesis", "size", "overlap", "score", "p_value", "fdr", "leading_edge"],
            delimiter="\t",
        )
        writer.writeheader()
        writer.writerows(table_rows)

    result = {
        "schema_version": SCHEMA_VERSION,
        "capability_id": CAPABILITY_ID,
        "operation_id": OPERATION_ID,
        "status": "COMPLETE",
        "blocked": False,
        "question": "Which pathways are enriched among dependencies changed under this feature contrast?",
        "methods": {
            "gene_universe": "complete_ranked_upstream_targets",
            "identifier_mapping": identifier_mapping,
            "pathway_collection": collection,
            "ranking_statistic": ranking_statistic,
            "direction_policy": "separate_families_for_stronger_weaker_unsigned",
            "min_set_size": min_size,
            "max_set_size": max_size,
            "method": "pre-ranked running enrichment score with permutations",
            "permutations": permutations,
            "seed": seed,
            "multiple_testing": "BH within hypothesis family",
        },
        "observations": {
            "n_pathways_tested": len(table_rows),
            "sets_below_min_size": too_small,
        },
        "tables": [{"path": "tables/pathway_enrichment.tsv"}],
        "upstream_digest": actual_digest,
        "upstream_contrast_id": upstream.get("contrast_id") or upstream.get("id"),
        "release": upstream.get("release") or config.get("release"),
        "created_at": datetime.now(timezone.utc).isoformat(),
        "warnings": [
            "Enrichment is not mechanism and is not drug actionability.",
            "Do not combine P values across stronger, weaker, and unsigned families.",
            "Generic PROGENy activity association is out of scope.",
        ],
    }
    emit_run(output_dir, result, extra_outputs=["tables/pathway_enrichment.tsv"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
