#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
  library(limma)
})

args <- commandArgs(trailingOnly = TRUE)
arg <- function(name, default = NULL) {
  hit <- grep(paste0("^--", name, "="), args, value = TRUE)
  if (!length(hit)) return(default)
  sub(paste0("^--", name, "="), "", hit[[1]])
}

required_arg <- function(name) {
  value <- arg(name)
  if (is.null(value) || !nzchar(value)) stop("missing required --", name, "= argument")
  value
}

clean_gene <- function(value) {
  toupper(sub(" \\([^()]++\\)$", "", trimws(as.character(value)), perl = TRUE))
}

safe_key <- function(value) {
  key <- gsub("[^A-Za-z0-9]+", "_", value)
  gsub("(^_+|_+$)", "", key)
}

scalar_or_null <- function(value) {
  if (!length(value) || is.na(value)) NULL else unname(value)
}

file_ref <- function(path, role, supplied_digest = NULL, hash_limit = 100 * 1024^2) {
  info <- file.info(path)
  digest <- supplied_digest
  digest_kind <- if (!is.null(digest) && nzchar(digest)) "supplied" else NULL
  if ((is.null(digest) || !nzchar(digest)) && info$size <= hash_limit) {
    digest <- unname(tools::md5sum(path))
    digest_kind <- "md5"
  }
  list(
    path = normalizePath(path, winslash = "/", mustWork = TRUE),
    bytes = unname(info$size),
    role = role,
    digest = scalar_or_null(digest),
    digest_kind = scalar_or_null(digest_kind)
  )
}

write_json_file <- function(value, path) {
  write_json(value, path, auto_unbox = TRUE, pretty = TRUE, digits = 17, null = "null")
}

canonical_digest <- function(value) {
  path <- tempfile(fileext = ".json")
  on.exit(unlink(path), add = TRUE)
  writeLines(toJSON(value, auto_unbox = TRUE, dataframe = "rows", digits = 17,
                    null = "null", pretty = FALSE), path, useBytes = TRUE)
  unname(tools::md5sum(path))
}

expression_path <- normalizePath(required_arg("expression-csv"), mustWork = TRUE)
effect_path <- normalizePath(required_arg("gene-effect-csv"), mustWork = TRUE)
model_path <- normalizePath(required_arg("model-csv"), mustWork = TRUE)
output_root <- normalizePath(required_arg("output-root"), mustWork = FALSE)
source_gene <- clean_gene(required_arg("source-gene"))
release <- arg("release", "26Q1")
scope <- tolower(arg("scope", "global"))
lineage_requested <- arg("lineage")
lower_quantile <- as.numeric(arg("lower-quantile", "0.3333333333333333"))
upper_quantile <- as.numeric(arg("upper-quantile", "0.6666666666666667"))
min_group_n <- as.integer(arg("min-group-n", "10"))
fdr_max <- as.numeric(arg("fdr-max", "0.05"))
min_abs_effect <- as.numeric(arg("min-abs-effect", "0"))

if (!scope %in% c("global", "lineage")) stop("--scope must be global or lineage")
if (scope == "lineage" && (is.null(lineage_requested) || !nzchar(lineage_requested))) {
  stop("--lineage is required for lineage scope")
}
if (!is.finite(lower_quantile) || !is.finite(upper_quantile) ||
    lower_quantile <= 0 || upper_quantile >= 1 || lower_quantile >= upper_quantile) {
  stop("quantiles must satisfy 0 < lower < upper < 1")
}
if (is.na(min_group_n) || min_group_n < 2) stop("--min-group-n must be at least 2")
if (!is.finite(fdr_max) || fdr_max <= 0 || fdr_max > 1) stop("--fdr-max must be in (0,1]")
if (!is.finite(min_abs_effect) || min_abs_effect < 0) stop("--min-abs-effect must be non-negative")

