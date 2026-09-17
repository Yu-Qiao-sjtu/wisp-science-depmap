#!/usr/bin/env Rscript

args <- commandArgs(trailingOnly = TRUE)
root <- if (length(args)) args[[1L]] else stop("Usage: validate_predictive_biomarker_model.R RESULT_DIR")

required <- c(
  "outer_cv_predictions.csv",
  "outer_cv_fold_metrics.csv",
  "outer_cv_pooled_metrics.csv",
  "outer_cv_features.csv",
  "feature_stability.csv",
  "manifest.json"
)
missing <- required[!file.exists(file.path(root, required))]
if (length(missing)) stop("Missing result files: ", paste(missing, collapse = ", "))

pred <- read.csv(file.path(root, "outer_cv_predictions.csv"), check.names = FALSE)
metrics <- read.csv(file.path(root, "outer_cv_pooled_metrics.csv"), check.names = FALSE)
features <- read.csv(file.path(root, "feature_stability.csv"), check.names = FALSE)

expected_models <- c("mean", "top1_linear", "lasso", "random_forest")
stopifnot(setequal(unique(pred$model), expected_models))
stopifnot(setequal(unique(metrics$model), expected_models))
stopifnot(all(table(pred$ModelID, pred$model) == 1L))
stopifnot(all(is.finite(pred$observed)), all(is.finite(pred$predicted)))
stopifnot(all(metrics$n == length(unique(pred$ModelID))))
stopifnot(all(features$outer_fold_fraction > 0 & features$outer_fold_fraction <= 1))

writeLines('{"status":"PASS"}', file.path(root, "validation.json"))
message("PASS: predictive biomarker result validated")
