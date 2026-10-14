#!/usr/bin/env python3
"""Plot PAE heatmap and summarize confidence metrics from structure-prediction outputs.

Supported inputs (auto-detected, any mix):
  - Boltz-2:      confidence_*_model_*.json  (keys: complex_plddt, ptm, iptm, confidence_score)
  - OpenFold3:    *_confidences_aggregated.json (avg_plddt, ptm, iptm, gpde, has_clash)
  - AlphaFold2:   scores_confidences_*.json (iptm, ptm) / predicted_aligned_error_*.npz / .json
  - AlphaFold Server / OpenFold3: full PAE matrix JSON ({"pae": [[...]], "atom_plddt"/"chain_iplddt": ...})

Usage:
  python plot_confidence.py confidence_model_0.json ..._confidences_aggregated.json ...

Outputs: one <input>_pae.png heatmap per file containing a PAE matrix, plus a
printed metric summary with threshold verdicts.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

METRIC_VERDICTS = [
    ("iptm", "ipTM (interface)", lambda v: "trust interface (>0.8)" if v > 0.8
        else "re-run with other seeds (0.6-0.8)" if v > 0.6 else "do NOT trust (<0.6)"),
    ("ptm", "pTM (fold)", lambda v: "topology OK (>0.5)" if v > 0.5 else "topology questionable"),
    ("avg_plddt", "avg pLDDT", lambda v: "good" if v > 80 else "usable" if v > 70 else "low"),
    ("complex_plddt", "complex pLDDT", lambda v: "good" if v > 0.7 else "low"),
    ("gpde", "gpDE (side-chain/dist)", lambda v: "good" if v > 0.7 else "low"),
    ("has_clash", "clash", lambda v: "CLASH - discard sample" if v else "none"),
    ("confidence_score", "ranking score", lambda v: "sample ranking only"),
]


def load_json(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:  # noqa: BLE001
        print(f"[skip] {path.name}: not readable JSON ({exc})")
        return None


def find_pae(data):
    """Return the PAE matrix from common layouts, or None."""
    if not isinstance(data, dict):
        return None
    for key in ("pae", "predicted_aligned_error", "aligned_error"):
        v = data.get(key)
        if isinstance(v, list) and v and isinstance(v[0], list):
            return v
    for key in ("pae", "predicted_aligned_error"):
        v = data.get(key)
        if isinstance(v, dict) and isinstance(v.get("pae"), list):
            return v["pae"]
    return None


def plot_pae(matrix, out_png: Path, title: str):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import numpy as np

    arr = np.asarray(matrix, dtype=float)
    fig, ax = plt.subplots(figsize=(6, 5), dpi=150)
    im = ax.imshow(arr, vmin=0, vmax=min(31.75, float(arr.max() + 1)), cmap="bwr_r")
    ax.set_title(title, fontsize=9)
    ax.set_xlabel("residue (col)")
    ax.set_ylabel("residue (row)")
    fig.colorbar(im, ax=ax, label="PAE (Å)")
    fig.tight_layout()
    fig.savefig(out_png)
    plt.close(fig)
    return arr


def main(argv):
    paths = [Path(a) for a in argv[1:]]
    if not paths:
        print(__doc__)
        return 1
    for path in paths:
        data = load_json(path)
        if data is None:
            continue
        print(f"\n== {path.name} ==")
        if isinstance(data, dict):
            for key, label, verdict in METRIC_VERDICTS:
                if key in data:
                    v = data[key]
                    try:
                        print(f"  {label}: {float(v):.3f}  -> {verdict(float(v))}")
                    except (TypeError, ValueError):
                        print(f"  {label}: {v}")
        matrix = find_pae(data)
        if matrix:
            png = path.with_suffix(path.suffix + ".pae.png")
            arr = plot_pae(matrix, png, path.stem)
            n = arr.shape[0]
            # off-diagonal block means: crude domain/interface readout
            import numpy as np
            iu = ~np.eye(n, dtype=bool)
            print(f"  PAE matrix: {n}x{n}, median(overall) = {np.median(arr):.2f} Å, "
                  f"median(off-diag) = {np.median(arr[iu]):.2f} Å")
            print(f"  heatmap -> {png}")
            print("  read: diagonal blocks = domain/chain interiors; "
                  "off-diagonal blocks = inter-domain/interface orientation "
                  "(<5 Å trust, >15 Å is a guess)")
        elif isinstance(data, dict):
            print("  (no PAE matrix in this file - metrics only)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
