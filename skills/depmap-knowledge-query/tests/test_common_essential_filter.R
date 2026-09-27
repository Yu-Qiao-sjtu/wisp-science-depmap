#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(arrow)
  library(jsonlite)
})

root <- tempfile("depmap-common-essential-")
tests <- file.path(root, "depmap-26q1-core", "lineage_dependency_tests")
dir.create(tests, recursive = TRUE)
dir.create(file.path(root, "depmap-26q1-full"), recursive = TRUE)

writeLines(
  '{"status":"complete","release":"26Q1","schema_version":1}',
  file.path(tests, "manifest.json"),
  useBytes = TRUE
)
write.csv(
  data.frame(symbol = c("CE000", "CE001", "CE002")),
  file.path(root, "depmap-26q1-core", "common_essential_genes.csv"),
  row.names = FALSE
)

symbols <- c("CE000", "CE001", "CE002", "LATE000", "LATE001")
write_parquet(data.frame(
  symbol = symbols,
  lineage = rep("Lung", length(symbols)),
  test_status = rep("tested", length(symbols)),
  lineage_n = rep(30L, length(symbols)),
  rest_n = rep(200L, length(symbols)),
  effect_mean_lineage = rep(-1, length(symbols)),
  effect_mean_rest = rep(-0.2, length(symbols)),
  effect_mean_difference = rep(-0.8, length(symbols)),
  effect_median_lineage = rep(-0.9, length(symbols)),
  effect_median_rest = rep(-0.1, length(symbols)),
  effect_median_difference = rep(-0.8, length(symbols)),
  welch_t = rep(-4, length(symbols)),
  p_lineage_more_dependent = rep(0.001, length(symbols)),
  fdr_lineage_more_dependent = rep(0.01, length(symbols)),
  dependency_probability_mean_lineage = rep(0.8, length(symbols)),
  dependency_probability_mean_rest = rep(0.2, length(symbols)),
  dependency_probability_mean_difference = rep(0.6, length(symbols)),
  effect_direction = rep("lineage_more_dependent", length(symbols)),
  rank_more_dependent = seq_along(symbols)
), file.path(tests, "01_Lung.parquet"))

script <- Sys.getenv("DEPMAP_QUERY_SCRIPT", unset = file.path(
  "skills", "depmap-knowledge-query", "scripts", "query_depmap_kb.R"
))
script <- normalizePath(script)

query <- function(mode, cursor = 0L) {
  stderr_path <- tempfile("depmap-query-stderr-")
  on.exit(unlink(stderr_path), add = TRUE)
  args <- c(
    shQuote(script), "--kb-root", shQuote(root), "--mode", mode,
    "--ranking", "selective", "--exclude-common-essential", "true",
    "--common-essential-source", "depmap_26q1", "--cursor", cursor,
    "--limit", "1"
  )
  if (mode == "lineage_dependency") {
    args <- c(args, "--lineage", "Lung")
  }
  output <- system2("Rscript", args, stdout = TRUE, stderr = stderr_path)
  exit_status <- attr(output, "status")
  if (is.null(exit_status)) exit_status <- 0L
  if (exit_status != 0L) {
    stop(paste(readLines(stderr_path, warn = FALSE), collapse = "\n"))
  }
  fromJSON(paste(output, collapse = "\n"), simplifyVector = FALSE)
}

first <- query("lineage_dependency")
stopifnot(
  identical(first$status, "FOUND"),
  identical(first$rows[[1]]$symbol, "LATE000"),
  identical(first$returned_count, 1L),
  identical(first$matched_row_count, 2L),
  identical(first$cursor, 0L),
  identical(first$next_cursor, 1L),
  identical(first$common_essential_version, "26Q1")
)

second <- query("lineage_dependency", cursor = first$next_cursor)
stopifnot(
  identical(second$rows[[1]]$symbol, "LATE001"),
  identical(second$matched_row_count, 2L),
  is.null(second$next_cursor)
)

writeLines(
  c("symbol", "NA"),
  file.path(root, "depmap-26q1-core", "common_essential_genes.csv"),
  useBytes = TRUE
)
missing_value_lineage <- query("lineage_dependency")
missing_value_pan_cancer <- query("pan_cancer_dependency")
stopifnot(
  identical(missing_value_lineage$status, "NOT_COMPUTED"),
  length(missing_value_lineage$rows) == 0L,
  identical(missing_value_pan_cancer$status, "NOT_COMPUTED"),
  length(missing_value_pan_cancer$lineages) == 0L
)

unlink(file.path(root, "depmap-26q1-core", "common_essential_genes.csv"))
missing_lineage <- query("lineage_dependency")
missing_pan_cancer <- query("pan_cancer_dependency")
stopifnot(
  identical(missing_lineage$status, "NOT_COMPUTED"),
  length(missing_lineage$rows) == 0L,
  identical(missing_lineage$common_essential_filter_applied, FALSE),
  identical(missing_pan_cancer$status, "NOT_COMPUTED"),
  length(missing_pan_cancer$lineages) == 0L
)

unlink(root, recursive = TRUE)
cat("common-essential local query contract: PASS\n")
