"""Shared integrity checks for indexed DepMap artifacts."""

from __future__ import annotations

import csv
import gzip
import hashlib
import json
import logging
import os
import shutil
import sqlite3
from contextlib import closing
from dataclasses import dataclass
from pathlib import Path


VERIFIED = "VERIFIED"
QUARANTINED = "QUARANTINED"
LOGGER = logging.getLogger("depmap_mcp.artifact_integrity")

READER_ARTIFACT_PATTERNS = {
    "core": (
        "depmap-26q1-core/gene_core_summary.parquet|"
        "depmap-26q1-core/lineage_blocks/%"
    ),
    "lineage_catalog": (
        "depmap-26q1-full/lineage_sparse_networks/%/%/manifest.json|"
        "depmap-26q1-full/lineage_cnv_amplification_dependency/%/manifest.json|"
        "depmap-26q1-full/lineage_prism_associations/%/%/manifest.json|"
        "depmap-26q1-full/lineage_gene_enrichment/%/manifest.json|"
        "depmap-26q1-full/subtype_dependency/manifest.json|"
        "depmap-26q1-full/subtype_dependency/contrast_catalog.csv|"
        "depmap-26q1-tcga/project_catalog.csv"
    ),
    "model_gene_effect": (
        "depmap-26q1-core/model_gene_effect.parquet|"
        "depmap-26q1-core/model_metadata.parquet"
    ),
}


def reader_artifact_pattern(query_mode: str, module_pattern: str) -> str:
    """Return the concrete inputs a reader is allowed to consume."""
    return READER_ARTIFACT_PATTERNS.get(query_mode, module_pattern)


def sqlite_like_pattern(pattern: str) -> str:
    """Compile a configured artifact glob to LIKE with literal path names."""
    return (
        pattern.strip()
        .replace("\\", "\\\\")
        .replace("_", "\\_")
        .replace("*", "%")
    )


@dataclass(frozen=True)
class ArtifactIntegrity:
    state: str
    method: str
    value: str
    reason_code: str | None = None
    diagnostic: str | None = None


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def index_digest_path(path: Path) -> Path:
    return path.with_suffix(path.suffix + ".sha256")


def index_publish_marker_path(path: Path) -> Path:
    return path.with_suffix(path.suffix + ".publishing.json")


def write_index_digest(path: Path, digest: str | None = None) -> str:
    """Atomically publish the detached digest for one completed SQLite index."""
    digest = digest or _sha256(path)
    destination = index_digest_path(path)
    temporary = destination.with_suffix(destination.suffix + ".tmp")
    temporary.write_text(digest + "\n", encoding="ascii")
    os.replace(temporary, destination)
    return digest


def _previous_index_path(path: Path, digest: str) -> Path:
    return path.with_name(f"{path.name}.previous-{digest}")


def _cleanup_previous_index_pairs(path: Path, keep: Path | None) -> None:
    """Remove rollback pairs older than the pair reserved for this publication."""
    prefix = f"{path.name}.previous-"
    for previous in path.parent.glob(f"{prefix}*"):
        if (
            not previous.is_file()
            or previous.name.endswith((".sha256", ".tmp"))
            or previous == keep
        ):
            continue
        try:
            previous.unlink()
            index_digest_path(previous).unlink(missing_ok=True)
        except OSError:
            # A Windows reader may still have the previous DB open. A later
            # successful publication retries the same bounded cleanup.
            LOGGER.warning(
                "could not remove obsolete query-index rollback pair name=%s",
                previous.name,
                exc_info=True,
            )
    for sidecar in path.parent.glob(f"{prefix}*.sha256"):
        database = sidecar.with_suffix("")
        if database.exists() or database == keep:
            continue
        try:
            sidecar.unlink()
        except OSError:
            LOGGER.warning(
                "could not remove orphan query-index rollback digest name=%s",
                sidecar.name,
                exc_info=True,
            )


