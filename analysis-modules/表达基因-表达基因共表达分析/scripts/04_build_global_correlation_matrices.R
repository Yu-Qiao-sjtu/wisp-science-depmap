#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
})

parse_args <- function(args) {
  out <- list(project_root = ".", dataset = "all", block_size = 128L, overwrite = FALSE, shard_index = 1L, shard_count = 1L)
  i <- 1L
  while (i <= length(args)) {
    if (args[[i]] == "--overwrite") {
      out$overwrite <- TRUE
      i <- i + 1L
      next
    }
    if (i == length(args)) stop("参数缺少值: ", args[[i]])
    key <- args[[i]]
    value <- args[[i + 1L]]
    if (key == "--project-root") out$project_root <- value
    else if (key == "--dataset") out$dataset <- tolower(value)
    else if (key == "--block-size") out$block_size <- as.integer(value)
    else if (key == "--shard-index") out$shard_index <- as.integer(value)
    else if (key == "--shard-count") out$shard_count <- as.integer(value)
    else stop("未知参数: ", key)
    i <- i + 2L
  }
  out$project_root <- normalizePath(out$project_root, winslash = "/", mustWork = TRUE)
  if (!out$dataset %in% c("all", "expression", "effect")) stop("--dataset 必须是 all/expression/effect")
  if (is.na(out$block_size) || out$block_size < 1L) stop("--block-size 必须是正整数")
  if (is.na(out$shard_count) || out$shard_count < 1L || is.na(out$shard_index) || out$shard_index < 1L || out$shard_index > out$shard_count) {
    stop("分片参数必须满足 1 <= shard-index <= shard-count")
  }
  out
}

clean_gene <- function(x) sub(" \\([0-9]+\\)$", "", x)

load_matrix <- function(project_root, dataset) {
  data_root <- file.path(project_root, "data")
  if (dataset == "effect") {
    path <- file.path(data_root, "CRISPRGeneEffect.csv")
    raw <- fread(path, showProgress = TRUE)
    ids <- as.character(raw[[1L]])
    genes <- clean_gene(names(raw)[-1L])
    mat <- as.matrix(raw[, names(raw)[-1L], with = FALSE])
    storage.mode(mat) <- "double"
    return(list(matrix = mat, genes = genes, sample_ids = ids, source = path, tm00 = "03"))
  }

  path <- file.path(data_root, "OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv")
  raw <- fread(path, showProgress = TRUE)
  metadata <- c("V1", "SequencingID", "ModelConditionID", "ModelID", "IsDefaultEntryForMC", "IsDefaultEntryForModel")
  if (!all(metadata %chin% names(raw))) stop("表达矩阵元数据字段不符合26Q1")
  raw <- raw[IsDefaultEntryForMC == "Yes"]
  raw <- raw[!duplicated(ModelID)]
  gene_cols <- setdiff(names(raw), metadata)
  ids <- as.character(raw$ModelID)
  genes <- clean_gene(gene_cols)
  mat <- as.matrix(raw[, ..gene_cols])
  storage.mode(mat) <- "double"
  list(matrix = mat, genes = genes, sample_ids = ids, source = path, tm00 = "02")
}

atomic_save_rds <- function(object, path) {
  tmp <- paste0(path, ".tmp")
  saveRDS(object, tmp, compress = FALSE)
  if (file.exists(path)) unlink(path)
  if (!file.rename(tmp, path)) stop("无法写入: ", path)
}

