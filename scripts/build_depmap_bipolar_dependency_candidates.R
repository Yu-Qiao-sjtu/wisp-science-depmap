#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
})

args <- commandArgs(trailingOnly = TRUE)
arg <- function(name, default = NULL) {
  hit <- grep(paste0("^--", name, "="), args, value = TRUE)
  if (!length(hit)) return(default)
  sub(paste0("^--", name, "="), "", hit[[1]])
}

knowledge_root <- normalizePath(arg("knowledge-root"), mustWork = TRUE)
cor_max <- as.numeric(arg("cor-max", "-0.3"))
fdr_max <- as.numeric(arg("fdr-max", "0.05"))
top_per_source <- as.integer(arg("top-per-source", "20"))
min_pair_n <- as.integer(arg("min-pair-n", "500"))
input_root <- file.path(knowledge_root, "depmap-26q1-full", "effect_correlation")
output_root <- path.expand(arg("output-root", file.path(
  knowledge_root, "depmap-26q1-full", "bipolar_dependency_candidates")))
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

order_dt <- fread(file.path(input_root, "gene_order.csv"))
genes <- toupper(order_dt$symbol)
blocks <- sort(list.files(file.path(input_root, "blocks"), pattern = "\\.rds$", full.names = TRUE))
parts <- vector("list", length(blocks))
retained <- 0L

for (block_i in seq_along(blocks)) {
  block <- readRDS(blocks[[block_i]])
  rows <- vector("list", nrow(block$correlation))
  for (row_i in seq_len(nrow(block$correlation))) {
    source_index <- block$source_start + row_i - 1L
    r <- block$correlation[row_i, ]
    n <- block$pair_n[row_i, ]
    valid <- is.finite(r) & is.finite(n) & n >= min_pair_n & seq_along(r) != source_index
    p <- rep(NA_real_, length(r))
    t_stat <- abs(r[valid]) * sqrt((n[valid] - 2) / pmax(1 - r[valid]^2, .Machine$double.eps))
    p[valid] <- 2 * pt(t_stat, df = n[valid] - 2, lower.tail = FALSE)
    fdr <- rep(NA_real_, length(r))
    fdr[valid] <- p.adjust(p[valid], method = "BH")
    keep <- which(valid & r <= cor_max & fdr <= fdr_max)
    if (!length(keep)) next
    keep <- head(keep[order(r[keep], fdr[keep])], top_per_source)
    rows[[row_i]] <- data.table(
      source_gene = genes[[source_index]],
      target_gene = genes[keep],
      correlation = as.numeric(r[keep]),
      pair_n = as.integer(n[keep]),
      p_value = p[keep],
      fdr = fdr[keep],
      rank_within_source = seq_along(keep)
    )
  }
  part <- rbindlist(rows, fill = TRUE)
  parts[[block_i]] <- part
  retained <- retained + nrow(part)
  if (block_i %% 10L == 0L || block_i == length(blocks)) {
    message(sprintf("[%d/%d] retained directed edges=%d", block_i, length(blocks), retained))
  }
  rm(block, rows, part)
  gc(FALSE)
}

directed <- rbindlist(parts, fill = TRUE)
directed[, pair_key := ifelse(source_gene < target_gene,
                              paste(source_gene, target_gene, sep = "::"),
                              paste(target_gene, source_gene, sep = "::"))]
pair_summary <- directed[, .(
  gene_a = sort(unique(c(source_gene, target_gene)))[1],
  gene_b = sort(unique(c(source_gene, target_gene)))[2],
  direction_count = .N,
  strongest_negative_correlation = min(correlation),
  best_fdr = min(fdr),
  min_pair_n = min(pair_n),
  reciprocal_top_k = uniqueN(source_gene) == 2
), by = pair_key]
setorder(pair_summary, -reciprocal_top_k, strongest_negative_correlation, best_fdr)
directed[, pair_key := NULL]

fwrite(directed, file.path(output_root, "directed_negative_edges.csv.gz"), compress = "gzip")
fwrite(pair_summary, file.path(output_root, "pair_summary.csv.gz"), compress = "gzip")
manifest <- list(
  schema_version = 1,
  release = "26Q1",
  family = "bipolar_dependency_candidates",
  status = "complete",
  correlation_max = cor_max,
  fdr_max = fdr_max,
  top_per_source = top_per_source,
  min_pair_n = min_pair_n,
  directed_edge_count = nrow(directed),
  unique_pair_count = nrow(pair_summary),
  reciprocal_top_k_pair_count = pair_summary[reciprocal_top_k == TRUE, .N],
  method = "negative Pearson Gene Effect correlations derived from the exhaustive effect-correlation matrix",
  multiple_testing = "BH FDR across all target genes separately within each source gene",
  interpretation = "inverse dependency patterns are hypothesis-generating pathway-state candidates, not causality or synthetic lethality",
  tm00_references = c("16", "17")
)
write_json(manifest, file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed bipolar candidate build: ", output_root)
