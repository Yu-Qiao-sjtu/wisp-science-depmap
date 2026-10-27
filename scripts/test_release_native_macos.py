"""DepMap does not publish macOS installers from the release workflow."""
from pathlib import Path
import unittest

SOURCE = Path(__file__).resolve().parent.parent


class SwiftUIReleaseRoutingTests(unittest.TestCase):
    def test_macos_release_workflow_does_not_publish_installers(self):
        workflow = (SOURCE / '.github/workflows/release-macos.yml').read_text()
        self.assertIn('wisp-depmap does not publish macOS installers.', workflow)
        self.assertNotIn('notarize', workflow.lower())
        self.assertNotIn('package_native_macos.py', workflow)


if __name__ == '__main__':
    unittest.main()
