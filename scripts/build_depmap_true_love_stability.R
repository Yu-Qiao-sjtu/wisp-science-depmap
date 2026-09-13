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
n_bootstrap <- as.integer(arg("n-bootstrap", "100"))
sample_fraction <- as.numeric(arg("sample-fraction", "0.8"))
stability_min <- as.numeric(arg("stability-min", "0.7"))
cor_max <- as.numeric(arg("cor-max", "-0.2"))
fdr_max <- as.numeric(arg("fdr-max", "0.05"))
seed <- as.integer(arg("seed", "2601"))
checkpoint_every <- as.integer(arg("checkpoint-every", "5"))

module_root <- file.path(knowledge_root, "depmap-26q1-full", "true_love_gene")
output_root <- path.expand(arg("output-root",
  file.path(module_root, "high_confidence_stability")))
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)
pair_path <- file.path(module_root, "strict_mutual_rank1_pairs.csv.gz")
effect_path <- file.path(data_root, "CRISPRGeneEffect.csv")
stopifnot(file.exists(pair_path), file.exists(effect_path))

pairs <- fread(pair_path)
pairs[, passes_effect_fdr := correlation_a_to_b <= cor_max &
  correlation_b_to_a <= cor_max & fdr_a_to_b <= fdr_max & fdr_b_to_a <= fdr_max]

message("Reading Gene Effect and constructing the complete-coverage competition universe")
effect_dt <- fread(effect_path, check.names = FALSE)
model_ids <- effect_dt[[1]]
effect_dt[[1]] <- NULL
genes <- toupper(sub(" \\([^()]++\\)$", "", names(effect_dt), perl = TRUE))
effect <- as.matrix(effect_dt)
storage.mode(effect) <- "double"
rownames(effect) <- model_ids
colnames(effect) <- genes
rm(effect_dt)

complete_gene <- colSums(is.finite(effect)) == nrow(effect)
competition_genes <- genes[complete_gene]
competition <- effect[, complete_gene, drop = FALSE]
gene_index <- setNames(seq_along(competition_genes), competition_genes)

eligible <- pairs[passes_effect_fdr == TRUE & gene_a %chin% competition_genes &
                    gene_b %chin% competition_genes]
excluded <- pairs[passes_effect_fdr == TRUE &
                    !(gene_a %chin% competition_genes & gene_b %chin% competition_genes)]
eligible[, candidate_pair_row := .I]
candidate_genes <- sort(unique(c(eligible$gene_a, eligible$gene_b)))
candidate_index <- unname(gene_index[candidate_genes])
candidate_lookup <- setNames(seq_along(candidate_genes), candidate_genes)
pair_a_row <- unname(candidate_lookup[eligible$gene_a])
pair_b_row <- unname(candidate_lookup[eligible$gene_b])
pair_a_col <- unname(gene_index[eligible$gene_a])
pair_b_col <- unname(gene_index[eligible$gene_b])

checkpoint_path <- file.path(output_root, "bootstrap.checkpoint.rds")
atomic_save_rds <- function(value, path) {
  temp <- paste0(path, ".tmp.", Sys.getpid())
  on.exit(unlink(temp), add = TRUE)
  saveRDS(value, temp, compress = "xz")
  if (!file.rename(temp, path)) stop("failed to publish checkpoint: ", path)
}

message(sprintf("Testing %d pairs (%d candidate genes) against %d complete-coverage genes",
                nrow(eligible), length(candidate_genes), length(competition_genes)))
set.seed(seed)
sample_n <- floor(nrow(competition) * sample_fraction)
stable_count <- integer(nrow(eligible))
pair_correlations <- matrix(NA_real_, nrow = nrow(eligible), ncol = n_bootstrap)
start_bootstrap <- 1L

if (file.exists(checkpoint_path)) {
  checkpoint <- tryCatch(readRDS(checkpoint_path), error = function(...) NULL)
  compatible <- !is.null(checkpoint) && identical(checkpoint$n_bootstrap, n_bootstrap) &&
    identical(checkpoint$sample_fraction, sample_fraction) && identical(checkpoint$seed, seed) &&
    identical(checkpoint$eligible_pair_count, nrow(eligible))
  if (compatible) {
    stable_count <- checkpoint$stable_count
    pair_correlations <- checkpoint$pair_correlations
    .Random.seed <- checkpoint$random_seed
    start_bootstrap <- checkpoint$completed_bootstrap + 1L
    message(sprintf("Resuming bootstrap checkpoint at replicate %d/%d",
                    start_bootstrap, n_bootstrap))
  } else {
    stop("incompatible or unreadable bootstrap checkpoint: ", checkpoint_path)
  }
}

