#!/usr/bin/env bash
set -euo pipefail

module_root="${1:-$(pwd)}"
cd "$module_root"

Rscript scripts/build_biomarker_target_eligibility.R \
  --gene-effect data/CRISPRGeneEffect.csv \
  --model data/Model.csv \
  --output-root results/predictive_biomarker/target_eligibility_26Q1

Rscript scripts/build_predictive_biomarker_model.R \
  --target ESR1 \
  --expression data/OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv \
  --gene-effect data/CRISPRGeneEffect.csv \
  --model data/Model.csv \
  --output-root results/predictive_biomarker/ESR1_26Q1 \
  --outer-folds 5 \
  --inner-folds 5 \
  --top-features 50 \
  --min-lineage-n 20

Rscript scripts/validate_predictive_biomarker_model.R \
  results/predictive_biomarker/ESR1_26Q1
