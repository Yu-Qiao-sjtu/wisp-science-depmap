import unittest

from scripts.depmap_release_tag import (
    ReleaseTagError,
    release_notes_tag,
    release_tag_allowed,
    select_release_commit,
)

ORIGIN_V13 = "79ad378c724a397f9b39ccf1b7db90d92622ceae"
UPSTREAM_V13 = "a6a04c0432d6eed0e40e785ecb5f1e5a47505773"


class ReleaseTagTests(unittest.TestCase):
    def test_historical_tag_uses_origin_when_upstream_differs(self):
        selected = select_release_commit("v0.13.0", ORIGIN_V13, UPSTREAM_V13)
        self.assertEqual(selected, ORIGIN_V13)

    def test_historical_tag_rejects_the_upstream_object(self):
        with self.assertRaises(ReleaseTagError):
            select_release_commit("v0.13.0", UPSTREAM_V13, UPSTREAM_V13)

    def test_future_bare_tag_is_rejected(self):
        with self.assertRaises(ReleaseTagError):
            select_release_commit("v0.14.0", "abc", "def")

    def test_only_product_and_historical_tags_are_publishable(self):
        self.assertTrue(release_tag_allowed("depmap-v0.14.0"))
        self.assertTrue(release_tag_allowed("v0.13.0"))
        self.assertFalse(release_tag_allowed("v0.14.0"))
        self.assertEqual(release_notes_tag("depmap-v0.14.0"), "v0.14.0")
        self.assertEqual(release_notes_tag("v0.13.0"), "v0.13.0")

    def test_product_prefix_selects_origin(self):
        selected = select_release_commit("depmap-v0.14.0", "abc123", "def456")
        self.assertEqual(selected, "abc123")


if __name__ == "__main__":
    unittest.main()
