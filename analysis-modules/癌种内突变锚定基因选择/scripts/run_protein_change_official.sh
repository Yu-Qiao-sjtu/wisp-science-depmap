#!/usr/bin/env bash
set -Eeuo pipefail
if (( $# < 2 || $# > 3 )); then
  echo 'Usage: run_protein_change_official.sh GENE PROTEIN_CHANGE [LINEAGE]' >&2
  exit 2
fi
gene=$1
change=$2
lineage=${3:-}
result_dir=$(find "$HOME" -maxdepth 9 -type d -name results_20260909_v6 -print -quit 2>/dev/null)
module_root=$(dirname "$result_dir")
catalog="$module_root/cancer_anchor_catalog_v2"
engine="$catalog/downstream_dependency/_shared/depmap_official_mutation_engine.py"
largest_named() { find "$HOME" -maxdepth 9 -type f -name "$1" -printf '%s\t%p\n' 2>/dev/null | sort -nr | head -1 | cut -f2-; }
effect=$(largest_named CRISPRGeneEffect.csv)
dependency=$(largest_named CRISPRGeneDependency.csv)
model=$(largest_named Model.csv)
mutation_long=$(largest_named OmicsSomaticMutations.csv)
coverage=$(largest_named OmicsSomaticMutationsMatrixDamaging.csv)
slug=$(python3 - "$gene" "$change" "${lineage:-Pan-cancer}" <<'PY'
import re,sys
clean=lambda x:re.sub(r'[^A-Za-z0-9.-]+','-',x).strip('-')
print('_'.join([clean(sys.argv[3]).lower(),clean(sys.argv[1]),clean(sys.argv[2])]))
PY
)
out="$catalog/downstream_dependency/04_depmap_official_gene_effect_v2/03_protein_change_on_demand/results/$slug"
args=(protein-change --gene-effect "$effect" --dependency "$dependency" --model "$model" --mutation-long "$mutation_long" --coverage-matrix "$coverage" --gene "$gene" --protein-change "$change" --out "$out")
[[ -n "$lineage" ]] && args+=(--lineage "$lineage")
python3 "$engine" "${args[@]}"