if (start_bootstrap <= n_bootstrap) for (b in seq.int(start_bootstrap, n_bootstrap)) {
  selected <- sample.int(nrow(competition), sample_n, replace = FALSE)
  boot <- competition[selected, , drop = FALSE]
  center <- colMeans(boot)
  boot <- sweep(boot, 2L, center, "-")
  scale_sd <- sqrt(colSums(boot^2) / pmax(nrow(boot) - 1L, 1L))
  valid_sd <- is.finite(scale_sd) & scale_sd > 0
  boot[, valid_sd] <- sweep(boot[, valid_sd, drop = FALSE], 2L,
                            scale_sd[valid_sd], "/")
  boot[, !valid_sd] <- 0
  correlations <- crossprod(boot[, candidate_index, drop = FALSE], boot) /
    pmax(nrow(boot) - 1L, 1L)
  correlations[cbind(seq_along(candidate_genes), candidate_index)] <- Inf
  rank1_col <- max.col(-correlations, ties.method = "first")
  reciprocal <- rank1_col[pair_a_row] == pair_b_col &
    rank1_col[pair_b_row] == pair_a_col
  stable_count <- stable_count + reciprocal
  pair_correlations[, b] <- correlations[cbind(pair_a_row, pair_b_col)]
  if (b %% 10L == 0L || b == n_bootstrap) {
    message(sprintf("[%d/%d] bootstrap replicates complete", b, n_bootstrap))
  }
  if (b %% checkpoint_every == 0L || b == n_bootstrap) {
    atomic_save_rds(list(
      schema_version = 1L, completed_bootstrap = b, n_bootstrap = n_bootstrap,
      sample_fraction = sample_fraction, seed = seed,
      eligible_pair_count = nrow(eligible), stable_count = stable_count,
      pair_correlations = pair_correlations, random_seed = .Random.seed
    ), checkpoint_path)
  }
  rm(boot, correlations)
  gc(FALSE)
}

eligible[, `:=`(
  bootstrap_reciprocal_count = stable_count,
  bootstrap_reciprocal_stability = stable_count / n_bootstrap,
  bootstrap_correlation_median = apply(pair_correlations, 1L, median, na.rm = TRUE),
  bootstrap_correlation_q025 = apply(pair_correlations, 1L, quantile, probs = 0.025,
                                     na.rm = TRUE, names = FALSE),
  bootstrap_correlation_q975 = apply(pair_correlations, 1L, quantile, probs = 0.975,
                                     na.rm = TRUE, names = FALSE)
)]
eligible[, passes_stability := bootstrap_reciprocal_stability >= stability_min]
setorder(eligible, -passes_stability, -bootstrap_reciprocal_stability,
         worst_direction_fdr, correlation_a_to_b)
final <- eligible[passes_stability == TRUE]
fwrite(eligible, file.path(output_root, "all_stability_results.csv.gz"), compress = "gzip")
fwrite(final, file.path(output_root, "final_high_confidence_true_love_genes.csv.gz"),
       compress = "gzip")
fwrite(excluded, file.path(output_root, "excluded_incomplete_coverage_pairs.csv.gz"),
       compress = "gzip")

examples <- data.table(
  example = c("ASB7::SUV39H1", "CCNF::E2F1"),
  gene_a = c("ASB7", "CCNF"), gene_b = c("SUV39H1", "E2F1")
)
examples[, result_row := vapply(seq_len(.N), function(i) {
  hit <- eligible[(gene_a == examples$gene_a[[i]] & gene_b == examples$gene_b[[i]]) |
                  (gene_a == examples$gene_b[[i]] & gene_b == examples$gene_a[[i]])]
  if (!nrow(hit)) return("not_eligible")
  sprintf("stability=%.2f;pass=%s", hit$bootstrap_reciprocal_stability[[1]],
          hit$passes_stability[[1]])
}, character(1))]
fwrite(examples, file.path(output_root, "tm00_example_stability.csv"))

manifest <- list(
  schema_version = 1, release = "26Q1", family = "true_love_gene",
  layer = "high_confidence_stability", status = "complete",
  input_strict_pair_count = nrow(pairs), effect_fdr_pair_count = pairs[passes_effect_fdr == TRUE, .N],
  complete_coverage_pair_count = nrow(eligible), excluded_incomplete_coverage_pair_count = nrow(excluded),
  competition_gene_count = length(competition_genes), model_count = nrow(competition),
  n_bootstrap = n_bootstrap, sample_fraction = sample_fraction, sample_n = sample_n,
  stability_min = stability_min, correlation_max = cor_max, fdr_max = fdr_max,
  final_high_confidence_pair_count = nrow(final), seed = seed,
  method = sprintf("%d repeated %.0f%% model subsamples; recompute each candidate gene's rank-1 negative Pearson correlation against the complete-coverage genome-wide competition universe",
                   n_bootstrap, 100 * sample_fraction),
  final_definition = "strict mutual negative rank 1, both directional FDR <= 0.05, full-data correlation <= -0.20, complete model coverage, and reciprocal rank-1 stability >= 0.70",
  interpretation = "stable reciprocal co-dependency is hypothesis-generating and does not establish direct regulation or causality",
  inputs = c(pair_path, effect_path)
)
write_json(manifest, file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
unlink(checkpoint_path)
message("Completed high-confidence True Love Gene stability analysis: ", output_root)
