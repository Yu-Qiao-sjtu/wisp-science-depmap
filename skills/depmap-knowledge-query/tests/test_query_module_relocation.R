#!/usr/bin/env Rscript

suppressPackageStartupMessages(library(jsonlite))

if (.Platform$OS.type == "windows") {
  cat("relocated mutation module query: SKIP (Windows R path encoding)\n")
  quit(save = "no", status = 0L)
}

root <- tempfile("depmap-relocated-module-")
module <- file.path(
  root, "analysis-modules", "癌种内突变锚定基因选择",
  "cancer_anchor_catalog_v2", "downstream_dependency",
  "05_precomputed_gene_effect_matrices", "damaging_mutation_dependency"
)
dir.create(file.path(module, "blocks"), recursive = TRUE)
dir.create(file.path(root, "depmap-26q1-full"), recursive = TRUE)
dir.create(file.path(root, "depmap-26q1-core"), recursive = TRUE)

writeLines('{"qa_status":"PASS","release":"26Q1"}',
           file.path(root, "depmap-26q1-qa.json"), useBytes = TRUE)
write.csv(data.frame(module = "fixture", status = "complete"),
          file.path(root, "depmap-26q1-module-catalog.csv"), row.names = FALSE)
write.csv(data.frame(symbol = "ARID1A"),
          file.path(module, "mutation_gene_order.csv"), row.names = FALSE)
write.csv(data.frame(symbol = "HMGCR"),
          file.path(module, "target_gene_order.csv"), row.names = FALSE)
saveRDS(list(
  mean_difference = matrix(-0.25, nrow = 1),
  mutation_n = matrix(12, nrow = 1),
  p_mutation_more_dependent = matrix(0.01, nrow = 1),
  fdr_mutation_more_dependent = matrix(0.02, nrow = 1)
), file.path(module, "blocks", "block_00001_00001.rds"))

script <- Sys.getenv("DEPMAP_QUERY_SCRIPT", unset = file.path(
  "skills", "depmap-knowledge-query", "scripts", "query_depmap_kb.R"
))
script <- normalizePath(script)
output <- system2("Rscript", c(
  shQuote(script), "--kb-root", shQuote(root), "--mode", "pair",
  "--module", "damaging_mutation_dependency",
  "--source", "ARID1A", "--target", "HMGCR"
), stdout = TRUE, stderr = TRUE)
exit_status <- attr(output, "status")
if (is.null(exit_status)) exit_status <- 0L
stopifnot(exit_status == 0L)

result <- fromJSON(paste(output, collapse = "\n"), simplifyVector = FALSE)
stopifnot(
  identical(result$mode, "pair"),
  identical(result$source, "ARID1A"),
  grepl("05_precomputed_gene_effect_matrices", result$provenance, fixed = TRUE),
  identical(result$result[[1]]$target, "HMGCR")
)

unlink(root, recursive = TRUE)
cat("relocated mutation module query: PASS\n")
