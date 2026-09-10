import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.parse import urlsplit

import build_tutorials


class TutorialBuildTests(unittest.TestCase):
    def test_articles_keep_examples_and_rebase_links_and_images(self):
        with tempfile.TemporaryDirectory() as directory:
            docs = Path(directory)
            (docs / "wechat").mkdir()
            (docs / "wechat/first.md").write_text(
                '# First tutorial\n\n[Next](second.md)\n\n'
                '[Reference](../reference.md)\n\n[External](https://example.com/help)\n\n'
                '![Screenshot](../assets/demo.png)\n\n'
                '| Input | Result |\n| --- | --- |\n| A | B |\n\n'
                '```markdown\n# Example title\n[Next](second.md)\n<script>alert(1)</script>\n```\n\n'
                'The final paragraph is included.\n', encoding="utf-8"
            )
            (docs / "wechat/second.md").write_text('# Second tutorial\n\nAnother article.\n', encoding="utf-8")
            with patch.object(build_tutorials, "DOCS", docs):
                html = build_tutorials.render_tutorials()
            self.assertIn('href="#second">Next</a>', html)
            self.assertIn('href="' + build_tutorials.REPOSITORY + 'reference.md"', html)
            self.assertIn('href="https://example.com/help"', html)
            self.assertIn('src="assets/demo.png" alt="Screenshot" loading="lazy"', html)
            self.assertIn('<th>Input</th>', html)
            self.assertIn('<td>B</td>', html)
            self.assertIn('# Example title\n[Next](second.md)', html)
            self.assertIn('&lt;script&gt;alert(1)&lt;/script&gt;', html)
            self.assertNotIn('<script>', html)
            self.assertIn('The final paragraph is included.', html)
            self.assertIn('Another article.', html)

    def test_checked_in_page_matches_all_markdown_sources(self):
        page = (build_tutorials.DOCS / "tutorials.html").read_text(encoding="utf-8")
        generated = page.split(build_tutorials.START)[1].split(build_tutorials.END)[0]
        self.assertEqual(generated.strip(), build_tutorials.render_tutorials())

    def test_article_links_and_screenshots_exist(self):
        parser = build_tutorials.MarkdownIt("commonmark")
        for source in (build_tutorials.DOCS / "wechat").glob("*.md"):
            for token in parser.parse(source.read_text(encoding="utf-8")):
                for child in token.children or []:
                    attribute = {"link_open": "href", "image": "src"}.get(child.type)
                    if not attribute:
                        continue
                    value = child.attrGet(attribute)
                    if urlsplit(value).scheme or value.startswith(("//", "#")):
                        continue
                    target = source.parent / urlsplit(value).path
                    with self.subTest(article=source.name, target=value):
                        self.assertTrue(target.is_file(), f"Missing tutorial resource: {target}")


if __name__ == "__main__":
    unittest.main()
