import csv
import gzip
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


class ExpressionThresholdDependencyContrastTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.script = Path(__file__).with_name(
            "run_expression_threshold_dependency_contrast.R"
        )

    def write_inputs(self, root: Path, constant_expression: bool = False):
        model_ids = [f"ACH-{i:04d}" for i in range(1, 19)]
        expression_values = [1.0] * len(model_ids) if constant_expression else list(range(1, 19))
        expression = root / "expression.csv"
        effect = root / "effect.csv"
        models = root / "Model.csv"

        with expression.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["ModelID", "FEATURE (1)", "OTHER (2)"])
            for index, model_id in enumerate(model_ids):
                writer.writerow([model_id, expression_values[index], index % 4])

        with effect.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["ModelID", "NEGATIVE (10)", "POSITIVE (11)", "NULL (12)", "CONFOUNDED (13)"])
            for index, model_id in enumerate(model_ids):
                high = index >= 12
                low = index < 6
                writer.writerow(
                    [
                        model_id,
                        (-2.0 if high else 0.0) + (index % 2) * 0.02,
                        (2.0 if high else 0.0) + (index % 2) * 0.02,
                        ((index * 7) % 5) / 10,
                        index / 10 if (index < 6 and index % 2 == 0) or (index >= 12 and index % 2 == 1) else "",
                    ]
                )

        with models.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["ModelID", "OncotreeLineage"])
            for index, model_id in enumerate(model_ids):
                writer.writerow([model_id, "LineageA" if index % 2 == 0 else "LineageB"])
        return expression, effect, models

    def run_analysis(self, root: Path, output_name: str, constant_expression=False, min_group_n=5):
        expression, effect, models = self.write_inputs(root, constant_expression)
        output = root / output_name
        command = [
            "Rscript",
            str(self.script),
            f"--expression-csv={expression}",
            f"--gene-effect-csv={effect}",
            f"--model-csv={models}",
            f"--output-root={output}",
            "--source-gene=FEATURE",
            "--release=fixture",
            "--scope=global",
            "--lower-quantile=0.3333333333333333",
            "--upper-quantile=0.6666666666666667",
            f"--min-group-n={min_group_n}",
            "--fdr-max=0.05",
        ]
        subprocess.run(command, check=True, capture_output=True, text=True)
        return output

    def test_recovers_seeded_directions_and_stable_digest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = self.run_analysis(root, "first")
            second = self.run_analysis(root, "second")
            result = json.loads((first / "result.json").read_text(encoding="utf-8"))
            qc = json.loads((first / "qc.json").read_text(encoding="utf-8"))
            coverage = json.loads((first / "coverage.json").read_text(encoding="utf-8"))
            manifest_first = json.loads((first / "run_manifest.json").read_text(encoding="utf-8"))
            manifest_second = json.loads((second / "run_manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "ok")
            self.assertEqual(qc["status"], "pass")
            self.assertEqual(coverage["state"], "validated")
            self.assertEqual(manifest_first["result_digest"], manifest_second["result_digest"])

            with gzip.open(first / "all_targets.csv.gz", "rt", encoding="utf-8") as handle:
                rows = {row["target_gene"]: row for row in csv.DictReader(handle)}
            self.assertLess(float(rows["NEGATIVE"]["effect_size_high_minus_low"]), -1.5)
            self.assertGreater(float(rows["POSITIVE"]["effect_size_high_minus_low"]), 1.5)
            self.assertEqual(rows["NEGATIVE"]["stronger_dependency_in_high"], "TRUE")
            self.assertEqual(rows["POSITIVE"]["stronger_dependency_in_high"], "FALSE")

    def test_lineage_confounded_target_is_not_reported_as_found(self):
        with tempfile.TemporaryDirectory() as directory:
            output = self.run_analysis(Path(directory), "confounded", min_group_n=2)
            with gzip.open(output / "all_targets.csv.gz", "rt", encoding="utf-8") as handle:
                rows = {row["target_gene"]: row for row in csv.DictReader(handle)}
            self.assertEqual(rows["CONFOUNDED"]["status"], "INELIGIBLE")
            self.assertEqual(rows["CONFOUNDED"]["effect_size_high_minus_low"], "")

    def test_constant_expression_is_typed_ineligible(self):
        with tempfile.TemporaryDirectory() as directory:
            output = self.run_analysis(Path(directory), "constant", constant_expression=True)
            result = json.loads((output / "result.json").read_text(encoding="utf-8"))
            coverage = json.loads((output / "coverage.json").read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "INELIGIBLE")
            self.assertIn("constant", result["reason"])
            self.assertEqual(coverage["state"], "ineligible")


if __name__ == "__main__":
    unittest.main()
