"""Catalog-driven dispatch for bounded DepMap scientific readers."""

from __future__ import annotations

import logging
import sqlite3
from contextlib import closing
from dataclasses import asdict, dataclass, replace
from pathlib import Path
from typing import Any, Awaitable, Callable

from services.depmap_mcp.artifact_integrity import (
    VERIFIED,
    reader_artifact_pattern,
    resolve_index_artifact,
    sqlite_like_pattern,
    verify_cataloged_artifact,
)
from services.depmap_mcp.portable_refs import PortableReferences


Runner = Callable[[Any, dict[str, Any]], Awaitable[dict[str, Any] | None]]
LOGGER = logging.getLogger("depmap_mcp.catalog_readers")

# API modes are more granular than the public intent catalog. Each one must
# still enter through a registered reader family before opening retained data.
MODE_ALIASES = {
    "catalog": "core",
    "core": "core",
    "model_gene_effect": "model_gene_effect",
    "cross_platform_validation": "cross_platform_validation",
    "top": "top",
    "lineage": "lineage",
    "pathway": "pathway",
    "drug": "drug",
    "lineage_network": "lineage_network",
    "lineage_cnv": "lineage_cnv",
    "lineage_drug": "lineage_drug",
    "enrichment": "enrichment",
    "subtype": "subtype",
    "coamplification": "coamplification",
    "three_d": "three_d",
    "tcga_expression_survival": "tcga_expression_survival",
    "gene": "core",
    "pair": "pair",
    "lineage_catalog": "lineage_catalog",
    "lineage_dependency": "lineage_dependency",
    "pan_cancer_dependency": "pan_cancer_dependency",
    "lineage_directions": "lineage_directions",
    "mutation_anchor": "mutation_anchor",
    "lineage_mutation_dependency": "lineage_mutation_dependency",
    "synthetic_lethal": "synthetic_lethal",
    "tf_dependency": "tf_dependency",
    "pathway_dependency": "pathway_dependency",
    "biomarker_target": "biomarker_target",
    "true_love": "true_love",
    "analysis_catalog": "analysis_catalog",
}


@dataclass(frozen=True)
class CatalogResolution:
    state: str
    query_mode: str
    reader_mode: str | None = None
    reader_id: str | None = None
    analysis_ids: tuple[str, ...] = ()
    artifact_uris: tuple[str, ...] = ()
    matrix_blocks: tuple[str, ...] = ()
    validated_provenance_count: int = 0
    reason: str | None = None

    def evidence(self, release: str, knowledge_root: Path) -> dict[str, Any]:
        value = asdict(self)
        references = PortableReferences(knowledge_root, release)
        value["artifact_uris"] = [
            references.catalog_reference(path) for path in self.artifact_uris
        ]
        value["matrix_blocks"] = [
            references.catalog_reference(path) for path in self.matrix_blocks
        ]
        return value


