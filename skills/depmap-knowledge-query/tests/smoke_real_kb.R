#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
})

args <- commandArgs(trailingOnly = TRUE)
arg_value <- function(name) {
  index <- match(name, args)
  if (is.na(index) || index == length(args)) stop("missing ", name)
  args[[index + 1L]]
}
kb_root <- normalizePath(arg_value("--kb-root"), winslash = "/", mustWork = TRUE)

full_args <- commandArgs(trailingOnly = FALSE)
file_arg <- grep("^--file=", full_args, value = TRUE)
this_file <- normalizePath(sub("^--file=", "", file_arg[[1L]]), winslash = "/", mustWork = TRUE)
query_helper <- normalizePath(
  file.path(dirname(this_file), "..", "scripts", "query_depmap_kb.R"),
  winslash = "/",
  mustWork = TRUE
)

query <- function(...) {
  command_args <- c(query_helper, "--kb-root", kb_root, ...)
  output <- system2("Rscript", command_args, stdout = TRUE, stderr = FALSE)
  fromJSON(paste(output, collapse = "\n"), simplifyVector = FALSE)
}

catalog <- query("--mode", "catalog")
stopifnot(identical(catalog$qa$qa_status, "PASS"))
stopifnot(length(catalog$modules) >= 26L)

core <- query("--mode", "core", "--gene", "KRAS")
stopifnot(identical(core$gene, "KRAS"))
stopifnot(core$summary[[1L]]$effect_n > 1000L)
stopifnot(length(core$lineages) >= 20L)

pair <- query(
  "--mode", "pair", "--module", "effect_correlation",
  "--source", "KRAS", "--target", "RAF1"
)
stopifnot(identical(pair$result[[1L]]$target, "RAF1"))
stopifnot(is.numeric(pair$result[[1L]]$value))
stopifnot(pair$result[[1L]]$n > 1000L)

top <- query(
  "--mode", "top", "--module", "effect_correlation",
  "--source", "KRAS", "--limit", "5"
)
stopifnot(length(top$result) == 5L)

lineage <- query(
  "--mode", "lineage", "--event", "damaging", "--lineage", "Lung",
  "--source", "ARID1A", "--target", "EZH2"
)
stopifnot(identical(lineage$status, "tested"))
stopifnot(lineage$result[[1L]]$n >= 3L)

gap <- query(
  "--mode", "lineage", "--event", "damaging", "--lineage", "Lung",
  "--source", "KRAS", "--target", "RAF1"
)
stopifnot(identical(gap$status, "not_testable"))

pathway <- query("--mode", "pathway", "--pathway", "Androgen", "--target", "KRAS")
stopifnot(length(pathway$result) == 1L)
stopifnot(pathway$result[[1L]]$n > 1000L)

drug <- query(
  "--mode", "drug", "--omic", "effect",
  "--drug", "DPC-000001", "--target", "KRAS"
)
stopifnot(identical(drug$target, "KRAS"))
stopifnot(drug$n > 0L)

tested <- c("catalog", "core", "pair", "top", "lineage", "coverage_gap", "pathway", "drug")
sparse_statuses <- c("FOUND", "NOT_RETAINED", "INELIGIBLE", "NOT_COMPUTED", "MODULE_UNAVAILABLE")

network_root <- file.path(kb_root, "depmap-26q1-full", "lineage_sparse_networks")
if (dir.exists(network_root)) {
  network <- query(
    "--mode", "lineage_network", "--family", "effect_correlation",
    "--lineage", "Breast", "--source", "ESR1", "--limit", "5",
    "--reciprocal", "true"
  )
  stopifnot(network$status %in% sparse_statuses)
  tested <- c(tested, "lineage_network")
}

cnv_root <- file.path(kb_root, "depmap-26q1-full", "lineage_cnv_amplification_dependency", "Breast")
if (file.exists(file.path(cnv_root, "source_gene_order.csv"))) {
  cnv_source <- fread(file.path(cnv_root, "source_gene_order.csv"), nrows = 1L)$symbol[[1L]]
  lineage_cnv <- query(
    "--mode", "lineage_cnv", "--lineage", "Breast",
    "--source", cnv_source, "--limit", "5"
  )
  stopifnot(lineage_cnv$status %in% sparse_statuses)
  tested <- c(tested, "lineage_cnv")
}

prism_root <- file.path(kb_root, "depmap-26q1-full", "lineage_prism_associations")
if (dir.exists(prism_root)) {
  lineage_drug <- query(
    "--mode", "lineage_drug", "--omic", "effect",
    "--lineage", "Breast", "--target", "ESR1", "--limit", "5"
  )
  stopifnot(lineage_drug$status %in% sparse_statuses)
  tested <- c(tested, "lineage_drug")
}

enrichment_root <- file.path(kb_root, "depmap-26q1-full", "lineage_gene_enrichment", "Breast")
if (file.exists(file.path(enrichment_root, "manifest.json"))) {
  enrichment <- query(
    "--mode", "enrichment", "--lineage", "Breast",
    "--source", "ESR1", "--limit", "5"
  )
  stopifnot(enrichment$status %in% sparse_statuses)
  tested <- c(tested, "enrichment")
}

cat(toJSON(list(
  status = "PASS",
  release = catalog$qa$release,
  tested = tested,
  fixtures = list(
    core_gene = "KRAS",
    pair = "KRAS-RAF1",
    lineage = "Lung ARID1A-EZH2",
    drug = "DPC-000001-KRAS"
  )
), auto_unbox = TRUE, pretty = TRUE), "\n")
