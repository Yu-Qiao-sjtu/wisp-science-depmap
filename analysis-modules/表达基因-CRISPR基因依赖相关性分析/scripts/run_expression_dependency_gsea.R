#!/usr/bin/env Rscript

# On-demand pre-ranked GSEA for one expression source gene. The expensive
# expression-versus-Gene-Effect correlations are reused from the complete
# 26Q1 block matrix; this script reads one source row, runs GSEA, and caches a
# reviewable result package.

suppressPackageStartupMessages({
  library(arrow)
  library(data.table)
  library(fgsea)
  library(ggplot2)
  library(jsonlite)
})

parse_args <- function(args) {
  z <- list(
    matrix_root = NULL,
    source_gene = NULL,
    collection = "hallmark",
    gmt = NULL,
    gene_set_id = NULL,
    cache_root = NULL,
    min_pair_fraction = 0.8,
    min_pair_n = NA_integer_,
    rank_metric = "negative_signed_t",
    min_size = 15L,
    max_size = 500L,
    top_n = 20L,
    seed = 20260908L,
    force = FALSE
  )
  i <- 1L
  while (i <= length(args)) {
    key <- args[[i]]
    if (key == "--force") {
      z$force <- TRUE
      i <- i + 1L
      next
    }
    if (i == length(args)) stop("missing value for ", key)
    value <- args[[i + 1L]]
    if (key == "--matrix-root") z$matrix_root <- value
    else if (key == "--source-gene") z$source_gene <- value
    else if (key == "--collection") z$collection <- tolower(value)
    else if (key == "--gmt") z$gmt <- value
    else if (key == "--gene-set-id") z$gene_set_id <- value
    else if (key == "--cache-root") z$cache_root <- value
    else if (key == "--min-pair-fraction") z$min_pair_fraction <- as.numeric(value)
    else if (key == "--min-pair-n") z$min_pair_n <- as.integer(value)
    else if (key == "--rank-metric") z$rank_metric <- value
    else if (key == "--min-size") z$min_size <- as.integer(value)
    else if (key == "--max-size") z$max_size <- as.integer(value)
    else if (key == "--top-n") z$top_n <- as.integer(value)
    else if (key == "--seed") z$seed <- as.integer(value)
    else stop("unknown argument: ", key)
    i <- i + 2L
  }
  if (is.null(z$matrix_root) || is.null(z$source_gene)) {
    stop(paste(
      "usage: Rscript run_expression_dependency_gsea.R",
      "--matrix-root PATH --source-gene GENE",
      "[--collection hallmark|reactome|custom] [--gmt FILE --gene-set-id ID]",
      "[--cache-root PATH] [--min-pair-fraction 0.8] [--min-pair-n N]",
      "[--rank-metric negative_signed_t|negative_correlation] [--force]"
    ))
  }
  z$matrix_root <- normalizePath(z$matrix_root, winslash = "/", mustWork = TRUE)
  if (is.null(z$cache_root)) {
    z$cache_root <- file.path(dirname(z$matrix_root), "expression_dependency_gsea_cache")
  }
  z$cache_root <- normalizePath(z$cache_root, winslash = "/", mustWork = FALSE)
  if (!is.null(z$gmt)) z$collection <- "custom"
  if (!z$collection %in% c("hallmark", "reactome", "custom")) {
    stop("--collection must be hallmark, reactome, or custom")
  }
  if (!is.null(z$gmt)) z$gmt <- normalizePath(z$gmt, winslash = "/", mustWork = TRUE)
  if (z$collection == "custom" && is.null(z$gmt)) stop("custom collection requires --gmt")
  if (!is.null(z$gmt) && is.null(z$gene_set_id)) {
    stop("--gene-set-id is required with --gmt so cache provenance is stable")
  }
  if (!z$rank_metric %in% c("negative_signed_t", "negative_correlation")) {
    stop("unsupported --rank-metric: ", z$rank_metric)
  }
  if (!is.finite(z$min_pair_fraction) || z$min_pair_fraction <= 0 || z$min_pair_fraction > 1) {
    stop("--min-pair-fraction must be in (0, 1]")
  }
  if (!is.na(z$min_pair_n) && z$min_pair_n < 3L) stop("--min-pair-n must be at least 3")
  if (z$min_size < 2L || z$max_size < z$min_size) stop("invalid gene-set size limits")
  if (z$top_n < 1L) stop("--top-n must be positive")
  z
}

