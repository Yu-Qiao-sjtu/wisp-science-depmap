#!/usr/bin/env Rscript

suppressPackageStartupMessages(library(jsonlite))
args <- commandArgs(trailingOnly = TRUE)
value_after <- function(flag, default = NULL) {
  i <- match(flag, args)
  if (is.na(i) || i == length(args)) default else args[[i + 1L]]
}

effect_csv <- value_after("--gene-effect")
model_csv <- value_after("--model")
output_root <- value_after("--output-root", "results/biomarker_target_eligibility_26Q1")
min_n <- as.integer(value_after("--min-n", "800"))
min_sd <- as.numeric(value_after("--min-sd", "0.05"))
min_lineages <- as.integer(value_after("--min-lineages", "5"))
if (is.null(effect_csv) || is.null(model_csv) || !file.exists(effect_csv) || !file.exists(model_csv)) {
  stop("Provide existing --gene-effect and --model CSV files")
}

effect <- read.csv(effect_csv, check.names = FALSE, stringsAsFactors = FALSE)
model <- read.csv(model_csv, check.names = FALSE, stringsAsFactors = FALSE)
names(effect)[1L] <- "ModelID"
names(effect)[-1L] <- sub(" \\([0-9]+\\)$", "", names(effect)[-1L])
common <- intersect(effect$ModelID, model$ModelID)
effect <- effect[match(common, effect$ModelID), , drop = FALSE]
model <- model[match(common, model$ModelID), , drop = FALSE]
lineage <- ifelse(is.na(model$OncotreeLineage) | model$OncotreeLineage == "", "Unknown", model$OncotreeLineage)

summarise_gene <- function(v) {
  ok <- is.finite(v)
  observed_lineages <- table(lineage[ok])
  c(
    sample_n = sum(ok),
    mean_gene_effect = mean(v[ok]),
    sd_gene_effect = sd(v[ok]),
    q05 = unname(quantile(v[ok], 0.05)),
    median = median(v[ok]),
    q95 = unname(quantile(v[ok], 0.95)),
    dependent_fraction_lt_minus_0_5 = mean(v[ok] < -0.5),
    strong_dependent_fraction_lt_minus_1 = mean(v[ok] < -1),
    lineage_n_observed = sum(observed_lineages > 0),
    lineage_n_ge_20 = sum(observed_lineages >= 20)
  )
}

stats <- t(vapply(effect[-1L], summarise_gene, numeric(10L)))
catalog <- data.frame(target_gene = rownames(stats), stats, row.names = NULL, check.names = FALSE)
catalog$eligible_for_nested_model <- catalog$sample_n >= min_n &
  catalog$sd_gene_effect >= min_sd & catalog$lineage_n_observed >= min_lineages
catalog$eligibility_reason <- ifelse(
  catalog$sample_n < min_n, "insufficient_gene_effect_coverage",
  ifelse(catalog$sd_gene_effect < min_sd, "low_gene_effect_variation",
         ifelse(catalog$lineage_n_observed < min_lineages, "insufficient_lineage_coverage", "eligible"))
)
catalog <- catalog[order(!catalog$eligible_for_nested_model, -catalog$sd_gene_effect, catalog$target_gene), ]

dir.create(output_root, recursive = TRUE, showWarnings = FALSE)
write.csv(catalog, file.path(output_root, "target_eligibility_catalog.csv"), row.names = FALSE)
write_json(list(
  status = "complete",
  target_gene_n = nrow(catalog),
  eligible_n = sum(catalog$eligible_for_nested_model),
  thresholds = list(min_n = min_n, min_sd = min_sd, min_lineages = min_lineages),
  policy = "All target genes are retained. Eligibility is a prioritization flag, not an exclusion from querying or user-requested modeling.",
  generated_at_utc = format(Sys.time(), tz = "UTC", usetz = TRUE)
), file.path(output_root, "manifest.json"), pretty = TRUE, auto_unbox = TRUE)
message("Completed target eligibility catalog: ", nrow(catalog), " genes; ",
        sum(catalog$eligible_for_nested_model), " eligible")
