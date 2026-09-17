#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(glmnet)
  library(randomForest)
  library(jsonlite)
})

args <- commandArgs(trailingOnly = TRUE)

arg_value <- function(flag, default = NULL) {
  hit <- match(flag, args)
  if (is.na(hit) || hit == length(args)) return(default)
  args[[hit + 1L]]
}

has_flag <- function(flag) flag %in% args
strip_gene_id <- function(x) sub(" \\([0-9]+\\)$", "", x)

target_gene <- toupper(arg_value("--target", "ESR1"))
expression_csv <- arg_value("--expression")
effect_csv <- arg_value("--gene-effect")
model_csv <- arg_value("--model")
feature_list_file <- arg_value("--feature-list")
output_root <- arg_value("--output-root", file.path("results", paste0(target_gene, "_predictive_biomarker")))
seed <- as.integer(arg_value("--seed", "20260917"))
outer_folds <- as.integer(arg_value("--outer-folds", "5"))
inner_folds <- as.integer(arg_value("--inner-folds", "5"))
top_features <- as.integer(arg_value("--top-features", "50"))
min_lineage_n <- as.integer(arg_value("--min-lineage-n", "20"))
run_lolo <- !has_flag("--skip-lolo")
test_mode <- has_flag("--test")

stopifnot(outer_folds >= 3L, inner_folds >= 3L, top_features >= 2L)

metric_row <- function(observed, predicted, model, split, held_out = NA_character_) {
  ok <- is.finite(observed) & is.finite(predicted)
  observed <- observed[ok]
  predicted <- predicted[ok]
  if (length(observed) < 3L) {
    return(data.frame(model = model, split = split, held_out = held_out, n = length(observed),
                      r2 = NA_real_, rmse = NA_real_, spearman = NA_real_))
  }
  sse <- sum((observed - predicted)^2)
  sst <- sum((observed - mean(observed))^2)
  data.frame(
    model = model,
    split = split,
    held_out = held_out,
    n = length(observed),
    r2 = if (sst > 0) 1 - sse / sst else NA_real_,
    rmse = sqrt(mean((observed - predicted)^2)),
    spearman = suppressWarnings(cor(observed, predicted, method = "spearman"))
  )
}

make_stratified_folds <- function(lineage, k, seed) {
  set.seed(seed)
  fold <- integer(length(lineage))
  for (value in unique(lineage)) {
    idx <- which(lineage == value)
    fold[idx] <- sample(rep(seq_len(k), length.out = length(idx)))
  }
  fold
}

impute_from_training <- function(train_x, test_x) {
  med <- apply(train_x, 2L, function(v) {
    value <- suppressWarnings(median(v[is.finite(v)], na.rm = TRUE))
    if (is.finite(value)) value else 0
  })
  fill <- function(x) {
    for (j in seq_len(ncol(x))) x[!is.finite(x[, j]), j] <- med[[j]]
    x
  }
  list(train = fill(train_x), test = fill(test_x), medians = med)
}

rank_training_features <- function(train_x, train_y, n_keep) {
  score <- apply(train_x, 2L, function(v) suppressWarnings(cor(v, train_y, use = "pairwise.complete.obs")))
  score[!is.finite(score)] <- 0
  names(sort(abs(score), decreasing = TRUE))[seq_len(min(n_keep, length(score)))]
}

fit_predict_split <- function(x, y, train_idx, test_idx, split_id, held_out = NA_character_) {
  selected_candidates <- rank_training_features(x[train_idx, , drop = FALSE], y[train_idx], top_features)
  train_x <- x[train_idx, selected_candidates, drop = FALSE]
  test_x <- x[test_idx, selected_candidates, drop = FALSE]
  imputed <- impute_from_training(train_x, test_x)
  train_x <- imputed$train
  test_x <- imputed$test
  train_y <- y[train_idx]

  set.seed(seed + split_id)
  inner_foldid <- sample(rep(seq_len(min(inner_folds, length(train_y))), length.out = length(train_y)))
  lasso <- cv.glmnet(train_x, train_y, alpha = 1, foldid = inner_foldid,
                     type.measure = "mse", standardize = TRUE)
  lasso_prediction <- as.numeric(predict(lasso, newx = test_x, s = "lambda.1se"))
  coefficient <- as.matrix(coef(lasso, s = "lambda.1se"))[, 1L]
  selected_lasso <- setdiff(names(coefficient)[coefficient != 0], "(Intercept)")

  top_one <- selected_candidates[[1L]]
  linear_fit <- lm(train_y ~ train_x[, top_one])
  linear_prediction <- as.numeric(coef(linear_fit)[[1L]] + coef(linear_fit)[[2L]] * test_x[, top_one])

  rf_features <- if (length(selected_lasso) >= 2L) selected_lasso else selected_candidates[seq_len(min(10L, length(selected_candidates)))]
  set.seed(seed + 10000L + split_id)
  rf <- randomForest(x = train_x[, rf_features, drop = FALSE], y = train_y,
                     ntree = 500L, importance = TRUE)
  rf_prediction <- as.numeric(predict(rf, test_x[, rf_features, drop = FALSE]))
  mean_prediction <- rep(mean(train_y), length(test_idx))

  predictions <- rbind(
    data.frame(row = test_idx, model = "mean", observed = y[test_idx], predicted = mean_prediction),
    data.frame(row = test_idx, model = "top1_linear", observed = y[test_idx], predicted = linear_prediction),
    data.frame(row = test_idx, model = "lasso", observed = y[test_idx], predicted = lasso_prediction),
    data.frame(row = test_idx, model = "random_forest", observed = y[test_idx], predicted = rf_prediction)
  )
  predictions$split <- split_id
  predictions$held_out <- held_out

  metrics <- do.call(rbind, lapply(split(predictions, predictions$model), function(d) {
    metric_row(d$observed, d$predicted, d$model[[1L]], split_id, held_out)
  }))
  feature_rows <- function(stage, genes, values) {
    if (!length(genes)) return(NULL)
    data.frame(split = rep(split_id, length(genes)), held_out = rep(held_out, length(genes)),
               stage = rep(stage, length(genes)), gene = genes, value = as.numeric(values))
  }
  features <- rbind(
    feature_rows("univariate_candidate", selected_candidates, seq_along(selected_candidates)),
    feature_rows("lasso_nonzero", selected_lasso, coefficient[selected_lasso]),
    feature_rows("random_forest_importance", rownames(importance(rf)), importance(rf)[, "%IncMSE"])
  )
  list(predictions = predictions, metrics = metrics, features = features)
}

