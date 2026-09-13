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
input_path <- file.path(knowledge_root, "depmap-26q1-full",
                        "progeny_prism_associations", "pathway_drug_associations.csv.gz")
output_root <- path.expand(arg("output-root", file.path(knowledge_root, "audits",
                                                         "progeny-prism-thresholds")))
dir.create(output_root, recursive = TRUE, showWarnings = FALSE)

min_n_values <- c(10L, 30L, 50L, 100L, 300L, 500L)
min_abs_r_values <- c(0, 0.1, 0.2, 0.3)
dt <- fread(input_path)

summaries <- list()
hits <- list()
for (min_n in min_n_values) {
  eligible <- copy(dt[n >= min_n & is.finite(p_value)])
  eligible[, calibrated_fdr := p.adjust(p_value, method = "BH"), by = pathway]
  for (min_abs_r in min_abs_r_values) {
    selected <- eligible[calibrated_fdr <= 0.05 & abs(pearson_r) >= min_abs_r]
    summaries[[length(summaries) + 1L]] <- data.table(
      min_n = min_n,
      min_abs_r = min_abs_r,
      eligible_tests = nrow(eligible),
      significant_associations = nrow(selected),
      unique_pathways = uniqueN(selected$pathway),
      unique_drugs = uniqueN(selected$drug_id),
      negative_associations = selected[pearson_r < 0, .N],
      positive_associations = selected[pearson_r > 0, .N]
    )
    if (min_abs_r == 0.2) {
      selected[, `:=`(calibration_min_n = min_n,
                      calibration_min_abs_r = min_abs_r)]
      hits[[length(hits) + 1L]] <- selected
    }
  }
}

summary_dt <- rbindlist(summaries)
hit_dt <- rbindlist(hits, fill = TRUE)
setorder(summary_dt, min_n, min_abs_r)
hit_dt[, abs_pearson_r := abs(pearson_r)]
setorder(hit_dt, calibration_min_n, calibrated_fdr, -abs_pearson_r)
fwrite(summary_dt, file.path(output_root, "threshold_sensitivity.csv"))
fwrite(hit_dt, file.path(output_root, "effect_filtered_hits.csv.gz"), compress = "gzip")
write_json(list(
  schema_version = 1,
  status = "complete",
  input = input_path,
  input_rows = nrow(dt),
  fdr_method = "BH across eligible drugs separately within each pathway",
  min_n_values = min_n_values,
  min_abs_r_values = min_abs_r_values,
  note = "Sensitivity audit only; it does not replace the canonical association table."
), file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)

message("Completed PROGENy-PRISM threshold sensitivity audit: ", output_root)
