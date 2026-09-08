#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
})

args <- commandArgs(trailingOnly = TRUE)
if (length(args) != 1L) stop("usage: Rscript 22_finalize_lineage_networks_server.R OUTPUT_ROOT")
root <- normalizePath(args[[1L]], winslash = "/", mustWork = TRUE)

shards <- list.files(root, pattern = "^shard_.+[.]csv$", full.names = TRUE)
if (!length(shards)) stop("no shard summaries found")
summary <- rbindlist(lapply(shards, fread), fill = TRUE)
setorder(summary, family, lineage)
fwrite(summary, file.path(root, "latest_run_summary.csv"), bom = TRUE)

expected <- data.table(
  family = c("effect_correlation", "expression_correlation", "expression_dependency"),
  expected_complete = c(24L, 26L, 24L)
)
observed <- summary[status == "complete", .(observed_complete = .N), by = family]
coverage <- merge(expected, observed, by = "family", all.x = TRUE)
coverage[is.na(observed_complete), observed_complete := 0L]
if (any(coverage$observed_complete != coverage$expected_complete)) {
  stop("eligible completion counts do not match the frozen contract")
}

manifest_files <- list.files(root, pattern = "manifest[.]json$", recursive = TRUE, full.names = TRUE)
manifests <- lapply(manifest_files, fromJSON)
manifest_rows <- rbindlist(lapply(manifests, function(x) data.table(
  family = as.character(x$family),
  lineage = as.character(x$lineage),
  status = as.character(x$status),
  source_gene_count = as.integer(x$source_gene_count),
  target_gene_count = as.integer(x$target_gene_count),
  block_count = as.integer(x$block_count)
)), fill = TRUE)
if (nrow(manifest_rows) != 74L) stop("expected 74 formal manifests, found ", nrow(manifest_rows))
if (any(manifest_rows$status != "complete")) stop("one or more formal manifests are incomplete")
if (any(is.na(manifest_rows$lineage) | !nzchar(trimws(manifest_rows$lineage)))) stop("blank lineage found")

qa <- list(
  schema_version = 1L,
  release = "26Q1",
  generated_at = format(Sys.time(), "%Y-%m-%dT%H:%M:%S%z"),
  status = "structural_pass",
  output_root = root,
  shard_summary_count = length(shards),
  formal_manifest_count = nrow(manifest_rows),
  coverage = coverage,
  ineligible = summary[status != "complete", .(family, lineage, sample_n, status)]
)
writeLines(toJSON(qa, auto_unbox = TRUE, pretty = TRUE, na = "null"),
           file.path(root, "server_completion_summary.json"), useBytes = TRUE)
cat("STRUCTURAL_PASS\n")
