#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
from run_event_contrast_power_sidecar import mde, t_quantile
SCRIPT = ROOT / "run_event_contrast_power_sidecar.py"


def write(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload), encoding="utf-8")


def digest_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run_sidecar(workdir: Path, upstream: dict, config: dict) -> tuple[int, dict]:
    workdir.mkdir(parents=True, exist_ok=True)
    upstream_path = workdir / "upstream.json"
    config_path = workdir / "config.json"
    out = workdir / "out"
    write(upstream_path, upstream)
    if "upstream_digest" not in config:
        config = {**config, "upstream_digest": digest_file(upstream_path)}
    write(config_path, config)
    completed = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--upstream-artifact",
            str(upstream_path),
            "--config-json",
            str(config_path),
            "--output-dir",
            str(out),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    result = json.loads((out / "result.json").read_text(encoding="utf-8"))
    return completed.returncode, result


def complete_upstream(**overrides) -> dict:
    payload = {
        "schema_version": 1,
        "status": "COMPLETE",
        "cohort": {"n_case": 12, "n_control": 80},
        "methods": {
            "variance_model": "pooled",
            "tested_target_count": 100,
            "alpha": 0.05,
            "fdr_policy": "BH",
            "empirical_sigma": 0.4,
        },
        "observations": {"observed_effect": -0.12},
        "fixture_note": "TP53 Hotspot counts are a regression fixture only",
    }
    payload.update(overrides)
    return payload


class EventContrastPowerTests(unittest.TestCase):
    def test_quantile_matches_two_sided_mapping(self) -> None:
        df = 38.0
        t80 = t_quantile(0.8, df)
        from run_event_contrast_power_sidecar import two_sided_t_p

        self.assertAlmostEqual(two_sided_t_p(t80, df), 2.0 * (1.0 - 0.8), places=6)

    def test_mde_closed_form_tolerance(self) -> None:
        got = mde(20, 20, 1.0, 0.05, 0.8)
        # Independent check: (t_{0.975,38}+t_{0.8,38}) * sqrt(1/20+1/20)
        expected = (t_quantile(0.975, 38) + t_quantile(0.8, 38)) * (2 / 20) ** 0.5
        self.assertAlmostEqual(got["minimum_detectable_effect"], expected, places=12)
        self.assertGreater(got["minimum_detectable_effect"], 0.8)
        self.assertLess(got["minimum_detectable_effect"], 1.0)

    def test_complete_sidecar_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            code1, first = run_sidecar(
                work / "a",
                complete_upstream(),
                {"desired_power": 0.8, "alpha": 0.05},
            )
            code2, second = run_sidecar(
                work / "b",
                complete_upstream(),
                {"desired_power": 0.8, "alpha": 0.05},
            )
        self.assertEqual(code1, 0)
        self.assertEqual(code2, 0)
        self.assertEqual(first["status"], "COMPLETE")
        self.assertEqual(first["digest"], second["digest"])
        self.assertFalse(first["biological_significance"])
        self.assertFalse(first["evidence_of_no_effect"])
        self.assertGreater(first["observations"]["bonferroni_mde"], first["observations"]["unadjusted_mde"])
        self.assertTrue(first["observations"]["observed_effect_is_not_power"])

    def test_missing_upstream(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            config_path = work / "config.json"
            write(config_path, {"desired_power": 0.8, "alpha": 0.05, "tested_target_count": 10, "empirical_sigma": 0.4})
            out = work / "out"
            code = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--upstream-artifact",
                    str(work / "missing.json"),
                    "--config-json",
                    str(config_path),
                    "--output-dir",
                    str(out),
                ],
                check=False,
            ).returncode
            result = json.loads((out / "result.json").read_text(encoding="utf-8"))
        self.assertNotEqual(code, 0)
        self.assertEqual(result["status"], "UPSTREAM_MISSING")

    def test_ineligible_upstream(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            _, result = run_sidecar(
                Path(tmp),
                complete_upstream(status="INELIGIBLE"),
                {"desired_power": 0.8, "alpha": 0.05},
            )
        self.assertEqual(result["status"], "UPSTREAM_INELIGIBLE")

    def test_stale_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            _, result = run_sidecar(
                work,
                complete_upstream(),
                {"desired_power": 0.8, "alpha": 0.05, "upstream_digest": "0" * 64},
            )
        self.assertEqual(result["status"], "STALE_UPSTREAM_DIGEST")

    def test_zero_variance(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            upstream = complete_upstream()
            upstream["methods"]["empirical_sigma"] = 0
            _, result = run_sidecar(Path(tmp), upstream, {"desired_power": 0.8, "alpha": 0.05})
        self.assertEqual(result["status"], "ZERO_VARIANCE")

    def test_tiny_groups(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            upstream = complete_upstream()
            upstream["cohort"] = {"n_case": 1, "n_control": 80}
            _, result = run_sidecar(Path(tmp), upstream, {"desired_power": 0.8, "alpha": 0.05})
        self.assertEqual(result["status"], "TINY_GROUPS")

    def test_invalid_alpha_and_power(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            _, bad_alpha = run_sidecar(
                Path(tmp) / "a",
                complete_upstream(),
                {"desired_power": 0.8, "alpha": 0},
            )
            _, bad_power = run_sidecar(
                Path(tmp) / "b",
                complete_upstream(),
                {"desired_power": 1.2, "alpha": 0.05},
            )
        self.assertEqual(bad_alpha["status"], "INVALID_ALPHA")
        self.assertEqual(bad_power["status"], "INVALID_POWER")

    def test_welch_policy_blocked(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            upstream = complete_upstream()
            upstream["methods"]["variance_model"] = "welch"
            _, result = run_sidecar(Path(tmp), upstream, {"desired_power": 0.8, "alpha": 0.05})
        self.assertEqual(result["status"], "UNEQUAL_VARIANCE_POLICY_UNSUPPORTED")

    def test_standard_run_filenames_and_no_bh_mde(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / "run"
            code, result = run_sidecar(work, complete_upstream(), {"desired_power": 0.8, "alpha": 0.05})
            out = work / "out"
            self.assertEqual(code, 0)
            self.assertTrue((out / "result.json").is_file())
            self.assertTrue((out / "qc.json").is_file())
            self.assertTrue((out / "run_manifest.json").is_file())
            self.assertNotIn("bh_complete_null_mde", result["observations"])

    def test_one_sided_direction_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            _, result = run_sidecar(
                Path(tmp),
                complete_upstream(),
                {"desired_power": 0.8, "alpha": 0.05, "direction": "one_sided"},
            )
        self.assertEqual(result["status"], "UNSUPPORTED_DIRECTION")

    def test_median_power_does_not_recurse(self) -> None:
        self.assertEqual(t_quantile(0.5, 38.0), 0.0)
        with tempfile.TemporaryDirectory() as tmp:
            code, result = run_sidecar(
                Path(tmp),
                complete_upstream(),
                {"desired_power": 0.5, "alpha": 0.05},
            )
        self.assertEqual(code, 0)
        self.assertEqual(result["status"], "COMPLETE")


if __name__ == "__main__":
    unittest.main()
