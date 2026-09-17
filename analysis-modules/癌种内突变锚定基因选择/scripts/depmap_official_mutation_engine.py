#!/usr/bin/env python3
"""Mutation-versus-negative CRISPR dependency analysis adapted from DepMap Context Explorer.

The adaptation changes only the context definition: mutation-positive models are the
in-group and mutation-matrix-negative models in the same lineage are the out-group.
"""
import argparse
import json
import re
import uuid
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq
from scipy import stats


GENE_SUFFIX = re.compile(r" \(\d+\)$")
META_COLUMNS = {
    "Unnamed: 0", "V1", "SequencingID", "ModelID", "ModelConditionID",
    "IsDefaultEntryForModel", "IsDefaultEntryForMC",
}
MIN_GROUP_SIZE = 5
DEP_THRESHOLD = 0.5
DEFAULT_FDR = 0.10
DEFAULT_EFFECT = -0.25
DEFAULT_MIN_FRAC_DEP_IN = 0.10

PAIR_COLUMNS = [
    "scope", "lineage", "anchor_gene", "event_type", "selection_tier",
    "oncokb_role", "dependency_gene", "n_mut", "n_wt",
    "mean_gene_effect_mut", "mean_gene_effect_wt", "delta_gene_effect",
    "ci95_low", "ci95_high", "t_stat", "pooled_df", "p_value",
    "fdr_by_anchor", "fdr_by_target", "n_dep_mut", "n_dep_wt",
    "n_non_dep_mut", "n_non_dep_wt", "frac_dep_mut", "frac_dep_wt",
    "dependency_odds_ratio", "log10_dependency_odds_ratio",
    "official_default_hit", "strict_fdr_hit",
]

SUMMARY_COLUMNS = [
    "lineage", "anchor_gene", "event_type", "selection_tier", "oncokb_role",
    "base_mut_n", "base_wt_n", "tested_target_count", "official_default_hit_count",
    "strict_fdr_hit_count", "best_official_target", "best_official_delta_gene_effect",
    "best_official_fdr", "best_exploratory_target", "best_exploratory_delta_gene_effect",
    "best_exploratory_fdr",
]


def clean_gene(value):
    return GENE_SUFFIX.sub("", str(value))


def truthy(series):
    return series.astype(str).str.lower().isin({"yes", "true", "1", "t"})


def write_json(path, value):
    Path(path).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def prepare_output(path):
    path = Path(path)
    if path.exists():
        raise SystemExit(f"Output already exists: {path.name}")
    work = path.parent / f".{path.name}.building-{uuid.uuid4().hex}"
    work.mkdir(parents=True)
    return path, work


def publish(work, target):
    work.rename(target)


def completed_manifest(path):
    p = Path(path) / "manifest.json"
    if not p.is_file():
        return None
    try:
        value = json.loads(p.read_text(encoding="utf-8"))
    except Exception:
        return None
    return value if value.get("status") in {"complete", "insufficient_sample"} else None


def read_numeric_matrix(path, requested_genes=None, default_only=False):
    path = Path(path)
    header = pd.read_csv(path, nrows=0).columns.tolist()
    gene_map = {clean_gene(c): c for c in header if c not in META_COLUMNS}
    genes = list(gene_map) if requested_genes is None else [g for g in dict.fromkeys(requested_genes) if g in gene_map]
    actual = [gene_map[g] for g in genes]
    meta = [c for c in header if c in META_COLUMNS]
    usecols = list(dict.fromkeys(meta + actual))
    frame = pd.read_csv(path, usecols=usecols, dtype={c: "float32" for c in actual}, low_memory=False)
    if default_only and "IsDefaultEntryForModel" in frame:
        frame = frame[truthy(frame["IsDefaultEntryForModel"])]
    if "ModelID" not in frame:
        aliases = [c for c in ("Unnamed: 0", "V1", "ModelConditionID") if c in frame]
        if not aliases:
            raise SystemExit(f"No supported model identifier in {path.name}")
        frame = frame.rename(columns={aliases[0]: "ModelID"})
    frame["ModelID"] = frame["ModelID"].astype(str)
    frame = frame.drop_duplicates("ModelID").set_index("ModelID")
    return frame.index.to_numpy(dtype=str), genes, frame[actual].to_numpy(dtype=np.float32, copy=False)