safe_name <- function(x) {
  ans <- gsub("(^_+|_+$)", "", gsub("[^A-Za-z0-9._-]+", "_", x))
  if (!nzchar(ans)) stop("cannot create a safe name from: ", x)
  ans
}

atomic_json <- function(x, path) {
  dir.create(dirname(path), recursive = TRUE, showWarnings = FALSE)
  tmp <- paste0(path, ".tmp")
  write_json(x, tmp, pretty = TRUE, auto_unbox = TRUE, na = "null", digits = NA)
  if (file.exists(path)) unlink(path)
  if (!file.rename(tmp, path)) stop("cannot publish ", path)
}

atomic_parquet <- function(x, path) {
  dir.create(dirname(path), recursive = TRUE, showWarnings = FALSE)
  tmp <- paste0(path, ".tmp")
  write_parquet(as.data.frame(x), tmp, compression = "zstd")
  if (file.exists(path)) unlink(path)
  if (!file.rename(tmp, path)) stop("cannot publish ", path)
}

read_gmt <- function(path) {
  lines <- readLines(path, warn = FALSE)
  rows <- lapply(lines, function(line) {
    fields <- strsplit(line, "\t", fixed = TRUE)[[1L]]
    if (length(fields) < 3L) return(NULL)
    data.table(term = fields[[1L]], gene = fields[-c(1L, 2L)])
  })
  unique(rbindlist(rows, use.names = TRUE, fill = TRUE))
}

load_gene_sets <- function(a) {
  if (!is.null(a$gmt)) {
    genes <- read_gmt(a$gmt)
    return(list(
      genes = genes,
      database = a$gene_set_id,
      source = a$gmt,
      package_version = NA_character_
    ))
  }
  if (!requireNamespace("msigdbr", quietly = TRUE)) {
    stop("msigdbr is required when --gmt is not supplied")
  }
  if (a$collection == "hallmark") {
    x <- msigdbr::msigdbr(species = "Homo sapiens", collection = "H")
  } else {
    x <- msigdbr::msigdbr(
      species = "Homo sapiens",
      collection = "C2",
      subcollection = "CP:REACTOME"
    )
  }
  versions <- unique(as.character(x$db_version))
  if (length(versions) != 1L) stop("MSigDB version is missing or ambiguous")
  list(
    genes = unique(as.data.table(x)[, .(term = gs_name, gene = gene_symbol)]),
    database = paste0("MSigDB-", versions[[1L]]),
    source = "msigdbr packaged data",
    package_version = as.character(packageVersion("msigdbr"))
  )
}

top_records <- function(x, positive, n) {
  z <- if (positive) x[NES > 0] else x[NES < 0]
  if (!nrow(z)) return(list())
  z <- z[order(fdr_within_source, -abs(NES), pathway)]
  z <- head(z, n)
  lapply(seq_len(nrow(z)), function(i) {
    list(
      pathway = z$pathway[[i]],
      NES = z$NES[[i]],
      p_value = z$p_value[[i]],
      fdr = z$fdr_within_source[[i]],
      pathway_size = z$pathway_size[[i]],
      leading_edge = strsplit(z$leading_edge[[i]], ";", fixed = TRUE)[[1L]]
    )
  })
}

a <- parse_args(commandArgs(trailingOnly = TRUE))
matrix_manifest_path <- file.path(a$matrix_root, "manifest.json")
source_order_path <- file.path(a$matrix_root, "expression_gene_order.csv")
target_order_path <- file.path(a$matrix_root, "dependency_gene_order.csv")
for (path in c(matrix_manifest_path, source_order_path, target_order_path, file.path(a$matrix_root, "blocks"))) {
  if (!file.exists(path)) stop("required matrix artifact is missing: ", path)
}

