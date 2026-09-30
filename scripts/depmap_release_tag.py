"""Choose the wisp-depmap release commit when a tag name also exists upstream."""

from __future__ import annotations

HISTORICAL_DEP_MAP_TAGS = {
    "v0.13.0": "79ad378c724a397f9b39ccf1b7db90d92622ceae",
}


class ReleaseTagError(Exception):
    """The requested tag does not identify one wisp-depmap release commit."""


def release_tag_allowed(tag: str) -> bool:
    """Future tags are depmap-v*. Only the published historical tag stays bare."""
    return tag.startswith("depmap-v") or tag in HISTORICAL_DEP_MAP_TAGS


def release_notes_tag(tag: str) -> str:
    """Release notes stay vX.Y.Z.md even when the git tag is depmap-vX.Y.Z."""
    if tag.startswith("depmap-"):
        return tag.removeprefix("depmap-")
    return tag


def select_release_commit(
    tag: str,
    origin_commit: str | None,
    upstream_commit: str | None = None,
) -> str:
    """Return the origin commit for a product tag.

    Bare ``v*`` names match upstream Wisp Science. Only the published historical
    tags are accepted, and only when ``origin_commit`` is that published object.
    A differing upstream object is ignored. Later releases must use ``depmap-v*``.
    """

    if tag.startswith("depmap-v"):
        if not origin_commit:
            raise ReleaseTagError(f"{tag} is not present on origin")
        return origin_commit

    expected = HISTORICAL_DEP_MAP_TAGS.get(tag)
    if expected is None:
        prefixed = tag if tag.startswith("v") else f"v{tag}"
        raise ReleaseTagError(
            f"{tag} uses the upstream Wisp Science tag namespace. "
            f"Publish this product as depmap-{prefixed}."
        )
    if not origin_commit:
        raise ReleaseTagError(
            f"{tag} must be read from origin. A local tag can point at upstream."
        )
    if not (origin_commit == expected or origin_commit.startswith(expected)):
        raise ReleaseTagError(
            f"origin {tag} is {origin_commit}, expected {expected}. "
            "Do not follow a local tag that points at a different object."
        )
    if upstream_commit and upstream_commit != origin_commit:
        return origin_commit
    return origin_commit
