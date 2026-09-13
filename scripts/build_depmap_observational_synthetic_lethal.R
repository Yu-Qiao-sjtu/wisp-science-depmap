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
top_per_event <- as.integer(arg("top-per-event", "100"))
mutation_module_root <- file.path(
  knowledge_root, "analysis-modules", "癌种内突变锚定基因选择",
  "cancer_anchor_catalog_v2", "downstream_dependency",
  "05_precomputed_gene_effect_matrices"
)
output_root <- file.path(mutation_module_root,
                         "observational_synthetic_lethal_candidates")
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

specs <- list(
  list(module = "damaging_mutation_dependency", event = "damaging_mutation",
       source_order = "mutation_gene_order.csv", n_key = "mutation_n",
       fdr_key = "fdr_mutation_more_dependent", class = "mutation_context_dependency"),
  list(module = "custom_missense_mutation_dependency", event = "custom_missense_mutation",
       source_order = "mutation_gene_order.csv", n_key = "mutation_n",
       fdr_key = "fdr_mutation_more_dependent", class = "mutation_context_dependency"),
  list(module = "hotspot_mutation_dependency", event = "hotspot_mutation",
       source_order = "mutation_gene_order.csv", n_key = "mutation_n",
       fdr_key = "fdr_mutation_more_dependent", class = "mutation_context_dependency"),
  list(module = "cnv_amplification_dependency", event = "cnv_amplification",
       source_order = "cnv_gene_order.csv", n_key = "amplified_n",
       fdr_key = "fdr_amplified_more_dependent", class = "amplification_context_dependency")
)

read_symbols <- function(path) {
  x <- fread(path)
  symbol_col <- intersect(c("symbol", "gene", "source_gene", "target_gene"), names(x))[[1]]
  toupper(x[[symbol_col]])
}

module_summaries <- list()
all_files <- character()
for (spec in specs) {
  relocated <- file.path(mutation_module_root, spec$module)
  root <- if (dir.exists(relocated)) relocated else
    file.path(knowledge_root, "depmap-26q1-full", spec$module)
  manifest_path <- file.path(root, "manifest.json")
  stopifnot(file.exists(manifest_path))
  sources <- read_symbols(file.path(root, spec$source_order))
  targets <- read_symbols(file.path(root, "target_gene_order.csv"))
  blocks <- sort(list.files(file.path(root, "blocks"), pattern = "\\.rds$", full.names = TRUE))
  module_parts <- vector("list", length(blocks))
  retained <- 0L
  for (block_i in seq_along(blocks)) {
    block <- readRDS(blocks[[block_i]])
    fdr <- block[[spec$fdr_key]]
    difference <- block$mean_difference
    event_n <- block[[spec$n_key]]
    control_n <- block$wildtype_n
    rows <- vector("list", nrow(fdr))
    for (row_i in seq_len(nrow(fdr))) {
      source_index <- block$source_start + row_i - 1L
      keep <- which(is.finite(fdr[row_i, ]) & fdr[row_i, ] <= fdr_max &
                    is.finite(difference[row_i, ]) & difference[row_i, ] < 0)
      if (!length(keep)) next
      keep <- keep[order(fdr[row_i, keep], difference[row_i, keep])]
      keep <- head(keep, top_per_event)
      keep <- keep[targets[keep] != sources[[source_index]]]
      if (!length(keep)) next
      rows[[row_i]] <- data.table(
        candidate_class = spec$class,
        event_type = spec$event,
        source_gene = sources[[source_index]],
        target_gene = targets[keep],
        event_n = as.integer(event_n[row_i, keep]),
        control_n = as.integer(control_n[row_i, keep]),
        mean_difference = as.numeric(difference[row_i, keep]),
        fdr = as.numeric(fdr[row_i, keep]),
        rank_within_event = seq_along(keep)
      )
    }
    part <- rbindlist(rows, fill = TRUE)
    module_parts[[block_i]] <- part
    retained <- retained + nrow(part)
    if (block_i %% 25L == 0L || block_i == length(blocks)) {
      message(sprintf("%s [%d/%d] retained=%d", spec$module, block_i, length(blocks), retained))
    }
    rm(block, fdr, difference, event_n, control_n, rows, part)
    gc(FALSE)
  }
  module_out <- rbindlist(module_parts, fill = TRUE)
  module_path <- file.path(output_root, paste0(spec$event, ".csv.gz"))
  fwrite(module_out, module_path, compress = "gzip")
  all_files <- c(all_files, module_path)
  module_summaries[[spec$event]] <- list(
    source_gene_count = uniqueN(module_out$source_gene),
    retained_candidate_count = nrow(module_out),
    source_module = spec$module
  )
  rm(module_parts, module_out)
  gc(FALSE)
}

all <- rbindlist(lapply(all_files, fread), fill = TRUE)
pair_summary <- all[, .(
  evidence_family_count = uniqueN(event_type),
  evidence_families = paste(sort(unique(event_type)), collapse = ";"),
  best_fdr = min(fdr, na.rm = TRUE),
  strongest_mean_difference = min(mean_difference, na.rm = TRUE),
  max_event_n = max(event_n, na.rm = TRUE),
  max_control_n = max(control_n, na.rm = TRUE)
), by = .(source_gene, target_gene)]
setorder(pair_summary, -evidence_family_count, best_fdr, strongest_mean_difference)
fwrite(pair_summary, file.path(output_root, "pair_evidence_summary.csv.gz"), compress = "gzip")

manifest <- list(
  schema_version = 1,
  release = "26Q1",
  family = "observational_synthetic_lethal_candidates",
  status = "complete",
  fdr_max = fdr_max,
  top_per_event = top_per_event,
  source_modules = module_summaries,
  retained_evidence_row_count = nrow(all),
  unique_pair_count = nrow(pair_summary),
  multi_family_pair_count = pair_summary[evidence_family_count > 1, .N],
  selection = "event group has significantly stronger CRISPR dependency than control; top rows retained per event source",
  multiple_testing = "uses the source module's existing BH FDR family; no cross-family p values are combined",
  interpretation = "hypothesis-generating contextual vulnerabilities; mutation-context rows are observational synthetic-lethal candidates, not validated synthetic lethality",
  tm00_references = c("05.2", "07", "08", "09", "10", "11", "12", "13")
)
write_json(manifest, file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed observational candidate build: ", output_root)
