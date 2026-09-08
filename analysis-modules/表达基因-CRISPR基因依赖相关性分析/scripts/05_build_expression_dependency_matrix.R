#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
})

parse_args <- function(args) {
  out <- list(project_root = ".", block_size = 128L, shard_index = 1L, shard_count = 1L, overwrite = FALSE)
  i <- 1L
  while (i <= length(args)) {
    if (args[[i]] == "--overwrite") {
      out$overwrite <- TRUE
      i <- i + 1L
      next
    }
    if (i == length(args)) stop("参数缺少值")
    key <- args[[i]]; value <- args[[i + 1L]]
    if (key == "--project-root") out$project_root <- value
    else if (key == "--block-size") out$block_size <- as.integer(value)
    else if (key == "--shard-index") out$shard_index <- as.integer(value)
    else if (key == "--shard-count") out$shard_count <- as.integer(value)
    else stop("未知参数: ", key)
    i <- i + 2L
  }
  out$project_root <- normalizePath(out$project_root, winslash = "/", mustWork = TRUE)
  if (out$shard_index < 1L || out$shard_index > out$shard_count) stop("分片参数错误")
  out
}

clean_gene <- function(x) sub(" \\([0-9]+\\)$", "", x)
atomic_save <- function(x, path) {
  tmp <- paste0(path, ".tmp")
  saveRDS(x, tmp, compress = FALSE)
  if (file.exists(path)) unlink(path)
  if (!file.rename(tmp, path)) stop("无法写入: ", path)
}

args <- parse_args(commandArgs(trailingOnly = TRUE))
data_root <- file.path(args$project_root, "data")
output_root <- file.path(args$project_root, "knowledge", "depmap-26q1-full", "expression_dependency")
block_root <- file.path(output_root, "blocks")
dir.create(block_root, recursive = TRUE, showWarnings = FALSE)

cat("读取并对齐表达与Gene Effect...\n")
expr <- fread(file.path(data_root, "OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv"), showProgress = TRUE)
expr_metadata <- c("V1", "SequencingID", "ModelConditionID", "ModelID", "IsDefaultEntryForMC", "IsDefaultEntryForModel")
expr <- expr[IsDefaultEntryForMC == "Yes"]
expr <- expr[!duplicated(ModelID)]
expr_gene_cols <- setdiff(names(expr), expr_metadata)
expr_genes <- clean_gene(expr_gene_cols)

effect <- fread(file.path(data_root, "CRISPRGeneEffect.csv"), showProgress = TRUE)
effect_ids <- as.character(effect[[1L]])
effect_gene_cols <- names(effect)[-1L]
effect_genes <- clean_gene(effect_gene_cols)

common_ids <- intersect(expr$ModelID, effect_ids)
expr_idx <- match(common_ids, expr$ModelID)
effect_idx <- match(common_ids, effect_ids)
expr_mat <- as.matrix(expr[expr_idx, ..expr_gene_cols])
effect_mat <- as.matrix(effect[effect_idx, ..effect_gene_cols])
storage.mode(expr_mat) <- "double"
storage.mode(effect_mat) <- "double"
colnames(expr_mat) <- expr_genes
colnames(effect_mat) <- effect_genes
rm(expr, effect)
invisible(gc())

fwrite(data.table(index = seq_along(expr_genes), symbol = expr_genes), file.path(output_root, "expression_gene_order.csv"), bom = TRUE)
fwrite(data.table(index = seq_along(effect_genes), symbol = effect_genes), file.path(output_root, "dependency_gene_order.csv"), bom = TRUE)
fwrite(data.table(index = seq_along(common_ids), ModelID = common_ids), file.path(output_root, "sample_order.csv"), bom = TRUE)

expr_available <- !is.na(expr_mat)
effect_available <- !is.na(effect_mat)
has_missing <- any(!expr_available) || any(!effect_available)
blocks <- split(seq_along(expr_genes), ceiling(seq_along(expr_genes) / args$block_size))
run_rows <- vector("list", length(blocks))

for (b in seq_along(blocks)) {
  if (((b - 1L) %% args$shard_count) + 1L != args$shard_index) next
  idx <- blocks[[b]]
  path <- file.path(block_root, sprintf("block_%05d_%05d.rds", min(idx), max(idx)))
  if (file.exists(path) && !args$overwrite) {
    cat(sprintf("[%d/%d] 已存在，跳过\n", b, length(blocks)))
    run_rows[[b]] <- list(start = min(idx), end = max(idx), status = "skipped_existing", path = path)
    next
  }
  cat(sprintf("[%d/%d] expression genes %d-%d\n", b, length(blocks), min(idx), max(idx)))
  correlations <- suppressWarnings(cor(expr_mat[, idx, drop = FALSE], effect_mat, use = "pairwise.complete.obs"))
  pair_n <- if (has_missing) {
    counts <- crossprod(expr_available[, idx, drop = FALSE], effect_available)
    storage.mode(counts) <- "integer"
    counts
  } else nrow(expr_mat)
  atomic_save(list(
    schema_version = 1L,
    source = "expression",
    target = "CRISPRGeneEffect",
    source_start = min(idx),
    source_end = max(idx),
    correlation = correlations,
    pair_n = pair_n
  ), path)
  run_rows[[b]] <- list(start = min(idx), end = max(idx), status = "completed", path = path)
}

manifest <- list(
  schema_version = 1L,
  release = "26Q1",
  tm00_references = c("04", "05"),
  generated_at = format(Sys.time(), "%Y-%m-%dT%H:%M:%S%z"),
  common_sample_count = length(common_ids),
  expression_gene_count = length(expr_genes),
  dependency_gene_count = length(effect_genes),
  block_size = args$block_size,
  block_count = length(blocks),
  shard_index = args$shard_index,
  shard_count = args$shard_count,
  method = "Pearson correlation; expression feature versus CRISPR Gene Effect; pairwise complete observations",
  interpretation = "negative correlation: higher expression associates with stronger (more negative) dependency",
  runs = run_rows
)
manifest_name <- if (args$shard_count == 1L) "manifest.json" else sprintf("manifest_shard_%02d_of_%02d.json", args$shard_index, args$shard_count)
writeLines(toJSON(manifest, auto_unbox = TRUE, pretty = TRUE, na = "null"), file.path(output_root, manifest_name), useBytes = TRUE)
cat("完成。\n")