dir.create(output_root, recursive = TRUE, showWarnings = FALSE)
analysis_id <- arg(
  "analysis-id",
  paste("expression_threshold_dependency", release, source_gene, scope,
        if (scope == "lineage") safe_key(lineage_requested) else "lineage_adjusted",
        sep = "__")
)
created_at <- format(Sys.time(), "%Y-%m-%dT%H:%M:%SZ", tz = "UTC")

input_refs <- list(
  file_ref(expression_path, "expression_log2_tpm_plus_1", arg("expression-digest")),
  file_ref(effect_path, "crispr_gene_effect", arg("gene-effect-digest")),
  file_ref(model_path, "model_metadata", arg("model-digest"))
)
parameters <- list(
  source_gene = source_gene,
  scope = scope,
  lineage = if (scope == "lineage") lineage_requested else NULL,
  threshold_policy = "prespecified_quantile_tails",
  lower_quantile = lower_quantile,
  upper_quantile = upper_quantile,
  middle_policy = "excluded",
  minimum_group_n = min_group_n,
  global_adjustment = if (scope == "global") "OncotreeLineage fixed effect" else NULL,
  test = "limma two-group contrast with empirical Bayes moderation",
  contrast = "expression-high minus expression-low Gene Effect",
  multiple_testing = "BH across all eligible CRISPR targets within this analysis",
  fdr_max = fdr_max,
  minimum_absolute_effect = min_abs_effect
)

write_terminal <- function(status, reason, checks, cohort = list(), warnings = list()) {
  manifest <- list(
    schema_version = 1,
    analysis_id = analysis_id,
    created_at = created_at,
    dataset_release = release,
    language = "R",
    entrypoint = "analysis-modules/表达基因-CRISPR基因依赖相关性分析/scripts/run_expression_threshold_dependency_contrast.R",
    reference_scripts = c("04_predivtive_biomarkers.R", "14_DCAF5_SMARCB1_Nature.R"),
    inputs = input_refs,
    parameters = parameters,
    software = list(R = R.version.string, data_table = as.character(packageVersion("data.table")),
                    jsonlite = as.character(packageVersion("jsonlite")), limma = as.character(packageVersion("limma"))),
    outputs = list()
  )
  result <- list(
    schema_version = 1,
    status = status,
    reason = reason,
    question = "expression-threshold groups to genome-wide CRISPR dependency contrast",
    targets = list(source_gene),
    cohort = cohort,
    methods = list(parameters),
    observations = list(),
    tables = list(),
    figures = list(),
    warnings = warnings
  )
  qc <- list(
    schema_version = 1,
    status = if (status == "INELIGIBLE") "blocked" else "fail",
    checks = checks,
    blocking_failures = list(reason),
    warnings = warnings
  )
  coverage <- list(
    schema = "wisp.scientific-coverage.v1",
    release = release,
    capability_id = "expression_threshold_dependency_contrast",
    analysis_id = analysis_id,
    state = tolower(status),
    reason = reason,
    result_digest = NULL
  )
  write_json_file(manifest, file.path(output_root, "run_manifest.json"))
  write_json_file(result, file.path(output_root, "result.json"))
  write_json_file(qc, file.path(output_root, "qc.json"))
  write_json_file(coverage, file.path(output_root, "coverage.json"))
  message(status, ": ", reason)
  quit(save = "no", status = 0)
}

expression_dt <- fread(expression_path, check.names = FALSE)
effect_dt <- fread(effect_path, check.names = FALSE)
models <- fread(model_path)
if (ncol(expression_dt) < 2 || ncol(effect_dt) < 2) stop("matrix inputs must have ModelID plus at least one gene")

expression_ids <- as.character(expression_dt[[1]])
effect_ids <- as.character(effect_dt[[1]])
expression_dt[[1]] <- NULL
effect_dt[[1]] <- NULL
expression_genes <- clean_gene(names(expression_dt))
effect_genes <- clean_gene(names(effect_dt))

