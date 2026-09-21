#!/usr/bin/env python3
from __future__ import annotations

import csv
import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
SCRIPT = ROOT / "run_feature_conditioned_pathway_enrichment.py"


def digest_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_universe(path: Path, rows: list[tuple[str, float]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t")
        writer.writerow(["gene", "statistic"])
        writer.writerows(rows)


def write_gmt(path: Path) -> None:
    path.write_text(
        "SEEDED_STRONGER\tdesc\tG1\tG2\tG3\tG4\tG5\n"
        "SEEDED_WEAKER\tdesc\tG16\tG17\tG18\tG19\tG20\n"
        "TINY\tdesc\tG1\tG2\n",
        encoding="utf-8",
    )


def run_job(workdir: Path, *, upstream: dict, universe_rows: list[tuple[str, float]], config: dict) -> dict:
    workdir.mkdir(parents=True, exist_ok=True)
    upstream_path = workdir / "upstream.json"
    universe_path = workdir / "universe.tsv"
    gmt_path = workdir / "sets.gmt"
    config_path = workdir / "config.json"
    out = workdir / "out"
    upstream_path.write_text(json.dumps(upstream), encoding="utf-8")
    write_universe(universe_path, universe_rows)
    write_gmt(gmt_path)
    config = {
        "min_set_size": 5,
        "max_set_size": 50,
        "permutations": 50,
        "seed": 7,
        "universe_size": len(universe_rows),
        "upstream_digest": digest_file(upstream_path),
        **config,
    }
    config_path.write_text(json.dumps(config), encoding="utf-8")
    subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--upstream-artifact",
            str(upstream_path),
            "--ranked-universe-tsv",
            str(universe_path),
            "--gmt",
            str(gmt_path),
            "--config-json",
            str(config_path),
            "--output-dir",
            str(out),
        ],
        check=False,
    )
    return json.loads((out / "result.json").read_text(encoding="utf-8"))


def seeded_universe() -> list[tuple[str, float]]:
    rows = []
    for i in range(1, 21):
        gene = f"G{i}"
        if i <= 5:
            stat = -2.0 - (5 - i) * 0.1
        elif i >= 16:
            stat = 2.0 + (i - 16) * 0.1
        else:
            stat = 0.01 * i
        rows.append((gene, stat))
    return rows


class FeaturePathwayTests(unittest.TestCase):
    def test_recovers_seeded_pathways_and_directions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            result = run_job(
                Path(tmp),
                upstream={"status": "COMPLETE", "contrast_id": "fixture", "release": "26Q1"},
                universe_rows=seeded_universe(),
                config={},
            )
            table = list(
                csv.DictReader((Path(tmp) / "out" / "tables" / "pathway_enrichment.tsv").open(encoding="utf-8"), delimiter="\t")
            )
        self.assertEqual(result["status"], "COMPLETE")
        stronger = [row for row in table if row["pathway"] == "SEEDED_STRONGER" and row["hypothesis"] == "stronger_dependency"]
        weaker = [row for row in table if row["pathway"] == "SEEDED_WEAKER" and row["hypothesis"] == "weaker_dependency"]
        self.assertEqual(len(stronger), 1)
        self.assertEqual(len(weaker), 1)
        self.assertLess(float(stronger[0]["p_value"]), 0.2)
        self.assertLess(float(weaker[0]["p_value"]), 0.2)
        self.assertGreater(float(stronger[0]["score"]), 0)
        self.assertGreater(float(weaker[0]["score"]), 0)

    def test_deterministic_for_fixed_seed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            a = run_job(Path(tmp) / "a", upstream={"status": "COMPLETE"}, universe_rows=seeded_universe(), config={"seed": 3})
            b = run_job(Path(tmp) / "b", upstream={"status": "COMPLETE"}, universe_rows=seeded_universe(), config={"seed": 3})
        self.assertEqual(a["methods"]["seed"], b["methods"]["seed"])
        self.assertEqual(a["observations"]["n_pathways_tested"], b["observations"]["n_pathways_tested"])

    def test_retained_only_and_stale_and_duplicates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            retained = run_job(
                Path(tmp) / "r",
                upstream={"status": "COMPLETE", "retained_only": True},
                universe_rows=seeded_universe(),
                config={},
            )
            stale = run_job(
                Path(tmp) / "s",
                upstream={"status": "COMPLETE"},
                universe_rows=seeded_universe(),
                config={"upstream_digest": "0" * 64},
            )
            dup = run_job(
                Path(tmp) / "d",
                upstream={"status": "COMPLETE"},
                universe_rows=seeded_universe() + [("G1", 0.0)],
                config={"universe_size": 21},
            )
            tiny = run_job(
                Path(tmp) / "t",
                upstream={"status": "COMPLETE"},
                universe_rows=seeded_universe(),
                config={"min_set_size": 50},
            )
        self.assertEqual(retained["status"], "RETAINED_ONLY_INPUT")
        self.assertEqual(stale["status"], "STALE_UPSTREAM_DIGEST")
        self.assertEqual(dup["status"], "DUPLICATE_MAPPINGS")
        self.assertEqual(tiny["status"], "SET_TOO_SMALL")

    def test_wrong_universe_size(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            result = run_job(
                Path(tmp),
                upstream={"status": "COMPLETE"},
                universe_rows=seeded_universe(),
                config={"universe_size": 999},
            )
        self.assertEqual(result["status"], "WRONG_UNIVERSE")


if __name__ == "__main__":
    unittest.main()
