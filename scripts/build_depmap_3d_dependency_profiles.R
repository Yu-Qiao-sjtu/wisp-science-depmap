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
data_root <- normalizePath(arg("data-root"), mustWork = TRUE)
knowledge_root <- normalizePath(arg("knowledge-root"), mustWork = TRUE)
output_root <- file.path(knowledge_root, "depmap-26q1-3d", "dependency_profiles")
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

meta <- fread(file.path(data_root, "screen_metadata.csv"))[PassesQC == TRUE]
read_matrix <- function(path) {
  x <- fread(path, check.names = FALSE)
  ids <- x[[1]]
  x[[1]] <- NULL
  genes <- toupper(sub(" \\([^()]++\\)$", "", names(x), perl = TRUE))
  mat <- as.matrix(x)
  storage.mode(mat) <- "double"
  rownames(mat) <- ids
  colnames(mat) <- genes
  mat
}
effect <- read_matrix(file.path(data_root, "screen_gene_effect.csv"))
dependency <- read_matrix(file.path(data_root, "screen_gene_dependency.csv"))
common_screens <- intersect(rownames(effect), rownames(dependency))
common_genes <- intersect(colnames(effect), colnames(dependency))
effect <- effect[common_screens, common_genes, drop = FALSE]
dependency <- dependency[common_screens, common_genes, drop = FALSE]
stopifnot(identical(rownames(effect), rownames(dependency)), identical(colnames(effect), colnames(dependency)))
meta <- meta[match(rownames(effect), ScreenID)]
genes <- colnames(effect)

controls_path <- file.path(data_root, "crispr_control_genes.csv")
controls <- if (file.exists(controls_path)) fread(controls_path) else data.table()
control_gene_col <- intersect(c("Gene", "gene", "symbol"), names(controls))
control_class_col <- intersect(c("Type", "type", "class", "Essentiality", "Category"), names(controls))
control_map <- if (length(control_gene_col) && length(control_class_col))
  setNames(as.character(controls[[control_class_col[[1]]]]),
           toupper(sub(" \\([^()]++\\)$", "", controls[[control_gene_col[[1]]]], perl = TRUE))) else character()

groups <- list(three_d_all = c("3DO", "3DN"), three_d_organoid = "3DO", three_d_cns = "3DN",
               nextgen_two_d = c("2DO", "2DN"), traditional_two_d = "2DS")
groups <- groups[vapply(groups, function(types) any(meta$ScreenType %chin% types), logical(1))]
catalog <- vector("list", length(groups))
for (i in seq_along(groups)) {
  key <- names(groups)[[i]]
  idx <- which(meta$ScreenType %chin% groups[[i]])
  e <- effect[idx, , drop = FALSE]
  d <- dependency[idx, , drop = FALSE]
  out <- data.table(
    gene = genes,
    valid_gene_effect_n = colSums(is.finite(e)),
    mean_gene_effect = colMeans(e, na.rm = TRUE),
    median_gene_effect = apply(e, 2L, median, na.rm = TRUE),
    strong_dependency_fraction = colMeans(e <= -0.5, na.rm = TRUE),
    very_strong_dependency_fraction = colMeans(e <= -1, na.rm = TRUE),
    valid_dependency_probability_n = colSums(is.finite(d)),
    mean_dependency_probability = colMeans(d, na.rm = TRUE),
    dependency_probability_gt_05_fraction = colMeans(d > 0.5, na.rm = TRUE),
    dependency_probability_gt_09_fraction = colMeans(d > 0.9, na.rm = TRUE)
  )
  out[, control_class := unname(control_map[gene])]
  out[, robust_group_dependency := valid_gene_effect_n >= max(5L, floor(length(idx) * 0.7)) &
        mean_gene_effect <= -0.5 & dependency_probability_gt_05_fraction >= 0.5]
  setorder(out, -robust_group_dependency, mean_gene_effect, -mean_dependency_probability)
  group_root <- file.path(output_root, key)
  dir.create(group_root, recursive = TRUE, showWarnings = FALSE)
  fwrite(out, file.path(group_root, "all_genes.csv.gz"), compress = "gzip")
  fwrite(out[robust_group_dependency == TRUE], file.path(group_root, "robust_dependencies.csv.gz"), compress = "gzip")
  manifest <- list(schema_version = 1, release = "NextGen Model Manuscript 2026",
    family = "3d_dependency_profiles", group = key, status = "complete",
    screen_types = groups[[i]], screen_count = length(idx), target_gene_count = ncol(e),
    robust_dependency_count = out[robust_group_dependency == TRUE, .N],
    robust_definition = "at least 70% valid screens (minimum 5), mean Gene Effect <= -0.5, and dependency probability >0.5 in at least half of screens")
  write_json(manifest, file.path(group_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
  catalog[[i]] <- data.table(group = key, screen_types = paste(groups[[i]], collapse = "+"),
    screen_count = length(idx), robust_dependency_count = out[robust_group_dependency == TRUE, .N], status = "complete")
}
catalog <- rbindlist(catalog)
fwrite(catalog, file.path(output_root, "group_catalog.csv"))
write_json(list(schema_version = 1, release = "NextGen Model Manuscript 2026",
  family = "3d_dependency_profiles", status = "complete", group_count = nrow(catalog)),
  file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed 3D dependency profiles: ", output_root)
