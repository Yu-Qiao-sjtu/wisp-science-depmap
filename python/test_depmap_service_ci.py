import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "test.yml"


class DepMapServiceCiContractTests(unittest.TestCase):
    def test_service_suites_are_present_and_wired_into_ci(self):
        expected_suites = (
            "services/depmap_mcp/tests",
            "services/depmap_api/tests",
        )
        for relative in expected_suites:
            suite = ROOT / relative
            self.assertTrue(suite.is_dir(), f"missing service test suite: {relative}")
            self.assertTrue(
                any(suite.glob("test_*.py")),
                f"service test suite has no test_*.py files: {relative}",
            )

        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("depmap-service-contracts:", workflow)
        self.assertIn("os: [ubuntu-latest, windows-latest]", workflow)
        self.assertIn(
            "python -m pip install -r services/depmap_mcp/requirements.txt",
            workflow,
        )
        for relative in expected_suites:
            command = (
                f'python -m unittest discover -s {relative} '
                '-p "test_*.py" -v'
            )
            self.assertIn(command, workflow)


if __name__ == "__main__":
    unittest.main()
