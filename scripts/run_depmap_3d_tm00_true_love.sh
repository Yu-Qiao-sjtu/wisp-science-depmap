#!/usr/bin/env bash
set -Eeuo pipefail
DATA=/home/data/gz0548/depmap-26q1/inputs/tm00-3d
KB=/home/data/gz0548/depmap-26q1/tm00-script17-3d-run
S=/home/data/gz0548/depmap-agent/analysis/depmap-3d
RUN=/home/data/gz0548/depmap-agent/runs/tm00-script17-3d
LOG=/home/data/gz0548/depmap-agent/logs/tm00_script17_3d.log
mkdir -p "$KB" "$RUN";exec 9>"$RUN/run.lock";flock -n 9||exit 2
complete(){ python3 - "$1" <<'PY'
import json,sys
try:ok=json.load(open(sys.argv[1])).get('status')=='complete'
except Exception:ok=False
raise SystemExit(0 if ok else 1)
PY
}
stage(){ local n=$1 m=$2;shift 2;if complete "$m";then echo "$(date -Is) $n SKIP_COMPLETE">>"$RUN/status.log";return;fi;echo "$(date -Is) $n RUNNING">>"$RUN/status.log";"$@">>"$LOG" 2>&1;complete "$m";echo "$(date -Is) $n COMPLETE">>"$RUN/status.log";}
trap 'echo "$(date -Is) pipeline FAILED_rc=$?">>"$RUN/status.log"' ERR
echo "$(date -Is) pipeline STARTED">>"$RUN/status.log"
stage raw_codependency "$KB/depmap-26q1-3d/codependency/manifest.json" Rscript "$S/build_depmap_3d_codependency.R" --data-root="$DATA" --knowledge-root="$KB" --block-size=128 --lineage-adjust=false
stage true_love "$KB/depmap-26q1-3d/true_love_gene/manifest.json" Rscript "$S/build_depmap_3d_true_love.R" --knowledge-root="$KB" --cor-max=-0.2 --fdr-max=0.05
stage qa "$KB/depmap-26q1-3d/tm00_script17_catalog.json" python3 "$S/validate_depmap_3d_tm00_true_love.py" --knowledge-root="$KB"
echo "$(date -Is) pipeline COMPLETE">>"$RUN/status.log"
