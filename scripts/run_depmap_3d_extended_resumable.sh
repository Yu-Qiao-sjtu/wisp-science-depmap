#!/usr/bin/env bash
set -Eeuo pipefail
DATA=/home/data/gz0548/depmap-agent/data/nextgen_2026
KB=/home/data/gz0548/depmap-26q1
SCRIPTS=/home/data/gz0548/depmap-agent/analysis/depmap-3d
RUN=/home/data/gz0548/depmap-agent/runs/depmap-3d-extended
LOG=/home/data/gz0548/depmap-agent/logs/depmap_3d_extended.log
mkdir -p "$RUN";exec 9>"$RUN/run.lock";flock -n 9||exit 2
complete(){ python3 - "$1" <<'PY'
import json,sys
try: ok=json.load(open(sys.argv[1])).get('status')=='complete'
except Exception: ok=False
raise SystemExit(0 if ok else 1)
PY
}
stage(){ local n=$1 m=$2;shift 2;if complete "$m";then echo "$(date -Is) $n SKIP_COMPLETE">>"$RUN/status.log";return;fi;echo "$(date -Is) $n RUNNING">>"$RUN/status.log";"$@" >>"$LOG" 2>&1;complete "$m";echo "$(date -Is) $n COMPLETE">>"$RUN/status.log";}
trap 'echo "$(date -Is) pipeline FAILED_rc=$?">>"$RUN/status.log"' ERR
echo "$(date -Is) pipeline STARTED">>"$RUN/status.log"
stage audit "$KB/depmap-26q1-3d/extended_analysis_audit/manifest.json" Rscript "$SCRIPTS/audit_depmap_3d_extended.R" --data-root="$DATA" --knowledge-root="$KB" --amp-threshold=2
stage true_love_stability "$KB/depmap-26q1-3d/true_love_stability/manifest.json" Rscript "$SCRIPTS/build_depmap_3d_true_love_stability.R" --data-root="$DATA" --knowledge-root="$KB" --n-bootstrap=500 --seeds=2601,2602 --sample-fraction=.8 --stability-min=.7 --cor-max=-.2
stage coamplification "$KB/depmap-26q1-3d/coamplification_dependency/manifest.json" Rscript "$SCRIPTS/build_depmap_3d_coamplification.R" --data-root="$DATA" --knowledge-root="$KB" --amp-threshold=2 --min-group-n=8
stage integrated_validation "$KB/depmap-26q1-3d/integrated_validation/manifest.json" Rscript "$SCRIPTS/build_depmap_3d_integrated_validation.R" --data-root="$DATA" --knowledge-root="$KB"
stage final_qa "$KB/depmap-26q1-3d/extended_catalog.json" python3 "$SCRIPTS/validate_depmap_3d_extended.py" --knowledge-root="$KB"
echo "$(date -Is) pipeline COMPLETE">>"$RUN/status.log"
