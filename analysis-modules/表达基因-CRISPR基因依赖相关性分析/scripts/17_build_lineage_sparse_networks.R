#!/usr/bin/env Rscript

# Build the missing lineage-specific forms of TM00 02/03/04/05 without
# materialising another dense lineage x gene x gene knowledge cube.
#
# Each source gene is still tested against every eligible target gene.  Only
# the strongest positive and negative associations are retained, together
# with n, p value, BH FDR and the rank within the complete tested row.  The
# output is resumable by source-gene block and is therefore suitable for a
# local smoke test followed by a sharded full run.

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
  library(arrow)
})

parse_args <- function(args) {
  z <- list(
    project_root = ".",
    output_root = NULL,
    families = c("effect_correlation", "expression_correlation", "expression_dependency"),
    lineages = "ALL",
    source_genes = "ALL",
    top_k = 100L,
    min_n = 10L,
    block_size = 64L,
    summary_name = "latest_run_summary.csv",
    overwrite = FALSE
  )
  i <- 1L
  while (i <= length(args)) {
    key <- args[[i]]
    if (key == "--overwrite") {
      z$overwrite <- TRUE
      i <- i + 1L
      next
    }
    if (i == length(args)) stop("missing value for ", key)
    value <- args[[i + 1L]]
    if (key == "--project-root") z$project_root <- value
    else if (key == "--output-root") z$output_root <- value
    else if (key == "--families") z$families <- trimws(strsplit(value, ",", fixed = TRUE)[[1L]])
    else if (key == "--lineages") z$lineages <- value
    else if (key == "--source-genes") z$source_genes <- value
    else if (key == "--top-k") z$top_k <- as.integer(value)
    else if (key == "--min-n") z$min_n <- as.integer(value)
    else if (key == "--block-size") z$block_size <- as.integer(value)
    else if (key == "--summary-name") z$summary_name <- value
    else stop("unknown argument: ", key)
    i <- i + 2L
  }
  z$project_root <- normalizePath(z$project_root, winslash = "/", mustWork = TRUE)
  if (is.null(z$output_root)) {
    z$output_root <- file.path(z$project_root, "knowledge", "depmap-26q1-full", "lineage_sparse_networks")
  }
  z$output_root <- normalizePath(z$output_root, winslash = "/", mustWork = FALSE)
  allowed <- c("effect_correlation", "expression_correlation", "expression_dependency")
  if (length(setdiff(z$families, allowed))) stop("unsupported families: ", paste(setdiff(z$families, allowed), collapse = ","))
  if (is.na(z$top_k) || z$top_k < 1L) stop("--top-k must be positive")
  if (is.na(z$min_n) || z$min_n < 3L) stop("--min-n must be at least 3")
  if (is.na(z$block_size) || z$block_size < 1L) stop("--block-size must be positive")
  if (!nzchar(z$summary_name) || basename(z$summary_name) != z$summary_name) {
    stop("--summary-name must be a plain file name")
  }
  z
}

clean_gene <- function(x) sub(" \\([0-9]+\\)$", "", x)
safe_name <- function(x) gsub("[^A-Za-z0-9]+", "_", x)

write_parquet_atomic <- function(x, path) {
  dir.create(dirname(path), recursive = TRUE, showWarnings = FALSE)
  tmp <- paste0(path, ".tmp")
  arrow::write_parquet(as.data.frame(x), tmp, compression = "zstd")
  if (file.exists(path)) unlink(path)
  if (!file.rename(tmp, path)) stop("cannot publish output: ", path)
}

write_json_atomic <- function(x, path) {
  dir.create(dirname(path), recursive = TRUE, showWarnings = FALSE)
  tmp <- paste0(path, ".tmp")
  writeLines(toJSON(x, auto_unbox = TRUE, pretty = TRUE, na = "null"), tmp, useBytes = TRUE)
  if (file.exists(path)) unlink(path)
  if (!file.rename(tmp, path)) stop("cannot publish manifest: ", path)
}

