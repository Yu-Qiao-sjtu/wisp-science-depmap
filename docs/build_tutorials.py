"""Render the tutorial page from docs/wechat; run with --check to detect drift."""

import argparse
import struct
from html import escape
from pathlib import Path
from urllib.parse import urljoin, urlsplit

from markdown_it import MarkdownIt


DOCS = Path(__file__).resolve().parent
START = "<!-- BEGIN GENERATED TUTORIALS -->"
END = "<!-- END GENERATED TUTORIALS -->"
REPOSITORY = "https://github.com/xuzhougeng/wisp-science/blob/main/docs/"
READING_ORDER = [
    "wisp-science-models", "wisp-science-browser", "wisp-science-servers-cli",
    "wisp-science-transfer", "wisp-science-mcp", "wisp-science-skills",
    "wisp-science-trajectory",
]


def article_id(path):
    return path.stem


def rewrite_url(value, source, articles):
    if urlsplit(value).scheme or value.startswith(("//", "#")):
        return value
    resolved = urljoin(source.relative_to(DOCS).as_posix(), value)
    target = DOCS / urlsplit(resolved).path
    if target in articles:
        return "#" + article_id(target)
    if target.suffix == ".md":
        return REPOSITORY + resolved
    return resolved


def render_tutorials():
    articles = sorted((DOCS / "wechat").glob("*.md"), key=lambda path: (
        READING_ORDER.index(path.stem) if path.stem in READING_ORDER else len(READING_ORDER),
        path.name,
    ))
    if not articles:
        raise ValueError("No tutorials found in docs/wechat")
    parser = MarkdownIt("commonmark", {"html": False}).enable("table")
    cards, bodies = [], []
    for number, source in enumerate(articles, 1):
        tokens = parser.parse(source.read_text(encoding="utf-8"))
        if tokens[0].type != "heading_open" or tokens[0].tag != "h1":
            raise ValueError(f"{source.name} must begin with a title")
        title = escape(tokens[1].content)
        anchor = article_id(source)
        tokens = tokens[3:]
        # Rewrite parsed links/images only; code examples remain verbatim.
        for token in tokens:
            for child in token.children or []:
                attribute = {"link_open": "href", "image": "src"}.get(child.type)
                if attribute:
                    child.attrSet(attribute, rewrite_url(child.attrGet(attribute), source, articles))
                if child.type == "image":
                    child.attrSet("loading", "lazy")
                    # Reserve screenshot space before lazy loading, so directory
                    # jumps remain aligned when earlier images enter view.
                    image_path = DOCS / urlsplit(child.attrGet("src")).path
                    if image_path.is_file() and image_path.suffix.lower() == ".png":
                        with image_path.open("rb") as image_file:
                            header = image_file.read(24)
                        if len(header) == 24 and header[:8] == b"\x89PNG\r\n\x1a\n":
                            width, height = struct.unpack(">II", header[16:24])
                            child.attrSet("width", str(width))
                            child.attrSet("height", str(height))
            if token.type == "table_open":
                token.attrSet("class", "doc-table doc-table-compact")
        body = parser.renderer.render(tokens, parser.options, {})
        body = body.replace('<table class="doc-table doc-table-compact">',
                            '<div class="table-wrap" tabindex="0" role="region" aria-label="教程表格">'
                            '<table class="doc-table doc-table-compact">')
        body = body.replace("</table>", "</table></div>")
        cards.append(f'<a class="tutorial-card" href="#{anchor}">'
                     f'<span class="eyebrow">{number:02d}</span><h2>{title}</h2></a>')
        bodies.append(
            f'<article class="tutorial-article" id="{anchor}" aria-labelledby="{anchor}-title">\n'
            f'<header><h2 id="{anchor}-title">{title}</h2>\n'
            f'<a href="{REPOSITORY}wechat/{source.name}" data-i18n="tutorials.source">查看原文</a>'
            f'</header>\n{body}\n'
            '<a class="tutorial-back" href="#tutorial-list" data-i18n="tutorials.back">返回教程目录</a>\n'
            '</article>'
        )
    return ('<div class="tutorial-cards" lang="zh-CN">\n' + "\n".join(cards) + '</div>\n'
            '<div class="tutorial-articles" lang="zh-CN">\n' + "\n".join(bodies) + '</div>')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    page = DOCS / "tutorials.html"
    original = page.read_text(encoding="utf-8")
    before, rest = original.split(START)
    _, after = rest.split(END)
    updated = before + START + "\n" + render_tutorials() + "\n" + END + after
    if args.check:
        if updated != original:
            raise SystemExit("Tutorials are out of date; run python3 docs/build_tutorials.py")
    else:
        page.write_text(updated, encoding="utf-8")


if __name__ == "__main__":
    main()
