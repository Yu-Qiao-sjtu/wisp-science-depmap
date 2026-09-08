#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(arrow)
})

args <- commandArgs(trailingOnly = TRUE)
if (length(args) != 2L) stop("usage: Rscript 18_verify_lineage_sparse_networks.R PROJECT_ROOT OUTPUT_ROOT")
project_root <- normalizePath(args[[1L]], winslash = "/", mustWork = TRUE)
output_root <- normalizePath(args[[2L]], winslash = "/", mustWork = TRUE)
data_root <- file.path(project_root, "data")
clean_gene <- function(x) sub(" \\([0-9]+\\)$", "", x)

model <- fread(file.path(data_root, "Model.csv"), select = c("ModelID", "OncotreeLineage"))
lung_ids <- model[OncotreeLineage == "Lung", ModelID]

effect <- fread(file.path(data_root, "CRISPRGeneEffect.csv"))
effect_ids <- as.character(effect[[1L]])
effect_cols <- names(effect)[-1L]
effect_genes <- clean_gene(effect_cols)

expression <- fread(file.path(data_root, "OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv"))
expression <- expression[IsDefaultEntryForMC == "Yes" & !duplicated(ModelID)]
expression_ids <- as.character(expression$ModelID)
expression_meta <- c("V1", "SequencingID", "ModelConditionID", "ModelID", "IsDefaultEntryForMC", "IsDefaultEntryForModel")
expression_cols <- setdiff(names(expression), expression_meta)
expression_genes <- clean_gene(expression_cols)

read_edges <- function(family) {
  paths <- list.files(file.path(output_root, family, "Lung", "blocks"), pattern = "[.]parquet$", full.names = TRUE)
  rbindlist(lapply(paths, function(path) as.data.table(read_parquet(path))))
}

value_column <- function(dt, genes, symbol) {
  idx <- match(symbol, genes)
  if (is.na(idx)) stop("gene not found: ", symbol)
  as.numeric(dt[[idx + 1L]])
}

check_family <- function(family) {
  edges <- read_edges(family)
  if (!nrow(edges)) stop("no edges for ", family)
  probes <- edges[seq_len(min(6L, .N))]
  for (i in seq_len(nrow(probes))) {
    source <- probes$source_gene[[i]]
    target <- probes$target_gene[[i]]
    if (family == "effect_correlation") {
      ids <- intersect(effect_ids, lung_ids)
      x <- value_column(effect, effect_genes, source)[match(ids, effect_ids)]
      y <- value_column(effect, effect_genes, target)[match(ids, effect_ids)]
    } else if (family == "expression_correlation") {
      ids <- intersect(expression_ids, lung_ids)
      x <- value_column(expression[, c("ModelID", expression_cols), with = FALSE], expression_genes, source)[match(ids, expression_ids)]
      y <- value_column(expression[, c("ModelID", expression_cols), with = FALSE], expression_genes, target)[match(ids, expression_ids)]
    } else {
      ids <- Reduce(intersect, list(expression_ids, effect_ids, lung_ids))
      x <- value_column(expression[, c("ModelID", expression_cols), with = FALSE], expression_genes, source)[match(ids, expression_ids)]
      y <- value_column(effect, effect_genes, target)[match(ids, effect_ids)]
    }
    ok <- is.finite(x) & is.finite(y)
    expected <- cor(x[ok], y[ok], method = "pearson")
    if (!isTRUE(all.equal(expected, probes$correlation[[i]], tolerance = 1e-10))) {
      stop(family, " mismatch for ", source, " / ", target, ": ", expected, " != ", probes$correlation[[i]])
    }
    if (sum(ok) != probes$pair_n[[i]]) stop(family, " pair_n mismatch for ", source, " / ", target)
  }
  cat(family, ": verified ", nrow(probes), " edges\n", sep = "")
}

for (family in c("effect_correlation", "expression_correlation", "expression_dependency")) check_family(family)
cat("PASS\n")
