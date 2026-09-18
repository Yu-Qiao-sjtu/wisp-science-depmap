"""Bounded scientific-query control plane.

Issue write-ups (a gene, a lineage, a trajectory) are regression evidence.
This module owns the durable obligations every exact scientific lookup must
satisfy: coverage before biology, filter-before-limit, and never inferring a
numeric threshold from absence in a truncated ranking.
"""

from __future__ import annotations

from collections.abc import Callable, Iterable, Iterator
from typing import Any, TypeVar

T = TypeVar("T")

EVIDENCE_STATUSES = {
    "FOUND",
    "NOT_RETAINED",
    "INELIGIBLE",
    "NOT_COMPUTED",
    "MODULE_UNAVAILABLE",
    "NOT_OBSERVED",
    "COVERAGE_GAP",
}


def classify_coverage(
    *,
    module_present: bool,
    complete: bool = True,
    table_present: bool = True,
) -> str | None:
    """Return a coverage status, or None when the table may be queried."""
    if not module_present:
        return "MODULE_UNAVAILABLE"
    if not complete:
        return "NOT_COMPUTED"
    if not table_present:
        return "COVERAGE_GAP"
    return None


def classify_exact_entity(
    *,
    observed: bool,
    eligible: bool | None = None,
    retained: bool | None = None,
) -> str:
    """Classify one keyed entity after coverage has already succeeded.

    Absence from a shortlist is not an input. Callers must pass whether the
    full menu/universe contained the key (`observed`) and, separately, whether
    recorded criteria passed (`eligible`) and whether a sparse retained table
    kept the row (`retained`).
    """
    if not observed:
        return "NOT_OBSERVED"
    if eligible is False:
        return "INELIGIBLE"
    if retained is False:
        return "NOT_RETAINED"
    return "FOUND"


def filter_before_limit(
    rows: Iterable[T],
    match: Callable[[T], bool],
    *,
    limit: int | None = None,
) -> list[T]:
    """Keep matching rows before any bound. Limit never applies to non-matches."""
    kept: list[T] = []
    for row in rows:
        if not match(row):
            continue
        kept.append(row)
        if limit is not None and len(kept) >= limit:
            break
    return kept


def bound_after_rank(
    rows: list[T],
    *,
    key: Callable[[T], Any],
    limit: int,
) -> tuple[list[T], int]:
    """Rank the full match set, then bound. Returns (page, matched_count)."""
    ordered = sorted(rows, key=key)
    return ordered[:limit], len(ordered)


def criteria_failures(
    *,
    observed: dict[str, int | None],
    required: dict[str, int],
) -> list[str]:
    """Compare recorded counts to declared thresholds. Keys are criterion ids."""
    failures: list[str] = []
    for name, minimum in required.items():
        value = observed.get(name)
        if value is None:
            failures.append("COUNTS_UNAVAILABLE")
            break
        if value < minimum:
            failures.append(f"TOO_FEW_{name.upper()}")
    return failures