read_effect <- function(data_root) {
  x <- fread(file.path(data_root, "CRISPRGeneEffect.csv"), showProgress = TRUE)
  ids <- as.character(x[[1L]])
  cols <- names(x)[-1L]
  genes <- clean_gene(cols)
  mat <- as.matrix(x[, ..cols])
  storage.mode(mat) <- "double"
  rownames(mat) <- ids
  colnames(mat) <- genes
  list(mat = mat, ids = ids, genes = genes)
}

read_expression <- function(data_root) {
  x <- fread(file.path(data_root, "OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv"), showProgress = TRUE)
  metadata <- c("V1", "SequencingID", "ModelConditionID", "ModelID", "IsDefaultEntryForMC", "IsDefaultEntryForModel")
  x <- x[IsDefaultEntryForMC == "Yes" & !duplicated(ModelID)]
  ids <- as.character(x$ModelID)
  cols <- setdiff(names(x), metadata)
  genes <- clean_gene(cols)
  mat <- as.matrix(x[, ..cols])
  storage.mode(mat) <- "double"
  rownames(mat) <- ids
  colnames(mat) <- genes
  list(mat = mat, ids = ids, genes = genes)
}

pairwise_cor <- function(x, y, min_n) {
  mx <- !is.na(x)
  my <- !is.na(y)
  x0 <- x
  y0 <- y
  x0[!mx] <- 0
  y0[!my] <- 0
  storage.mode(mx) <- "double"
  storage.mode(my) <- "double"
  n <- crossprod(mx, my)
  sx <- crossprod(x0, my)
  sy <- crossprod(mx, y0)
  sxx <- crossprod(x0 * x0, my)
  syy <- crossprod(mx, y0 * y0)
  sxy <- crossprod(x0, y0)
  numerator <- sxy - sx * sy / n
  denominator <- sqrt((sxx - sx * sx / n) * (syy - sy * sy / n))
  r <- numerator / denominator
  r[n < min_n | !is.finite(r)] <- NA_real_
  list(r = r, n = n)
}

select_edges <- function(r, n, source_genes, target_genes, top_k, min_n, same_space) {
  rows <- vector("list", nrow(r))
  for (i in seq_len(nrow(r))) {
    rv <- as.numeric(r[i, ])
    nv <- as.integer(n[i, ])
    if (same_space) {
      self <- match(source_genes[[i]], target_genes)
      if (!is.na(self)) rv[[self]] <- NA_real_
    }
    valid <- which(is.finite(rv) & nv >= min_n)
    if (!length(valid)) next
    positive <- valid[order(-rv[valid], target_genes[valid])]
    positive <- positive[rv[positive] > 0]
    negative <- valid[order(rv[valid], target_genes[valid])]
    negative <- negative[rv[negative] < 0]
    positive <- head(positive, top_k)
    negative <- head(negative, top_k)
    keep <- unique(c(positive, negative))
    if (!length(keep)) next
    p <- 2 * pt(-abs(rv[valid] * sqrt((nv[valid] - 2) / pmax(1 - rv[valid]^2, .Machine$double.eps))), df = nv[valid] - 2)
    fdr <- p.adjust(p, method = "BH")
    stat <- data.table(target_index = valid, p_value = p, fdr = fdr)
    z <- data.table(
      source_gene = source_genes[[i]],
      target_gene = target_genes[keep],
      target_index = keep,
      correlation = rv[keep],
      pair_n = nv[keep],
      rank_absolute = match(keep, valid[order(-abs(rv[valid]), target_genes[valid])]),
      rank_positive = match(keep, positive),
      rank_negative = match(keep, negative)
    )
    z <- stat[z, on = "target_index"]
    z[, target_index := NULL]
    rows[[i]] <- z
  }
  ans <- rbindlist(rows, fill = TRUE)
  if (!ncol(ans)) {
    ans <- data.table(
      source_gene = character(),
      target_gene = character(),
      correlation = double(),
      pair_n = integer(),
      p_value = double(),
      fdr = double(),
      rank_absolute = integer(),
      rank_positive = integer(),
      rank_negative = integer()
    )
  }
  ans
}

