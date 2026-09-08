#!/usr/bin/env Rscript

suppressPackageStartupMessages(library(data.table))

args <- commandArgs(trailingOnly = TRUE)
if (length(args) != 3L) {
  stop("usage: Rscript query_expression_dependency_pair.R EXPRESSION_DEPENDENCY_ROOT EXPRESSION_GENE DEPENDENCY_GENE")
}

root <- normalizePath(args[[1L]], winslash = "/", mustWork = TRUE)
source_gene <- args[[2L]]
target_gene <- args[[3L]]

source_order <- fread(file.path(root, "expression_gene_order.csv"))
target_order <- fread(file.path(root, "dependency_gene_order.csv"))
source_index <- source_order[symbol == source_gene, index]
target_index <- target_order[symbol == target_gene, index]

if (length(source_index) != 1L) stop("expression source gene not found or duplicated: ", source_gene)
if (length(target_index) != 1L) stop("dependency target gene not found or duplicated: ", target_gene)

block_paths <- list.files(file.path(root, "blocks"), pattern = "^block_[0-9]+_[0-9]+[.]rds$", full.names = TRUE)
block_ranges <- t(vapply(basename(block_paths), function(x) {
  stem <- sub("[.]rds$", "", sub("^block_", "", x))
  as.integer(strsplit(stem, "_", fixed = TRUE)[[1L]])
}, integer(2L)))
block_index <- which(source_index >= block_ranges[, 1L] & source_index <= block_ranges[, 2L])
if (length(block_index) != 1L) stop("cannot resolve source block for index ", source_index)

block <- readRDS(block_paths[[block_index]])
row_index <- source_index - block$source_start + 1L
r <- as.numeric(block$correlation[row_index, target_index])
n <- if (length(block$pair_n) == 1L) as.integer(block$pair_n) else as.integer(block$pair_n[row_index, target_index])
p <- if (is.finite(r) && n >= 3L) {
  2 * pt(-abs(r * sqrt((n - 2) / pmax(1 - r^2, .Machine$double.eps))), df = n - 2)
} else {
  NA_real_
}

answer <- data.table(
  expression_gene = source_gene,
  dependency_gene = target_gene,
  correlation = r,
  pair_n = n,
  p_value_unadjusted = p,
  interpretation = if (is.na(r)) {
    "not estimable"
  } else if (r < 0) {
    "higher expression associates with stronger dependency"
  } else if (r > 0) {
    "higher expression associates with weaker dependency"
  } else {
    "no linear association"
  },
  source_block = basename(block_paths[[block_index]])
)

fwrite(answer, file = "", bom = FALSE)