matrix_manifest <- read_json(matrix_manifest_path, simplifyVector = TRUE)
source_order <- fread(source_order_path)
target_order <- fread(target_order_path)
source_hit <- source_order[toupper(symbol) == toupper(a$source_gene)]
if (nrow(source_hit) != 1L) stop("expression source gene not found or duplicated: ", a$source_gene)
source_gene <- as.character(source_hit$symbol[[1L]])
source_index <- as.integer(source_hit$index[[1L]])

block_paths <- list.files(
  file.path(a$matrix_root, "blocks"),
  pattern = "^block_[0-9]+_[0-9]+[.]rds$",
  full.names = TRUE
)
block_ranges <- t(vapply(basename(block_paths), function(x) {
  stem <- sub("[.]rds$", "", sub("^block_", "", x))
  as.integer(strsplit(stem, "_", fixed = TRUE)[[1L]])
}, integer(2L)))
block_index <- which(source_index >= block_ranges[, 1L] & source_index <= block_ranges[, 2L])
if (length(block_index) != 1L) stop("cannot resolve source block for index ", source_index)
source_block_path <- block_paths[[block_index]]
block <- readRDS(source_block_path)
row_index <- source_index - block$source_start + 1L
r <- as.numeric(block$correlation[row_index, ])
pair_n <- if (length(block$pair_n) == 1L) {
  rep(as.integer(block$pair_n), length(r))
} else {
  as.integer(block$pair_n[row_index, ])
}
if (length(r) != nrow(target_order) || length(pair_n) != nrow(target_order)) {
  stop("matrix row length does not match dependency_gene_order.csv")
}

global_sample_n <- as.integer(matrix_manifest$common_sample_count)
min_pair_n <- if (is.na(a$min_pair_n)) {
  as.integer(ceiling(a$min_pair_fraction * global_sample_n))
} else {
  a$min_pair_n
}
ranked <- data.table(
  target_gene = as.character(target_order$symbol),
  correlation = r,
  pair_n = pair_n
)
ranked <- ranked[is.finite(correlation) & pair_n >= min_pair_n]
if (a$rank_metric == "negative_signed_t") {
  bounded_r <- pmax(pmin(ranked$correlation, 1 - 1e-12), -1 + 1e-12)
  ranked[, rank_score := -bounded_r * sqrt((pair_n - 2) / pmax(1 - bounded_r^2, .Machine$double.eps))]
} else {
  ranked[, rank_score := -correlation]
}
ranked <- ranked[is.finite(rank_score)]
ranked[, abs_rank_score := abs(rank_score)]
setorder(ranked, -abs_rank_score, target_gene)
ranked <- ranked[!duplicated(target_gene)]
ranked[, abs_rank_score := NULL]
setorder(ranked, -rank_score, target_gene)
if (nrow(ranked) < a$max_size) {
  stop("too few ranked dependency genes after pair_n filtering: ", nrow(ranked))
}

gene_sets <- load_gene_sets(a)
membership <- unique(gene_sets$genes[gene %in% ranked$target_gene])
pathways <- split(membership$gene, membership$term)
pathways <- lapply(pathways, unique)
pathways <- pathways[lengths(pathways) >= a$min_size & lengths(pathways) <= a$max_size]
if (!length(pathways)) stop("no eligible pathways overlap the ranked dependency genes")

