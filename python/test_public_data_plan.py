"""Offline checks for the public-data skill's standalone planner workflow."""

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


PLANNER = (
    Path(__file__).resolve().parents[1]
    / "skills/public-data-access/scripts/public_data_plan.py"
)


class PublicDataPlanTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="wisp public data ")
        self.addCleanup(temporary.cleanup)
        self.project = Path(temporary.name)
        self.plan = self.project / "plans/geo.json"
        self.raw = self.project / "data/public/geo/GSE12345/raw"

    def run_planner(self, *args, expected_code=0):
        result = subprocess.run(
            [sys.executable, str(PLANNER), *args],
            cwd=self.project,
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(result.returncode, expected_code, result.stderr)
        return result

    def create_geo_plan(self, transport="https", expected_code=0):
        return self.run_planner(
            "init",
            "--provider", "geo",
            "--identifier", "GSE12345",
            "--data-type", "series-matrix",
            "--transport", transport,
            "--output-dir", "data/public/geo/GSE12345/raw",
            "--max-files", "2",
            "--max-bytes", "10MiB",
            "--no-resume",
            "--plan", "plans/geo.json",
            expected_code=expected_code,
        )

    def test_standalone_plan_validation_does_not_download_or_approve(self):
        self.create_geo_plan()
        plan = json.loads(self.plan.read_text(encoding="utf-8"))
        self.assertFalse(self.raw.exists())
        self.assertEqual(plan["dataset"]["provider"], "geo")
        self.assertEqual(plan["acquisition"]["transport"], "https")
        self.assertFalse(plan["acquisition"]["resume"])
        self.assertFalse(plan["acquisition"]["overwrite"])
        self.assertEqual(plan["acquisition"]["max_bytes"], 10 * 1024 * 1024)
        self.assertEqual(plan["approval"]["status"], "pending")
        before = self.plan.read_bytes()
        validation = self.run_planner("validate", "plans/geo.json")
        self.assertEqual(json.loads(validation.stdout)["errors"], [])
        self.assertEqual(self.plan.read_bytes(), before)
        self.assertFalse(self.raw.exists())
        self.create_geo_plan(expected_code=2)
        self.assertEqual(self.plan.read_bytes(), before)

    def test_adapter_provenance_is_bound_to_manifest_and_files_are_hashed(self):
        self.create_geo_plan()
        plan = json.loads(self.plan.read_text(encoding="utf-8"))
        plan["provenance"].update({
            "adapter": "geokit",
            "adapter_version": "test-fixture",
            "query_url": "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE12345",
        })
        # Synthetic local content: no provider download or R package required.
        self.plan.write_text(json.dumps(plan), encoding="utf-8")
        self.raw.mkdir(parents=True)
        content = b"ID_REF\tGSM100\nprobe_a\t1\n"
        (self.raw / "matrix.txt").write_bytes(content)
        self.run_planner("validate", "plans/geo.json")
        self.run_planner("manifest", "plans/geo.json")
        manifest_path = self.raw / "manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        self.assertEqual(manifest["provider"], "geo")
        self.assertEqual(manifest["identifier"], "GSE12345")
        self.assertEqual(manifest["summary"], {
            "file_count": 1, "total_bytes": len(content),
        })
        self.assertEqual(manifest["files"], [{
            "path": "matrix.txt",
            "bytes": len(content),
            "sha256": hashlib.sha256(content).hexdigest(),
        }])
        canonical_plan = json.dumps(
            plan, sort_keys=True, ensure_ascii=False, separators=(",", ":")
        ).encode("utf-8")
        self.assertEqual(
            manifest["plan_sha256"], hashlib.sha256(canonical_plan).hexdigest()
        )
        before = manifest_path.read_bytes()
        self.run_planner("manifest", "plans/geo.json", expected_code=2)
        self.assertEqual(manifest_path.read_bytes(), before)
        self.run_planner("manifest", "plans/geo.json", "--replace")
        refreshed = json.loads(manifest_path.read_text(encoding="utf-8"))
        self.assertEqual(refreshed["files"], manifest["files"])

    def test_package_name_is_not_a_geo_transport(self):
        self.create_geo_plan(transport="geokit", expected_code=2)
        self.assertFalse(self.plan.exists())
        self.assertFalse(self.raw.exists())


if __name__ == "__main__":
    unittest.main()