if (anyDuplicated(expression_ids) || anyDuplicated(effect_ids)) stop("matrix ModelID values must be unique")
if (anyDuplicated(expression_genes)) stop("expression gene identifiers are ambiguous after normalization")
if (anyDuplicated(effect_genes)) stop("Gene Effect identifiers are ambiguous after normalization")
source_index <- match(source_gene, expression_genes)
if (is.na(source_index)) {
  write_terminal("INELIGIBLE", "source gene is absent from the expression universe",
                 list(list(name = "source_gene_present", status = "fail", detail = source_gene)))
}

model_id_col <- intersect(c("ModelID", "ModelId", "model_id"), names(models))[1]
lineage_col <- intersect(c("OncotreeLineage", "lineage", "Lineage"), names(models))[1]
if (is.na(model_id_col) || is.na(lineage_col)) stop("Model.csv must contain ModelID and OncotreeLineage")
models <- models[!is.na(get(model_id_col)) & !is.na(get(lineage_col))]
if (anyDuplicated(models[[model_id_col]])) stop("Model.csv ModelID values must be unique")

common <- Reduce(intersect, list(expression_ids, effect_ids, as.character(models[[model_id_col]])))
if (!length(common)) stop("expression, Gene Effect, and Model inputs have no common ModelID")
expression_order <- match(common, expression_ids)
effect_order <- match(common, effect_ids)
model_order <- match(common, models[[model_id_col]])
source_values <- as.numeric(expression_dt[[source_index]][expression_order])
effect <- as.matrix(effect_dt[effect_order])
storage.mode(effect) <- "double"
colnames(effect) <- effect_genes
lineages <- as.character(models[[lineage_col]][model_order])

scope_keep <- rep(TRUE, length(common))
if (scope == "lineage") scope_keep <- !is.na(lineages) & lineages == lineage_requested
complete_source <- scope_keep & is.finite(source_values)
cohort_before <- sum(scope_keep)
cohort_after_source <- sum(complete_source)
if (!cohort_after_source) {
  write_terminal("INELIGIBLE", "no finite source-expression values remain in the requested scope",
                 list(list(name = "source_expression_available", status = "fail", detail = source_gene)),
                 cohort = list(requested = if (scope == "lineage") lineage_requested else "all", n_before = cohort_before, n_after = 0))
}

scoped_values <- source_values[complete_source]
if (length(unique(scoped_values)) < 2) {
  write_terminal("INELIGIBLE", "source expression is constant in the requested scope",
                 list(list(name = "source_expression_variable", status = "fail", detail = source_gene)),
                 cohort = list(requested = if (scope == "lineage") lineage_requested else "all", n_before = cohort_before, n_after = cohort_after_source))
}
cuts <- as.numeric(quantile(scoped_values, probs = c(lower_quantile, upper_quantile), type = 7, na.rm = TRUE, names = FALSE))
if (!all(is.finite(cuts)) || cuts[[1]] >= cuts[[2]]) {
  write_terminal("INELIGIBLE", "quantile cutpoints are not distinct in the requested scope",
                 list(list(name = "thresholds_distinct", status = "fail", detail = paste(cuts, collapse = ","))),
                 cohort = list(requested = if (scope == "lineage") lineage_requested else "all", n_before = cohort_before, n_after = cohort_after_source))
}

group <- rep(NA_character_, length(common))
group[complete_source & source_values <= cuts[[1]]] <- "low"
group[complete_source & source_values >= cuts[[2]]] <- "high"
selected <- !is.na(group)
low_n <- sum(group == "low", na.rm = TRUE)
high_n <- sum(group == "high", na.rm = TRUE)
if (low_n < min_group_n || high_n < min_group_n) {
  write_terminal("INELIGIBLE", "expression threshold groups do not meet the prespecified minimum size",
                 list(list(name = "minimum_group_size", status = "fail", detail = paste0("low=", low_n, ", high=", high_n, ", required=", min_group_n))),
                 cohort = list(requested = if (scope == "lineage") lineage_requested else "all", n_before = cohort_before, n_after = sum(selected), low_n = low_n, high_n = high_n,
                               lower_cutpoint = cuts[[1]], upper_cutpoint = cuts[[2]]))
}