def resolve_index_artifact(path: Path) -> tuple[Path, ArtifactIntegrity]:
    """Resolve the last complete index/digest pair during an atomic publication."""
    marker = index_publish_marker_path(path)
    try:
        value = json.loads(marker.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return path, verify_index_artifact(path)
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        return path, ArtifactIntegrity(
            QUARANTINED,
            "sha256",
            "",
            "INVALID_INDEX_PUBLISH_MARKER",
            str(exc),
        )
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        return path, ArtifactIntegrity(
            QUARANTINED,
            "sha256",
            "",
            "INVALID_INDEX_PUBLISH_MARKER",
        )
    previous_name = value.get("previous_name")
    previous_digest = value.get("previous_sha256")
    if previous_name is None and previous_digest is None:
        return path, ArtifactIntegrity(
            QUARANTINED,
            "sha256",
            "",
            "INDEX_PUBLISH_IN_PROGRESS",
        )
    if (
        not isinstance(previous_name, str)
        or not isinstance(previous_digest, str)
        or len(previous_digest) != 64
        or any(value not in "0123456789abcdef" for value in previous_digest)
        or previous_name != _previous_index_path(path, previous_digest).name
    ):
        return path, ArtifactIntegrity(
            QUARANTINED,
            "sha256",
            "",
            "INVALID_INDEX_PUBLISH_MARKER",
        )
    previous = path.parent / previous_name
    integrity = verify_index_artifact(previous)
    if integrity.state != VERIFIED or integrity.value != previous_digest:
        return previous, ArtifactIntegrity(
            QUARANTINED,
            "sha256",
            integrity.value,
            integrity.reason_code or "INVALID_INDEX_PUBLISH_MARKER",
            integrity.diagnostic,
        )
    return previous, integrity


def publish_index_artifact(temporary: Path, output: Path) -> str:
    """Publish an index/digest pair while readers retain the prior verified pair."""
    output.parent.mkdir(parents=True, exist_ok=True)
    new_digest = write_index_digest(temporary)
    active, active_integrity = resolve_index_artifact(output)
    previous: Path | None = None
    if active_integrity.state == VERIFIED:
        previous = _previous_index_path(output, active_integrity.value)
        if previous != active:
            previous_integrity = verify_index_artifact(previous)
            if (
                previous_integrity.state != VERIFIED
                or previous_integrity.value != active_integrity.value
            ):
                previous_temporary = previous.with_suffix(previous.suffix + ".tmp")
                shutil.copyfile(active, previous_temporary)
                os.replace(previous_temporary, previous)
                write_index_digest(previous, active_integrity.value)
                previous_integrity = verify_index_artifact(previous)
                if (
                    previous_integrity.state != VERIFIED
                    or previous_integrity.value != active_integrity.value
                ):
                    raise OSError("could not preserve the previous verified index pair")

    # A reader may have selected the rollback path while the prior publication
    # marker was visible but not opened it yet. Keep this publication's pair
    # for the full interval; prune only pairs left by older publications before
    # exposing the next marker.
    _cleanup_previous_index_pairs(output, previous)

    marker = index_publish_marker_path(output)
    marker_temporary = marker.with_suffix(marker.suffix + ".tmp")
    marker_temporary.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "previous_name": previous.name if previous else None,
                "previous_sha256": active_integrity.value if previous else None,
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    os.replace(marker_temporary, marker)
    os.replace(temporary, output)
    os.replace(index_digest_path(temporary), index_digest_path(output))
    marker.unlink()
    return new_digest


def verify_index_artifact(path: Path) -> ArtifactIntegrity:
    """Validate the SQLite index against its detached build-time digest."""
    sidecar = index_digest_path(path)
    try:
        expected = sidecar.read_text(encoding="ascii").strip().lower()
    except (OSError, UnicodeError) as exc:
        return ArtifactIntegrity(
            QUARANTINED,
            "sha256",
            "",
            "INDEX_DIGEST_UNAVAILABLE",
            str(exc),
        )
    if len(expected) != 64 or any(value not in "0123456789abcdef" for value in expected):
        return ArtifactIntegrity(
            QUARANTINED,
            "sha256",
            "",
            "INVALID_INDEX_DIGEST",
        )
    current = inspect_artifact(path, "database")
    if current.state != VERIFIED:
        return current
    if current.value != expected:
        return ArtifactIntegrity(
            QUARANTINED,
            "sha256",
            current.value,
            "CHECKSUM_MISMATCH",
        )
    return current


def _validate_table(path: Path) -> None:
    compressed = path.name.lower().endswith((".csv.gz", ".tsv.gz"))
    opener = gzip.open if compressed else open
    delimiter = "\t" if path.name.lower().endswith((".tsv", ".tsv.gz")) else ","
    with opener(path, "rt", encoding="utf-8-sig", newline="") as handle:
        reader = csv.reader(handle, delimiter=delimiter, strict=True)
        header = next(reader, None)
        if header is None:
            raise ValueError("missing header")
        normalized = [value.strip().casefold() for value in header]
        if (
            not normalized
            or any(not value for value in normalized)
            or len(set(normalized)) != len(normalized)
        ):
            raise ValueError("blank or duplicate header")
        width = len(header)
        for row in reader:
            if len(row) != width:
                raise ValueError("ragged row")


def _validate_json(path: Path) -> None:
    with path.open(encoding="utf-8-sig") as handle:
        value = json.load(handle)
    if path.name.lower() == "manifest.json" and not isinstance(value, dict):
        raise ValueError("manifest must be an object")


def _validate_sqlite(path: Path) -> None:
    with closing(
        sqlite3.connect(f"file:{path.as_posix()}?mode=ro&immutable=1", uri=True)
    ) as db:
        if db.execute("PRAGMA quick_check").fetchone()[0] != "ok":
            raise ValueError("sqlite quick_check failed")


def _validate_parquet(path: Path) -> None:
    size = path.stat().st_size
    if size < 12:
        raise ValueError("parquet file is too short")
    with path.open("rb") as handle:
        if handle.read(4) != b"PAR1":
            raise ValueError("missing parquet header")
        handle.seek(-8, 2)
        footer = handle.read(8)
        if footer[4:] != b"PAR1":
            raise ValueError("missing parquet footer")
        metadata_length = int.from_bytes(footer[:4], byteorder="little")
        if metadata_length <= 0 or metadata_length > size - 12:
            raise ValueError("invalid parquet metadata length")
    try:
        import pyarrow.parquet as parquet

        parquet.read_metadata(path)
    except Exception as exc:
        raise ValueError("invalid parquet metadata") from exc


def _validate_r_object(path: Path) -> None:
    with path.open("rb") as handle:
        data = handle.read(128)
    if data.startswith(b"\x1f\x8b"):
        with gzip.open(path, "rb") as handle:
            data = handle.read(128)
    if path.suffix.lower() == ".rds":
        serialization = data
    elif data.startswith((b"RDX2\n", b"RDX3\n", b"RDA2\n", b"RDA3\n")):
        serialization = data[5:]
    else:
        raise ValueError("invalid R serialization header")
    if serialization.startswith(b"X\n"):
        if len(serialization) <= 14:
            raise ValueError("truncated R serialization")
        format_version = int.from_bytes(serialization[2:6], byteorder="big")
        if format_version not in (2, 3):
            raise ValueError("invalid R serialization version")
        return
    if serialization.startswith(b"A\n"):
        fields = serialization[2:].split(b"\n", 3)
        if len(fields) != 4 or not fields[3]:
            raise ValueError("truncated R serialization")
        try:
            versions = tuple(int(value) for value in fields[:3])
        except ValueError as exc:
            raise ValueError("invalid R serialization version") from exc
        if versions[0] not in (2, 3):
            raise ValueError("invalid R serialization version")
        return
    raise ValueError("invalid R serialization header")


def _validate_format(path: Path, artifact_kind: str) -> None:
    lower_name = path.name.lower()
    suffix = path.suffix.lower()
    if artifact_kind == "compressed_table" or lower_name.endswith(
        (".csv.gz", ".tsv.gz")
    ):
        _validate_table(path)
    elif suffix in {".csv", ".tsv"}:
        _validate_table(path)
    elif suffix == ".json":
        _validate_json(path)
    elif suffix in {".sqlite", ".db"}:
        _validate_sqlite(path)
    elif suffix == ".parquet":
        _validate_parquet(path)
    elif suffix in {".rds", ".rdata"}:
        _validate_r_object(path)


def inspect_artifact(path: Path, artifact_kind: str) -> ArtifactIntegrity:
    """Hash and validate an artifact without exposing diagnostic detail to callers."""
    try:
        fingerprint = _sha256(path)
    except OSError as exc:
        return ArtifactIntegrity(
            QUARANTINED,
            "sha256",
            "",
            "UNREADABLE_ARTIFACT",
            str(exc),
        )
    try:
        _validate_format(path, artifact_kind)
    except (EOFError, gzip.BadGzipFile, OSError) as exc:
        reason = (
            "TRUNCATED_COMPRESSED_ARTIFACT"
            if path.name.lower().endswith((".csv.gz", ".tsv.gz"))
            else "UNREADABLE_ARTIFACT"
        )
        return ArtifactIntegrity(
            QUARANTINED, "sha256", fingerprint, reason, str(exc)
        )
    except (UnicodeError, csv.Error, ValueError) as exc:
        suffix = path.suffix.lower()
        reason = (
            "INVALID_TABLE_SCHEMA"
            if artifact_kind == "compressed_table" or suffix in {".csv", ".tsv"}
            else "INVALID_JSON"
            if suffix == ".json"
            else "INVALID_SQLITE"
            if suffix in {".sqlite", ".db"}
            else "INVALID_PARQUET"
            if suffix == ".parquet"
            else "INVALID_R_OBJECT"
            if suffix in {".rds", ".rdata"}
            else "INVALID_ARTIFACT_FORMAT"
        )
        return ArtifactIntegrity(
            QUARANTINED, "sha256", fingerprint, reason, str(exc)
        )
    except sqlite3.Error as exc:
        return ArtifactIntegrity(
            QUARANTINED, "sha256", fingerprint, "INVALID_SQLITE", str(exc)
        )
    return ArtifactIntegrity(VERIFIED, "sha256", fingerprint)


def verify_cataloged_artifact(
    path: Path,
    artifact_kind: str,
    integrity_method: str,
    integrity_value: str,
) -> ArtifactIntegrity:
    """Verify current bytes against one immutable catalog record."""
    if integrity_method != "sha256" or not integrity_value:
        return ArtifactIntegrity(
            QUARANTINED,
            integrity_method,
            integrity_value,
            "UNSUPPORTED_INTEGRITY_METHOD",
        )
    current = inspect_artifact(path, artifact_kind)
    if current.state != VERIFIED:
        return current
    if current.value != integrity_value:
        return ArtifactIntegrity(
            QUARANTINED,
            integrity_method,
            current.value,
            "CHECKSUM_MISMATCH",
        )
    return current


def verify_declared_checksum(
    integrity: ArtifactIntegrity, expected_sha256: str | None
) -> ArtifactIntegrity:
    """Apply an optional manifest-declared SHA-256 to a format-verified artifact."""
    if expected_sha256 is None or integrity.state != VERIFIED:
        return integrity
    expected = expected_sha256.strip().lower()
    if expected.startswith("sha256:"):
        expected = expected.removeprefix("sha256:")
    if len(expected) != 64 or any(value not in "0123456789abcdef" for value in expected):
        return ArtifactIntegrity(
            QUARANTINED,
            integrity.method,
            integrity.value,
            "INVALID_DECLARED_CHECKSUM",
        )
    if integrity.value != expected:
        return ArtifactIntegrity(
            QUARANTINED,
            integrity.method,
            integrity.value,
            "CHECKSUM_MISMATCH",
        )
    return integrity