def align_matrix(ids, source_ids, values):
    lookup = {x: i for i, x in enumerate(source_ids)}
    out = np.full((len(ids), values.shape[1]), np.nan, dtype=np.float32)
    dest = [i for i, x in enumerate(ids) if x in lookup]
    if dest:
        src = [lookup[ids[i]] for i in dest]
        out[np.asarray(dest), :] = values[np.asarray(src), :]
    return out, len(dest)


def load_effect_and_dependency(effect_path, dependency_path):
    effect_ids, effect_genes, effect = read_numeric_matrix(effect_path)
    dep_ids, dep_genes, dep = read_numeric_matrix(dependency_path)
    if len(effect_ids) < 1000 or len(effect_genes) < 10000:
        raise SystemExit("Gene Effect input is not the complete release matrix")
    dep_lookup = {g: i for i, g in enumerate(dep_genes)}
    common_genes = [g for g in effect_genes if g in dep_lookup]
    effect_lookup = {g: i for i, g in enumerate(effect_genes)}
    effect = effect[:, [effect_lookup[g] for g in common_genes]]
    dep = dep[:, [dep_lookup[g] for g in common_genes]]
    dep, overlap = align_matrix(effect_ids, dep_ids, dep)

    finite = np.isfinite(dep)
    dep_count = np.sum(finite & (dep > DEP_THRESHOLD), axis=0)
    denom = finite.sum(axis=0)
    dep_fraction = np.divide(dep_count, denom, out=np.full(dep_count.shape, np.nan, dtype=float), where=denom > 0)
    # Exact public Context Explorer base filter. Its extra rescue of strongly-selective
    # genes is unavailable in the release downloads and is recorded in the manifest.
    eligible = (dep_count >= 3) & (dep_fraction <= 0.95)
    return (
        effect_ids,
        np.asarray(common_genes, dtype=object)[eligible].tolist(),
        effect[:, eligible],
        dep[:, eligible],
        overlap,
        pd.DataFrame({
            "dependency_gene": np.asarray(common_genes, dtype=object),
            "dependent_model_count": dep_count,
            "profiled_model_count": denom,
            "dependent_fraction": dep_fraction,
            "passes_context_explorer_base_filter": eligible,
        }),
    )


def load_events(path, target_ids, requested_genes):
    source_ids, genes, values = read_numeric_matrix(path, requested_genes=requested_genes, default_only=True)
    aligned, overlap = align_matrix(target_ids, source_ids, values)
    return genes, aligned, overlap


def bh_adjust(values):
    values = np.asarray(values, dtype=float)
    out = np.full(values.shape, np.nan)
    valid = np.isfinite(values)
    if not valid.any():
        return out
    p = values[valid]
    order = np.argsort(p)
    ranked = p[order]
    q = ranked * len(ranked) / np.arange(1, len(ranked) + 1)
    q = np.minimum.accumulate(q[::-1])[::-1]
    restored = np.empty_like(q)
    restored[order] = np.clip(q, 0, 1)
    out[valid] = restored
    return out


def bh_adjust_columns(matrix):
    matrix = np.asarray(matrix, dtype=float)
    out = np.full(matrix.shape, np.nan)
    if not matrix.size:
        return out
    valid = np.isfinite(matrix)
    counts = valid.sum(axis=0)
    filled = np.where(valid, matrix, np.inf)
    order = np.argsort(filled, axis=0)
    sorted_p = np.take_along_axis(filled, order, axis=0)
    ranks = np.arange(1, matrix.shape[0] + 1, dtype=float)[:, None]
    with np.errstate(invalid="ignore"):
        adjusted = sorted_p * counts[None, :] / ranks
    adjusted[~np.isfinite(sorted_p)] = np.inf
    adjusted = np.minimum.accumulate(adjusted[::-1], axis=0)[::-1]
    adjusted = np.clip(adjusted, 0, 1)
    np.put_along_axis(out, order, adjusted, axis=0)
    out[~valid] = np.nan
    return out


