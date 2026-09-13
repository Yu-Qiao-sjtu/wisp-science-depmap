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
min_lineage_n <- as.integer(arg("min-lineage-n", "10"))
fdr_max <- as.numeric(arg("fdr-max", "0.05"))
output_root <- file.path(knowledge_root, "depmap-26q1-full", "lineage_selective_dependency")
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

effect_path <- file.path(data_root, "CRISPRGeneEffect.csv")
model_path <- file.path(data_root, "Model.csv")
stopifnot(file.exists(effect_path), file.exists(model_path))

message("Reading CRISPR Gene Effect")
effect_dt <- fread(effect_path, check.names = FALSE)
model_ids <- effect_dt[[1]]
effect_dt[[1]] <- NULL
genes <- sub(" \\([^()]++\\)$", "", names(effect_dt), perl = TRUE)
effect <- as.matrix(effect_dt)
storage.mode(effect) <- "double"
rownames(effect) <- model_ids
colnames(effect) <- genes
rm(effect_dt)

models <- fread(model_path, select = c("ModelID", "OncotreeLineage"))
models <- models[!is.na(ModelID) & !is.na(OncotreeLineage) & nzchar(OncotreeLineage)]
common <- intersect(rownames(effect), models$ModelID)
effect <- effect[common, , drop = FALSE]
lineage <- models$OncotreeLineage[match(common, models$ModelID)]
lineage_counts <- sort(table(lineage), decreasing = TRUE)
eligible <- names(lineage_counts[lineage_counts >= min_lineage_n])
pan_mean <- colMeans(effect, na.rm = TRUE)

safe_key <- function(value) gsub("(^_+|_+$)", "", gsub("[^A-Za-z0-9]+", "_", value))
summaries <- vector("list", length(eligible))

for (i in seq_along(eligible)) {
  label <- eligible[[i]]
  in_group <- lineage == label
  group <- factor(ifelse(in_group, "lineage", "rest"), levels = c("rest", "lineage"))
  design <- model.matrix(~ 0 + group)
  colnames(design) <- levels(group)
  fit <- lmFit(t(effect), design)
  fit <- contrasts.fit(fit, makeContrasts(lineage - rest, levels = design))
  fit <- eBayes(fit, robust = TRUE)
  result <- topTable(fit, number = Inf, sort.by = "none")
  out <- data.table(
    gene = genes,
    lineage = label,
    lineage_n = sum(in_group),
    rest_n = sum(!in_group),
    lineage_mean_gene_effect = colMeans(effect[in_group, , drop = FALSE], na.rm = TRUE),
    rest_mean_gene_effect = colMeans(effect[!in_group, , drop = FALSE], na.rm = TRUE),
    pan_mean_gene_effect = pan_mean,
    effect_size = result$logFC,
    moderated_t = result$t,
    p_value = result$P.Value,
    fdr = result$adj.P.Val
  )
  out[, stronger_lineage_dependency := effect_size < 0]
  out[, passes_fdr := !is.na(fdr) & fdr <= fdr_max]
  setorder(out, fdr, effect_size)
  lineage_root <- file.path(output_root, safe_key(label))
  dir.create(lineage_root, recursive = TRUE, showWarnings = FALSE)
  fwrite(out, file.path(lineage_root, "all_genes.csv.gz"), compress = "gzip")
  fwrite(out[passes_fdr == TRUE & stronger_lineage_dependency == TRUE],
         file.path(lineage_root, "selective_hits.csv.gz"), compress = "gzip")
  manifest <- list(
    schema_version = 1,
    release = "26Q1",
    family = "lineage_selective_dependency",
    lineage = label,
    status = "complete",
    lineage_n = sum(in_group),
    rest_n = sum(!in_group),
    target_gene_count = ncol(effect),
    selective_fdr_hit_count = out[passes_fdr == TRUE & stronger_lineage_dependency == TRUE, .N],
    method = "limma two-group contrast with empirical Bayes moderation; lineage minus all other annotated lineages",
    multiple_testing = "BH FDR across all CRISPR targets within lineage",
    interpretation = "negative effect_size means stronger dependency in the requested lineage",
    fdr_max = fdr_max,
    tm00_reference = "14 (generalized from fixed disease-context contrast)",
    inputs = c(effect_path, model_path)
  )
  write_json(manifest, file.path(lineage_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
  summaries[[i]] <- data.table(
    lineage = label,
    lineage_key = safe_key(label),
    lineage_n = sum(in_group),
    rest_n = sum(!in_group),
    selective_fdr_hit_count = manifest$selective_fdr_hit_count,
    status = "complete"
  )
  message(sprintf("[%d/%d] %s: n=%d, selective FDR hits=%d", i, length(eligible), label,
                  sum(in_group), manifest$selective_fdr_hit_count))
}

catalog <- rbindlist(summaries)
fwrite(catalog, file.path(output_root, "lineage_catalog.csv"))
write_json(list(
  schema_version = 1,
  release = "26Q1",
  family = "lineage_selective_dependency",
  status = "complete",
  eligible_lineage_count = nrow(catalog),
  min_lineage_n = min_lineage_n,
  target_gene_count = ncol(effect),
  method = "limma two-group contrast with empirical Bayes moderation",
  multiple_testing = "BH FDR across all CRISPR targets separately within each lineage",
  tm00_reference = "14"
), file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)

message("Completed lineage selective dependency build: ", output_root)
