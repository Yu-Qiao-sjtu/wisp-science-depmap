#!/usr/bin/env Rscript

if (!requireNamespace("jsonlite", quietly = TRUE)) {
  stop("test_inspector.R requires jsonlite")
}

script_arg <- grep("^--file=", commandArgs(trailingOnly = FALSE), value = TRUE)
test_path <- normalizePath(sub("^--file=", "", script_arg), winslash = "/", mustWork = TRUE)
skill_root <- dirname(dirname(test_path))
inspector <- file.path(skill_root, "scripts", "inspect_depmap_project.R")
manifest <- jsonlite::read_json(
  file.path(skill_root, "references", "capability-manifest.json"),
  simplifyVector = FALSE
)

fixture <- tempfile("depmap-inspector-")
dir.create(file.path(fixture, "data"), recursive = TRUE)
dir.create(file.path(fixture, "tm00-script", "scripts"), recursive = TRUE)
gsea_root <- file.path(
  fixture,
  "analysis-modules",
  "表达基因-CRISPR基因依赖相关性分析"
)
utf8_paths_supported <- isTRUE(l10n_info()[["UTF-8"]])
if (utf8_paths_supported) {
  dir.create(file.path(gsea_root, "results", "expression_dependency"), recursive = TRUE)
  dir.create(file.path(gsea_root, "scripts"), recursive = TRUE)
  file.create(file.path(gsea_root, "scripts", "run_expression_dependency_gsea.R"))
}
on.exit(unlink(fixture, recursive = TRUE, force = TRUE), add = TRUE)

for (capability in manifest$capabilities) {
  for (script in unlist(capability$scripts)) {
    file.create(file.path(fixture, "tm00-script", "scripts", script))
  }
}
for (dataset in manifest$datasets) {
  if (identical(dataset$kind, "raw")) {
    path <- file.path(fixture, dataset$path)
    dir.create(dirname(path), recursive = TRUE, showWarnings = FALSE)
    file.create(path)
  }
}
writeLines("DepMap Public 26Q1", file.path(fixture, "data", "README.txt"))

run_inspector <- function(capability, operation = NULL) {
  output <- file.path(
    fixture,
    paste0(capability, if (is.null(operation)) "" else paste0("-", operation), ".json")
  )
  operation_args <- if (is.null(operation)) character() else c("--operation", operation)
  status <- system2(
    file.path(R.home("bin"), "Rscript"),
    c(shQuote(inspector), "--project-root", shQuote(fixture),
      "--capability", capability, operation_args, "--output", shQuote(output)),
    stdout = TRUE,
    stderr = TRUE
  )
  if (!is.null(attr(status, "status")) && attr(status, "status") != 0L) {
    stop(paste(status, collapse = "\n"))
  }
  jsonlite::read_json(output, simplifyVector = FALSE)$capabilities[[1L]]
}

stopifnot(identical(run_inspector("core_ingestion")$status, "ready"))

mutation <- run_inspector("mutation_to_target")
stopifnot(identical(mutation$status, "preprocessing_required"))
stopifnot(identical(
  unname(unlist(mutation$execution_plan)),
  c("07_mutant_dependency_22Q2.R", "09_batch_from_mut_to_target_23Q2.R")
))
stopifnot(length(unlist(mutation$missing_inputs)) == 0L)

synthetic <- run_inspector("synthetic_lethal_screen")
stopifnot(identical(
  unname(unlist(synthetic$execution_plan)),
  c("01_read_depmap.r", "03_co_dependency.R", "05.2_synthetic_lethal.R")
))

drug_auc <- run_inspector("drug_auc_cross_validation")
stopifnot(identical(drug_auc$status, "missing_inputs"))
stopifnot("amg193_auc_csv" %in% unlist(drug_auc$missing_inputs))

gsea <- run_inspector("gene_to_dependency", "pathway_enrichment")
expected_gsea_status <- if (utf8_paths_supported) "ready" else "preprocessing_required"
stopifnot(identical(gsea$status, expected_gsea_status))
stopifnot(identical(gsea$operation_id, "pathway_enrichment"))
stopifnot(identical(gsea$operation, "gsea"))
stopifnot(identical(gsea$execution_mode, "on_demand_cached"))
stopifnot(identical(unname(unlist(gsea$supported_scopes)), "global"))
stopifnot(identical(
  basename(tail(unname(unlist(gsea$execution_plan)), 1L)),
  "run_expression_dependency_gsea.R"
))
stopifnot(identical(gsea$defaults$collection, "hallmark"))
stopifnot(identical(gsea$defaults$rank_metric, "negative_signed_t"))
stopifnot(identical(gsea$defaults$min_pair_fraction, 0.8))

cat("DepMap inspector fixture tests passed.\n")