selected_effect <- effect[selected, , drop = FALSE]
selected_group <- factor(group[selected], levels = c("low", "high"))
target_low_n <- colSums(is.finite(selected_effect[selected_group == "low", , drop = FALSE]))
target_high_n <- colSums(is.finite(selected_effect[selected_group == "high", , drop = FALSE]))
count_eligible <- target_low_n >= min_group_n & target_high_n >= min_group_n
if (!any(count_eligible)) {
  write_terminal("INELIGIBLE", "no Gene Effect target has enough complete cases in both expression groups",
                 list(list(name = "eligible_target_universe", status = "fail", detail = "0 targets")),
                 cohort = list(requested = if (scope == "lineage") lineage_requested else "all", n_before = cohort_before, n_after = sum(selected), low_n = low_n, high_n = high_n,
                               lower_cutpoint = cuts[[1]], upper_cutpoint = cuts[[2]]))
}

if (scope == "global") {
  selected_lineage <- droplevels(factor(lineages[selected]))
  design <- model.matrix(~ 0 + selected_group + selected_lineage)
} else {
  design <- model.matrix(~ 0 + selected_group)
}
colnames(design)[seq_len(2)] <- c("low", "high")
if (qr(design)$rank < ncol(design)) stop("analysis design is rank deficient")

fit_index <- which(count_eligible)
fit <- lmFit(t(selected_effect[, count_eligible, drop = FALSE]), design)
fit <- contrasts.fit(fit, makeContrasts(high - low, levels = design))
fit <- eBayes(fit, robust = TRUE)
stats <- topTable(fit, number = Inf, sort.by = "none")
estimable_fit <- is.finite(stats$logFC) & is.finite(stats$t) & is.finite(stats$P.Value)
eligible <- rep(FALSE, length(effect_genes))
eligible[fit_index[estimable_fit]] <- TRUE
if (!any(eligible)) {
  write_terminal("INELIGIBLE", "no Gene Effect target has an estimable adjusted high-minus-low contrast",
                 list(list(name = "estimable_target_universe", status = "fail", detail = "0 targets")),
                 cohort = list(requested = if (scope == "lineage") lineage_requested else "all", n_before = cohort_before, n_after = sum(selected), low_n = low_n, high_n = high_n,
                               lower_cutpoint = cuts[[1]], upper_cutpoint = cuts[[2]]))
}
estimable_stats <- stats[estimable_fit, , drop = FALSE]

out <- data.table(
  target_gene = effect_genes,
  source_gene = source_gene,
  scope = scope,
  lineage = if (scope == "lineage") lineage_requested else NA_character_,
  status = ifelse(eligible, "FOUND", "INELIGIBLE"),
  low_n = as.integer(target_low_n),
  high_n = as.integer(target_high_n),
  low_mean_gene_effect = NA_real_,
  high_mean_gene_effect = NA_real_,
  effect_size_high_minus_low = NA_real_,
  moderated_t = NA_real_,
  p_value = NA_real_,
  fdr = NA_real_
)
eligible_index <- which(eligible)
out[eligible_index, low_mean_gene_effect := colMeans(selected_effect[selected_group == "low", eligible, drop = FALSE], na.rm = TRUE)]
out[eligible_index, high_mean_gene_effect := colMeans(selected_effect[selected_group == "high", eligible, drop = FALSE], na.rm = TRUE)]
out[eligible_index, effect_size_high_minus_low := estimable_stats$logFC]
out[eligible_index, moderated_t := estimable_stats$t]
out[eligible_index, p_value := estimable_stats$P.Value]
out[eligible_index, fdr := p.adjust(estimable_stats$P.Value, method = "BH")]
out[, stronger_dependency_in_high := status == "FOUND" & effect_size_high_minus_low < 0]
out[, passes_retention := status == "FOUND" & is.finite(fdr) & fdr <= fdr_max & abs(effect_size_high_minus_low) >= min_abs_effect]
setorder(out, -passes_retention, fdr, effect_size_high_minus_low, target_gene)

