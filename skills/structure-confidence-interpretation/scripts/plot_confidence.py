#!/usr/bin/env python3
"""Plot a PAE heatmap and summarize confidence metrics from structure predictions.

Supported inputs (auto-detected, any mix):
  - Boltz-2:      confidence_*_model_*.json
                  (complex_plddt, ptm, iptm, confidence_score,
                   complex_pde / complex_ipde in Å, lower is better)
  - OpenFold3:    *_confidences_aggregated.json
                  (avg_plddt, ptm, iptm, gpde in 0–1, has_clash)
  - AlphaFold2:   scores JSON, or predicted_aligned_error_*.json / *.npz
  - AlphaFold Server / AF3: full PAE JSON
                  ({"pae": [[...]], "token_chain_ids": [...]})
  - Chai-1:       scores.model_*.npz (pae, iptm, ptm, aggregate_score)

NPZ files are loaded with numpy. A missing matplotlib skips the PNG and still
prints the metric and block summary. Heatmaps use a fixed 0–31.75 Å scale.

Interface PAE is the median of each chain-pair block (from token_chain_ids or
another chain-id array). Inter-domain blocks are reported separately. The
off-diagonal median of the whole matrix is not an interface score.

Usage:
  python plot_confidence.py confidence_model_0.json predicted_aligned_error.npz
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

# AlphaFold's conventional PAE ceiling. Keeping it fixed makes samples comparable.
PAE_VMIN_A = 0.0
PAE_VMAX_A = 31.75
DOMAIN_SPLIT_A = 5.0
DOMAIN_MIN_LEN = 4

CHAIN_ID_KEYS = (
    "token_chain_ids",
    "chain_ids",
    "pae_chain_ids",
    "asym_id",
    "asym_ids",
)
DOMAIN_ID_KEYS = (
    "token_domain_ids",
    "domain_ids",
    "domain_index",
)


def verdict_iptm(value):
    if value > 0.8:
        return "trust interface (>0.8)"
    if value > 0.6:
        return "re-run with other seeds (0.6-0.8)"
    return "do NOT trust (<0.6)"


def verdict_ptm(value):
    if value > 0.5:
        return "topology OK (>0.5)"
    return "topology questionable"


def verdict_avg_plddt(value):
    if value > 80:
        return "good"
    if value > 70:
        return "usable"
    return "low"


def verdict_complex_plddt(value):
    if value > 0.7:
        return "good"
    return "low"


def verdict_raw_pde(value):
    """Boltz complex_pde / complex_ipde are errors in Å. Lower is more confident."""
    if value < 2:
        return "tight (<2 Å; lower is better)"
    if value < 5:
        return "moderate (2-5 Å; lower is better)"
    return "uncertain (>5 Å; lower is better)"


def verdict_gpde(value):
    """OpenFold3 gpDE is a 0–1 score. Closer to 1 is better, unlike raw PDE."""
    if value > 0.8:
        return "good (closer to 1 is better)"
    if value > 0.7:
        return "usable (closer to 1 is better)"
    return "low (closer to 1 is better)"


def verdict_clash(value):
    if value:
        return "CLASH - discard sample"
    return "none"


def verdict_ranking(_value):
    return "sample ranking only"


METRIC_VERDICTS = [
    ("iptm", "ipTM (interface)", verdict_iptm),
    ("ptm", "pTM (fold)", verdict_ptm),
    ("avg_plddt", "avg pLDDT", verdict_avg_plddt),
    ("complex_plddt", "complex pLDDT", verdict_complex_plddt),
    ("complex_pde", "complex PDE (Å)", verdict_raw_pde),
    ("complex_ipde", "complex iPDE (Å)", verdict_raw_pde),
    ("gpde", "gpDE (0-1)", verdict_gpde),
    ("has_clash", "clash", verdict_clash),
    ("confidence_score", "ranking score", verdict_ranking),
    ("aggregate_score", "Chai aggregate score", verdict_ranking),
    ("sample_ranking_score", "sample ranking score", verdict_ranking),
]


def load_json(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:  # noqa: BLE001
        print(f"[skip] {path.name}: not readable JSON ({exc})")
        return None


def array_to_data(value):
    if getattr(value, "shape", None) == ():
        item = value.item()
        if isinstance(item, bytes):
            return item.decode("utf-8", "replace")
        return item
    kind = getattr(getattr(value, "dtype", None), "kind", "")
    if kind in "SU":
        return [
            item.decode("utf-8", "replace") if isinstance(item, bytes) else item
            for item in value.tolist()
        ]
    return value.tolist()


def load_npz(path: Path):
    try:
        import numpy as np
    except ImportError:
        print(
            f"[skip] {path.name}: numpy is required to read NPZ "
            "(install numpy in this Python: pip install numpy)"
        )
        return None
    try:
        with np.load(path, allow_pickle=False) as archive:
            return {key: array_to_data(archive[key]) for key in archive.files}
    except Exception as exc:  # noqa: BLE001
        print(f"[skip] {path.name}: not a readable NPZ ({exc})")
        return None


def load_confidence(path: Path):
    if path.suffix.lower() == ".npz":
        return load_npz(path)
    return load_json(path)


def is_matrix(value):
    return isinstance(value, list) and bool(value) and isinstance(value[0], list)


def pae_value(value):
    if is_matrix(value):
        return value
    if isinstance(value, dict):
        for key in ("pae", "predicted_aligned_error", "aligned_error"):
            inner = value.get(key)
            if is_matrix(inner):
                return inner
    return None


def find_pae(data):
    """Return the PAE matrix from common layouts, or None."""
    if not isinstance(data, dict):
        return None
    for key in ("pae", "predicted_aligned_error", "aligned_error"):
        found = pae_value(data.get(key))
        if found is not None:
            return found
    return None


def is_scalar_metric(value):
    # bool is a subclass of int, so clash flags (True/False or 0/1) count.
    return isinstance(value, (int, float))


def metric_lines(data):
    lines = []
    for key, label, verdict in METRIC_VERDICTS:
        if key not in data:
            continue
        value = data[key]
        if not is_scalar_metric(value):
            continue
        try:
            number = float(value)
            lines.append(f"  {label}: {number:.3f}  -> {verdict(number)}")
        except (TypeError, ValueError):
            lines.append(f"  {label}: {value}")
    return lines


def median(values):
    ordered = sorted(float(value) for value in values)
    count = len(ordered)
    if count == 0:
        return None
    mid = count // 2
    if count % 2:
        return ordered[mid]
    return (ordered[mid - 1] + ordered[mid]) / 2.0


def internal_median(matrix, indices):
    indices = list(indices)
    if len(indices) < 2:
        return None
    values = [
        matrix[row][col]
        for row in indices
        for col in indices
        if row != col
    ]
    return median(values)


def off_block_median(matrix, left, right):
    left = list(left)
    right = list(right)
    values = [matrix[row][col] for row in left for col in right]
    values.extend(matrix[row][col] for row in right for col in left)
    return median(values)


def orientation_verdict(median_a):
    if median_a < 5:
        return "trust (<5 Å)"
    if median_a <= 15:
        return "uncertain (5-15 Å)"
    return "orientation is a guess (>15 Å)"


def coerce_labels(value, length):
    if value is None or isinstance(value, (str, bytes)):
        return None
    try:
        labels = list(value)
    except TypeError:
        return None
    if len(labels) != length:
        return None
    converted = []
    for item in labels:
        if isinstance(item, bytes):
            item = item.decode("utf-8", "replace")
        if isinstance(item, float) and item != item:
            return None
        converted.append(str(item))
    return converted


def labels_from(data, keys, length):
    notes = []
    if not isinstance(data, dict):
        return None, notes
    for key in keys:
        if key not in data or data[key] is None:
            continue
        labels = coerce_labels(data[key], length)
        if labels is not None:
            return labels, notes
        notes.append(f"  [note] {key} does not match the {length}x{length} PAE matrix; ignoring it")
    return None, notes


def group_indices(labels):
    order = []
    groups = {}
    for index, label in enumerate(labels):
        if label not in groups:
            groups[label] = []
            order.append(label)
        groups[label].append(index)
    return order, groups


def residue_cross_median(matrix, start, end, residue):
    values = [matrix[index][residue] for index in range(start, end)]
    values.extend(matrix[residue][index] for index in range(start, end))
    return median(values)


def merge_short_spans(spans, min_len):
    spans = list(spans)
    changed = True
    while changed and len(spans) > 1:
        changed = False
        for index, (start, end) in enumerate(spans):
            if end - start >= min_len:
                continue
            if index == 0:
                next_start, next_end = spans[1]
                spans[1] = (start, next_end)
                del spans[0]
            else:
                prev_start, _prev_end = spans[index - 1]
                spans[index - 1] = (prev_start, end)
                del spans[index]
            changed = True
            break
    return spans


def infer_contiguous_domains(matrix):
    """Split one chain into contiguous low-PAE spans.

    A boundary is kept only when the next residue's PAE against the current
    span is at least 5 Å. This is a fallback for files that have no domain
    ids; explicit chain ids still drive the interface readout.
    """
    length = len(matrix)
    if length < DOMAIN_MIN_LEN * 2:
        return None
    spans = []
    start = 0
    while start < length:
        end = start + 1
        while end < length:
            if residue_cross_median(matrix, start, end, end) >= DOMAIN_SPLIT_A:
                break
            end += 1
        spans.append((start, end))
        start = end
    spans = merge_short_spans(spans, DOMAIN_MIN_LEN)
    if len(spans) < 2:
        return None
    labels = [""] * length
    for index, (start, end) in enumerate(spans, start=1):
        for residue in range(start, end):
            labels[residue] = f"d{index}"
    return labels


def pairwise_lines(matrix, labels, kind):
    order, groups = group_indices(labels)
    lines = []
    for label in order:
        value = internal_median(matrix, groups[label])
        if value is None:
            lines.append(f"  {kind} {label} internal: no residue pairs")
        else:
            lines.append(f"  {kind} {label} internal: median {value:.2f} Å")
    if kind == "chain" and len(order) < 2:
        lines.append("  single chain; interface PAE not reported")
        return lines
    for index, left in enumerate(order):
        for right in order[index + 1:]:
            value = off_block_median(matrix, groups[left], groups[right])
            verdict = orientation_verdict(value)
            if kind == "chain":
                lines.append(f"  interface {left}-{right}: median {value:.2f} Å -> {verdict}")
            else:
                lines.append(f"  inter-domain {left}-{right}: median {value:.2f} Å -> {verdict}")
    return lines


def within_chain_domain_lines(matrix, chains, domains):
    lines = []
    order, groups = group_indices(chains)
    for chain in order:
        indices = groups[chain]
        local = [domains[index] for index in indices]
        if len(set(local)) < 2:
            continue
        by_domain = {}
        domain_order = []
        for original, label in zip(indices, local):
            if label not in by_domain:
                by_domain[label] = []
                domain_order.append(label)
            by_domain[label].append(original)
        for index, left in enumerate(domain_order):
            for right in domain_order[index + 1:]:
                value = off_block_median(matrix, by_domain[left], by_domain[right])
                lines.append(
                    f"  inter-domain {chain}:{left}-{right}: median {value:.2f} Å -> {orientation_verdict(value)}"
                )
    return lines


def block_lines(matrix, data):
    length = len(matrix)
    if length == 0 or any(len(row) != length for row in matrix):
        return ["  PAE matrix is not square; interface blocks not reported"]
    flat = [value for row in matrix for value in row]
    lines = [
        f"  PAE matrix: {length}x{length}, median(overall) = {median(flat):.2f} Å, "
        f"color scale 0-{PAE_VMAX_A:g} Å"
    ]
    chains, chain_notes = labels_from(data, CHAIN_ID_KEYS, length)
    domains, domain_notes = labels_from(data, DOMAIN_ID_KEYS, length)
    lines.extend(chain_notes)
    lines.extend(domain_notes)
    inferred = False
    if chains is None and domains is None:
        domains = infer_contiguous_domains(matrix)
        inferred = domains is not None
    if chains is None:
        lines.append("  no chain ids; interface PAE not reported")
        if domains is None:
            lines.append("  no domain blocks; orientation PAE not reported")
            return lines
        if inferred:
            lines.append("  inferred contiguous domains where intra-domain PAE stays below 5 Å")
        lines.extend(pairwise_lines(matrix, domains, "domain"))
        return lines
    lines.extend(pairwise_lines(matrix, chains, "chain"))
    if domains is not None:
        lines.extend(within_chain_domain_lines(matrix, chains, domains))
    elif len(set(chains)) == 1:
        inferred_domains = infer_contiguous_domains(matrix)
        if inferred_domains is not None:
            lines.append("  inferred contiguous domains where intra-domain PAE stays below 5 Å")
            lines.extend(pairwise_lines(matrix, inferred_domains, "domain"))
    return lines


def import_pyplot():
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    return plt


def plot_pae(matrix, out_png: Path, title: str):
    try:
        plt = import_pyplot()
    except ImportError:
        print(
            "  [skip plot] matplotlib is not installed in this Python. "
            "The metric and block summary above is still valid. "
            "Install matplotlib in the same interpreter (pip install matplotlib) "
            "and rerun to write the PAE heatmap."
        )
        return
    fig, ax = plt.subplots(figsize=(6, 5), dpi=150)
    image = ax.imshow(matrix, vmin=PAE_VMIN_A, vmax=PAE_VMAX_A, cmap="bwr_r")
    ax.set_title(title, fontsize=9)
    ax.set_xlabel("scored residue")
    ax.set_ylabel("aligned residue")
    fig.colorbar(image, ax=ax, label="PAE (Å)")
    fig.tight_layout()
    fig.savefig(out_png)
    plt.close(fig)


def report_pae(data, matrix, path: Path):
    for line in block_lines(matrix, data):
        print(line)
    png = path.with_suffix(path.suffix + ".pae.png")
    plot_pae(matrix, png, path.stem)
    if png.exists():
        print(f"  heatmap -> {png}")


def main(argv):
    paths = [Path(arg) for arg in argv[1:]]
    if not paths:
        print(__doc__)
        return 1
    for path in paths:
        data = load_confidence(path)
        if data is None:
            continue
        print(f"\n== {path.name} ==")
        if isinstance(data, dict):
            for line in metric_lines(data):
                print(line)
            matrix = find_pae(data)
        else:
            matrix = None
        if matrix:
            report_pae(data, matrix, path)
        elif isinstance(data, dict):
            print("  (no PAE matrix in this file - metrics only)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
