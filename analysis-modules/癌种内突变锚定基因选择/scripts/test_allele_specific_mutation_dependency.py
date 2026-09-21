import csv
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class AlleleSpecificMutationDependencyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.script = Path(__file__).with_name("run_allele_specific_mutation_dependency.py")

    def write_inputs(self, root: Path, *, allele_n=6, extra_change=False, ambiguous=False, multi=False):
        models = root / "Model.csv"
        effect = root / "effect.csv"
        mutations = root / "mutations.csv"
        model_ids = [f"ACH-{i:04d}" for i in range(1, 19)]
        with models.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["ModelID", "OncotreeLineage"])
            for index, model_id in enumerate(model_ids):
                writer.writerow([model_id, "LineageA" if index < 9 else "LineageB"])
        with effect.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["ModelID", "NEG (1)", "POS (2)"])
            for index, model_id in enumerate(model_ids):
                mutant = index < allele_n
                writer.writerow(
                    [
                        model_id,
                        -2.0 if mutant else 0.0,
                        1.5 if mutant else 0.0,
                    ]
                )
        with mutations.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["ModelID", "HugoSymbol", "ProteinChange"])
            if ambiguous:
                writer.writerow(["ACH-0001", "FEATURE", "p.?"])
            else:
                for index, model_id in enumerate(model_ids):
                    if index < allele_n:
                        writer.writerow([model_id, "FEATURE", "p.R249S"])
                        if multi:
                            writer.writerow([model_id, "FEATURE", "p.R273H"])
                    elif extra_change and index == allele_n:
                        writer.writerow([model_id, "FEATURE", "p.R273H"])
        return mutations, effect, models

    def run_analysis(self, root: Path, name: str, **kwargs):
        mutations, effect, models = self.write_inputs(root, **kwargs)
        output = root / name
        subprocess.run(
            [
                sys.executable,
                str(self.script),
                f"--mutation-long-csv={mutations}",
                f"--gene-effect-csv={effect}",
                f"--model-csv={models}",
                f"--output-root={output}",
                "--gene=FEATURE",
                "--protein-change=p.R249S",
                "--release=fixture",
                "--scope=global",
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        return output

    def test_eligible_allele_is_digest_stable_and_recovers_direction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = self.run_analysis(root, "first")
            second = self.run_analysis(root, "second")
            result = json.loads((first / "result.json").read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "ok")
            self.assertEqual(result["case_n"], 6)
            digest = json.loads((first / "run_manifest.json").read_text(encoding="utf-8"))[
                "result_digest"
            ]
            self.assertEqual(
                digest,
                json.loads((second / "run_manifest.json").read_text(encoding="utf-8"))[
                    "result_digest"
                ],
            )

    def test_insufficient_case_emits_no_association_table(self):
        with tempfile.TemporaryDirectory() as directory:
            output = self.run_analysis(Path(directory), "small", allele_n=2)
            result = json.loads((output / "result.json").read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "INELIGIBLE")
            self.assertEqual(result["reason"], "insufficient_case")
            self.assertFalse((output / "all_targets.csv.gz").exists())

    def test_ambiguous_and_multi_allelic_stop_before_scan(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ambiguous = self.run_analysis(root, "amb", ambiguous=True)
            multi = self.run_analysis(root, "multi", multi=True)
            self.assertEqual(
                json.loads((ambiguous / "result.json").read_text(encoding="utf-8"))["reason"],
                "ambiguous_variant",
            )
            self.assertEqual(
                json.loads((multi / "result.json").read_text(encoding="utf-8"))["reason"],
                "insufficient_case",
            )
            self.assertEqual(
                json.loads((multi / "result.json").read_text(encoding="utf-8"))["multi_allelic_n"],
                6,
            )


if __name__ == "__main__":
    unittest.main()
