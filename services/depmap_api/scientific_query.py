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
    "NOT_TESTED",
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


def classify_tested_entity(
    *,
    in_table: bool,
    tested: bool | None,
    retained: bool | None,
) -> str:
    """Exact gene-in-lineage (or gene-across-lineages) after coverage succeeded.

    `in_table` means the full tested/untested universe contained the key.
    Truncated ranking absence is not an input.
    """
    if not in_table or tested is False:
        return "NOT_TESTED"
    if retained is False:
        return "NOT_RETAINED"
    return "FOUND"


def annotate_common_essential(
    rows: list[dict[str, Any]],
    *,
    labels: set[str] | None,
    source: str,
    exclude: bool,
    symbol_key: str = "symbol",
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    """Join one common-essential sidecar in a single pass. Housekeeping is out of scope."""
    if labels is None:
        annotated = [
            {**row, "is_common_essential": None, "common_essential_source": None}
            for row in rows
        ]
        return annotated, {
            "annotation_status": "ANNOTATION_UNAVAILABLE",
            "filter_applied": False,
            "before_count": len(annotated),
            "after_count": len(annotated),
            "removed_count": 0,
            "source": source,
        }
    annotated = []
    for row in rows:
        symbol = str(row.get(symbol_key) or "").strip().upper()
        flagged = symbol in labels
        annotated.append(
            {
                **row,
                "is_common_essential": flagged,
                "common_essential_source": source,
            }
        )
    before = len(annotated)
    kept = [row for row in annotated if not (exclude and row["is_common_essential"])]
    return kept, {
        "annotation_status": "AVAILABLE",
        "filter_applied": exclude,
        "before_count": before,
        "after_count": len(kept),
        "removed_count": before - len(kept),
        "source": source,
    }


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
