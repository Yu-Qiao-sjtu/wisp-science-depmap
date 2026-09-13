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
source_gene <- toupper(arg("source"))
partner_gene <- toupper(arg("partner"))
lineage_filter <- arg("lineage", NULL)
threshold <- as.numeric(arg("threshold", "2"))
min_group_n <- as.integer(arg("min-group-n", "5"))
stopifnot(nzchar(source_gene), nzchar(partner_gene), source_gene != partner_gene)

effect_path <- file.path(data_root, "CRISPRGeneEffect.csv")
cnv_path <- file.path(data_root, "OmicsCNGeneMC_WES.csv")
model_path <- file.path(data_root, "Model.csv")
condition_path <- file.path(data_root, "ModelCondition.csv")
stopifnot(file.exists(effect_path), file.exists(cnv_path), file.exists(model_path),
          file.exists(condition_path))

read_matrix <- function(path) {
  x <- fread(path, check.names = FALSE)
  ids <- x[[1]]
  x[[1]] <- NULL
  genes <- sub(" \\([^()]++\\)$", "", names(x), perl = TRUE)
  mat <- as.matrix(x)
  storage.mode(mat) <- "double"
  rownames(mat) <- ids
  colnames(mat) <- genes
  mat
}

message("Reading Gene Effect and copy number matrices")
effect <- read_matrix(effect_path)
cnv_dt <- fread(cnv_path, check.names = FALSE)
condition_ids <- cnv_dt[[1]]
default_entry <- cnv_dt[[2]] == "Yes"
cnv_dt[[1]] <- NULL
cnv_dt[[1]] <- NULL
cnv_genes <- sub(" \\([^()]++\\)$", "", names(cnv_dt), perl = TRUE)
cnv <- as.matrix(cnv_dt)
storage.mode(cnv) <- "double"
colnames(cnv) <- cnv_genes
conditions <- fread(condition_path, select = c("ModelConditionID", "ModelID"))
model_ids <- conditions$ModelID[match(condition_ids, conditions$ModelConditionID)]
keep_default <- !is.na(model_ids) & !is.na(default_entry) & default_entry
cnv <- cnv[keep_default, , drop = FALSE]
rownames(cnv) <- model_ids[keep_default]
if (!all(c(source_gene, partner_gene) %in% colnames(cnv))) {
  stop("source or partner gene is absent from copy-number matrix")
}
common <- intersect(rownames(effect), rownames(cnv))
if (!is.null(lineage_filter)) {
  models <- fread(model_path, select = c("ModelID", "OncotreeLineage"))
  eligible <- models[OncotreeLineage == lineage_filter, ModelID]
  common <- intersect(common, eligible)
}
effect <- effect[common, , drop = FALSE]
cnv <- cnv[common, , drop = FALSE]

source_amp <- is.finite(cnv[, source_gene]) & cnv[, source_gene] >= threshold
partner_amp <- is.finite(cnv[, partner_gene]) & cnv[, partner_gene] >= threshold
group_a <- source_amp & partner_amp
group_b <- source_amp & !partner_amp
if (sum(group_a) < min_group_n || sum(group_b) < min_group_n) {
  stop(sprintf("ineligible groups: coamplified_n=%d source_only_n=%d; require >=%d",
               sum(group_a), sum(group_b), min_group_n))
}

selected <- group_a | group_b
group <- factor(ifelse(group_a[selected], "coamplified", "source_only"),
                levels = c("source_only", "coamplified"))
design <- model.matrix(~ 0 + group)
colnames(design) <- levels(group)
fit <- lmFit(t(effect[selected, , drop = FALSE]), design)
fit <- contrasts.fit(fit, makeContrasts(coamplified - source_only, levels = design))
fit <- eBayes(fit, robust = TRUE)
stats <- topTable(fit, number = Inf, sort.by = "none")
out <- data.table(
  gene = colnames(effect),
  source_gene = source_gene,
  partner_gene = partner_gene,
  lineage = if (is.null(lineage_filter)) "ALL" else lineage_filter,
  coamplified_n = sum(group_a),
  source_only_n = sum(group_b),
  coamplified_mean_gene_effect = colMeans(effect[group_a, , drop = FALSE], na.rm = TRUE),
  source_only_mean_gene_effect = colMeans(effect[group_b, , drop = FALSE], na.rm = TRUE),
  effect_size = stats$logFC,
  moderated_t = stats$t,
  p_value = stats$P.Value,
  fdr = stats$adj.P.Val
)
out[, stronger_coamplified_dependency := effect_size < 0]
setorder(out, fdr, effect_size)

safe <- function(x) gsub("(^_+|_+$)", "", gsub("[^A-Za-z0-9]+", "_", x))
scope <- if (is.null(lineage_filter)) "ALL" else safe(lineage_filter)
output_root <- file.path(knowledge_root, "depmap-26q1-full", "coamplification_dependency",
                         paste0(source_gene, "__", partner_gene), scope)
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)
fwrite(out, file.path(output_root, "all_genes.csv.gz"), compress = "gzip")
fwrite(out[fdr <= 0.05 & stronger_coamplified_dependency == TRUE],
       file.path(output_root, "selective_hits.csv.gz"), compress = "gzip")
manifest <- list(
  schema_version = 1,
  release = "26Q1",
  family = "coamplification_dependency",
  status = "complete",
  source_gene = source_gene,
  partner_gene = partner_gene,
  lineage = if (is.null(lineage_filter)) "ALL" else lineage_filter,
  copy_number_threshold = threshold,
  group_a = paste0(source_gene, ">=", threshold, " and ", partner_gene, ">=", threshold),
  group_b = paste0(source_gene, ">=", threshold, " and ", partner_gene, "<", threshold),
  coamplified_n = sum(group_a),
  source_only_n = sum(group_b),
  target_gene_count = ncol(effect),
  selective_fdr_hit_count = out[fdr <= 0.05 & stronger_coamplified_dependency == TRUE, .N],
  method = "limma two-group contrast with empirical Bayes moderation",
  multiple_testing = "BH FDR across all CRISPR targets for this requested comparison",
  interpretation = "negative effect_size means stronger dependency in coamplified models",
  tm00_reference = "13 (generalized from MYCN/DDX1 fixed case)",
  inputs = c(effect_path, cnv_path, model_path, condition_path)
)
write_json(manifest, file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed coamplification Run: ", output_root)
