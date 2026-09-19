import json
import tempfile
import unittest
from pathlib import Path

from scripts.upstream_base_monitor import load_marker, plan, version, write_marker


class UpstreamBaseMonitorTests(unittest.TestCase):
    def test_stable_semver_only(self):
        self.assertEqual(version("v1.14.2"), (1, 14, 2))
        for invalid in ("1.14.2", "v1.14", "v1.14.0-rc.1", "main"):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                version(invalid)

    def test_plan_detects_new_release_and_rejects_downgrade(self):
        marker = {
            "repository": "xuzhougeng/wisp-science",
            "tag": "v1.13.0",
            "commit": "old-sha",
            "channel": "stable-release",
        }
        self.assertFalse(plan(marker, "v1.13.0", "old-sha")["upgrade"])
        upgrade = plan(marker, "v1.14.0", "new-sha")
        self.assertTrue(upgrade["upgrade"])
        self.assertEqual(upgrade["branch"], "automation/upstream-v1.14.0")
        with self.assertRaises(ValueError):
            plan(marker, "v1.12.0", "older-sha")

    def test_marker_round_trip(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "upstream.json"
            write_marker(path, "xuzhougeng/wisp-science", "v1.14.0", "new-sha")
            marker = load_marker(path)
            self.assertEqual(marker["tag"], "v1.14.0")
            self.assertEqual(marker["commit"], "new-sha")
            self.assertNotIn("path", json.dumps(marker).lower())


if __name__ == "__main__":
    unittest.main()
