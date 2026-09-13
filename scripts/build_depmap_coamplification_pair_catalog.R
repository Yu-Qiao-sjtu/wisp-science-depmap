#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
  library(Matrix)
})

args <- commandArgs(trailingOnly = TRUE)
arg <- function(name, default = NULL) {
  hit <- grep(paste0("^--", name, "="), args, value = TRUE)
  if (!length(hit)) return(default)
  sub(paste0("^--", name, "="), "", hit[[1]])
}

data_root <- normalizePath(arg("data-root"), mustWork = TRUE)
knowledge_root <- normalizePath(arg("knowledge-root"), mustWork = TRUE)
threshold <- as.numeric(arg("threshold", "2"))
min_source_amp_n <- as.integer(arg("min-source-amp-n", "10"))
min_coamp_n <- as.integer(arg("min-coamp-n", "5"))
min_source_only_n <- as.integer(arg("min-source-only-n", "5"))
output_root <- file.path(knowledge_root, "depmap-26q1-full",
                         "coamplification_dependency", "pair_catalog")
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

cnv_path <- file.path(data_root, "OmicsCNGeneMC_WES.csv")
condition_path <- file.path(data_root, "ModelCondition.csv")
stopifnot(file.exists(cnv_path), file.exists(condition_path))

message("Reading and aligning default copy-number profiles")
cnv <- fread(cnv_path, check.names = FALSE)
condition_ids <- cnv[[1]]
default_entry <- cnv[[2]] == "Yes"
cnv[[1]] <- NULL
cnv[[1]] <- NULL
genes <- toupper(sub(" \\([^()]++\\)$", "", names(cnv), perl = TRUE))
conditions <- fread(condition_path, select = c("ModelConditionID", "ModelID"))
model_ids <- conditions$ModelID[match(condition_ids, conditions$ModelConditionID)]
keep <- default_entry & !is.na(model_ids)
cnv <- as.matrix(cnv[keep])
storage.mode(cnv) <- "double"
rownames(cnv) <- model_ids[keep]
colnames(cnv) <- genes

amp <- is.finite(cnv) & cnv >= threshold
amp_n <- colSums(amp)
eligible_gene <- amp_n >= min_source_amp_n
amp <- amp[, eligible_gene, drop = FALSE]
amp_n <- amp_n[eligible_gene]
genes <- genes[eligible_gene]
rm(cnv)
gc(FALSE)
message(sprintf("Eligible amplified genes: %d; models: %d", ncol(amp), nrow(amp)))

amp_sparse <- Matrix(amp * 1L, sparse = TRUE)
co <- crossprod(amp_sparse)
triples <- summary(co)
triples <- as.data.table(triples)
triples <- triples[i < j & x >= min_coamp_n]
setnames(triples, c("i", "j", "x"), c("gene_a_index", "gene_b_index", "coamplified_n"))
triples[, `:=`(
  gene_a = genes[gene_a_index],
  gene_b = genes[gene_b_index],
  gene_a_amp_n = as.integer(amp_n[gene_a_index]),
  gene_b_amp_n = as.integer(amp_n[gene_b_index])
)]
triples[, `:=`(
  gene_a_only_n = gene_a_amp_n - coamplified_n,
  gene_b_only_n = gene_b_amp_n - coamplified_n,
  jaccard = coamplified_n / (gene_a_amp_n + gene_b_amp_n - coamplified_n)
)]

direction_a <- triples[gene_a_only_n >= min_source_only_n, .(
  source_gene = gene_a,
  partner_gene = gene_b,
  source_amp_n = gene_a_amp_n,
  partner_amp_n = gene_b_amp_n,
  coamplified_n,
  source_only_n = gene_a_only_n,
  partner_only_n = gene_b_only_n,
  jaccard
)]
direction_b <- triples[gene_b_only_n >= min_source_only_n, .(
  source_gene = gene_b,
  partner_gene = gene_a,
  source_amp_n = gene_b_amp_n,
  partner_amp_n = gene_a_amp_n,
  coamplified_n,
  source_only_n = gene_b_only_n,
  partner_only_n = gene_a_only_n,
  jaccard
)]
directional <- rbindlist(list(direction_a, direction_b))
setorder(directional, -coamplified_n, -jaccard, source_gene, partner_gene)
directional[, pair_id := sprintf("COAMP-%09d", .I)]
setcolorder(directional, c("pair_id", setdiff(names(directional), "pair_id")))

fwrite(data.table(gene = genes, amplified_n = as.integer(amp_n)),
       file.path(output_root, "eligible_gene_catalog.csv.gz"), compress = "gzip")
fwrite(triples[, .(gene_a, gene_b, gene_a_amp_n, gene_b_amp_n, coamplified_n,
                   gene_a_only_n, gene_b_only_n, jaccard)],
       file.path(output_root, "unordered_pair_catalog.csv.gz"), compress = "gzip")
fwrite(directional, file.path(output_root, "directional_pair_catalog.csv.gz"),
       compress = "gzip")

manifest <- list(
  schema_version = 1,
  release = "26Q1",
  family = "coamplification_pair_catalog",
  status = "complete",
  copy_number_threshold = threshold,
  model_count = nrow(amp),
  input_gene_count = length(eligible_gene),
  eligible_gene_count = length(genes),
  min_source_amp_n = min_source_amp_n,
  min_coamplified_n = min_coamp_n,
  min_source_only_n = min_source_only_n,
  unordered_pair_count = nrow(triples),
  directional_pair_count = nrow(directional),
  selection = "source amplified and partner coamplified versus source amplified without partner amplification",
  method = "sparse binary cross-product of default ModelCondition copy-number profiles",
  tm00_reference = "13",
  inputs = c(cnv_path, condition_path)
)
write_json(manifest, file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed coamplification pair catalog: ", output_root)
