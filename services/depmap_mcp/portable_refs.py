"""Portable resource references for the DepMap MCP boundary."""

from __future__ import annotations

import re
from pathlib import Path, PurePosixPath
from typing import Any
from urllib.parse import quote, unquote


PRIVATE_LOCATION_TEXT = "[private location omitted]"


def opaque_location() -> dict[str, str]:
    """Return a path-free record for a location that cannot be disclosed."""

    return {
        "reference_type": "opaque_location",
        "state": "OMITTED",
        "reason": "absolute path outside the public knowledge root",
    }


class PortableReferences:
    """Normalize public knowledge-root references and hide private locations."""

    _ABSOLUTE_PATTERNS = (
        re.compile(r"(?<![\w:])[A-Za-z]:[\\/][^\s\"'<>|,;\]\)}]+"),
        re.compile(r"(?<![\w:])\\\\[^\s\\/]+[\\/][^\s\\/]+(?:[\\/][^\s\"'<>|,;\]\)}]+)*"),
        re.compile(r"(?<![\w:])//[^\s/]+/[^\s/]+(?:/[^\s\"'<>|,;\]\)}]+)*"),
        re.compile(r"(?<![\w:/])/[^/\s\"'<>|,;\]\)}]+(?:/[^/\s\"'<>|,;\]\)}]+)*"),
    )
    _PUBLIC_URI_PATTERN = re.compile(r"depmap://[^\s\"'<>|,;\]\)}]+")

    def __init__(self, knowledge_root: Path, release: str) -> None:
        self.knowledge_root = knowledge_root.resolve()
        self.release = release
        self.uri_prefix = f"depmap://{release}/"
        self._root_variants = tuple(
            sorted(
                {
                    str(self.knowledge_root).rstrip("\\/"),
                    self.knowledge_root.as_posix().rstrip("/"),
                },
                key=len,
                reverse=True,
            )
        )

    @staticmethod
    def _is_absolute(value: str) -> bool:
        return bool(
            re.fullmatch(r"[A-Za-z]:[\\/].+", value)
            or re.fullmatch(r"\\\\[^\\/]+[\\/][^\\/]+(?:[\\/].*)?", value)
            or re.fullmatch(r"//[^/]+/[^/]+(?:/.*)?", value)
            or re.fullmatch(r"/[^/\r\n]+(?:/[^/\r\n]+)*", value)
        )

    @staticmethod
    def _normalize_relative(value: str) -> str | None:
        normalized = value.replace("\\", "/").lstrip("/")
        path = PurePosixPath(normalized)
        if not normalized or path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
            return None
        return path.as_posix()

    def public_uri(self, relative: str) -> str | None:
        normalized = self._normalize_relative(relative)
        if normalized is None or self._is_absolute(relative.strip()):
            return None
        return self.uri_prefix + quote(normalized, safe="/-._~")

    def _exact_internal_relative(self, value: str) -> str | None:
        comparable = value.replace("\\", "/")
        for root in self._root_variants:
            root_comparable = root.replace("\\", "/")
            if comparable.casefold() == root_comparable.casefold():
                return ""
            prefix = root_comparable + "/"
            if comparable.casefold().startswith(prefix.casefold()):
                return comparable[len(prefix) :]
        return None

    def parse_public_uri(self, uri: str) -> str | None:
        if not uri.startswith(self.uri_prefix):
            return None
        relative = unquote(uri[len(self.uri_prefix) :])
        normalized = self._normalize_relative(relative)
        if normalized is None or self._is_absolute(relative.strip()):
            return None
        return normalized

    def _replace_public_roots(self, value: str) -> str:
        safe = value
        for root in self._root_variants:
            if not root:
                continue
            pattern = re.compile(
                re.escape(root)
                + r"(?=$|[\\/])(?P<tail>(?:[\\/][^\s\"'<>|,;\]\)}]*)?)",
                re.IGNORECASE,
            )

            def replace_root(match: re.Match[str]) -> str:
                relative = match.group("tail").lstrip("\\/").replace("\\", "/")
                return self.public_uri(relative) if relative else self.uri_prefix.rstrip("/")

            safe = pattern.sub(replace_root, safe)
        return safe

    def _replace_uri_tokens(self, value: str) -> str:
        def replace_uri(match: re.Match[str]) -> str:
            relative = self.parse_public_uri(match.group(0))
            return (
                self.public_uri(relative)
                if relative is not None
                else PRIVATE_LOCATION_TEXT
            )

        return self._PUBLIC_URI_PATTERN.sub(replace_uri, value)

    def text(self, value: str) -> str:
        """Sanitize a free-text preview without changing its JSON type."""

        safe = self._replace_uri_tokens(self._replace_public_roots(value))
        for pattern in self._ABSOLUTE_PATTERNS:
            safe = pattern.sub(PRIVATE_LOCATION_TEXT, safe)
        return safe

    def value(self, value: str) -> str | dict[str, str]:
        stripped = value.strip()
        if stripped.startswith("depmap://"):
            relative = self.parse_public_uri(stripped)
            return self.public_uri(relative) if relative is not None else opaque_location()

        internal_relative = self._exact_internal_relative(stripped)
        if internal_relative is not None:
            return (
                self.public_uri(internal_relative)
                if internal_relative
                else self.uri_prefix.rstrip("/")
            )

        safe = self._replace_public_roots(value)
        safe_stripped = safe.strip()
        if safe_stripped.startswith("depmap://") and self.parse_public_uri(safe_stripped) is not None:
            return safe_stripped
        if self._is_absolute(safe_stripped):
            return opaque_location()

        return self.text(safe)

    def key(self, value: str) -> str:
        portable = self.value(value)
        return portable if isinstance(portable, str) else PRIVATE_LOCATION_TEXT

    def catalog_reference(self, value: str) -> str | dict[str, str]:
        stripped = value.strip()
        if stripped.startswith("depmap://") or self._is_absolute(stripped):
            portable = self.value(stripped)
            return portable
        uri = self.public_uri(stripped)
        return uri if uri is not None else opaque_location()
