"""Catalog-driven dispatch for bounded DepMap scientific readers."""

from __future__ import annotations

import sqlite3
from contextlib import closing
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Awaitable, Callable


Runner = Callable[[Any, dict[str, Any]], Awaitable[dict[str, Any]]]

# API modes are more granular than the public intent catalog. Each one must
# still enter through a registered reader family before opening retained data.
MODE_ALIASES = {
    "catalog": "core",
    "core": "core",
    "top": "core",
    "lineage": "core",
    "pathway": "core",
    "drug": "drug",
    "lineage_network": "core",
    "lineage_cnv": "core",
    "lineage_drug": "drug",
    "enrichment": "core",
    "subtype": "subtype",
    "coamplification": "coamplification",
    "three_d": "three_d",
    "tcga_expression_survival": "tcga_expression_survival",
    "gene": "core",
    "pair": "pair",
    "lineage_catalog": "lineage_catalog",
    "lineage_dependency": "lineage_dependency",
    "lineage_directions": "lineage_directions",
    "mutation_anchor": "mutation_anchor",
    "synthetic_lethal": "synthetic_lethal",
    "tf_dependency": "tf_dependency",
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
    reason: str | None = None

    def evidence(self, release: str) -> dict[str, Any]:
        value = asdict(self)
        value["artifact_uris"] = [f"depmap://{release}/{path}" for path in self.artifact_uris]
        value["matrix_blocks"] = [f"depmap://{release}/{path}" for path in self.matrix_blocks]
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

    def resolve(self, query: dict[str, Any]) -> CatalogResolution:
        mode = str(query.get("mode") or "")
        reader_mode = MODE_ALIASES.get(mode, mode)
        if not self.enabled:
            return CatalogResolution("CATALOG_UNAVAILABLE", mode, reason="query index is not installed")
        try:
            with closing(sqlite3.connect(f"file:{self.index.as_posix()}?mode=ro&immutable=1", uri=True)) as db:
                db.row_factory = sqlite3.Row
                reader = db.execute(
                    "SELECT query_mode,adapter,module_pattern FROM reader_registry WHERE query_mode=?",
                    (reader_mode,),
                ).fetchone()
                if reader is None:
                    return CatalogResolution("READER_UNAVAILABLE", mode, reader_mode=reader_mode, reason="no registered reader")
                patterns = [part.strip() for part in str(reader["module_pattern"]).split("|") if part.strip()]
                likes = []
                for pattern in patterns:
                    like = pattern.replace("*", "%")
                    likes.append(like if "%" in like else f"%{like}%")
                predicates = " OR ".join("module LIKE ? OR analysis_unit LIKE ?" for _ in likes)
                parameters = tuple(value for like in likes for value in (like, like))
                analyses = db.execute(
                    f"""
                    SELECT analysis_id FROM analysis_catalog
                    WHERE completion_state='COMPLETE' AND ({predicates})
                    ORDER BY manifest_mtime_ns DESC LIMIT 32
                    """,
                    parameters,
                ).fetchall()
                analysis_ids = tuple(row[0] for row in analyses)
                artifacts: tuple[str, ...] = ()
                if analysis_ids:
                    placeholders = ",".join("?" for _ in analysis_ids)
                    artifacts = tuple(
                        row[0]
                        for row in db.execute(
                            f"""
                            SELECT f.artifact_path FROM artifact_catalog f
                            JOIN analysis_relation r ON r.analysis_id=f.analysis_id AND r.artifact_path=f.artifact_path
                            WHERE f.analysis_id IN ({placeholders}) AND r.role IN ('result','data','manifest')
                            ORDER BY CASE r.role WHEN 'result' THEN 0 WHEN 'data' THEN 1 ELSE 2 END,
                                     f.mtime_ns DESC LIMIT 32
                            """,
                            analysis_ids,
                        )
                    )
                genes = {
                    str(query[key]).upper()
                    for key in ("gene", "source", "target", "transcription_factor")
                    if query.get(key)
                }
                blocks: tuple[str, ...] = ()
                if genes:
                    placeholders = ",".join("?" for _ in genes)
                    blocks = tuple(
                        row[0]
                        for row in db.execute(
                            f"SELECT DISTINCT block_path FROM matrix_block_index WHERE gene IN ({placeholders}) ORDER BY block_path",
                            tuple(sorted(genes)),
                        )
                    )
                # Indexed-content readers legitimately query SQLite content tables
                # and do not need a file candidate for each returned row.
                indexed = reader_mode in {"true_love", "tf_dependency", "biomarker_target"}
                if not analyses and not indexed:
                    return CatalogResolution("NOT_INDEXED", mode, reader_mode, reader["adapter"], reason="no COMPLETE matching analysis")
                return CatalogResolution("RESOLVED", mode, reader_mode, reader["adapter"], analysis_ids, artifacts, blocks)
        except sqlite3.Error as exc:
            return CatalogResolution("CATALOG_ERROR", mode, reader_mode=reader_mode, reason=str(exc))

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
            return resolution, {
                "status": "MODULE_UNAVAILABLE",
                "reason": f"catalog reader resolution failed: {resolution.state}",
            }
        return resolution, await runner(settings, query)
