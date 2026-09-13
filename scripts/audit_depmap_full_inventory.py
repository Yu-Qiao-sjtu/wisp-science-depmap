#!/usr/bin/env python3
"""Checkpointed byte-level inventory for the complete gz0548 DepMap workspace."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sqlite3
import time
from pathlib import Path


CHUNK_SIZE = 16 * 1024 * 1024


def connect(path: Path) -> sqlite3.Connection:
    db = sqlite3.connect(path)
    db.execute("PRAGMA journal_mode=WAL")
    db.execute("PRAGMA synchronous=FULL")
    db.execute(
        """CREATE TABLE IF NOT EXISTS files (
        path TEXT PRIMARY KEY, root TEXT NOT NULL, size INTEGER NOT NULL,
        mtime_ns INTEGER NOT NULL, sha256 TEXT, status TEXT NOT NULL,
        error TEXT, audited_at REAL NOT NULL)"""
    )
    db.commit()
    return db


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(CHUNK_SIZE):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-root", required=True)
    parser.add_argument("--root", action="append", required=True)
    parser.add_argument("--progress-every", type=int, default=100)
    args, unknown = parser.parse_known_args()
    nonempty_unknown = [value for value in unknown if value.strip()]
    if nonempty_unknown:
        parser.error("unrecognized arguments: " + " ".join(nonempty_unknown))

    output_root = Path(args.output_root).resolve()
    output_root.mkdir(parents=True, exist_ok=True)
    os.chmod(output_root, 0o700)
    db = connect(output_root / "file_inventory.sqlite")
    roots = [Path(value).resolve(strict=True) for value in args.root]
    files = sorted(path for root in roots for path in root.rglob("*") if path.is_file())

    completed = errors = changed = total_bytes = 0
    started_at = time.time()
    for index, path in enumerate(files, 1):
        root = next(root for root in roots if path == root or root in path.parents)
        try:
            before = path.stat()
            existing = db.execute(
                "SELECT size, mtime_ns, sha256, status FROM files WHERE path=?", (str(path),)
            ).fetchone()
            if existing and existing[0] == before.st_size and existing[1] == before.st_mtime_ns \
                    and existing[2] and existing[3] == "ok":
                completed += 1
                total_bytes += before.st_size
                continue
            digest = sha256_file(path)
            after = path.stat()
            status = "ok" if (before.st_size, before.st_mtime_ns) == \
                (after.st_size, after.st_mtime_ns) else "changed_during_hash"
            changed += status != "ok"
            db.execute(
                "INSERT OR REPLACE INTO files VALUES (?,?,?,?,?,?,?,?)",
                (str(path), str(root), after.st_size, after.st_mtime_ns,
                 digest if status == "ok" else None, status, None, time.time()),
            )
            completed += status == "ok"
            total_bytes += after.st_size
        except Exception as exc:  # retain every failure in the audit database
            errors += 1
            db.execute(
                "INSERT OR REPLACE INTO files VALUES (?,?,?,?,?,?,?,?)",
                (str(path), str(root), 0, 0, None, "error", repr(exc), time.time()),
            )
        db.commit()
        if index % args.progress_every == 0 or index == len(files):
            print(
                f"[{index}/{len(files)}] ok={completed} changed={changed} "
                f"errors={errors} bytes={total_bytes}", flush=True
            )

    counts = dict(db.execute("SELECT status, COUNT(*) FROM files GROUP BY status"))
    summary = {
        "schema_version": 1,
        "status": "complete" if not errors and not changed else "completed_with_exceptions",
        "roots": [str(root) for root in roots],
        "discovered_file_count": len(files),
        "status_counts": counts,
        "total_bytes": db.execute("SELECT COALESCE(SUM(size),0) FROM files").fetchone()[0],
        "elapsed_seconds": round(time.time() - started_at, 3),
        "hash": "SHA-256",
    }
    temporary = output_root / "inventory_summary.json.tmp"
    temporary.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, output_root / "inventory_summary.json")
    print(json.dumps(summary), flush=True)


if __name__ == "__main__":
    main()
