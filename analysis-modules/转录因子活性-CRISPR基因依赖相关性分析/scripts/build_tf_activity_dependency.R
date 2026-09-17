#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
  library(decoupleR)
  library(dorothea)
  library(dplyr)
  library(tidyr)
})

args <- commandArgs(trailingOnly = TRUE)
value_after <- function(flag, default = NULL) {
  i <- match(flag, args)
  if (is.na(i)) return(default)
  if (i == length(args)) stop(flag, " 缺少值")
  args[[i + 1L]]
}
has_flag <- function(flag) flag %in% args

project_root <- normalizePath(value_after("--project-root", "."), winslash = "/", mustWork = TRUE)
data_root <- normalizePath(value_after("--data-root", file.path(project_root, "data")), winslash = "/", mustWork = TRUE)
output_root <- value_after("--output-root", file.path(project_root, "analysis-modules", "转录因子活性-CRISPR基因依赖相关性分析", "results", "tf_activity_dependency_26Q1"))
block_size <- as.integer(value_after("--block-size", "32"))
min_pair_n <- as.integer(value_after("--min-pair-n", "800"))
test_mode <- has_flag("--test")
overwrite <- has_flag("--overwrite")
if (is.na(block_size) || block_size < 1L) stop("--block-size 必须是正整数")
if (is.na(min_pair_n) || min_pair_n < 3L) stop("--min-pair-n 必须至少为3")
if (test_mode) min_pair_n <- 3L

clean_gene <- function(x) sub(" \\([0-9]+\\)$", "", x)
atomic_rds <- function(x, path, compress = FALSE) {
  tmp <- paste0(path, ".tmp")
  saveRDS(x, tmp, compress = compress)
  if (file.exists(path)) unlink(path)
  if (!file.rename(tmp, path)) stop("无法原子写入 ", path)
}
atomic_json <- function(x, path) {
  tmp <- paste0(path, ".tmp")
  write_json(x, tmp, auto_unbox = TRUE, pretty = TRUE, na = "null")
  if (file.exists(path)) unlink(path)
  if (!file.rename(tmp, path)) stop("无法原子写入 ", path)
}
md5 <- function(path) unname(tools::md5sum(path))

expression_path <- file.path(data_root, "OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv")
effect_path <- file.path(data_root, "CRISPRGeneEffect.csv")
if (!file.exists(expression_path) || !file.exists(effect_path)) stop("缺少26Q1表达或Gene Effect输入")
if (dir.exists(output_root) && file.exists(file.path(output_root, "manifest.json")) && !overwrite) {
  stop("输出已经存在；如需重算请使用新目录或 --overwrite")
}
dir.create(file.path(output_root, "blocks"), recursive = TRUE, showWarnings = FALSE)

cat("读取表达矩阵\n")
expr_raw <- fread(expression_path, showProgress = !test_mode)
metadata <- c("V1", "SequencingID", "ModelConditionID", "ModelID", "IsDefaultEntryForMC", "IsDefaultEntryForModel")
if (!all(metadata %chin% names(expr_raw))) stop("表达矩阵元数据字段不符合26Q1")
expr_raw <- expr_raw[IsDefaultEntryForMC == "Yes"]
expr_raw <- expr_raw[!duplicated(ModelID)]
gene_cols <- setdiff(names(expr_raw), metadata)
if (test_mode) {
  expr_raw <- expr_raw[seq_len(min(40L, nrow(expr_raw)))]
  gene_cols <- gene_cols[seq_len(min(500L, length(gene_cols)))]
}
expr_ids <- as.character(expr_raw$ModelID)
expr_genes <- clean_gene(gene_cols)
if (anyDuplicated(expr_genes)) stop("表达基因清理Entrez后重复")
expr_mat <- as.matrix(expr_raw[, ..gene_cols])
storage.mode(expr_mat) <- "double"
rownames(expr_mat) <- expr_ids
colnames(expr_mat) <- expr_genes
rm(expr_raw); gc()

