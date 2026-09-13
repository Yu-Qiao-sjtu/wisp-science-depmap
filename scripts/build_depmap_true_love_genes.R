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

knowledge_root <- normalizePath(arg("knowledge-root"), mustWork = TRUE)
fdr_max <- as.numeric(arg("fdr-max", "0.05"))
min_pair_n <- as.integer(arg("min-pair-n", "500"))
input_root <- file.path(knowledge_root, "depmap-26q1-full", "effect_correlation")
output_root <- file.path(knowledge_root, "depmap-26q1-full", "true_love_gene")
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

order_path <- file.path(input_root, "gene_order.csv")
block_root <- file.path(input_root, "blocks")
stopifnot(file.exists(order_path), dir.exists(block_root))
order_dt <- fread(order_path)
genes <- toupper(order_dt$symbol)
blocks <- sort(list.files(block_root, pattern = "\\.rds$", full.names = TRUE))
stopifnot(length(blocks) > 0L)

best_rows <- vector("list", length(blocks))
for (block_i in seq_along(blocks)) {
  block <- readRDS(blocks[[block_i]])
  rows <- vector("list", nrow(block$correlation))
  for (row_i in seq_len(nrow(block$correlation))) {
    source_index <- block$source_start + row_i - 1L
    r <- as.numeric(block$correlation[row_i, ])
    n <- as.numeric(block$pair_n[row_i, ])
    valid <- is.finite(r) & is.finite(n) & n >= min_pair_n & seq_along(r) != source_index
    if (!any(valid)) next
    candidates <- which(valid)
    target_index <- candidates[[which.min(r[candidates])]]
    t_stat <- abs(r[valid]) * sqrt((n[valid] - 2) /
      pmax(1 - r[valid]^2, .Machine$double.eps))
    p_all <- 2 * pt(t_stat, df = n[valid] - 2, lower.tail = FALSE)
    target_pos <- match(target_index, which(valid))
    p_value <- p_all[[target_pos]]
    fdr <- p.adjust(p_all, method = "BH")[[target_pos]]
    rows[[row_i]] <- data.table(
      source_index = source_index,
      source_gene = genes[[source_index]],
      rank_1_target_index = target_index,
      rank_1_target_gene = genes[[target_index]],
      correlation = r[[target_index]],
      pair_n = as.integer(n[[target_index]]),
      p_value = p_value,
      fdr_within_source = fdr
    )
  }
  best_rows[[block_i]] <- rbindlist(rows, fill = TRUE)
  if (block_i %% 10L == 0L || block_i == length(blocks)) {
    message(sprintf("[%d/%d] processed source genes=%d", block_i, length(blocks),
      sum(vapply(best_rows[seq_len(block_i)], nrow, integer(1)))))
  }
}

directed <- rbindlist(best_rows, fill = TRUE)
setorder(directed, source_index)
fwrite(directed, file.path(output_root, "rank1_negative_partner_by_gene.csv.gz"),
       compress = "gzip")

lookup <- setNames(seq_len(nrow(directed)), directed$source_index)
reciprocal <- logical(nrow(directed))
for (i in seq_len(nrow(directed))) {
  reverse_i <- unname(lookup[as.character(directed$rank_1_target_index[[i]])])
  reciprocal[[i]] <- length(reverse_i) == 1L && !is.na(reverse_i) &&
    directed$rank_1_target_index[[reverse_i]] == directed$source_index[[i]]
}

forward <- directed[reciprocal == TRUE & source_index < rank_1_target_index]
reverse_rows <- directed[match(forward$rank_1_target_index, source_index)]
true_love <- data.table(
  gene_a = forward$source_gene,
  gene_b = forward$rank_1_target_gene,
  correlation_a_to_b = forward$correlation,
  correlation_b_to_a = reverse_rows$correlation,
  pair_n_a_to_b = forward$pair_n,
  pair_n_b_to_a = reverse_rows$pair_n,
  p_value_a_to_b = forward$p_value,
  p_value_b_to_a = reverse_rows$p_value,
  fdr_a_to_b = forward$fdr_within_source,
  fdr_b_to_a = reverse_rows$fdr_within_source
)
true_love[, both_directions_fdr_pass :=
  fdr_a_to_b <= fdr_max & fdr_b_to_a <= fdr_max]
true_love[, worst_direction_fdr := pmax(fdr_a_to_b, fdr_b_to_a)]
true_love[, strongest_absolute_correlation :=
  pmax(abs(correlation_a_to_b), abs(correlation_b_to_a))]
setorder(true_love, -both_directions_fdr_pass, worst_direction_fdr,
         -strongest_absolute_correlation, gene_a, gene_b)
true_love[, true_love_pair_id := sprintf("TLG-26Q1-%05d", .I)]
setcolorder(true_love, c("true_love_pair_id", setdiff(names(true_love), "true_love_pair_id")))
fwrite(true_love, file.path(output_root, "strict_mutual_rank1_pairs.csv.gz"),
       compress = "gzip")

examples <- data.table(
  example = c("ASB7::SUV39H1", "CCNF::E2F1"),
  gene_a = c("ASB7", "CCNF"), gene_b = c("SUV39H1", "E2F1")
)
examples[, present_in_26q1_strict_pairs := vapply(seq_len(.N), function(i) {
  any((true_love$gene_a == gene_a[[i]] & true_love$gene_b == gene_b[[i]]) |
      (true_love$gene_a == gene_b[[i]] & true_love$gene_b == gene_a[[i]]))
}, logical(1))]
fwrite(examples, file.path(output_root, "tm00_example_validation.csv"))

manifest <- list(
  schema_version = 1,
  release = "26Q1",
  family = "true_love_gene",
  status = "complete",
  source_gene_count = nrow(directed),
  strict_mutual_rank1_pair_count = nrow(true_love),
  both_directions_fdr_pair_count = true_love[both_directions_fdr_pass == TRUE, .N],
  fdr_max = fdr_max,
  min_pair_n = min_pair_n,
  ranking = "most negative Pearson Gene Effect correlation, excluding self",
  strict_definition = "A's rank-1 negative co-dependency partner is B and B's rank-1 negative co-dependency partner is A",
  multiple_testing = "BH FDR across all eligible target genes separately within each source gene; strict pairs retain both directional FDR values",
  interpretation = "reciprocal rank-1 co-dependency is a hypothesis-generating functional relationship, not proof of direct regulation or causality",
  tm00_reference = "17_bipolar_dependency_ASB7_as_example.R",
  inputs = c(order_path, blocks)
)
write_json(manifest, file.path(output_root, "manifest.json"), auto_unbox = TRUE,
           pretty = TRUE)
message("Completed strict True Love Gene build: ", output_root)
