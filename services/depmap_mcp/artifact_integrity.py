"""Shared integrity checks for indexed DepMap artifacts."""

from __future__ import annotations

import csv
import gzip
import hashlib
import json
import os
import sqlite3
from contextlib import closing
from dataclasses import dataclass
from pathlib import Path


VERIFIED = "VERIFIED"
QUARANTINED = "QUARANTINED"

READER_ARTIFACT_PATTERNS = {
    "core": (
        "depmap-26q1-core/gene_core_summary.parquet|"
        "depmap-26q1-core/lineage_blocks/%"
    ),
    "model_gene_effect": (
        "depmap-26q1-core/model_gene_effect.parquet|"
        "depmap-26q1-core/model_metadata.parquet"
    ),
}


def reader_artifact_pattern(query_mode: str, module_pattern: str) -> str:
    """Return the concrete inputs a reader is allowed to consume."""
    return READER_ARTIFACT_PATTERNS.get(query_mode, module_pattern)


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


def write_index_digest(path: Path) -> str:
    """Atomically publish the detached digest for one completed SQLite index."""
    digest = _sha256(path)
    destination = index_digest_path(path)
    temporary = destination.with_suffix(destination.suffix + ".tmp")
    temporary.write_text(digest + "\n", encoding="ascii")
    os.replace(temporary, destination)
    return digest


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


def _validate_r_object(path: Path) -> None:
    with path.open("rb") as handle:
        prefix = handle.read(5)
    if prefix.startswith(b"\x1f\x8b"):
        with gzip.open(path, "rb") as handle:
            prefix = handle.read(5)
    valid = (
        prefix.startswith((b"X\n", b"A\n"))
        if path.suffix.lower() == ".rds"
        else prefix.startswith((b"RDX2\n", b"RDX3\n", b"RDA2\n", b"RDA3\n"))
    )
    if not valid:
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