def group_moments(values):
    n = np.isfinite(values).sum(axis=0).astype(np.int32)
    sums = np.nansum(values, axis=0, dtype=float)
    squares = np.nansum(values.astype(float) ** 2, axis=0)
    mean = np.divide(sums, n, out=np.full(sums.shape, np.nan), where=n > 0)
    numerator = squares - np.divide(sums ** 2, n, out=np.zeros_like(sums), where=n > 0)
    var = np.divide(numerator, n - 1, out=np.full(sums.shape, np.nan), where=n > 1)
    return n, mean, np.maximum(var, 0)


def pooled_t_statistics(effect, mut_mask, wt_mask):
    n1, m1, v1 = group_moments(effect[mut_mask, :])
    n0, m0, v0 = group_moments(effect[wt_mask, :])
    delta = m1 - m0
    df = n1 + n0 - 2
    pooled = np.divide((n1 - 1) * v1 + (n0 - 1) * v0, df, out=np.full(delta.shape, np.nan), where=df > 0)
    se = np.sqrt(pooled * (np.divide(1.0, n1, out=np.full(delta.shape, np.nan), where=n1 > 0) + np.divide(1.0, n0, out=np.full(delta.shape, np.nan), where=n0 > 0)))
    t_stat = np.divide(delta, se, out=np.full(delta.shape, np.nan), where=se > 0)
    p_value = 2 * stats.t.sf(np.abs(t_stat), df)
    zero = (se == 0) & np.isfinite(delta)
    p_value[zero & (delta == 0)] = 1.0
    p_value[zero & (delta != 0)] = 0.0
    t_stat[zero & (delta == 0)] = 0.0
    t_stat[zero & (delta != 0)] = np.sign(delta[zero & (delta != 0)]) * np.inf
    crit = stats.t.ppf(0.975, df)
    low, high = delta - crit * se, delta + crit * se
    low[zero], high[zero] = delta[zero], delta[zero]
    valid = (n1 >= MIN_GROUP_SIZE) & (n0 >= MIN_GROUP_SIZE) & np.isfinite(p_value)
    for x in (delta, low, high, t_stat, p_value):
        x[~valid] = np.nan
    return {"n_mut": n1, "n_wt": n0, "mean_mut": m1, "mean_wt": m0, "delta": delta,
            "ci_low": low, "ci_high": high, "t_stat": t_stat, "df": df, "p": p_value, "valid": valid}


def dependency_counts(probability, mut_mask, wt_mask):
    mut = probability[mut_mask, :]
    wt = probability[wt_mask, :]
    mut_known, wt_known = np.isfinite(mut), np.isfinite(wt)
    n_dep_mut = np.sum(mut_known & (mut > DEP_THRESHOLD), axis=0)
    n_dep_wt = np.sum(wt_known & (wt > DEP_THRESHOLD), axis=0)
    n_non_mut = np.sum(mut_known & (mut <= DEP_THRESHOLD), axis=0)
    n_non_wt = np.sum(wt_known & (wt <= DEP_THRESHOLD), axis=0)
    frac_mut = np.divide(n_dep_mut, n_dep_mut + n_non_mut, out=np.full(n_dep_mut.shape, np.nan, dtype=float), where=(n_dep_mut + n_non_mut) > 0)
    frac_wt = np.divide(n_dep_wt, n_dep_wt + n_non_wt, out=np.full(n_dep_wt.shape, np.nan, dtype=float), where=(n_dep_wt + n_non_wt) > 0)
    numerator = n_non_wt * n_dep_mut
    denominator = n_dep_wt * n_non_mut
    odds = np.divide(numerator, denominator, out=np.zeros(numerator.shape, dtype=float), where=denominator != 0)
    odds[denominator == 0] = 100.0
    odds[(n_dep_mut == 0) & (n_dep_wt == 0)] = 1.0
    log_odds = odds.copy()
    positive = odds > 0
    log_odds[positive] = np.log10(odds[positive])
    return n_dep_mut, n_dep_wt, n_non_mut, n_non_wt, frac_mut, frac_wt, odds, log_odds


def empty_csv(path, columns):
    pd.DataFrame(columns=columns).to_csv(path, index=False)