all_path <- file.path(output_root, "all_targets.csv.gz")
retained_path <- file.path(output_root, "retained_hits.csv.gz")
fwrite(out, all_path, compress = "gzip")
fwrite(out[passes_retention == TRUE], retained_path, compress = "gzip")
result_digest <- canonical_digest(list(parameters = parameters, cutpoints = cuts, rows = out))

checks <- list(
  list(name = "release_identified", status = "pass", detail = release),
  list(name = "model_ids_unique", status = "pass", detail = paste(length(common), "aligned models")),
  list(name = "source_gene_present", status = "pass", detail = source_gene),
  list(name = "thresholds_prespecified", status = "pass", detail = paste(lower_quantile, upper_quantile, sep = ",")),
  list(name = "minimum_group_size", status = "pass", detail = paste0("low=", low_n, ", high=", high_n)),
  list(name = "effect_direction_declared", status = "pass", detail = "negative high-minus-low means stronger dependency in expression-high models"),
  list(name = "target_universe_recorded", status = "pass", detail = paste(sum(eligible), "eligible of", length(effect_genes))),
  list(name = "multiple_testing_declared", status = "pass", detail = "BH within this analysis")
)
outputs <- list(
  list(path = "all_targets.csv.gz", role = "complete_target_universe"),
  list(path = "retained_hits.csv.gz", role = "bounded_retained_hits"),
  list(path = "result.json", role = "result_contract"),
  list(path = "qc.json", role = "quality_contract"),
  list(path = "coverage.json", role = "coverage_record")
)
manifest <- list(
  schema_version = 1,
  analysis_id = analysis_id,
  created_at = created_at,
  dataset_release = release,
  language = "R",
  entrypoint = "analysis-modules/表达基因-CRISPR基因依赖相关性分析/scripts/run_expression_threshold_dependency_contrast.R",
  reference_scripts = c("04_predivtive_biomarkers.R", "14_DCAF5_SMARCB1_Nature.R"),
  inputs = input_refs,
  parameters = parameters,
  software = list(R = R.version.string, data_table = as.character(packageVersion("data.table")),
                  jsonlite = as.character(packageVersion("jsonlite")), limma = as.character(packageVersion("limma"))),
  outputs = outputs,
  result_digest = result_digest
)
result <- list(
  schema_version = 1,
  status = "ok",
  question = "expression-threshold groups to genome-wide CRISPR dependency contrast",
  targets = list(source_gene),
  cohort = list(requested = if (scope == "lineage") lineage_requested else "all", n_before = cohort_before,
                n_after = sum(selected), low_n = low_n, high_n = high_n,
                lower_cutpoint = cuts[[1]], upper_cutpoint = cuts[[2]]),
  methods = list(parameters),
  observations = list(
    list(name = "eligible_target_count", value = sum(eligible)),
    list(name = "retained_target_count", value = out[passes_retention == TRUE, .N]),
    list(name = "direction", value = "negative high-minus-low means stronger dependency in expression-high models")
  ),
  tables = outputs[1:2],
  figures = list(),
  warnings = list("This observational group contrast does not establish causality.")
)
qc <- list(schema_version = 1, status = "pass", checks = checks, blocking_failures = list(), warnings = list())
coverage <- list(
  schema = "wisp.scientific-coverage.v1",
  release = release,
  capability_id = "expression_threshold_dependency_contrast",
  analysis_id = analysis_id,
  state = "validated",
  scope = scope,
  lineage = if (scope == "lineage") lineage_requested else NULL,
  source_gene = source_gene,
  target_universe_count = length(effect_genes),
  eligible_target_count = sum(eligible),
  retained_target_count = out[passes_retention == TRUE, .N],
  result_digest = result_digest
)
write_json_file(manifest, file.path(output_root, "run_manifest.json"))
write_json_file(result, file.path(output_root, "result.json"))
write_json_file(qc, file.path(output_root, "qc.json"))
write_json_file(coverage, file.path(output_root, "coverage.json"))
message("Completed expression-threshold dependency contrast: ", output_root)