finalize_reciprocal <- function(root, top_k) {
  paths <- list.files(file.path(root, "blocks"), pattern = "[.]parquet$", full.names = TRUE)
  if (!length(paths)) return(NULL)
  edges <- rbindlist(lapply(paths, function(path) as.data.table(arrow::read_parquet(path))))
  edges[, direction := fifelse(correlation > 0, "positive", "negative")]
  edges[, reverse_source := target_gene]
  edges[, reverse_target := source_gene]
  reverse <- edges[, .(reverse_source = source_gene, reverse_target = target_gene,
                       reverse_correlation = correlation,
                       reverse_rank_positive = rank_positive,
                       reverse_rank_negative = rank_negative)]
  pairs <- merge(edges, reverse, by = c("reverse_source", "reverse_target"), allow.cartesian = FALSE)
  pairs <- pairs[source_gene < target_gene & sign(correlation) == sign(reverse_correlation)]
  pairs[, reciprocal_rank_max := pmax(
    fifelse(direction == "positive", rank_positive, rank_negative),
    fifelse(direction == "positive", reverse_rank_positive, reverse_rank_negative),
    na.rm = TRUE
  )]
  pairs <- pairs[reciprocal_rank_max <= top_k]
  pairs[, reciprocal_score := sign(correlation) * sqrt(abs(correlation * reverse_correlation)) / reciprocal_rank_max]
  pairs[, reciprocal_score_abs := abs(reciprocal_score)]
  setorder(pairs, reciprocal_rank_max, -reciprocal_score_abs, source_gene, target_gene)
  pairs[, reciprocal_score_abs := NULL]
  write_parquet_atomic(pairs, file.path(root, "reciprocal_pairs.parquet"))
  nrow(pairs)
}

a <- parse_args(commandArgs(trailingOnly = TRUE))
data_root <- file.path(a$project_root, "data")
dir.create(a$output_root, recursive = TRUE, showWarnings = FALSE)

model <- fread(file.path(data_root, "Model.csv"), select = c("ModelID", "OncotreeLineage"))
lineage_map <- setNames(trimws(as.character(model$OncotreeLineage)), as.character(model$ModelID))
effect <- NULL
expression <- NULL
get_effect <- function() {
  if (is.null(effect)) effect <<- read_effect(data_root)
  effect
}
get_expression <- function() {
  if (is.null(expression)) expression <<- read_expression(data_root)
  expression
}

available_lineages <- sort(unique(lineage_map[!is.na(lineage_map) & nzchar(lineage_map)]))
requested_lineages <- if (toupper(a$lineages) == "ALL") available_lineages else trimws(strsplit(a$lineages, ",", fixed = TRUE)[[1L]])
unknown_lineages <- setdiff(requested_lineages, available_lineages)
if (length(unknown_lineages)) stop("unknown lineages: ", paste(unknown_lineages, collapse = ", "))
requested_source_genes <- if (toupper(a$source_genes) == "ALL") NULL else unique(trimws(strsplit(a$source_genes, ",", fixed = TRUE)[[1L]]))

