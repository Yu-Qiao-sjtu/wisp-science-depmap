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
data_root <- normalizePath(arg("data-root"), mustWork = TRUE)
knowledge_root <- normalizePath(arg("knowledge-root"), mustWork = TRUE)
fdr_max <- as.numeric(arg("fdr-max", "0.05"))
effect_max <- as.numeric(arg("effect-max", "-0.2"))
output_root <- file.path(knowledge_root, "depmap-26q1-3d", "differential_dependency")
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

metadata <- fread(file.path(data_root, "screen_metadata.csv"))
metadata <- metadata[PassesQC == TRUE]
effect_dt <- fread(file.path(data_root, "screen_gene_effect.csv"), check.names = FALSE)
screen_ids <- effect_dt[[1]]
effect_dt[[1]] <- NULL
genes <- toupper(sub(" \\([^()]++\\)$", "", names(effect_dt), perl = TRUE))
effect <- as.matrix(effect_dt)
storage.mode(effect) <- "double"
rownames(effect) <- screen_ids
colnames(effect) <- genes
rm(effect_dt)
metadata <- metadata[match(rownames(effect), ScreenID)]
stopifnot(all(metadata$ScreenID == rownames(effect)))

aggregate_models <- function(types) {
  idx <- which(metadata$ScreenType %chin% types)
  keys <- unique(metadata[idx, .(ModelID, ScreenType, OncotreeLineage)])
  rows <- lapply(seq_len(nrow(keys)), function(i) {
    hit <- idx[metadata$ModelID[idx] == keys$ModelID[[i]] &
               metadata$ScreenType[idx] == keys$ScreenType[[i]]]
    if (length(hit) == 1L) effect[hit, ] else apply(effect[hit, , drop = FALSE], 2L, median, na.rm = TRUE)
  })
  mat <- do.call(rbind, rows)
  rownames(mat) <- paste(keys$ModelID, keys$ScreenType, sep = "::")
  list(matrix = mat, metadata = keys)
}

run_contrast <- function(key, types, three_d_types, covariate) {
  cohort <- aggregate_models(types)
  info <- cohort$metadata
  mat <- cohort$matrix
  group <- info$ScreenType %chin% three_d_types
  keep <- rep(TRUE, nrow(info))
  if (covariate == "lineage") {
    shared <- info[, .(has_3d = any(group), has_2d = any(!group)), by = OncotreeLineage][has_3d & has_2d, OncotreeLineage]
    keep <- info$OncotreeLineage %chin% shared
  }
  info <- info[keep]
  mat <- mat[keep, , drop = FALSE]
  group <- group[keep]
  if (sum(group) < 5L || sum(!group) < 5L) stop(key, ": insufficient groups after matching")
  tested_gene <- colSums(is.finite(mat[group, , drop = FALSE])) >= 5L &
    colSums(is.finite(mat[!group, , drop = FALSE])) >= 5L
  mat <- mat[, tested_gene, drop = FALSE]
  tested_genes <- genes[tested_gene]
  if (covariate == "lineage" && uniqueN(info$OncotreeLineage) > 1L) {
    design <- model.matrix(~ factor(info$OncotreeLineage) + group)
  } else if (covariate == "compartment") {
    compartment <- ifelse(info$ScreenType %chin% c("2DO", "3DO"), "Organoid", "CNS")
    design <- model.matrix(~ factor(compartment) + group)
  } else {
    design <- model.matrix(~ group)
  }
  coef_i <- which(colnames(design) == "groupTRUE")
  fit <- eBayes(lmFit(t(mat), design), trend = TRUE)
  result <- as.data.table(topTable(fit, coef = coef_i, number = Inf, sort.by = "none", adjust.method = "BH"))
  result[, `:=`(gene = tested_genes, contrast = key, three_d_n = sum(group), two_d_n = sum(!group),
                stronger_3d_dependency = logFC < 0,
                passes_fdr = adj.P.Val <= fdr_max,
                high_confidence_3d_dependency = adj.P.Val <= fdr_max & logFC <= effect_max)]
  setcolorder(result, c("contrast", "gene", "three_d_n", "two_d_n", "logFC", "AveExpr", "t", "P.Value", "adj.P.Val", "B",
                        "stronger_3d_dependency", "passes_fdr", "high_confidence_3d_dependency"))
  contrast_root <- file.path(output_root, key)
  dir.create(contrast_root, recursive = TRUE, showWarnings = FALSE)
  fwrite(result, file.path(contrast_root, "all_genes.csv.gz"), compress = "gzip")
  fwrite(result[high_confidence_3d_dependency == TRUE], file.path(contrast_root, "high_confidence_hits.csv.gz"), compress = "gzip")
  manifest <- list(schema_version = 1, release = "NextGen Model Manuscript 2026",
    family = "3d_differential_dependency", contrast = key, status = "complete",
    three_d_n = sum(group), two_d_n = sum(!group), lineage_count = uniqueN(info$OncotreeLineage),
    tested_gene_count = ncol(mat), excluded_low_coverage_gene_count = length(genes) - ncol(mat),
    fdr_max = fdr_max, effect_max = effect_max,
    high_confidence_hit_count = result[high_confidence_3d_dependency == TRUE, .N],
    method = "limma empirical-Bayes model on ModelID-level Gene Effect; repeated traditional screens median-aggregated",
    covariate = covariate, interpretation = "negative logFC means stronger dependency in 3D",
    paired = FALSE, inputs = c(file.path(data_root, "screen_metadata.csv"), file.path(data_root, "screen_gene_effect.csv")))
  write_json(manifest, file.path(contrast_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
  data.table(contrast = key, three_d_n = sum(group), two_d_n = sum(!group),
             lineage_count = uniqueN(info$OncotreeLineage),
             high_confidence_hit_count = result[high_confidence_3d_dependency == TRUE, .N], status = "complete")
}

catalog <- rbindlist(list(
  run_contrast("3DO_vs_2DO", c("3DO", "2DO"), "3DO", "lineage"),
  run_contrast("3DN_vs_2DN", c("3DN", "2DN"), "3DN", "none"),
  run_contrast("all3D_vs_nextgen2D", c("3DO", "3DN", "2DO", "2DN"), c("3DO", "3DN"), "compartment"),
  run_contrast("all3D_vs_lineage_matched_2DS", c("3DO", "3DN", "2DS"), c("3DO", "3DN"), "lineage")
))
fwrite(catalog, file.path(output_root, "contrast_catalog.csv"))
write_json(list(schema_version = 1, release = "NextGen Model Manuscript 2026",
  family = "3d_differential_dependency", status = "complete", contrast_count = nrow(catalog),
  multiple_testing = "BH FDR across all genes separately within each contrast",
  design_boundary = "unpaired model-level comparisons; ScreenType effect may retain platform/model-generation differences"),
  file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed 3D differential dependency module: ", output_root)
