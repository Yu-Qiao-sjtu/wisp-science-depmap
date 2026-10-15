#!/usr/bin/env python3
"""Standard-library regression tests for the offline docking verifier."""

from __future__ import annotations

import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().with_name("verify.py")
SPEC = importlib.util.spec_from_file_location("docking_fixture_verify", MODULE_PATH)
assert SPEC and SPEC.loader
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)
GENERATOR_PATH = MODULE_PATH.parents[2] / "generate_docking_scoring_fixtures.py"
GENERATOR_SPEC = importlib.util.spec_from_file_location(
    "docking_fixture_generator", GENERATOR_PATH
)
assert GENERATOR_SPEC and GENERATOR_SPEC.loader
GENERATOR = importlib.util.module_from_spec(GENERATOR_SPEC)
GENERATOR_SPEC.loader.exec_module(GENERATOR)


class DockingFixtureManifestTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = VERIFY.load_json(VERIFY.MANIFEST_PATH)
        cls.cases = {case["id"]: case for case in cls.manifest["scoring_cases"]}

    def test_manifest_and_checksums_verify_offline(self) -> None:
        fixtures, cases = VERIFY.verify()
        self.assertEqual(fixtures, 26)
        self.assertEqual(cases, 8)

    def test_all_oracles_are_generated_and_manifest_managed(self) -> None:
        fixtures = {entry["id"]: entry for entry in self.manifest["fixtures"]}
        for case in self.manifest["scoring_cases"]:
            self.assertEqual(case["oracle_status"], "generated")
            self.assertEqual(set(case["expected"]), set(VERIFY.EXPECTED_FIELDS))
            fixture_id = f"oracle-{case['id']}"
            self.assertIn(fixture_id, fixtures)
            self.assertEqual(fixtures[fixture_id]["path"], case["oracle_output_path"])
            self.assertEqual(fixtures[fixture_id]["kind"], "oracle_json")
            self.assertEqual(fixtures[fixture_id]["origin"], "project_generated")

    def test_case_box_seed_method_and_version_are_pinned(self) -> None:
        expected_geometry = {
            "clash-vina": ([0, 0, 0], [8, 8, 8]),
            "clash-vinardo": ([0, 0, 0], [8, 8, 8]),
            "carbon-contact-vina": ([3.8, 0, 0], [8, 8, 8]),
            "carbon-contact-vinardo": ([3.8, 0, 0], [8, 8, 8]),
            "hbond-hd-vina": ([3, 0, 0], [8, 8, 8]),
            "hbond-hd-vinardo": ([3, 0, 0], [8, 8, 8]),
            "1iep-vina": ([15.19, 53.9025, 16.917], [20, 28, 24]),
            "1iep-vinardo": ([15.19, 53.9025, 16.917], [20, 28, 24]),
        }
        self.assertEqual(set(self.cases), set(expected_geometry))
        for case_id, (center, size) in expected_geometry.items():
            case = self.cases[case_id]
            self.assertEqual(case["box_center"], center)
            self.assertEqual(case["box_size"], size)
            self.assertEqual(case["spacing"], 0.375)
            self.assertEqual(case["seed"], 1)
            self.assertEqual(case["oracle_method"], "vina.Vina.score()")
            self.assertEqual(case["oracle_version"], "1.2.7")

    def test_clash_fixture_is_coincident_and_exercises_positive_soft_curl(self) -> None:
        fixtures = {entry["id"]: entry for entry in self.manifest["fixtures"]}
        for fixture_id in ("clash-receptor", "clash-ligand"):
            entry = fixtures[fixture_id]
            self.assertEqual(entry["expected"]["coincident_pair_position_angstrom"], [0, 0, 0])
            atom = VERIFY.atom_lines(VERIFY.safe_path(entry["path"]))[0]
            self.assertEqual(
                [float(atom[30:38]), float(atom[38:46]), float(atom[46:54])],
                [0.0, 0.0, 0.0],
            )
        for case_id in ("clash-vina", "clash-vinardo"):
            case = self.cases[case_id]
            self.assertGreater(case["expected"]["estimated_free_energy_of_binding_kcal_mol"], 0)
            self.assertGreater(case["expected"]["final_intermolecular_energy_kcal_mol"], 0)

    def test_nested_branch_fixture_has_body_atoms_and_preserves_order_regression(self) -> None:
        entry = next(
            item for item in self.manifest["fixtures"] if item["id"] == "branch-order-nested"
        )
        self.assertEqual(entry["expected"]["atom_serial_order"], [10, 30, 20, 25, 40])
        self.assertEqual(entry["expected"]["branch_parents"], [None, 0])
        path = VERIFY.safe_path(entry["path"])
        VERIFY.require_branch_bodies(path)
        serials = [int(line[6:11]) for line in VERIFY.atom_lines(path)]
        self.assertLess(serials.index(20), serials.index(40))
        self.assertLess(serials.index(25), serials.index(40))

    def test_exact_raw_vectors_and_seven_field_mapping(self) -> None:
        exact = {
            "clash-vina": [11.415, 11.415, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "clash-vinardo": [12.064, 12.064, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "carbon-contact-vina": [-0.071, -0.071, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "carbon-contact-vinardo": [-0.046, -0.046, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "hbond-hd-vina": [-0.223, -0.223, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "hbond-hd-vinardo": [-0.266, -0.266, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "1iep-vina": [-12.513, -17.634, 0.0, 0.0, 0.0, -0.485, 5.121, -0.485],
            "1iep-vinardo": [-9.18, -12.936, 0.0, 0.0, 0.0, -0.457, 3.756, -0.457],
        }
        for case_id, raw in exact.items():
            output = VERIFY.load_json(VERIFY.safe_path(self.cases[case_id]["oracle_output_path"]))
            self.assertEqual(output["api_result"]["raw_vector_columns"], VERIFY.RAW_COLUMNS)
            self.assertEqual(output["api_result"]["raw_vector_kcal_mol"], raw)
            self.assertEqual(output["expected_mapping"], VERIFY.EXPECTED_MAPPING)
            self.assertEqual(output["expected"], VERIFY.expected_from_raw(raw))
            self.assertEqual(self.cases[case_id]["expected"], output["expected"])

    def test_nonzero_other_inter_maps_to_internal_not_intermolecular(self) -> None:
        raw = [1.111, 2.222, 3.333, 4.444, 5.555, 6.666, 7.777, 8.888]
        self.assertEqual(
            VERIFY.expected_from_raw(raw),
            {
                "estimated_free_energy_of_binding_kcal_mol": 1.111,
                "final_intermolecular_energy_kcal_mol": 5.555,
                "ligand_receptor_energy_kcal_mol": 2.222,
                "final_total_internal_energy_kcal_mol": 16.665,
                "ligand_internal_energy_kcal_mol": 6.666,
                "torsional_free_energy_kcal_mol": 7.777,
                "unbound_system_energy_kcal_mol": 8.888,
            },
        )

    def test_generator_and_verifier_share_exact_mapping_semantics(self) -> None:
        raw = [1.111, 2.222, 3.333, 4.444, 5.555, 6.666, 7.777, 8.888]
        self.assertEqual(GENERATOR.EXPECTED_MAPPING, VERIFY.EXPECTED_MAPPING)
        self.assertEqual(GENERATOR.map_expected(raw), VERIFY.expected_from_raw(raw))

    def test_official_git_blob_oids_match_fixture_bytes(self) -> None:
        checked = []
        for entry in self.manifest["fixtures"]:
            source = entry["source"]
            if entry["origin"] == "official_upstream" and "git_blob" in source:
                payload = VERIFY.safe_path(entry["path"]).read_bytes()
                self.assertEqual(VERIFY.git_blob_oid(payload), source["git_blob"])
                checked.append(entry["id"])
        self.assertEqual(
            checked,
            [
                "license-autodock-vina",
                "vina-1iep-ligand",
                "vina-1iep-receptor",
                "vina-1iep-receptor-pdb",
            ],
        )

    def test_wheel_identity_versions_and_install_provenance_are_pinned(self) -> None:
        self.assertEqual(self.manifest["oracle"]["wheel_artifacts"], VERIFY.WHEEL_ARTIFACTS)
        for case in self.manifest["scoring_cases"]:
            output = VERIFY.load_json(VERIFY.safe_path(case["oracle_output_path"]))
            for package, expected in VERIFY.WHEEL_ARTIFACTS.items():
                record = output["packages"][package]
                self.assertEqual(record["distribution_version"], expected["version"])
                self.assertEqual(record["module_version"], expected["version"])
                self.assertEqual(record["wheel_filename"], expected["filename"])
                self.assertEqual(record["wheel_sha256"], expected["sha256"])
                self.assertEqual(
                    record["installation_provenance"],
                    {
                        "method": "direct_url_archive_hash",
                        "wheel_filename": expected["filename"],
                        "sha256": expected["sha256"],
                    },
                )

    def test_oracle_json_contains_no_host_absolute_paths(self) -> None:
        for case in self.manifest["scoring_cases"]:
            output = VERIFY.load_json(VERIFY.safe_path(case["oracle_output_path"]))
            VERIFY.assert_no_host_absolute_paths(output, case["id"])

    def test_verifier_rejects_tampered_raw_vector_mapping(self) -> None:
        case = copy.deepcopy(self.cases["1iep-vina"])
        original = VERIFY.load_json(VERIFY.safe_path(case["oracle_output_path"]))
        tampered = copy.deepcopy(original)
        tampered["expected"]["final_intermolecular_energy_kcal_mol"] = 0.0
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "oracle.json"
            path.write_text(json.dumps(tampered), encoding="utf-8")
            with self.assertRaisesRegex(VERIFY.VerificationError, "oracle expected mapping"):
                VERIFY.check_oracle_output(
                    case,
                    path,
                    {entry["id"]: entry for entry in self.manifest["fixtures"]},
                )

    def test_git_blob_oid_detects_tampered_payload(self) -> None:
        entry = next(
            item for item in self.manifest["fixtures"] if item["id"] == "vina-1iep-ligand"
        )
        payload = VERIFY.safe_path(entry["path"]).read_bytes()
        self.assertEqual(GENERATOR.git_blob_oid(payload), VERIFY.git_blob_oid(payload))
        self.assertNotEqual(
            VERIFY.git_blob_oid(payload + b"\n"),
            entry["source"]["git_blob"],
        )
        with self.assertRaisesRegex(ValueError, "Git blob OID mismatch"):
            GENERATOR.verify_official_git_blob(entry, payload + b"\n")

    def test_selected_case_preflight_checks_every_input_before_generation(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        selected = {"1iep-vina", "clash-vina"}
        prepared = GENERATOR.preflight_selected_cases(VERIFY.ROOT, manifest, selected)
        self.assertEqual(
            [item["case"]["id"] for item in prepared],
            ["1iep-vina", "clash-vina"],
        )
        official_entry = next(
            item for item in manifest["fixtures"] if item["id"] == "vina-1iep-receptor"
        )
        official_entry["source"]["git_blob"] = "0" * 40
        with self.assertRaisesRegex(ValueError, "Git blob OID mismatch"):
            GENERATOR.preflight_selected_cases(VERIFY.ROOT, manifest, selected)

    def test_preflight_failure_causes_zero_vina_calls_and_zero_oracle_writes(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        selected = {"1iep-vina", "clash-vina"}
        official_entry = next(
            item for item in manifest["fixtures"] if item["id"] == "vina-1iep-ligand"
        )
        official_entry["source"]["git_blob"] = "0" * 40
        vina_calls = []
        writes = []
        with self.assertRaisesRegex(ValueError, "Git blob OID mismatch"):
            prepared = GENERATOR.preflight_selected_cases(VERIFY.ROOT, manifest, selected)
            for item in prepared:
                vina_calls.append(item["case"]["id"])
                writes.append(item["destination"])
        self.assertEqual(vina_calls, [])
        self.assertEqual(writes, [])

    def test_generator_refresh_rejects_tampered_official_blob_before_rehash(self) -> None:
        entry = next(
            copy.deepcopy(item)
            for item in self.manifest["fixtures"]
            if item["id"] == "vina-1iep-ligand"
        )
        original_sha = entry["sha256"]
        original_bytes = entry["bytes"]
        payload = VERIFY.safe_path(entry["path"]).read_bytes() + b"\n"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root.joinpath(*entry["path"].split("/"))
            path.parent.mkdir(parents=True)
            path.write_bytes(payload)
            with self.assertRaisesRegex(ValueError, "Git blob OID mismatch"):
                GENERATOR.refresh_fixture_hashes(root, {"fixtures": [entry]})
        self.assertEqual(entry["sha256"], original_sha)
        self.assertEqual(entry["bytes"], original_bytes)

    def test_verifier_rejects_host_absolute_path_in_oracle(self) -> None:
        with self.assertRaisesRegex(VERIFY.VerificationError, "host absolute path"):
            VERIFY.assert_no_host_absolute_paths(
                {"python_path": "/mnt/d/private/venv/bin/python"}, "test"
            )
        with self.assertRaisesRegex(VERIFY.VerificationError, "host absolute path"):
            VERIFY.assert_no_host_absolute_paths(
                {"wheel_path": "D:\\private\\vina.whl"}, "test"
            )

    def test_vina_and_vinardo_tolerances_are_independent_objects(self) -> None:
        self.assertEqual(set(self.manifest["tolerances"]), {"vina", "vinardo"})
        self.assertIsNot(
            self.manifest["tolerances"]["vina"],
            self.manifest["tolerances"]["vinardo"],
        )


if __name__ == "__main__":
    unittest.main()