database_key <- safe_name(gene_sets$database)
release_key <- safe_name(as.character(matrix_manifest$release))
parameter_key <- paste0(
  a$rank_metric,
  "__minpair-", sprintf("%04d", min_pair_n),
  "__size-", a$min_size, "-", a$max_size,
  "__top-", a$top_n,
  "__seed-", a$seed
)
cache_dir <- file.path(
  a$cache_root,
  release_key,
  "global",
  safe_name(a$collection),
  database_key,
  sprintf("%05d_%s", source_index, safe_name(source_gene)),
  parameter_key
)
result_json_path <- file.path(cache_dir, "result.json")
if (file.exists(result_json_path) && !a$force) {
  cached_result <- read_json(result_json_path, simplifyVector = FALSE)
  cached_result$cache$hit <- TRUE
  cat(toJSON(cached_result, pretty = TRUE, auto_unbox = TRUE, na = "null", digits = NA), "\n")
  quit(status = 0L)
}
dir.create(cache_dir, recursive = TRUE, showWarnings = FALSE)

stats <- setNames(ranked$rank_score, ranked$target_gene)
set.seed(a$seed)
gsea <- suppressWarnings(fgseaMultilevel(
  pathways = pathways,
  stats = stats,
  minSize = a$min_size,
  maxSize = a$max_size,
  eps = 0,
  scoreType = "std"
))
gsea <- as.data.table(gsea)
if (!nrow(gsea)) stop("GSEA returned no pathways")
gsea[, leading_edge := vapply(leadingEdge, paste, collapse = ";", FUN.VALUE = character(1L))]
gsea[, leadingEdge := NULL]
setnames(gsea, c("pathway", "pval", "padj", "size"),
         c("pathway", "p_value", "fdr_within_source", "pathway_size"))
gsea[, `:=`(
  source_gene = source_gene,
  scope = "global",
  gene_set_database = gene_sets$database,
  collection = toupper(a$collection),
  rank_metric = a$rank_metric,
  ranked_gene_count = nrow(ranked),
  min_pair_n = min_pair_n,
  direction = fifelse(
    NES > 0,
    "higher source expression associates with stronger pathway dependency",
    "higher source expression associates with weaker pathway dependency"
  )
)]
setcolorder(gsea, c(
  "source_gene", "scope", "gene_set_database", "collection", "pathway",
  "NES", "p_value", "fdr_within_source", "pathway_size", "leading_edge",
  "direction", "rank_metric", "ranked_gene_count", "min_pair_n", "ES"
))
gsea[, abs_NES := abs(NES)]
setorder(gsea, fdr_within_source, -abs_NES, pathway)
gsea[, abs_NES := NULL]

table_path <- file.path(cache_dir, "enrichment.parquet")
ranked_path <- file.path(cache_dir, "ranked_dependency_targets.parquet")
figure_path <- file.path(cache_dir, "top_pathways.pdf")
atomic_parquet(gsea, table_path)
atomic_parquet(ranked, ranked_path)

plot_rows <- rbind(
  head(gsea[NES > 0][order(fdr_within_source, -NES)], ceiling(a$top_n / 2)),
  head(gsea[NES < 0][order(fdr_within_source, NES)], floor(a$top_n / 2))
)
if (nrow(plot_rows)) {
  plot_rows[, label := factor(pathway, levels = rev(pathway[order(NES)]))]
  p <- ggplot(plot_rows, aes(x = NES, y = label, color = -log10(pmax(fdr_within_source, .Machine$double.xmin)))) +
    geom_point(size = 3) +
    geom_vline(xintercept = 0, color = "grey70") +
    scale_color_viridis_c(option = "C", name = "-log10(FDR)") +
    labs(
      title = paste(source_gene, toupper(a$collection), "dependency GSEA"),
      subtitle = paste0("rank=", a$rank_metric, "; pair_n >= ", min_pair_n),
      x = "Normalized enrichment score (NES)",
      y = NULL
    ) +
    theme_bw(base_size = 10)
  ggsave(figure_path, p, width = 10, height = max(5, 0.28 * nrow(plot_rows) + 2))
}

