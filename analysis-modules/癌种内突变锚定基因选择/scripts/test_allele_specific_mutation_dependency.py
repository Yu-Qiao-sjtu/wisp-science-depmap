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

    def write_inputs(
        self,
        root: Path,
        *,
        allele_n=6,
        extra_change=False,
        requested_ambiguous=False,
        junk_protein_change=False,
        multi=False,
        unprofiled=False,
        nondefault=False,
    ):
        models = root / "Model.csv"
        effect = root / "effect.csv"
        mutations = root / "mutations.csv"
        coverage = root / "coverage.csv"
        model_ids = [f"ACH-{i:04d}" for i in range(1, 19)]
        if unprofiled:
            model_ids.append("ACH-0099")
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
                writer.writerow([model_id, -2.0 if mutant else 0.0, 1.5 if mutant else 0.0])
        with coverage.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["ModelID", "FEATURE"])
            for model_id in model_ids:
                if model_id == "ACH-0099":
                    continue
                writer.writerow([model_id, 0])
        with mutations.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["ModelID", "HugoSymbol", "ProteinChange", "IsDefaultEntryForModel"])
            if requested_ambiguous:
                writer.writerow(["ACH-0001", "FEATURE", "p.?", "True"])
            else:
                for index, model_id in enumerate(model_ids):
                    if model_id == "ACH-0099":
                        continue
                    if index < allele_n:
                        writer.writerow([model_id, "FEATURE", "p.R249S", "False" if nondefault else "True"])
                        if multi:
                            writer.writerow([model_id, "FEATURE", "p.R273H", "True"])
                    elif extra_change and index == allele_n:
                        writer.writerow([model_id, "FEATURE", "p.R273H", "True"])
                if junk_protein_change:
                    writer.writerow(["ACH-0018", "FEATURE", "p.?", "True"])
        return mutations, effect, models, coverage

    def run_analysis(self, root: Path, name: str, protein_change="p.R249S", **kwargs):
        mutations, effect, models, coverage = self.write_inputs(root, **kwargs)
        output = root / name
        completed = subprocess.run(
            [
                sys.executable,
                str(self.script),
                f"--mutation-long-csv={mutations}",
                f"--gene-effect-csv={effect}",
                f"--model-csv={models}",
                f"--coverage-matrix-csv={coverage}",
                f"--output-root={output}",
                "--gene=FEATURE",
                f"--protein-change={protein_change}",
                "--release=fixture",
                "--scope=global",
            ],
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            self.fail(completed.stderr)
        return output

    def test_eligible_allele_is_digest_stable_and_uses_result_contract(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = self.run_analysis(root, "first")
            second = self.run_analysis(root, "second")
            result = json.loads((first / "result.json").read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "ok")
            self.assertEqual(result["schema_version"], 1)
            self.assertEqual(result["case_n"], 6)
            self.assertIn("question", result)
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

    def test_requested_ambiguous_allele_is_ineligible_but_unrelated_junk_is_skipped(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            requested = self.run_analysis(root, "amb", protein_change="p.?")
            skipped = self.run_analysis(root, "junk", junk_protein_change=True)
            self.assertEqual(
                json.loads((requested / "result.json").read_text(encoding="utf-8"))["reason"],
                "ambiguous_variant",
            )
            self.assertEqual(
                json.loads((skipped / "result.json").read_text(encoding="utf-8"))["status"],
                "ok",
            )

    def test_unprofiled_models_are_missing_not_controls(self):
        with tempfile.TemporaryDirectory() as directory:
            output = self.run_analysis(Path(directory), "cov", unprofiled=True)
            result = json.loads((output / "result.json").read_text(encoding="utf-8"))
            self.assertEqual(result["missing_n"], 1)
            self.assertEqual(result["control_n"], 12)

    def test_nondefault_entries_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            output = self.run_analysis(Path(directory), "nd", nondefault=True)
            result = json.loads((output / "result.json").read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "INELIGIBLE")
            self.assertEqual(result["reason"], "insufficient_case")

    def test_multi_allelic_models_are_not_cases(self):
        with tempfile.TemporaryDirectory() as directory:
            output = self.run_analysis(Path(directory), "multi", multi=True)
            result = json.loads((output / "result.json").read_text(encoding="utf-8"))
            self.assertEqual(result["reason"], "insufficient_case")
            self.assertEqual(result["multi_allelic_n"], 6)


if __name__ == "__main__":
    unittest.main()
