#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! "$1" =~ ^[a-z0-9_-]+$ ]]; then
  echo "usage: run_private_r_job.sh <opaque-job-id>" >&2
  exit 2
fi

private_root="${WISP_PRIVATE_RUN_ROOT:-${HOME}/.wisp-private-run}"
spec_path="${private_root}/specs/$1"
runner_path="${private_root}/runner.R"

if [[ ! -r "${spec_path}" || ! -r "${runner_path}" ]]; then
  echo "private runner or job specification is unavailable" >&2
  exit 1
fi

export WISP_PRIVATE_JOB_SPEC="${spec_path}"
exec Rscript "${runner_path}"
