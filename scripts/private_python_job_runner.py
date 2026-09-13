#!/usr/bin/env python3
"""Run a Python job without exposing its target or arguments in process argv."""

from __future__ import annotations

import os
import runpy
import sys
from pathlib import Path


def main() -> None:
    spec_path = os.environ.get("WISP_PRIVATE_JOB_SPEC", "")
    if not spec_path:
        raise SystemExit("WISP_PRIVATE_JOB_SPEC is required")
    lines = Path(spec_path).read_text(encoding="utf-8").splitlines()
    if not lines or not lines[0]:
        raise SystemExit("private job specification is empty")
    target = str(Path(lines[0]).resolve(strict=True))
    sys.argv = [target, *lines[1:]]
    runpy.run_path(target, run_name="__main__")


if __name__ == "__main__":
    main()
