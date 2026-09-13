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
min_group_n <- as.integer(arg("min-group-n", "25"))
max_amp_n <- as.integer(arg("max-amp-n", "100"))
max_jaccard <- as.numeric(arg("max-jaccard", "0.4"))
fdr_max <- as.numeric(arg("fdr-max", "0.05"))
shard_size <- as.integer(arg("shard-size", "256"))
top_per_pair <- as.integer(arg("top-per-pair", "100"))

module_root <- file.path(knowledge_root, "depmap-26q1-full", "coamplification_dependency")
catalog_root <- file.path(module_root, "pair_catalog")
output_root <- file.path(module_root, "exhaustive_high_confidence")
shard_root <- file.path(output_root, "shards")
dir.create(shard_root, recursive = TRUE, showWarnings = FALSE)

effect_path <- file.path(data_root, "CRISPRGeneEffect.csv")
cnv_path <- file.path(data_root, "OmicsCNGeneMC_WES.csv")
condition_path <- file.path(data_root, "ModelCondition.csv")
pair_path <- file.path(catalog_root, "directional_pair_catalog.csv.gz")
stopifnot(file.exists(effect_path), file.exists(cnv_path), file.exists(condition_path),
          file.exists(pair_path))

read_effect <- function(path) {
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

message("Reading Gene Effect")
effect <- read_effect(effect_path)
target_genes <- colnames(effect)

message("Reading and aligning default copy-number profiles")
cnv_dt <- fread(cnv_path, check.names = FALSE)
condition_ids <- cnv_dt[[1]]
default_entry <- cnv_dt[[2]] == "Yes"
cnv_dt[[1]] <- NULL
cnv_dt[[1]] <- NULL
cnv_genes <- toupper(sub(" \\([^()]++\\)$", "", names(cnv_dt), perl = TRUE))
conditions <- fread(condition_path, select = c("ModelConditionID", "ModelID"))
model_ids <- conditions$ModelID[match(condition_ids, conditions$ModelConditionID)]
keep <- default_entry & !is.na(model_ids)
cnv <- as.matrix(cnv_dt[keep])
storage.mode(cnv) <- "double"
rownames(cnv) <- model_ids[keep]
colnames(cnv) <- cnv_genes
rm(cnv_dt)

common <- intersect(rownames(effect), rownames(cnv))
effect <- effect[common, , drop = FALSE]
cnv <- cnv[common, , drop = FALSE]
valid_effect <- is.finite(effect)
effect_zero <- effect
effect_zero[!valid_effect] <- 0
effect_sq <- effect_zero^2

message("Filtering directional pair catalog")
pairs <- fread(pair_path)
pairs <- pairs[
  source_gene %chin% target_genes & partner_gene %chin% target_genes &
    coamplified_n >= min_group_n & source_only_n >= min_group_n &
    partner_only_n >= min_group_n & source_amp_n <= max_amp_n &
    partner_amp_n <= max_amp_n & jaccard <= max_jaccard
]
setorder(pairs, source_gene, partner_gene)
pairs[, screen_pair_id := sprintf("COAMP-HC-%06d", .I)]
setcolorder(pairs, c("screen_pair_id", setdiff(names(pairs), "screen_pair_id")))
fwrite(pairs, file.path(output_root, "screen_pair_catalog.csv.gz"), compress = "gzip")
message("High-confidence directional pairs: ", nrow(pairs))
if (!nrow(pairs)) stop("no pair satisfies the fixed high-confidence screen contract")

cnv_index <- setNames(seq_along(cnv_genes), cnv_genes)
shard_starts <- seq.int(1L, nrow(pairs), by = shard_size)
summaries <- vector("list", length(shard_starts))

for (shard_i in seq_along(shard_starts)) {
  start <- shard_starts[[shard_i]]
  end <- min(start + shard_size - 1L, nrow(pairs))
  output_path <- file.path(shard_root, sprintf("shard_%06d_%06d.csv.gz", start, end))
  manifest_path <- file.path(shard_root, sprintf("shard_%06d_%06d.manifest.json", start, end))
  if (file.exists(output_path) && file.exists(manifest_path)) {
    old <- fromJSON(manifest_path)
    if (identical(old$status, "complete")) {
      summaries[[shard_i]] <- data.table(start = start, end = end,
                                          retained_hit_count = old$retained_hit_count,
                                          status = "skipped_existing")
      next
    }
  }

  part <- pairs[start:end]
  source_idx <- unname(cnv_index[part$source_gene])
  partner_idx <- unname(cnv_index[part$partner_gene])
  group_a <- matrix(FALSE, nrow = nrow(part), ncol = nrow(cnv))
  group_b <- matrix(FALSE, nrow = nrow(part), ncol = nrow(cnv))
  for (i in seq_len(nrow(part))) {
    source_amp <- is.finite(cnv[, source_idx[[i]]]) & cnv[, source_idx[[i]]] >= threshold
    partner_amp <- is.finite(cnv[, partner_idx[[i]]]) & cnv[, partner_idx[[i]]] >= threshold
    group_a[i, ] <- source_amp & partner_amp
    group_b[i, ] <- source_amp & !partner_amp
  }
  ga <- Matrix(group_a * 1, sparse = TRUE)
  gb <- Matrix(group_b * 1, sparse = TRUE)
  n_a <- as.matrix(ga %*% (valid_effect * 1))
  n_b <- as.matrix(gb %*% (valid_effect * 1))
  sum_a <- as.matrix(ga %*% effect_zero)
  sum_b <- as.matrix(gb %*% effect_zero)
  sq_a <- as.matrix(ga %*% effect_sq)
  sq_b <- as.matrix(gb %*% effect_sq)
  mean_a <- sum_a / n_a
  mean_b <- sum_b / n_b
  var_a <- pmax((sq_a - (sum_a^2 / n_a)) / pmax(n_a - 1, 1), 0)
  var_b <- pmax((sq_b - (sum_b^2 / n_b)) / pmax(n_b - 1, 1), 0)
  se2 <- var_a / n_a + var_b / n_b
  difference <- mean_a - mean_b
  t_stat <- difference / sqrt(se2)
  df <- se2^2 / ((var_a / n_a)^2 / pmax(n_a - 1, 1) +
                 (var_b / n_b)^2 / pmax(n_b - 1, 1))
  p_less <- pt(t_stat, df = df, lower.tail = TRUE)
  p_less[!is.finite(p_less) | n_a < min_group_n | n_b < min_group_n] <- NA_real_

  rows <- vector("list", nrow(part))
  for (i in seq_len(nrow(part))) {
    fdr <- rep(NA_real_, ncol(effect))
    eligible <- is.finite(p_less[i, ])
    fdr[eligible] <- p.adjust(p_less[i, eligible], method = "BH")
    hit <- which(eligible & fdr <= fdr_max & difference[i, ] < 0)
    if (!length(hit)) next
    hit <- head(hit[order(fdr[hit], difference[i, hit])], top_per_pair)
    rows[[i]] <- data.table(
      screen_pair_id = part$screen_pair_id[[i]],
      source_gene = part$source_gene[[i]],
      partner_gene = part$partner_gene[[i]],
      target_gene = target_genes[hit],
      coamplified_n = as.integer(n_a[i, hit]),
      source_only_n = as.integer(n_b[i, hit]),
      coamplified_mean_gene_effect = mean_a[i, hit],
      source_only_mean_gene_effect = mean_b[i, hit],
      mean_difference = difference[i, hit],
      welch_t = t_stat[i, hit],
      welch_df = df[i, hit],
      p_coamplified_more_dependent = p_less[i, hit],
      fdr_coamplified_more_dependent = fdr[hit],
      rank_within_pair = seq_along(hit)
    )
  }
  out <- rbindlist(rows, fill = TRUE)
  if (!ncol(out)) {
    out <- data.table(
      screen_pair_id = character(), source_gene = character(),
      partner_gene = character(), target_gene = character(),
      coamplified_n = integer(), source_only_n = integer(),
      coamplified_mean_gene_effect = numeric(),
      source_only_mean_gene_effect = numeric(), mean_difference = numeric(),
      welch_t = numeric(), welch_df = numeric(),
      p_coamplified_more_dependent = numeric(),
      fdr_coamplified_more_dependent = numeric(), rank_within_pair = integer()
    )
  }
  fwrite(out, output_path, compress = "gzip")
  shard_manifest <- list(
    schema_version = 1, release = "26Q1", family = "coamplification_dependency",
    status = "complete", pair_start = start, pair_end = end,
    pair_count = nrow(part), target_gene_count = ncol(effect),
    retained_hit_count = nrow(out), fdr_max = fdr_max,
    method = "Welch t-test; coamplified versus source-only; BH FDR per directional pair"
  )
  write_json(shard_manifest, manifest_path, auto_unbox = TRUE, pretty = TRUE)
  summaries[[shard_i]] <- data.table(start = start, end = end,
                                      retained_hit_count = nrow(out), status = "complete")
  message(sprintf("[%d/%d] pairs %d-%d retained=%d", shard_i, length(shard_starts),
                  start, end, nrow(out)))
  rm(part, group_a, group_b, ga, gb, n_a, n_b, sum_a, sum_b, sq_a, sq_b,
     mean_a, mean_b, var_a, var_b, se2, difference, t_stat, df, p_less, rows, out)
  gc(FALSE)
}

shard_files <- sort(list.files(shard_root, pattern = "\\.csv\\.gz$", full.names = TRUE))
all_hits <- rbindlist(lapply(shard_files, fread), fill = TRUE)
if (nrow(all_hits)) setorder(all_hits, fdr_coamplified_more_dependent, mean_difference)
fwrite(all_hits, file.path(output_root, "significant_hits.csv.gz"), compress = "gzip")
summary_dt <- rbindlist(summaries)
fwrite(summary_dt, file.path(output_root, "shard_catalog.csv"))

manifest <- list(
  schema_version = 1,
  release = "26Q1",
  family = "coamplification_dependency",
  layer = "exhaustive_high_confidence",
  status = "complete",
  copy_number_threshold = threshold,
  min_group_n = min_group_n,
  max_amp_n = max_amp_n,
  max_jaccard = max_jaccard,
  directional_pair_count = nrow(pairs),
  target_gene_count = ncol(effect),
  nominal_test_count = nrow(pairs) * ncol(effect),
  retained_significant_hit_count = nrow(all_hits),
  pair_with_retained_hit_count = if (nrow(all_hits)) uniqueN(all_hits$screen_pair_id) else 0L,
  fdr_max = fdr_max,
  top_per_pair = top_per_pair,
  method = "Welch t-test using matrix-aggregated sufficient statistics",
  multiple_testing = "BH FDR across all CRISPR targets separately within each directional coamplification pair",
  interpretation = "negative mean difference means stronger dependency in source+partner coamplified models than source-only models",
  long_tail_policy = "eligible pairs outside this discovery layer remain in pair_catalog and run through the on-demand pair runner",
  tm00_reference = "13",
  inputs = c(effect_path, cnv_path, condition_path, pair_path)
)
write_json(manifest, file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed exhaustive high-confidence coamplification dependency screen: ", output_root)
