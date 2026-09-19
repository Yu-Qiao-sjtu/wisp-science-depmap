#!/usr/bin/env python3
"""Plan and record a release-scoped Wisp Science base upgrade."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


SEMVER = re.compile(r"^v(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)$")


def version(tag: str) -> tuple[int, int, int]:
    match = SEMVER.fullmatch(tag)
    if not match:
        raise ValueError(f"unsupported stable release tag: {tag}")
    return tuple(int(match.group(key)) for key in ("major", "minor", "patch"))


def load_marker(path: Path) -> dict[str, str]:
    value = json.loads(path.read_text(encoding="utf-8"))
    required = {"repository", "tag", "commit", "channel"}
    if not isinstance(value, dict) or not required.issubset(value):
        raise ValueError("upstream marker is incomplete")
    return {key: str(value[key]) for key in required}


def plan(marker: dict[str, str], latest_tag: str, latest_sha: str) -> dict[str, object]:
    current = version(marker["tag"])
    latest = version(latest_tag)
    if latest < current:
        raise ValueError("upstream release is older than the recorded base")
    return {
        "upgrade": latest > current or latest_sha != marker["commit"],
        "current_tag": marker["tag"],
        "current_sha": marker["commit"],
        "latest_tag": latest_tag,
        "latest_sha": latest_sha,
        "branch": f"automation/upstream-{latest_tag}",
    }


def write_marker(path: Path, repository: str, tag: str, commit: str) -> None:
    version(tag)
    path.write_text(
        json.dumps(
            {
                "repository": repository,
                "tag": tag,
                "commit": commit,
                "channel": "stable-release",
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    subcommands = parser.add_subparsers(dest="command", required=True)
    check = subcommands.add_parser("check")
    check.add_argument("--marker", type=Path, required=True)
    check.add_argument("--latest-tag", required=True)
    check.add_argument("--latest-sha", required=True)
    record = subcommands.add_parser("record")
    record.add_argument("--marker", type=Path, required=True)
    record.add_argument("--repository", required=True)
    record.add_argument("--tag", required=True)
    record.add_argument("--sha", required=True)
    args = parser.parse_args()
    if args.command == "check":
        print(json.dumps(plan(load_marker(args.marker), args.latest_tag, args.latest_sha)))
    else:
        write_marker(args.marker, args.repository, args.tag, args.sha)


if __name__ == "__main__":
    main()
