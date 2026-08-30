#!/usr/bin/env Rscript

# Real-data smoke test for the narrow-column execution path used by generated
# DepMap R analyses. Outputs are test artifacts, not biological conclusions.

args <- commandArgs(trailingOnly = TRUE)
arg_value <- function(name, default = NULL) {
  index <- match(name, args)
  if (is.na(index) || index == length(args)) return(default)
  args[[index + 1L]]
}

if (!requireNamespace("data.table", quietly = TRUE)) stop("data.table is required")
if (!requireNamespace("jsonlite", quietly = TRUE)) stop("jsonlite is required")

project_root <- normalizePath(arg_value("--project-root", "."), winslash = "/", mustWork = TRUE)
output_dir <- arg_value("--output-dir")
if (is.null(output_dir)) stop("--output-dir is required")
gene_a <- arg_value("--gene-a", "ESR1")
gene_b <- arg_value("--gene-b", "FOXA1")
input_path <- file.path(project_root, "data", "CRISPRGeneEffect.csv")
if (!file.exists(input_path)) stop("CRISPRGeneEffect.csv is missing")

header <- names(data.table::fread(input_path, nrows = 0L, showProgress = FALSE))
symbols <- sub("\\s+\\(\\d+\\)$", "", header)
column_for <- function(symbol) {
  hits <- which(symbols == symbol)
  if (length(hits) != 1L) stop(sprintf("Expected one column for %s, found %d", symbol, length(hits)))
  header[[hits]]
}
column_a <- column_for(gene_a)
column_b <- column_for(gene_b)

values <- data.table::fread(
  input_path,
  select = c(header[[1L]], column_a, column_b),
  showProgress = FALSE,
  data.table = FALSE
)
names(values) <- c("ModelID", gene_a, gene_b)
complete <- stats::complete.cases(values[[gene_a]], values[[gene_b]])
tested <- values[complete, , drop = FALSE]
if (nrow(tested) < 3L) stop("Too few complete models for the smoke correlation")
correlation <- stats::cor.test(tested[[gene_a]], tested[[gene_b]], method = "pearson")

dir.create(output_dir, recursive = TRUE, showWarnings = FALSE)
result_path <- file.path(output_dir, "result.json")
figure_path <- file.path(output_dir, "gene-effect-correlation.png")
jsonlite::write_json(list(
  schema_version = 1L,
  status = "ok",
  purpose = "execution smoke test only",
  release = "26Q1",
  input = "data/CRISPRGeneEffect.csv",
  selected_columns = c(header[[1L]], column_a, column_b),
  rows_read = nrow(values),
  complete_models = nrow(tested),
  method = "Pearson correlation",
  estimate = unname(correlation$estimate),
  p_value = correlation$p.value
), result_path, pretty = TRUE, auto_unbox = TRUE)

grDevices::png(figure_path, width = 1200L, height = 900L, res = 144L)
graphics::plot(
  tested[[gene_a]], tested[[gene_b]], pch = 16L,
  col = grDevices::adjustcolor("#2C7FB8", alpha.f = 0.55),
  xlab = sprintf("%s Gene Effect", gene_a),
  ylab = sprintf("%s Gene Effect", gene_b),
  main = "DepMap 26Q1 narrow-column R smoke test"
)
graphics::abline(stats::lm(tested[[gene_b]] ~ tested[[gene_a]]), col = "#D95F0E", lwd = 2L)
grDevices::dev.off()

cat(normalizePath(result_path, winslash = "/", mustWork = TRUE), "\n")
cat(normalizePath(figure_path, winslash = "/", mustWork = TRUE), "\n")