if (test_mode) {
  set.seed(seed)
  n <- 180L
  p <- 80L
  lineage <- rep(c("A", "B", "C", "D", "E", "F"), each = n / 6L)
  x <- matrix(rnorm(n * p), nrow = n, dimnames = list(sprintf("M%03d", seq_len(n)), paste0("G", seq_len(p))))
  y <- -0.9 * x[, "G1"] + 0.55 * x[, "G2"] + 0.25 * (lineage %in% c("A", "B")) + rnorm(n, sd = 0.65)
  model_ids <- rownames(x)
} else {
  required <- c(expression_csv, effect_csv, model_csv)
  if (any(vapply(required, is.null, logical(1L))) || !all(file.exists(required))) {
    stop("Provide existing --expression, --gene-effect and --model CSV files")
  }
  message("Reading expression matrix")
  expression <- read.csv(expression_csv, check.names = FALSE, stringsAsFactors = FALSE)
  message("Reading Gene Effect matrix")
  effect <- read.csv(effect_csv, check.names = FALSE, stringsAsFactors = FALSE)
  model <- read.csv(model_csv, check.names = FALSE, stringsAsFactors = FALSE)

  if (!"ModelID" %in% names(expression)) stop("Expression CSV lacks ModelID")
  if ("IsDefaultEntryForMC" %in% names(expression)) {
    expression <- expression[expression$IsDefaultEntryForMC == "Yes", , drop = FALSE]
  }
  expression_gene_columns <- grepl(" \\([0-9]+\\)$", names(expression))
  expression <- expression[, c("ModelID", names(expression)[expression_gene_columns]), drop = FALSE]
  names(effect)[1L] <- "ModelID"
  names(expression)[-1L] <- strip_gene_id(names(expression)[-1L])
  names(effect)[-1L] <- strip_gene_id(names(effect)[-1L])
  expression <- expression[!duplicated(expression$ModelID), , drop = FALSE]
  effect <- effect[!duplicated(effect$ModelID), , drop = FALSE]
  if (!target_gene %in% names(effect)) stop("Target gene absent from Gene Effect: ", target_gene)
  if (!all(c("ModelID", "OncotreeLineage") %in% names(model))) stop("Model.csv lacks ModelID/OncotreeLineage")

  common <- Reduce(intersect, list(expression$ModelID, effect$ModelID, model$ModelID))
  expression <- expression[match(common, expression$ModelID), , drop = FALSE]
  effect <- effect[match(common, effect$ModelID), , drop = FALSE]
  model <- model[match(common, model$ModelID), , drop = FALSE]
  y <- as.numeric(effect[[target_gene]])
  valid_y <- is.finite(y)
  expression <- expression[valid_y, , drop = FALSE]
  model <- model[valid_y, , drop = FALSE]
  y <- y[valid_y]
  model_ids <- expression$ModelID
  lineage <- ifelse(is.na(model$OncotreeLineage) | model$OncotreeLineage == "", "Unknown", model$OncotreeLineage)
  x <- as.matrix(data.frame(lapply(expression[-1L], as.numeric), check.names = FALSE))
  rownames(x) <- model_ids
  keep <- apply(x, 2L, function(v) sum(is.finite(v)) >= max(20L, ceiling(0.8 * nrow(x))) &&
                  sd(v, na.rm = TRUE) > 0)
  x <- x[, keep, drop = FALSE]
  if (!is.null(feature_list_file)) {
    if (!file.exists(feature_list_file)) stop("Feature list does not exist: ", feature_list_file)
    feature_table <- read.delim(feature_list_file, header = FALSE, stringsAsFactors = FALSE,
                                sep = "\t", quote = "", comment.char = "")
    requested_features <- unique(toupper(trimws(feature_table[[1L]])))
    requested_features <- requested_features[nzchar(requested_features)]
    selected_features <- intersect(colnames(x), requested_features)
    if (length(selected_features) < 2L) stop("Feature list overlaps fewer than two expression genes")
    x <- x[, selected_features, drop = FALSE]
  }
}