def analyze_event_set(effect, probability, genes, events, out_dir, scope, lineage):
    out_dir.mkdir(parents=True, exist_ok=True)
    if not events:
        empty_csv(out_dir / "anchor_summary.csv", SUMMARY_COLUMNS)
        empty_csv(out_dir / "top_dependency_hits.csv", PAIR_COLUMNS)
        return {"source_event_count": 0, "tested_pair_count": 0, "official_default_hit_count": 0, "strict_fdr_hit_count": 0}

    computed, p_rows = [], []
    for event in events:
        s = pooled_t_statistics(effect, event["mutant_mask"], event["wildtype_mask"])
        computed.append(s)
        p_rows.append(s["p"])
    p_matrix = np.vstack(p_rows)
    reverse_q = bh_adjust_columns(p_matrix)
    writer = None
    summaries, tops = [], []
    totals = {"source_event_count": len(events), "tested_pair_count": 0, "official_default_hit_count": 0, "strict_fdr_hit_count": 0}
    try:
        for i, (event, s) in enumerate(zip(events, computed)):
            valid = s["valid"]
            if not valid.any():
                summaries.append({
                    "lineage": lineage or "Pan-cancer", "anchor_gene": event["gene"], "event_type": event["event_type"],
                    "selection_tier": event.get("selection_tier", ""), "oncokb_role": event.get("oncokb_role", ""),
                    "base_mut_n": event["base_mut_n"], "base_wt_n": event["base_wt_n"], "tested_target_count": 0,
                    "official_default_hit_count": 0, "strict_fdr_hit_count": 0,
                    "best_official_target": "", "best_official_delta_gene_effect": np.nan, "best_official_fdr": np.nan,
                    "best_exploratory_target": "", "best_exploratory_delta_gene_effect": np.nan, "best_exploratory_fdr": np.nan,
                })
                continue
            q = bh_adjust(s["p"])
            idx = np.where(valid)[0]
            d = dependency_counts(probability, event["mutant_mask"], event["wildtype_mask"])
            frame = pd.DataFrame({
                "scope": scope, "lineage": lineage or "Pan-cancer", "anchor_gene": event["gene"],
                "event_type": event["event_type"], "selection_tier": event.get("selection_tier", ""),
                "oncokb_role": event.get("oncokb_role", ""), "dependency_gene": np.asarray(genes, dtype=object)[idx],
                "n_mut": s["n_mut"][idx], "n_wt": s["n_wt"][idx],
                "mean_gene_effect_mut": s["mean_mut"][idx], "mean_gene_effect_wt": s["mean_wt"][idx],
                "delta_gene_effect": s["delta"][idx], "ci95_low": s["ci_low"][idx], "ci95_high": s["ci_high"][idx],
                "t_stat": s["t_stat"][idx], "pooled_df": s["df"][idx], "p_value": s["p"][idx],
                "fdr_by_anchor": q[idx], "fdr_by_target": reverse_q[i, idx],
                "n_dep_mut": d[0][idx], "n_dep_wt": d[1][idx], "n_non_dep_mut": d[2][idx], "n_non_dep_wt": d[3][idx],
                "frac_dep_mut": d[4][idx], "frac_dep_wt": d[5][idx],
                "dependency_odds_ratio": d[6][idx], "log10_dependency_odds_ratio": d[7][idx],
            })
            frame["official_default_hit"] = (frame.fdr_by_anchor <= DEFAULT_FDR) & (frame.delta_gene_effect < DEFAULT_EFFECT) & (frame.frac_dep_mut >= DEFAULT_MIN_FRAC_DEP_IN)
            frame["strict_fdr_hit"] = (frame.fdr_by_anchor <= 0.05) & (frame.delta_gene_effect < DEFAULT_EFFECT) & (frame.frac_dep_mut >= DEFAULT_MIN_FRAC_DEP_IN)
            frame = frame[PAIR_COLUMNS]
            table = pa.Table.from_pandas(frame, preserve_index=False)
            if writer is None:
                writer = pq.ParquetWriter(out_dir / "all_pairs.parquet", table.schema, compression="zstd")
            writer.write_table(table)
            official = frame[frame.official_default_hit].sort_values(["fdr_by_anchor", "delta_gene_effect"])
            strict = frame[frame.strict_fdr_hit]
            exploratory = frame.sort_values(["fdr_by_anchor", "p_value", "delta_gene_effect"]).head(20)
            tops.append(pd.concat([official.head(20), exploratory]).drop_duplicates(["anchor_gene", "event_type", "dependency_gene"]).head(40))
            totals["tested_pair_count"] += len(frame)
            totals["official_default_hit_count"] += len(official)
            totals["strict_fdr_hit_count"] += len(strict)
            best_o = official.iloc[0] if len(official) else None
            best_e = exploratory.iloc[0] if len(exploratory) else None
            summaries.append({
                "lineage": lineage or "Pan-cancer", "anchor_gene": event["gene"], "event_type": event["event_type"],
                "selection_tier": event.get("selection_tier", ""), "oncokb_role": event.get("oncokb_role", ""),
                "base_mut_n": event["base_mut_n"], "base_wt_n": event["base_wt_n"], "tested_target_count": len(frame),
                "official_default_hit_count": len(official), "strict_fdr_hit_count": len(strict),
                "best_official_target": "" if best_o is None else best_o.dependency_gene,
                "best_official_delta_gene_effect": np.nan if best_o is None else best_o.delta_gene_effect,
                "best_official_fdr": np.nan if best_o is None else best_o.fdr_by_anchor,
                "best_exploratory_target": "" if best_e is None else best_e.dependency_gene,
                "best_exploratory_delta_gene_effect": np.nan if best_e is None else best_e.delta_gene_effect,
                "best_exploratory_fdr": np.nan if best_e is None else best_e.fdr_by_anchor,
            })
    finally:
        if writer is not None:
            writer.close()
    pd.DataFrame(summaries, columns=SUMMARY_COLUMNS).to_csv(out_dir / "anchor_summary.csv", index=False)
    pd.concat(tops, ignore_index=True).to_csv(out_dir / "top_dependency_hits.csv", index=False) if tops else empty_csv(out_dir / "top_dependency_hits.csv", PAIR_COLUMNS)
    return totals


