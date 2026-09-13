#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(fgsea)
  library(jsonlite)
  library(msigdbr)
})

args <- commandArgs(trailingOnly = TRUE)
arg <- function(name, default = NULL) {
  hit <- grep(paste0("^--", name, "="), args, value = TRUE)
  if (!length(hit)) return(default)
  sub(paste0("^--", name, "="), "", hit[[1]])
}

knowledge_root <- normalizePath(arg("knowledge-root"), mustWork = TRUE)
min_size <- as.integer(arg("min-size", "15"))
max_size <- as.integer(arg("max-size", "500"))
input_root <- file.path(knowledge_root, "depmap-26q1-full", "lineage_selective_dependency")
output_root <- file.path(knowledge_root, "depmap-26q1-full", "lineage_selective_enrichment")
catalog_path <- file.path(input_root, "lineage_catalog.csv")
stopifnot(file.exists(catalog_path))
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

message("Loading MSigDB Hallmark and Reactome collections")
hallmark <- as.data.table(msigdbr(species = "Homo sapiens", collection = "H"))
reactome <- as.data.table(msigdbr(species = "Homo sapiens", collection = "C2",
                                  subcollection = "CP:REACTOME"))
sets_dt <- rbindlist(list(
  hallmark[, .(collection = "Hallmark", term = gs_name, gene = gene_symbol)],
  reactome[, .(collection = "Reactome", term = gs_name, gene = gene_symbol)]
))
pathways <- split(sets_dt$gene, paste(sets_dt$collection, sets_dt$term, sep = "::"))
catalog <- fread(catalog_path)
summaries <- vector("list", nrow(catalog))

for (i in seq_len(nrow(catalog))) {
  item <- catalog[i]
  input_path <- file.path(input_root, item$lineage_key, "all_genes.csv.gz")
  stats_dt <- fread(input_path, select = c("gene", "effect_size"))
  stats_dt <- stats_dt[is.finite(effect_size)]
  stats_dt <- stats_dt[!duplicated(gene)]
  ranks <- -stats_dt$effect_size
  names(ranks) <- stats_dt$gene
  ranks <- sort(ranks, decreasing = TRUE)
  result <- suppressWarnings(fgseaMultilevel(
    pathways = pathways,
    stats = ranks,
    minSize = min_size,
    maxSize = max_size,
    eps = 1e-50
  ))
  result <- as.data.table(result)
  result[, c("collection", "term") := tstrsplit(pathway, "::", fixed = TRUE)]
  result[, leadingEdge := vapply(leadingEdge, paste, collapse = ";", FUN.VALUE = character(1))]
  result[, lineage := item$lineage]
  setcolorder(result, c("lineage", "collection", "term", "size", "ES", "NES",
                        "pval", "padj", "log2err", "leadingEdge", "pathway"))
  result[, abs_NES := abs(NES)]
  setorder(result, padj, -abs_NES)
  result[, abs_NES := NULL]
  lineage_root <- file.path(output_root, item$lineage_key)
  dir.create(lineage_root, recursive = TRUE, showWarnings = FALSE)
  fwrite(result, file.path(lineage_root, "enrichment.csv.gz"), compress = "gzip")
  fwrite(result[padj <= 0.05], file.path(lineage_root, "fdr_significant.csv.gz"), compress = "gzip")
  manifest <- list(
    schema_version = 1,
    release = "26Q1",
    family = "lineage_selective_enrichment",
    lineage = item$lineage,
    status = "complete",
    tested_term_count = nrow(result),
    fdr_significant_term_count = result[padj <= 0.05, .N],
    method = "fgseaMultilevel over genes ranked by negative lineage-minus-rest Gene Effect",
    multiple_testing = "fgsea adjusted p values across Hallmark and Reactome terms for this lineage",
    interpretation = "positive NES indicates enrichment toward stronger lineage-selective dependencies",
    collections = c("MSigDB Hallmark", "MSigDB Reactome"),
    tm00_references = c("03", "05", "13"),
    input = input_path
  )
  write_json(manifest, file.path(lineage_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
  summaries[[i]] <- data.table(
    lineage = item$lineage,
    lineage_key = item$lineage_key,
    tested_term_count = nrow(result),
    fdr_significant_term_count = manifest$fdr_significant_term_count,
    status = "complete"
  )
  message(sprintf("[%d/%d] %s: FDR terms=%d", i, nrow(catalog), item$lineage,
                  manifest$fdr_significant_term_count))
}

summary_dt <- rbindlist(summaries)
fwrite(summary_dt, file.path(output_root, "lineage_catalog.csv"))
write_json(list(
  schema_version = 1,
  release = "26Q1",
  family = "lineage_selective_enrichment",
  status = "complete",
  lineage_count = nrow(summary_dt),
  collections = c("MSigDB Hallmark", "MSigDB Reactome"),
  method = "fgseaMultilevel over lineage-selective dependency ranks",
  tm00_references = c("03", "05", "13")
), file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed lineage-selective enrichment: ", output_root)
