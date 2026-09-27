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

        cargo_toml = (ROOT / "src-tauri" / "Cargo.toml").read_text(
            encoding="utf-8"
        )
        bin_target = cargo_toml.split("[[bin]]", maxsplit=1)[1].split(
            "[build-dependencies]", maxsplit=1
        )[0]
        self.assertIn('name = "wisp-tauri"', bin_target)
        self.assertIn("test = false", bin_target)

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
