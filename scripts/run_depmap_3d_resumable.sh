#!/usr/bin/env bash
set -Eeuo pipefail

DATA_ROOT=/home/data/gz0548/depmap-agent/data/nextgen_2026
KNOWLEDGE_ROOT=/home/data/gz0548/depmap-26q1
SCRIPT_ROOT=/home/data/gz0548/depmap-agent/analysis/depmap-3d
RUN_ROOT=/home/data/gz0548/depmap-agent/runs/depmap-3d-complete
LOG_ROOT=/home/data/gz0548/depmap-agent/logs
STATUS_FILE="$RUN_ROOT/status.tsv"

mkdir -p "$RUN_ROOT" "$LOG_ROOT"
exec 9>"$RUN_ROOT/run.lock"
if ! flock -n 9; then
  echo "A resumable 3D pipeline is already running." >&2
  exit 2
fi

is_complete() {
  python3 - "$1" <<'PY'
import json, pathlib, sys
p = pathlib.Path(sys.argv[1])
try:
    ok = json.loads(p.read_text()).get("status") == "complete"
except Exception:
    ok = False
raise SystemExit(0 if ok else 1)
PY
}

record() {
  printf '%s\t%s\t%s\n' "$(date --iso-8601=seconds)" "$1" "$2" >> "$STATUS_FILE"
}

run_stage() {
  local name=$1 manifest=$2
  shift 2
  if is_complete "$manifest"; then
    record "$name" SKIPPED_COMPLETE
    return 0
  fi
  record "$name" RUNNING
  "$@" >>"$LOG_ROOT/depmap_3d_resumable.log" 2>&1
  is_complete "$manifest"
  record "$name" COMPLETE
}

trap 'rc=$?; record pipeline "FAILED_rc=$rc"; exit "$rc"' ERR
record pipeline STARTED

run_stage codependency "$KNOWLEDGE_ROOT/depmap-26q1-3d/codependency/manifest.json" \
  Rscript "$SCRIPT_ROOT/build_depmap_3d_codependency.R" \
  --data-root="$DATA_ROOT" --knowledge-root="$KNOWLEDGE_ROOT" --block-size=128

run_stage true_love_gene "$KNOWLEDGE_ROOT/depmap-26q1-3d/true_love_gene/manifest.json" \
  Rscript "$SCRIPT_ROOT/build_depmap_3d_true_love.R" \
  --knowledge-root="$KNOWLEDGE_ROOT" --cor-max=-0.2 --fdr-max=0.05

run_stage omics_dependency "$KNOWLEDGE_ROOT/depmap-26q1-3d/omics_dependency/manifest.json" \
  Rscript "$SCRIPT_ROOT/build_depmap_3d_omics_dependency.R" \
  --data-root="$DATA_ROOT" --knowledge-root="$KNOWLEDGE_ROOT" \
  --cor-min=0.3 --fdr-max=0.05 --top-k=20 --min-event-n=5

run_stage final_qa "$KNOWLEDGE_ROOT/depmap-26q1-3d/catalog.json" \
  python3 "$SCRIPT_ROOT/validate_depmap_3d_knowledge.py" \
  --knowledge-root="$KNOWLEDGE_ROOT"

record pipeline COMPLETE