created_at <- format(Sys.time(), "%Y-%m-%dT%H:%M:%S%z")
software <- list(
  R = R.version.string,
  arrow = as.character(packageVersion("arrow")),
  data_table = as.character(packageVersion("data.table")),
  fgsea = as.character(packageVersion("fgsea")),
  ggplot2 = as.character(packageVersion("ggplot2")),
  jsonlite = as.character(packageVersion("jsonlite")),
  msigdbr = gene_sets$package_version
)
run_manifest <- list(
  schema_version = 1L,
  analysis_id = paste("expression-dependency-gsea", source_gene, a$collection, sep = ":"),
  created_at = created_at,
  dataset_release = matrix_manifest$release,
  language = "R",
  entrypoint = "analysis-modules/表达基因-CRISPR基因依赖相关性分析/scripts/run_expression_dependency_gsea.R",
  invocation = paste(shQuote(c("Rscript", commandArgs(trailingOnly = FALSE))), collapse = " "),
  reference_scripts = c("05_from_gene_to_dependency.R", "05_build_expression_dependency_matrix.R"),
  inputs = list(
    list(path = matrix_manifest_path, bytes = file.info(matrix_manifest_path)$size, role = "matrix_manifest"),
    list(path = source_block_path, bytes = file.info(source_block_path)$size, role = "expression_dependency_block"),
    list(path = gene_sets$source, bytes = if (file.exists(gene_sets$source)) file.info(gene_sets$source)$size else NA_real_, role = "gene_sets")
  ),
  parameters = list(
    source_gene = source_gene,
    scope = "global",
    collection = a$collection,
    gene_set_database = gene_sets$database,
    min_pair_fraction = a$min_pair_fraction,
    min_pair_n = min_pair_n,
    rank_metric = a$rank_metric,
    min_size = a$min_size,
    max_size = a$max_size,
    seed = a$seed
  ),
  software = software,
  outputs = c(table_path, ranked_path, figure_path, result_json_path, file.path(cache_dir, "qc.json"))
)

result <- list(
  schema_version = 1L,
  status = "ok",
  question = paste0(source_gene, " expression-associated CRISPR dependency pathway enrichment"),
  targets = list(list(source_gene = source_gene, measurement = "expression")),
  cohort = list(
    requested = "global",
    n_before = global_sample_n,
    pair_n_threshold = min_pair_n,
    ranked_dependency_gene_count = nrow(ranked)
  ),
  methods = c("Pearson expression-to-Gene-Effect correlation", a$rank_metric, "fgseaMultilevel", "BH FDR within source gene"),
  observations = list(
    positive_NES = top_records(gsea, TRUE, min(10L, a$top_n)),
    negative_NES = top_records(gsea, FALSE, min(10L, a$top_n))
  ),
  tables = c(table_path, ranked_path),
  figures = if (file.exists(figure_path)) figure_path else character(),
  warnings = c(
    "Association and enrichment are observational and do not establish causality or synthetic lethality.",
    "Positive NES means higher source expression associates with stronger dependency because Gene Effect is more negative."
  ),
  cache = list(hit = FALSE, directory = cache_dir)
)

qc <- list(
  schema_version = 1L,
  status = "pass",
  checks = list(
    list(name = "release_identified", status = "pass", detail = matrix_manifest$release),
    list(name = "source_gene_unique", status = "pass", detail = paste0(source_gene, " index=", source_index)),
    list(name = "pair_n_filter", status = "pass", detail = paste0(nrow(ranked), " targets with pair_n >= ", min_pair_n)),
    list(name = "gene_set_overlap", status = "pass", detail = paste0(length(pathways), " eligible pathways")),
    list(name = "effect_direction_declared", status = "pass", detail = "more negative Gene Effect means stronger dependency; rank sign is inverted"),
    list(name = "multiple_testing", status = "pass", detail = "fgsea BH FDR within the requested source gene and collection")
  ),
  blocking_failures = list(),
  warnings = list()
)

atomic_json(run_manifest, file.path(cache_dir, "run_manifest.json"))
atomic_json(qc, file.path(cache_dir, "qc.json"))
atomic_json(result, result_json_path)
cat(toJSON(result, pretty = TRUE, auto_unbox = TRUE, na = "null", digits = NA), "\n")
