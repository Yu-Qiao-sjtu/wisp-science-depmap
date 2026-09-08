#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
})

args <- commandArgs(trailingOnly = TRUE)
if (length(args) != 3L) {
  stop("usage: Rscript query_gene_pair.R EFFECT_CORRELATION_ROOT GENE1 GENE2")
}

root <- normalizePath(args[[1L]], winslash = "/", mustWork = TRUE)
gene1 <- toupper(trimws(args[[2L]]))
gene2 <- toupper(trimws(args[[3L]]))
gene_order <- fread(file.path(root, "gene_order.csv"))

if (!all(c("index", "symbol") %in% names(gene_order))) {
  stop("gene_order.csv must contain index and symbol")
}

find_gene <- function(symbol) {
  # data.table resolves the column name before the local variable; use an
  # explicit vector comparison to avoid that ambiguity.
  query_symbol <- symbol
  hit <- gene_order[which(toupper(gene_order[["symbol"]]) == query_symbol)]
  if (nrow(hit) != 1L) stop("gene missing or duplicated: ", symbol)
  hit
}

source <- find_gene(gene1)
target <- find_gene(gene2)
blocks <- list.files(file.path(root, "blocks"), pattern = "^block_[0-9]+_[0-9]+[.]rds$", full.names = TRUE)
parts <- do.call(rbind, regmatches(basename(blocks), regexec("^block_([0-9]+)_([0-9]+)[.]rds$", basename(blocks))))
start <- as.integer(parts[, 2L])
end <- as.integer(parts[, 3L])
block_path <- blocks[start <= source$index & end >= source$index]
if (length(block_path) != 1L) stop("cannot resolve source block for ", gene1)

block <- readRDS(block_path)
source_offset <- source$index - block$source_start + 1L
r <- as.numeric(block$correlation[source_offset, target$index])
n <- if (length(block$pair_n) == 1L) {
  as.integer(block$pair_n)
} else {
  as.integer(block$pair_n[source_offset, target$index])
}

p <- if (is.finite(r) && n >= 3L && abs(r) < 1) {
  2 * pt(-abs(r * sqrt((n - 2) / (1 - r^2))), df = n - 2)
} else if (is.finite(r) && abs(r) == 1) {
  0
} else {
  NA_real_
}

direction <- if (is.na(r)) "unavailable" else if (r > 0) "positive" else if (r < 0) "negative" else "zero"
strength <- if (is.na(r)) "unavailable" else if (abs(r) >= 0.7) "strong" else if (abs(r) >= 0.3) "moderate" else "weak"

cat(toJSON(list(
  dataset = "CRISPRGeneEffect",
  analysis = "global_gene_gene_codependency",
  gene1 = gene1,
  gene2 = gene2,
  correlation = r,
  pair_n = n,
  p_value_recomputed = p,
  direction = direction,
  strength = strength,
  block = basename(block_path)
), auto_unbox = TRUE, pretty = TRUE, digits = 16), "\n")