build_dataset <- function(project_root, dataset, block_size, overwrite, shard_index, shard_count) {
  cat("加载 ", dataset, " 矩阵...\n", sep = "")
  obj <- load_matrix(project_root, dataset)
  mat <- obj$matrix
  genes <- obj$genes
  if (anyDuplicated(genes)) stop(dataset, " 清理Entrez后出现重复基因名")
  colnames(mat) <- genes

  output_root <- file.path(project_root, "knowledge", "depmap-26q1-full", paste0(dataset, "_correlation"))
  block_root <- file.path(output_root, "blocks")
  dir.create(block_root, recursive = TRUE, showWarnings = FALSE)
  fwrite(data.table(index = seq_along(genes), symbol = genes), file.path(output_root, "gene_order.csv"), bom = TRUE)
  fwrite(data.table(index = seq_along(obj$sample_ids), sample_id = obj$sample_ids), file.path(output_root, "sample_order.csv"), bom = TRUE)

  blocks <- split(seq_along(genes), ceiling(seq_along(genes) / block_size))
  missing_total <- sum(is.na(mat))
  constant_genes <- which(vapply(seq_len(ncol(mat)), function(j) {
    x <- mat[, j]
    stats::sd(x, na.rm = TRUE) == 0 || sum(!is.na(x)) < 3L
  }, logical(1)))
  availability <- if (missing_total > 0L) !is.na(mat) else NULL
  run_rows <- vector("list", length(blocks))

  for (b in seq_along(blocks)) {
    if (((b - 1L) %% shard_count) + 1L != shard_index) next
    idx <- blocks[[b]]
    path <- file.path(block_root, sprintf("block_%05d_%05d.rds", min(idx), max(idx)))
    if (file.exists(path) && !overwrite) {
      cat(sprintf("[%s %d/%d] 已存在，跳过\n", dataset, b, length(blocks)))
      run_rows[[b]] <- list(start = min(idx), end = max(idx), status = "skipped_existing", path = path)
      next
    }
    cat(sprintf("[%s %d/%d] genes %d-%d\n", dataset, b, length(blocks), min(idx), max(idx)))
    correlation <- suppressWarnings(cor(mat[, idx, drop = FALSE], mat, use = "pairwise.complete.obs", method = "pearson"))
    pair_n <- if (is.null(availability)) {
      nrow(mat)
    } else {
      counts <- crossprod(availability[, idx, drop = FALSE], availability)
      storage.mode(counts) <- "integer"
      counts
    }
    atomic_save_rds(list(
      schema_version = 1L,
      dataset = dataset,
      source_start = min(idx),
      source_end = max(idx),
      correlation = correlation,
      pair_n = pair_n
    ), path)
    run_rows[[b]] <- list(start = min(idx), end = max(idx), status = "completed", path = path)
  }

  manifest <- list(
    schema_version = 1L,
    release = "26Q1",
    dataset = dataset,
    tm00_reference = obj$tm00,
    generated_at = format(Sys.time(), "%Y-%m-%dT%H:%M:%S%z"),
    source = obj$source,
    sample_count = nrow(mat),
    gene_count = ncol(mat),
    block_size = block_size,
    block_count = length(blocks),
    shard_index = shard_index,
    shard_count = shard_count,
    missing_value_count = missing_total,
    pair_n_storage = if (is.null(availability)) "scalar_per_block" else "complete-pair matrix per block",
    constant_or_insufficient_gene_count = length(constant_genes),
    constant_or_insufficient_gene_indices = constant_genes,
    method = "Pearson correlation, pairwise complete observations",
    outputs = list(gene_order = "gene_order.csv", sample_order = "sample_order.csv", blocks = "blocks/*.rds"),
    runs = run_rows
  )
  manifest_name <- if (shard_count == 1L) "manifest.json" else sprintf("manifest_shard_%02d_of_%02d.json", shard_index, shard_count)
  writeLines(toJSON(manifest, auto_unbox = TRUE, pretty = TRUE, na = "null"), file.path(output_root, manifest_name), useBytes = TRUE)
  rm(mat, availability)
  invisible(gc())
}

args <- parse_args(commandArgs(trailingOnly = TRUE))
datasets <- if (args$dataset == "all") c("expression", "effect") else args$dataset
for (dataset in datasets) build_dataset(args$project_root, dataset, args$block_size, args$overwrite, args$shard_index, args$shard_count)
cat("全部相关矩阵构建完成。\n")
