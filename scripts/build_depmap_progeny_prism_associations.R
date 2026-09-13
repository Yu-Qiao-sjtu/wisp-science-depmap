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
min_n <- as.integer(arg("min-n", "10"))
module_root <- file.path(knowledge_root, "depmap-26q1-full", "progeny_prism_associations")
dir.create(module_root, recursive = TRUE, showWarnings = FALSE)

score_path <- file.path(knowledge_root, "depmap-26q1-full", "progeny_dependency",
                        "progeny_pathway_scores.csv")
auc_path <- file.path(data_root, "PRISM_Repurposing_AUC_Matrix.csv")
compound_path <- file.path(data_root, "PRISM_Repurposing_Compound_Conditions.csv")
stopifnot(file.exists(score_path), file.exists(auc_path), file.exists(compound_path))

scores <- fread(score_path, check.names = FALSE)
auc <- fread(auc_path, check.names = FALSE)
score_ids <- scores[[1]]
auc_ids <- auc[[1]]
scores[[1]] <- NULL
auc[[1]] <- NULL
common <- intersect(score_ids, auc_ids)
stopifnot(length(common) >= min_n)
score_mat <- as.matrix(scores[match(common, score_ids)])
auc_mat <- as.matrix(auc[match(common, auc_ids)])
storage.mode(score_mat) <- "double"
storage.mode(auc_mat) <- "double"
rownames(score_mat) <- common
rownames(auc_mat) <- common

drug_meta <- unique(fread(compound_path, select = c("CompoundID", "CompoundName")))
drug_name <- setNames(drug_meta$CompoundName, drug_meta$CompoundID)
pathways <- colnames(score_mat)
drugs <- colnames(auc_mat)
results <- vector("list", length(pathways))

for (i in seq_along(pathways)) {
  pathway <- pathways[[i]]
  x <- score_mat[, i]
  rows <- vector("list", length(drugs))
  for (j in seq_along(drugs)) {
    y <- auc_mat[, j]
    keep <- is.finite(x) & is.finite(y)
    n <- sum(keep)
    if (n < min_n || sd(x[keep]) == 0 || sd(y[keep]) == 0) next
    test <- suppressWarnings(cor.test(x[keep], y[keep], method = "pearson"))
    rows[[j]] <- list(
      pathway = pathway,
      drug_id = drugs[[j]],
      drug_name = unname(drug_name[drugs[[j]]]),
      n = n,
      pearson_r = unname(test$estimate),
      p_value = test$p.value
    )
  }
  section <- rbindlist(rows, fill = TRUE)
  if (nrow(section)) {
    section[, fdr := p.adjust(p_value, method = "BH")]
    section[, abs_pearson_r := abs(pearson_r)]
    setorder(section, fdr, -abs_pearson_r)
    section[, abs_pearson_r := NULL]
  }
  results[[i]] <- section
  message(sprintf("[%d/%d] %s: %d eligible drugs", i, length(pathways), pathway, nrow(section)))
}

out <- rbindlist(results, fill = TRUE)
fwrite(out, file.path(module_root, "pathway_drug_associations.csv.gz"), compress = "gzip")
fwrite(out[fdr <= 0.05], file.path(module_root, "fdr_significant.csv.gz"), compress = "gzip")
manifest <- list(
  schema_version = 1,
  release = "26Q1",
  family = "progeny_prism_associations",
  status = "complete",
  common_sample_count = length(common),
  pathway_count = length(pathways),
  drug_count = length(drugs),
  retained_test_count = nrow(out),
  fdr_significant_count = out[fdr <= 0.05, .N],
  min_n = min_n,
  method = "pairwise Pearson correlation between PROGENy pathway activity and PRISM AUC",
  multiple_testing = "BH FDR across drugs separately within each pathway",
  interpretation = "negative correlation means higher pathway activity associates with lower PRISM AUC (greater sensitivity)",
  tm00_reference = "05.3",
  inputs = c(score_path, auc_path, compound_path)
)
write_json(manifest, file.path(module_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed PROGENy-PRISM build: ", module_root)