if (length(y) < 50L) stop("At least 50 models with target Gene Effect are required")
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

outer_fold <- make_stratified_folds(lineage, outer_folds, seed)
outer_results <- lapply(seq_len(outer_folds), function(fold) {
  fit_predict_split(x, y, which(outer_fold != fold), which(outer_fold == fold), fold)
})

outer_predictions <- do.call(rbind, lapply(outer_results, `[[`, "predictions"))
outer_predictions$ModelID <- model_ids[outer_predictions$row]
outer_predictions$lineage <- lineage[outer_predictions$row]
outer_metrics <- do.call(rbind, lapply(split(outer_predictions, outer_predictions$model), function(d) {
  metric_row(d$observed, d$predicted, d$model[[1L]], "pooled_outer_cv")
}))
outer_fold_metrics <- do.call(rbind, lapply(outer_results, `[[`, "metrics"))
outer_features <- do.call(rbind, lapply(outer_results, `[[`, "features"))

lolo_predictions <- data.frame()
lolo_metrics <- data.frame()
lolo_features <- data.frame()
eligible_lineages <- names(which(table(lineage) >= min_lineage_n))
if (run_lolo && length(eligible_lineages)) {
  lolo_results <- lapply(seq_along(eligible_lineages), function(i) {
    held <- eligible_lineages[[i]]
    fit_predict_split(x, y, which(lineage != held), which(lineage == held), 1000L + i, held)
  })
  lolo_predictions <- do.call(rbind, lapply(lolo_results, `[[`, "predictions"))
  lolo_predictions$ModelID <- model_ids[lolo_predictions$row]
  lolo_predictions$lineage <- lineage[lolo_predictions$row]
  lolo_metrics <- do.call(rbind, lapply(lolo_results, `[[`, "metrics"))
  lolo_features <- do.call(rbind, lapply(lolo_results, `[[`, "features"))
}

feature_stability <- aggregate(split ~ stage + gene, outer_features, function(v) length(unique(v)))
names(feature_stability)[names(feature_stability) == "split"] <- "outer_fold_count"
feature_stability$outer_fold_fraction <- feature_stability$outer_fold_count / outer_folds
feature_stability <- feature_stability[order(feature_stability$stage, -feature_stability$outer_fold_fraction, feature_stability$gene), ]

write.csv(outer_predictions, file.path(output_root, "outer_cv_predictions.csv"), row.names = FALSE)
write.csv(outer_fold_metrics, file.path(output_root, "outer_cv_fold_metrics.csv"), row.names = FALSE)
write.csv(outer_metrics, file.path(output_root, "outer_cv_pooled_metrics.csv"), row.names = FALSE)
write.csv(outer_features, file.path(output_root, "outer_cv_features.csv"), row.names = FALSE)
write.csv(feature_stability, file.path(output_root, "feature_stability.csv"), row.names = FALSE)
if (nrow(lolo_predictions)) {
  write.csv(lolo_predictions, file.path(output_root, "leave_one_lineage_out_predictions.csv"), row.names = FALSE)
  write.csv(lolo_metrics, file.path(output_root, "leave_one_lineage_out_metrics.csv"), row.names = FALSE)
  write.csv(lolo_features, file.path(output_root, "leave_one_lineage_out_features.csv"), row.names = FALSE)
}

manifest <- list(
  status = "complete",
  analysis = "nested_expression_biomarker_prediction",
  target_gene = target_gene,
  test_mode = test_mode,
  sample_n = length(y),
  expression_feature_n = ncol(x),
  feature_set = if (is.null(feature_list_file)) "all_expression_genes" else "user_supplied_gene_list",
  feature_list_file = feature_list_file,
  lineage_n = length(unique(lineage)),
  eligible_lolo_lineages = eligible_lineages,
  models = c("mean", "top1_linear", "lasso", "random_forest"),
  outer_folds = outer_folds,
  inner_folds = inner_folds,
  top_features_selected_inside_outer_training = top_features,
  lasso_lambda = "lambda.1se",
  random_forest_ntree = 500L,
  leakage_guards = c(
    "univariate candidate selection is repeated inside each outer training split",
    "missing-value medians are learned from outer training data only",
    "LASSO lambda is selected by inner CV using outer training data only",
    "outer test observations are used only for final scoring"
  ),
  interpretation = "Predicts CRISPR Gene Effect from baseline expression; it does not establish drug efficacy, causality, or clinical validity.",
  seed = seed,
  generated_at_utc = format(Sys.time(), tz = "UTC", usetz = TRUE)
)
write_json(manifest, file.path(output_root, "manifest.json"), pretty = TRUE, auto_unbox = TRUE)

message("Completed predictive biomarker model: ", normalizePath(output_root, winslash = "/", mustWork = FALSE))