class CatalogReaderRegistry:
    """Resolve every bounded query through the SQLite reader catalog."""

    def __init__(self, knowledge_root: Path, release: str) -> None:
        self.knowledge_root = knowledge_root.resolve()
        self.release = release
        self.index = self.knowledge_root / "depmap-26q1-query-index.sqlite"

    @property
    def enabled(self) -> bool:
        return self.index.is_file()

    def _verify_index(self):
        return resolve_index_artifact(self.index)

    def resolve(self, query: dict[str, Any]) -> CatalogResolution:
        mode = str(query.get("mode") or "")
        reader_mode = MODE_ALIASES.get(mode, mode)
        if not self.enabled:
            return CatalogResolution("CATALOG_UNAVAILABLE", mode, reason="query index is not installed")
        active_index, index_integrity = self._verify_index()
        if index_integrity.state != VERIFIED:
            LOGGER.error(
                "query index integrity failure reason=%s diagnostic=%s",
                index_integrity.reason_code,
                index_integrity.diagnostic,
            )
            return CatalogResolution(
                "CATALOG_ERROR",
                mode,
                reader_mode=reader_mode,
                reason="query index integrity validation failed",
            )
        try:
            with closing(sqlite3.connect(f"file:{active_index.as_posix()}?mode=ro&immutable=1", uri=True)) as db:
                db.row_factory = sqlite3.Row
                reader = db.execute(
                    "SELECT r.query_mode,r.adapter,r.module_pattern,"
                    "rc.analysis_id AS coverage_analysis_id,ca.release AS coverage_release,"
                    "rc.coverage_state FROM reader_registry r LEFT JOIN reader_coverage rc "
                    "ON rc.query_mode=r.query_mode LEFT JOIN analysis_catalog ca "
                    "ON ca.analysis_id=rc.analysis_id WHERE r.query_mode=?",
                    (reader_mode,),
                ).fetchone()
                if reader is None:
                    return CatalogResolution("READER_UNAVAILABLE", mode, reader_mode=reader_mode, reason="no registered reader")
                if reader["coverage_state"] != "AVAILABLE" or not reader["coverage_analysis_id"]:
                    state = (
                        "CORRUPT_ARTIFACT"
                        if reader["coverage_state"] == "CORRUPT_ARTIFACT"
                        else "NOT_INDEXED"
                    )
                    return CatalogResolution(
                        state, mode, reader_mode, reader["adapter"],
                        reason=(
                            "registered artifact failed integrity validation"
                            if state == "CORRUPT_ARTIFACT"
                            else "registered reader has no current release-scoped coverage record"
                        ),
                    )
                likes = []
                for pattern in str(reader["module_pattern"]).split("|"):
                    like = sqlite_like_pattern(pattern)
                    if not like:
                        continue
                    like = like if "%" in like else f"%{like}%"
                    likes.append(like)
                    if like.endswith("/%"):
                        likes.append(like[:-2])
                predicates = " OR ".join(
                    "module LIKE ? ESCAPE '\\' OR analysis_unit LIKE ? ESCAPE '\\'"
                    for _ in likes
                )
                parameters = tuple(value for like in likes for value in (like, like))
                requested_module = str(query.get("module") or query.get("family") or "").strip()
                base_predicates, base_parameters = predicates, parameters
                if requested_module:
                    predicates = f"({predicates}) AND analysis_unit LIKE ? ESCAPE '\\'"
                    parameters = (
                        *parameters,
                        sqlite_like_pattern(f"*{requested_module}*"),
                    )
                analyses = db.execute(
                    f"""
                    SELECT analysis_id FROM analysis_catalog
                    WHERE completion_state='COMPLETE' AND (release=? OR release IS NULL)
                      AND ({predicates})
                    ORDER BY manifest_mtime_ns DESC LIMIT 32
                    """,
                    (self.release, *parameters),
                ).fetchall()
                if not analyses and requested_module:
                    analyses = db.execute(
                        f"""SELECT analysis_id FROM analysis_catalog
                        WHERE completion_state='COMPLETE' AND (release=? OR release IS NULL)
                          AND ({base_predicates})
                        ORDER BY manifest_mtime_ns DESC LIMIT 32""",
                        (self.release, *base_parameters),
                    ).fetchall()
                analysis_ids = tuple(
                    dict.fromkeys(
                        (
                            [reader["coverage_analysis_id"]]
                            if reader["coverage_analysis_id"]
                            and reader["coverage_release"] in (None, self.release)
                            else []
                        )
                        + [row[0] for row in analyses]
                    )
                )
                artifacts: tuple[str, ...] = ()
                if analysis_ids:
                    placeholders = ",".join("?" for _ in analysis_ids)
                    artifact_likes = tuple(
                        sqlite_like_pattern(part)
                        for part in reader_artifact_pattern(
                            reader_mode,
                            str(reader["module_pattern"]),
                        ).split("|")
                        if part.strip()
                    )
                    artifact_predicates = " OR ".join(
                        "f.artifact_path LIKE ? ESCAPE '\\'"
                        for _ in artifact_likes
                    )
                    artifacts = tuple(
                        row[0]
                        for row in db.execute(
                            f"""
                            SELECT f.artifact_path FROM artifact_catalog f
                            WHERE f.analysis_id IN ({placeholders})
                              AND f.integrity_state='VERIFIED'
                              AND ({artifact_predicates})
                            ORDER BY f.artifact_path
                            """,
                            (*analysis_ids, *artifact_likes),
                        )
                    )
                genes = {
                    str(query[key]).upper()
                    for key in ("gene", "source", "target", "transcription_factor")
                    if query.get(key)
                }
                blocks: tuple[str, ...] = ()
                if genes:
                    gene_placeholders = ",".join("?" for _ in genes)
                    analysis_placeholders = ",".join("?" for _ in analysis_ids)
                    blocks = tuple(
                        row[0]
                        for row in db.execute(
                            f"""SELECT DISTINCT block_path FROM matrix_block_index
                            WHERE gene IN ({gene_placeholders})
                              AND analysis_id IN ({analysis_placeholders})
                            ORDER BY block_path LIMIT 32""",
                            (*sorted(genes), *analysis_ids),
                        )
                    ) if analysis_ids else ()
                # Indexed-content readers legitimately query SQLite content tables
                # and do not need a file candidate for each returned row.
                indexed = reader_mode in {"true_love", "tf_dependency", "biomarker_target"}
                if not analysis_ids and not indexed:
                    return CatalogResolution("NOT_INDEXED", mode, reader_mode, reader["adapter"], reason="no COMPLETE matching analysis")
                return CatalogResolution("RESOLVED", mode, reader_mode, reader["adapter"], analysis_ids, artifacts, blocks)
        except sqlite3.Error:
            LOGGER.exception("catalog reader resolution failed mode=%s", reader_mode)
            return CatalogResolution(
                "CATALOG_ERROR",
                mode,
                reader_mode=reader_mode,
                reason="catalog reader resolution failed",
            )

    async def read(
        self,
        settings: Any,
        query: dict[str, Any],
        runner: Runner,
    ) -> tuple[CatalogResolution, dict[str, Any]]:
        resolution = self.resolve(query)
        # Tests and local development may run without the optional index. In a
        # deployed indexed knowledge base, missing/broken routing is terminal.
        if self.enabled and resolution.state != "RESOLVED":
            reason_code = resolution.state
            if resolution.state == "CATALOG_ERROR":
                _active_index, index_integrity = self._verify_index()
                if index_integrity.state != VERIFIED:
                    reason_code = (
                        index_integrity.reason_code
                        or "INTEGRITY_CATALOG_UNAVAILABLE"
                    )
            return resolution, {
                "status": "MODULE_UNAVAILABLE",
                "reason_code": reason_code,
            }
        if self.enabled:
            _found, error = self._revalidate_cataloged_paths(
                resolution.artifact_uris,
                resolution.analysis_ids,
            )
            if error:
                return resolution, {
                    "status": "MODULE_UNAVAILABLE",
                    "reason_code": error,
                }
        bound_query = {
            **query,
            "_catalog_analysis_ids": list(resolution.analysis_ids),
            "_catalog_artifacts": list(resolution.artifact_uris),
            "_catalog_matrix_blocks": list(resolution.matrix_blocks),
            "_catalog_reader_id": resolution.reader_id,
        }
        result = await runner(settings, bound_query if self.enabled else query)
        if result is None:
            return resolution, {
                "status": "NOT_RETAINED",
                "reason": "the catalog adapter returned no result for this bounded query",
                "rows": [],
                "returned_count": 0,
            }
        if not isinstance(result, dict):
            return resolution, {
                "status": "MODULE_UNAVAILABLE",
                "reason": "the catalog adapter returned an invalid result type",
            }
        if self.enabled:
            resolution, error = self._bind_result_provenance(resolution, result)
            if error:
                return resolution, {
                    "status": "MODULE_UNAVAILABLE",
                    "reason_code": error,
                }
        return resolution, result

    def _bind_result_provenance(
        self, resolution: CatalogResolution, result: Any
    ) -> tuple[CatalogResolution, str | None]:
        """Bind the adapter's actual inputs back to COMPLETE indexed artifacts."""
        values: list[str] = []

        def visit(value: Any) -> None:
            if isinstance(value, dict):
                for key, item in value.items():
                    if key == "provenance" and isinstance(item, str):
                        values.append(item)
                    elif key == "provenance" and isinstance(item, list):
                        values.extend(str(path) for path in item if isinstance(path, str))
                    else:
                        visit(item)
            elif isinstance(value, list):
                for item in value:
                    visit(item)

        visit(result)
        if not values:
            return resolution, None
        relative: list[str] = []
        validated_index_count = 0
        prefix = f"depmap://{self.release}/"
        for value in values:
            if value.startswith(prefix):
                path = value[len(prefix):]
            else:
                candidate = Path(value)
                try:
                    path = candidate.resolve().relative_to(self.knowledge_root).as_posix()
                except (OSError, ValueError):
                    LOGGER.error(
                        "reader returned provenance outside the knowledge root path=%r",
                        value,
                    )
                    return resolution, "PROVENANCE_OUTSIDE_KNOWLEDGE_ROOT"
            if path == self.index.name:
                _active_index, index_integrity = self._verify_index()
                if index_integrity.state != VERIFIED:
                    LOGGER.error(
                        "query index provenance integrity failure reason=%s diagnostic=%s",
                        index_integrity.reason_code,
                        index_integrity.diagnostic,
                    )
                    return (
                        resolution,
                        index_integrity.reason_code or "INTEGRITY_CATALOG_UNAVAILABLE",
                    )
                validated_index_count += 1
                continue
            relative.append(path)
        if not relative:
            return replace(
                resolution,
                validated_provenance_count=validated_index_count,
            ), None
        unique = tuple(dict.fromkeys(relative))
        found, error = self._revalidate_cataloged_paths(
            unique,
            resolution.analysis_ids,
        )
        if error:
            return resolution, error
        assert found is not None
        return replace(
            resolution,
            analysis_ids=tuple(dict.fromkeys(found[path][1] for path in unique)),
            artifact_uris=unique,
            validated_provenance_count=len(unique) + validated_index_count,
        ), None

    def _revalidate_cataloged_paths(
        self,
        paths: tuple[str, ...],
        allowed_analysis_ids: tuple[str, ...] = (),
    ) -> tuple[dict[str, tuple[Any, ...]] | None, str | None]:
        if not paths:
            return {}, None
        unique = tuple(dict.fromkeys(paths))
        placeholders = ",".join("?" for _ in unique)
        completion_predicate = "a.completion_state='COMPLETE'"
        parameters: tuple[Any, ...] = unique
        if allowed_analysis_ids:
            allowed_placeholders = ",".join("?" for _ in allowed_analysis_ids)
            completion_predicate = (
                f"(a.completion_state='COMPLETE' "
                f"OR f.analysis_id IN ({allowed_placeholders}))"
            )
            parameters = (*unique, *allowed_analysis_ids)
        active_index, index_integrity = self._verify_index()
        if index_integrity.state != VERIFIED:
            LOGGER.error(
                "query index integrity failure reason=%s diagnostic=%s",
                index_integrity.reason_code,
                index_integrity.diagnostic,
            )
            return None, index_integrity.reason_code or "INTEGRITY_CATALOG_UNAVAILABLE"
        with closing(sqlite3.connect(f"file:{active_index.as_posix()}?mode=ro&immutable=1", uri=True)) as db:
            rows = db.execute(
                f"""SELECT f.artifact_path,f.analysis_id,f.artifact_kind,
                            f.integrity_method,f.integrity_value
                FROM artifact_catalog f
                JOIN analysis_catalog a ON a.analysis_id=f.analysis_id
                WHERE f.artifact_path IN ({placeholders})
                  AND f.integrity_state='VERIFIED'
                  AND {completion_predicate}""",
                parameters,
            ).fetchall()
        found = {row[0]: row for row in rows}
        missing = [path for path in unique if path not in found]
        if missing:
            LOGGER.error(
                "reader used provenance outside COMPLETE catalog entries paths=%r",
                missing,
            )
            return None, "PROVENANCE_NOT_CATALOGED"
        for path in unique:
            row = found[path]
            integrity = verify_cataloged_artifact(
                self.knowledge_root / path,
                row[2],
                row[3],
                row[4],
            )
            if integrity.state != VERIFIED:
                LOGGER.error(
                    "reader provenance integrity failure path=%s reason=%s diagnostic=%s",
                    path,
                    integrity.reason_code,
                    integrity.diagnostic,
                )
                return None, integrity.reason_code or "CORRUPT_ARTIFACT"
        return found, None