def method_manifest_common(target_count, overlap):
    return {
        "release": "DepMap Public 26Q1",
        "primary_metric": "Chronos CRISPR Gene Effect",
        "effect": "mean_mutant_minus_mean_matrix_negative; negative means stronger dependency in mutant models",
        "test": "two-sided pooled-variance independent t-test (scipy.stats.ttest_ind equal_var=True equivalent)",
        "minimum_complete_cases_per_group_per_target": MIN_GROUP_SIZE,
        "multiple_testing_primary": "Benjamini-Hochberg within each mutation anchor across eligible dependency targets",
        "official_default_hit": {"fdr_max": DEFAULT_FDR, "delta_gene_effect_max": DEFAULT_EFFECT, "min_mutant_dependent_fraction": DEFAULT_MIN_FRAC_DEP_IN},
        "secondary_metric": "CRISPR Gene Dependency probability; dependent when probability > 0.5",
        "eligible_dependency_target_count": target_count,
        "gene_effect_dependency_model_overlap": overlap,
        "target_filter": "dependency in >=3 and <=95% of profiled CRISPR models",
        "official_adaptation": "DepMap Context Explorer method with mutation-positive as in-group and mutation-matrix-negative within the same lineage as out-group",
        "known_difference_from_portal": "The portal also rescues genes tagged strongly selective in an internal TDA table; that table is not a 26Q1 release download, so the public downloadable base filter is used here.",
        "privacy": "Only module-relative result locations and public input labels are recorded.",
    }


def write_readme(path, title, scope):
    path.write_text(f"""# {title}

{scope}

## 主结果

- 使用 `CRISPRGeneEffect`（Chronos 连续 Gene Effect），`delta_gene_effect = Mut均值 - 矩阵阴性组均值`；负值表示突变组敲除该靶基因后更受抑制。
- 双侧、等方差独立样本 t 检验；这是 DepMap Context Explorer 当前公开实现实际使用的 `equal_var=True`。
- 每个靶基因要求 Mut 与矩阵阴性组各至少 5 个非缺失值；固定一个突变锚点后，对全部合格靶基因做 BH 校正。
- 官网默认命中规则：FDR≤0.10、`delta_gene_effect < -0.25`、Mut 组依赖比例≥0.10。

## 辅助结果

`CRISPRGeneDependency` 只用于统计概率>0.5的依赖比例与优势比。它不再作为连续差异检验的主指标。

## 文件

- `all_pairs.parquet`：全部检验结果。
- `anchor_summary.csv`：每个锚点的样本数、命中数与最佳候选。
- `top_dependency_hits.csv`：官网默认命中及探索性前列结果。
- `manifest.json`：版本、统计口径和限制。

“阴性”表示发布的对应突变矩阵中数值为0，是分析对照状态；不能扩展为该位点在生物学上绝对野生型。结果是观察性关联候选，需实验验证。
""", encoding="utf-8")


