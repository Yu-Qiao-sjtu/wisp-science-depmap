import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class WindowsTauriCiContractTests(unittest.TestCase):
    def test_test_harness_embeds_common_controls_manifest_and_runs_in_ci(self):
        build_script = (ROOT / "src-tauri" / "build.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn("cargo:rustc-link-arg=/MANIFEST:EMBED", build_script)
        self.assertIn("cargo:rustc-link-arg=/MANIFESTINPUT:", build_script)

        manifest = (
            ROOT / "src-tauri" / "examples" / "webview_recovery_smoke.manifest"
        ).read_text(encoding="utf-8")
        self.assertIn('name="Microsoft.Windows.Common-Controls"', manifest)
        self.assertIn('version="6.0.0.0"', manifest)

        workflow = (ROOT / ".github" / "workflows" / "test.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("Run Windows Tauri library tests", workflow)
        self.assertIn("if: runner.os == 'Windows'", workflow)
        self.assertIn("cargo test -p wisp-tauri --lib", workflow)


if __name__ == "__main__":
    unittest.main()