data("dorothea_hs", package = "dorothea")
network <- dorothea_hs[dorothea_hs$confidence %in% c("A", "B", "C"), c("tf", "target", "mor", "confidence")]
network <- network[network$target %in% expr_genes, ]
if (test_mode) {
  counts <- table(network$tf)
  keep_tf <- names(counts[counts >= 5L])
  network <- network[network$tf %in% head(keep_tf, 5L), ]
}
if (!nrow(network)) stop("表达矩阵与DoRothEA网络没有可用交集")

cat("推断DoRothEA A-C ULM转录因子活性\n")
scores_long <- decouple(
  mat = t(expr_mat),
  network = network[, c("tf", "target", "mor")],
  .source = "tf", .target = "target",
  statistics = "ulm", args = list(.mor = "mor"), minsize = 5
)
tf_frame <- scores_long |>
  filter(statistic == "ulm") |>
  select(condition, source, score) |>
  pivot_wider(id_cols = condition, names_from = source, values_from = score) |>
  as.data.frame()
rownames(tf_frame) <- tf_frame$condition
tf_frame$condition <- NULL
tf_mat_all <- as.matrix(tf_frame)
storage.mode(tf_mat_all) <- "double"
rm(tf_frame, scores_long, expr_mat); gc()

cat("读取Gene Effect矩阵\n")
effect_raw <- fread(effect_path, showProgress = !test_mode)
effect_ids <- as.character(effect_raw[[1L]])
target_cols <- names(effect_raw)[-1L]
if (test_mode) target_cols <- target_cols[seq_len(min(300L, length(target_cols)))]
target_genes <- clean_gene(target_cols)
if (anyDuplicated(target_genes)) stop("Gene Effect基因清理Entrez后重复")
effect_mat <- as.matrix(effect_raw[, ..target_cols])
storage.mode(effect_mat) <- "double"
rownames(effect_mat) <- effect_ids
colnames(effect_mat) <- target_genes
rm(effect_raw); gc()

common_ids <- intersect(rownames(tf_mat_all), rownames(effect_mat))
if (length(common_ids) < 3L) stop("TF活性与Gene Effect共同样本不足")
tf_mat <- tf_mat_all[common_ids, , drop = FALSE]
effect_mat <- effect_mat[common_ids, , drop = FALSE]
tf_names <- colnames(tf_mat)
if (!length(tf_names)) stop("没有推断出TF活性")

fwrite(data.table(index = seq_along(common_ids), ModelID = common_ids), file.path(output_root, "sample_order.csv"), bom = TRUE)
fwrite(data.table(index = seq_along(tf_names), TF = tf_names), file.path(output_root, "tf_order.csv"), bom = TRUE)
fwrite(data.table(index = seq_along(target_genes), target_gene = target_genes), file.path(output_root, "target_gene_order.csv"), bom = TRUE)
fwrite(as.data.table(network), file.path(output_root, "dorothea_network_used.csv.gz"))
atomic_rds(tf_mat_all, file.path(output_root, "tf_activity_all_expression_models.rds"), compress = TRUE)
atomic_rds(tf_mat, file.path(output_root, "tf_activity_common_models.rds"), compress = TRUE)