def event_from_row(row, matrices, scope_indices):
    genes, values, lookup = matrices[row.matrix]
    if row.gene not in lookup:
        return None
    x = values[scope_indices, lookup[row.gene]]
    mut = np.isfinite(x) & (x > 0)
    wt = np.isfinite(x) & (x == 0)
    if mut.sum() < MIN_GROUP_SIZE or wt.sum() < MIN_GROUP_SIZE:
        return None
    return {"gene": row.gene, "event_type": row.matrix, "selection_tier": row.selection_tier,
            "oncokb_role": "" if pd.isna(row.oncokb_role) else row.oncokb_role,
            "base_mut_n": int(mut.sum()), "base_wt_n": int(wt.sum()), "mutant_mask": mut, "wildtype_mask": wt}


def command_lineage(args):
    if completed_manifest(args.central_out):
        print(json.dumps({"status": "complete", "analysis": "lineage_official", "resumed": True}), flush=True)
        return
    catalog_root = Path(args.catalog_root)
    lineage_catalog = pd.read_csv(catalog_root / "lineage_catalog.csv")
    cards = pd.read_csv(args.anchor_cards)
    cards = cards[cards.selection_tier.isin(["A_role_matched_strict", "B_strict_unclassified", "C_exploratory"])].copy()
    ids, genes, effect, probability, overlap, target_audit = load_effect_and_dependency(args.gene_effect, args.dependency)
    model = pd.read_csv(args.model, usecols=["ModelID", "OncotreeLineage"]).drop_duplicates("ModelID")
    lineages = model.set_index(model.ModelID.astype(str)).OncotreeLineage.reindex(ids).to_numpy(dtype=object)
    dam_req = cards.loc[cards.matrix.eq("Damaging"), "gene"].unique().tolist()
    hot_req = cards.loc[cards.matrix.eq("Hotspot"), "gene"].unique().tolist()
    dam_g, dam, dam_overlap = load_events(args.damaging, ids, dam_req)
    hot_g, hot, hot_overlap = load_events(args.hotspot, ids, hot_req)
    matrices = {"Damaging": (dam_g, dam, {g: i for i, g in enumerate(dam_g)}), "Hotspot": (hot_g, hot, {g: i for i, g in enumerate(hot_g)})}
    central_target, central_work = prepare_output(args.central_out)
    target_audit.to_csv(central_work / "dependency_target_filter_audit.csv", index=False)
    rows, totals = [], {"source_event_count": 0, "tested_pair_count": 0, "official_default_hit_count": 0, "strict_fdr_hit_count": 0}
    for row in lineage_catalog.itertuples(index=False):
        lineage = row.lineage
        scope_idx = np.where(lineages == lineage)[0]
        final = catalog_root / row.folder / "dependency_analysis" / "depmap_official_gene_effect_v2"
        existing = completed_manifest(final)
        if existing:
            result = {k: int(existing.get(k, 0)) for k in totals}
        else:
            events = []
            for candidate in cards[cards.lineage.eq(lineage)].itertuples(index=False):
                event = event_from_row(candidate, matrices, scope_idx)
                if event is not None:
                    events.append(event)
            target, work = prepare_output(final)
            result = analyze_event_set(effect[scope_idx], probability[scope_idx], genes, events, work, "lineage", lineage)
            manifest = {"status": "complete", "lineage": lineage, "cohort_model_count": len(scope_idx), **method_manifest_common(len(genes), overlap), **result,
                        "mutation_matrix_model_overlap": {"Damaging": dam_overlap, "Hotspot": hot_overlap},
                        "anchor_routing": "06 selector cards: TSG->Damaging, oncogene->Hotspot, unclassified->Damaging; dual-role genes may retain both explicitly labeled definitions",
                        "input_labels": ["CRISPRGeneEffect.csv", "CRISPRGeneDependency.csv", "Model.csv", "OmicsSomaticMutationsMatrixDamaging.csv", "OmicsSomaticMutationsMatrixHotspot.csv", "anchor_gene_cards_all.csv"]}
            write_json(work / "manifest.json", manifest)
            write_readme(work / "README.md", f"{lineage}：突变锚点 × 全基因依赖（DepMap 官网方法适配版）", "Mut 与矩阵阴性对照均限定在当前癌种。突变类型继承 06 锚点选择器的角色路由。")
            publish(work, target)
        for k in totals:
            totals[k] += result[k]
        rows.append({"lineage": lineage, "folder": str(Path(row.folder) / "dependency_analysis/depmap_official_gene_effect_v2"), "cohort_model_count": len(scope_idx), **result})
        print(json.dumps({"progress": "lineage_complete", "lineage": lineage, **result}, ensure_ascii=False), flush=True)
    pd.DataFrame(rows).to_csv(central_work / "lineage_run_catalog.csv", index=False)
    manifest = {"status": "complete", "lineage_count": len(rows), "lineages_with_source_events": sum(x["source_event_count"] > 0 for x in rows), **method_manifest_common(len(genes), overlap), **totals}
    write_json(central_work / "manifest.json", manifest)
    write_readme(central_work / "README.md", "癌种内突变锚点—全基因依赖（DepMap 官网方法适配版）", "`lineage_run_catalog.csv` 指向每个癌种的独立结果目录；旧的 Dependency 概率版保留为辅助历史结果。")
    publish(central_work, central_target)
    print(json.dumps({"status": "complete", "analysis": "lineage_official", **totals}, ensure_ascii=False), flush=True)


