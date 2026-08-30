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

run_inspector <- function(capability) {
  output <- file.path(fixture, paste0(capability, ".json"))
  status <- system2(
    file.path(R.home("bin"), "Rscript"),
    c(shQuote(inspector), "--project-root", shQuote(fixture),
      "--capability", capability, "--output", shQuote(output)),
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

cat("DepMap inspector fixture tests passed.\n")