run_rows <- list()
for (family in a$families) {
  if (family == "effect_correlation") {
    obj <- get_effect()
    source_obj <- obj
    target_obj <- obj
    same_space <- TRUE
    tm00 <- "03/17"
  } else if (family == "expression_correlation") {
    obj <- get_expression()
    source_obj <- obj
    target_obj <- obj
    same_space <- TRUE
    tm00 <- "02"
  } else {
    source_obj <- get_expression()
    target_obj <- get_effect()
    same_space <- FALSE
    tm00 <- "04/05"
  }

  common_models <- intersect(source_obj$ids, target_obj$ids)
  for (lineage in requested_lineages) {
    lineage_ids <- names(lineage_map)[!is.na(lineage_map) & lineage_map == lineage]
    ids <- intersect(common_models, lineage_ids)
    if (length(ids) < a$min_n) {
      run_rows[[length(run_rows) + 1L]] <- data.table(family = family, lineage = lineage, sample_n = length(ids), status = "ineligible_sample_n")
      next
    }
    x <- source_obj$mat[match(ids, source_obj$ids), , drop = FALSE]
    y <- target_obj$mat[match(ids, target_obj$ids), , drop = FALSE]
    source_genes <- colnames(x)
    if (!is.null(requested_source_genes)) {
      source_genes <- intersect(requested_source_genes, source_genes)
      if (!length(source_genes)) stop("none of --source-genes are eligible for ", family)
    }
    target_genes <- colnames(y)
    root <- file.path(a$output_root, family, safe_name(lineage))
    dir.create(file.path(root, "blocks"), recursive = TRUE, showWarnings = FALSE)
    fwrite(data.table(source_index = seq_along(source_genes), symbol = source_genes), file.path(root, "source_gene_order.csv"), bom = TRUE)
    fwrite(data.table(target_index = seq_along(target_genes), symbol = target_genes), file.path(root, "target_gene_order.csv"), bom = TRUE)
    blocks <- split(seq_along(source_genes), ceiling(seq_along(source_genes) / a$block_size))
    for (b in seq_along(blocks)) {
      idx <- blocks[[b]]
      path <- file.path(root, "blocks", sprintf("block_%05d_%05d.parquet", min(idx), max(idx)))
      if (file.exists(path) && !a$overwrite) {
        cat("skip existing ", path, "\n", sep = "")
        next
      }
      cat(sprintf("%s | %s | block %d/%d | %d models | %d sources x %d targets\n",
                  family, lineage, b, length(blocks), length(ids), length(idx), length(target_genes)))
      stats <- pairwise_cor(x[, match(source_genes[idx], colnames(x)), drop = FALSE], y, a$min_n)
      edges <- select_edges(stats$r, stats$n, source_genes[idx], target_genes, a$top_k, a$min_n, same_space)
      edges[, `:=`(family = family, lineage = lineage, lineage_sample_n = length(ids), tm00_reference = tm00)]
      setcolorder(edges, c("family", "lineage", "source_gene", "target_gene", "correlation", "pair_n", "p_value", "fdr",
                           "rank_absolute", "rank_positive", "rank_negative", "lineage_sample_n", "tm00_reference"))
      write_parquet_atomic(edges, path)
    }
    complete <- is.null(requested_source_genes) && length(list.files(file.path(root, "blocks"), pattern = "[.]parquet$")) == length(blocks)
    reciprocal_n <- NULL
    if (complete && same_space) reciprocal_n <- finalize_reciprocal(root, a$top_k)
    manifest <- list(
      schema_version = 1L,
      release = "26Q1",
      generated_at = format(Sys.time(), "%Y-%m-%dT%H:%M:%S%z"),
      family = family,
      tm00_reference = tm00,
      lineage = lineage,
      lineage_sample_n = length(ids),
      min_n = a$min_n,
      top_k_each_direction = a$top_k,
      source_gene_count = length(source_genes),
      target_gene_count = length(target_genes),
      source_scope = if (is.null(requested_source_genes)) "ALL" else requested_source_genes,
      storage = "Sparse top positive and negative edges after testing every target in each source row",
      status = if (complete) "complete" else "partial_test",
      reciprocal_pair_count = reciprocal_n,
      block_count = length(blocks)
    )
    write_json_atomic(manifest, file.path(root, "manifest.json"))
    run_rows[[length(run_rows) + 1L]] <- data.table(
      family = family, lineage = lineage, sample_n = length(ids), source_gene_n = length(source_genes),
      target_gene_n = length(target_genes), status = manifest$status,
      reciprocal_pair_n = if (is.null(reciprocal_n)) NA_integer_ else reciprocal_n
    )
    rm(x, y)
    invisible(gc())
  }
}

summary <- rbindlist(run_rows, fill = TRUE)
fwrite(summary, file.path(a$output_root, a$summary_name), bom = TRUE)
cat("completed\n")