def command_hotspot(args):
    if completed_manifest(args.out):
        print(json.dumps({"status": "complete", "analysis": "hotspot_official", "resumed": True}), flush=True)
        return
    ids, genes, effect, probability, overlap, target_audit = load_effect_and_dependency(args.gene_effect, args.dependency)
    hot_g, hot, hot_overlap = load_events(args.hotspot, ids, None)
    events, source = [], []
    for i, gene in enumerate(hot_g):
        x = hot[:, i]
        mut, wt = np.isfinite(x) & (x > 0), np.isfinite(x) & (x == 0)
        if mut.sum() >= MIN_GROUP_SIZE and wt.sum() >= MIN_GROUP_SIZE:
            events.append({"gene": gene, "event_type": "Hotspot", "selection_tier": "pan_cancer_hotspot", "oncokb_role": "",
                           "base_mut_n": int(mut.sum()), "base_wt_n": int(wt.sum()), "mutant_mask": mut, "wildtype_mask": wt})
            source.append({"gene": gene, "mut_n": int(mut.sum()), "wt_n": int(wt.sum()), "mut_rate": float(mut.sum() / (mut.sum() + wt.sum()))})
    target, work = prepare_output(args.out)
    result = analyze_event_set(effect, probability, genes, events, work, "pan_cancer", None)
    pd.DataFrame(source).to_csv(work / "eligible_hotspot_events.csv", index=False)
    target_audit.to_csv(work / "dependency_target_filter_audit.csv", index=False)
    manifest = {"status": "complete", "scope": "pan-cancer", **method_manifest_common(len(genes), overlap), **result,
                "hotspot_matrix_model_overlap": hot_overlap,
                "confounding_control": "Pan-cancer mutation groups can differ in lineage composition; confirm candidates in lineage-specific results or a covariate model.",
                "input_labels": ["CRISPRGeneEffect.csv", "CRISPRGeneDependency.csv", "OmicsSomaticMutationsMatrixHotspot.csv"]}
    write_json(work / "manifest.json", manifest)
    write_readme(work / "README.md", "泛癌 Hotspot—全基因依赖（DepMap 官网方法适配版）", "泛癌结果用于候选发现；癌种组成可能造成混杂，命中项应回到癌种内复核。")
    publish(work, target)
    print(json.dumps({"status": "complete", "analysis": "hotspot_official", **result}, ensure_ascii=False), flush=True)


def normalize_change(x):
    x = str(x).strip()
    return x if x.startswith("p.") else "p." + x