cat("计算TF活性 × Gene Effect全量相关\n")
top_rows <- list()
run_rows <- list()
blocks <- split(seq_along(tf_names), ceiling(seq_along(tf_names) / block_size))
for (b in seq_along(blocks)) {
  idx <- blocks[[b]]
  path <- file.path(output_root, "blocks", sprintf("block_%04d_%04d.rds", min(idx), max(idx)))
  x <- tf_mat[, idx, drop = FALSE]
  correlation <- suppressWarnings(cor(x, effect_mat, use = "pairwise.complete.obs", method = "pearson"))
  pair_n <- vapply(seq_len(ncol(x)), function(j) colSums(!is.na(effect_mat) & !is.na(x[, j])), numeric(ncol(effect_mat)))
  pair_n <- t(pair_n)
  storage.mode(pair_n) <- "integer"
  dimnames(pair_n) <- dimnames(correlation)
  correlation[pair_n < 3L] <- NA_real_
  denom <- pmax(1 - correlation^2, .Machine$double.eps)
  t_stat <- correlation * sqrt(pmax(pair_n - 2, 0) / denom)
  p_value <- 2 * pt(-abs(t_stat), df = pmax(pair_n - 2, 1))
  p_value[pair_n < 3L | !is.finite(correlation)] <- NA_real_
  fdr <- matrix(NA_real_, nrow = nrow(p_value), ncol = ncol(p_value), dimnames = dimnames(p_value))
  for (i in seq_len(nrow(p_value))) {
    eligible <- pair_n[i, ] >= min_pair_n & !is.na(p_value[i, ])
    fdr[i, eligible] <- p.adjust(p_value[i, eligible], method = "BH")
  }
  dimnames(fdr) <- dimnames(correlation)
  block <- list(tf_index = idx, tf_names = tf_names[idx], target_genes = target_genes,
                correlation = correlation, pair_n = pair_n, p_value = p_value, fdr = fdr)
  atomic_rds(block, path, compress = FALSE)
  for (i in seq_len(nrow(correlation))) {
    eligible <- pair_n[i, ] >= min_pair_n & !is.na(correlation[i, ])
    ord_pos <- head(which(eligible)[order(correlation[i, eligible], decreasing = TRUE, na.last = NA)], 100L)
    ord_neg <- head(which(eligible)[order(correlation[i, eligible], decreasing = FALSE, na.last = NA)], 100L)
    add <- function(ii, direction) data.table(
      TF = rownames(correlation)[i], target_gene = target_genes[ii],
      correlation = correlation[i, ii], pair_n = pair_n[i, ii],
      p_value = p_value[i, ii], fdr = fdr[i, ii], direction = direction,
      rank = seq_along(ii)
    )
    top_rows[[length(top_rows) + 1L]] <- add(ord_pos, "positive")
    top_rows[[length(top_rows) + 1L]] <- add(ord_neg, "negative")
  }
  run_rows[[b]] <- list(start = min(idx), end = max(idx), file = basename(path), status = "complete")
  cat(sprintf("block %d/%d complete\n", b, length(blocks)))
}
top_hits <- rbindlist(top_rows)
fwrite(top_hits, file.path(output_root, "top_hits.csv.gz"))
sig_hits <- top_hits[!is.na(fdr) & fdr <= 0.05 & abs(correlation) >= 0.2]
fwrite(sig_hits, file.path(output_root, "top_hits_fdr05_absr02.csv.gz"))

manifest <- list(
  schema_version = 1, status = "complete", release = "26Q1",
  module_id = "tf_activity_crispr_dependency", generated_at = format(Sys.time(), "%Y-%m-%dT%H:%M:%S%z"),
  source_script = "TM00 05_from_gene_to_dependency.R fourth section",
  activity_method = "decoupleR ULM", network = "dorothea_hs confidence A-C", minsize = 5,
  expression_default_rule = "IsDefaultEntryForMC == Yes; first ModelID retained",
  expression_model_count = nrow(tf_mat_all), common_model_count = length(common_ids),
  expression_gene_count = length(expr_genes), network_edge_count = nrow(network),
  tf_count = length(tf_names), target_gene_count = length(target_genes), block_count = length(blocks),
  minimum_pair_n_for_fdr_and_ranking = min_pair_n,
  correlation = "Pearson pairwise complete observations",
  multiple_testing = "BH independently across Gene Effect targets with pair_n at or above the declared minimum within each TF",
  interpretation = list(negative = "higher TF activity accompanies more negative Gene Effect, hence stronger dependency",
                        positive = "higher TF activity accompanies weaker dependency"),
  inputs = list(expression = list(file = basename(expression_path), md5 = md5(expression_path)),
                gene_effect = list(file = basename(effect_path), md5 = md5(effect_path))),
  packages = list(R = R.version.string, decoupleR = as.character(packageVersion("decoupleR")), dorothea = as.character(packageVersion("dorothea"))),
  test_mode = test_mode, runs = run_rows
)
atomic_json(manifest, file.path(output_root, "manifest.json"))
cat("完成: ", output_root, "\n", sep = "")