def command_protein(args):
    if completed_manifest(args.out):
        print(json.dumps({"status": "complete", "analysis": "protein_official", "resumed": True}), flush=True)
        return
    ids, genes, effect, probability, overlap, target_audit = load_effect_and_dependency(args.gene_effect, args.dependency)
    model = pd.read_csv(args.model, usecols=["ModelID", "OncotreeLineage"]).drop_duplicates("ModelID")
    lineages = model.set_index(model.ModelID.astype(str)).OncotreeLineage.reindex(ids).to_numpy(dtype=object)
    coverage_ids, _, _ = read_numeric_matrix(args.coverage_matrix, requested_genes=[], default_only=True)
    coverage = set(coverage_ids)
    gene, change = args.gene.strip(), normalize_change(args.protein_change)
    mutants = set()
    for chunk in pd.read_csv(args.mutation_long, usecols=["ModelID", "IsDefaultEntryForModel", "HugoSymbol", "ProteinChange"], chunksize=100000, low_memory=False):
        keep = truthy(chunk.IsDefaultEntryForModel) & chunk.HugoSymbol.astype(str).eq(gene) & chunk.ProteinChange.astype(str).map(normalize_change).eq(change)
        mutants.update(chunk.loc[keep, "ModelID"].astype(str))
    known = np.asarray([x in coverage for x in ids])
    if args.lineage:
        known &= lineages == args.lineage
    mut = known & np.asarray([x in mutants for x in ids])
    wt = known & ~mut
    events = []
    if mut.sum() >= MIN_GROUP_SIZE and wt.sum() >= MIN_GROUP_SIZE:
        events = [{"gene": gene, "event_type": f"ProteinChange:{change}", "selection_tier": "on_demand", "oncokb_role": "",
                   "base_mut_n": int(mut.sum()), "base_wt_n": int(wt.sum()), "mutant_mask": mut, "wildtype_mask": wt}]
    target, work = prepare_output(args.out)
    result = analyze_event_set(effect, probability, genes, events, work, "lineage" if args.lineage else "pan_cancer", args.lineage)
    pd.DataFrame({"ModelID": sorted(ids[mut])}).to_csv(work / "mutant_models.csv", index=False)
    target_audit.to_csv(work / "dependency_target_filter_audit.csv", index=False)
    status = "complete" if events else "insufficient_sample"
    manifest = {"status": status, "gene": gene, "protein_change": change, "lineage": args.lineage or "Pan-cancer", "mut_n": int(mut.sum()), "wt_n": int(wt.sum()),
                **method_manifest_common(len(genes), overlap), **result,
                "denominator": "Default mutation-profile models represented in the released genotyped matrix; absence of the exact retained ProteinChange is the analytical comparison state.",
                "input_labels": ["CRISPRGeneEffect.csv", "CRISPRGeneDependency.csv", "Model.csv", "OmicsSomaticMutations.csv", "OmicsSomaticMutationsMatrixDamaging.csv"]}
    write_json(work / "manifest.json", manifest)
    write_readme(work / "README.md", f"{gene} {change}：按需全基因依赖（DepMap 官网方法适配版）", f"分析范围：{args.lineage or 'Pan-cancer'}。精确 ProteinChange 阳性与矩阵覆盖范围内未检出该位点的模型比较。")
    publish(work, target)
    print(json.dumps({"status": status, "analysis": "protein_official", "gene": gene, "protein_change": change, "lineage": args.lineage or "Pan-cancer", "mut_n": int(mut.sum()), "wt_n": int(wt.sum()), **result}, ensure_ascii=False), flush=True)


def build_parser():
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="command", required=True)
    lineage = sub.add_parser("lineage-batch")
    for name in ["gene-effect", "dependency", "model", "damaging", "hotspot", "anchor-cards", "catalog-root", "central-out"]:
        lineage.add_argument("--" + name, required=True)
    lineage.set_defaults(func=command_lineage)
    hotspot = sub.add_parser("hotspot-pan")
    for name in ["gene-effect", "dependency", "hotspot", "out"]:
        hotspot.add_argument("--" + name, required=True)
    hotspot.set_defaults(func=command_hotspot)
    protein = sub.add_parser("protein-change")
    for name in ["gene-effect", "dependency", "model", "mutation-long", "coverage-matrix", "gene", "protein-change", "out"]:
        protein.add_argument("--" + name, required=True)
    protein.add_argument("--lineage")
    protein.set_defaults(func=command_protein)
    return p


if __name__ == "__main__":
    a = build_parser().parse_args()
    a.func(a)
