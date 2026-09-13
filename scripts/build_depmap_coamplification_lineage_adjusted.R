#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
  library(limma)
})

args <- commandArgs(trailingOnly = TRUE)
arg <- function(name, default = NULL) {
  hit <- grep(paste0("^--", name, "="), args, value = TRUE)
  if (!length(hit)) return(default)
  sub(paste0("^--", name, "="), "", hit[[1]])
}

data_root <- normalizePath(arg("data-root"), mustWork = TRUE)
knowledge_root <- normalizePath(arg("knowledge-root"), mustWork = TRUE)
threshold <- as.numeric(arg("threshold", "2"))
min_informative_n <- as.integer(arg("min-informative-n", "5"))
fdr_max <- as.numeric(arg("fdr-max", "0.05"))
shard_size <- as.integer(arg("shard-size", "64"))
top_per_pair <- as.integer(arg("top-per-pair", "100"))

module_root <- file.path(knowledge_root, "depmap-26q1-full", "coamplification_dependency")
input_root <- file.path(module_root, "exhaustive_high_confidence")
output_root <- file.path(module_root, "lineage_adjusted")
shard_root <- file.path(output_root, "shards")
dir.create(shard_root, recursive = TRUE, showWarnings = FALSE)

effect_path <- file.path(data_root, "CRISPRGeneEffect.csv")
cnv_path <- file.path(data_root, "OmicsCNGeneMC_WES.csv")
condition_path <- file.path(data_root, "ModelCondition.csv")
model_path <- file.path(data_root, "Model.csv")
pair_path <- file.path(input_root, "screen_pair_catalog.csv.gz")
stopifnot(file.exists(effect_path), file.exists(cnv_path), file.exists(condition_path),
          file.exists(model_path), file.exists(pair_path))

message("Reading Gene Effect")
effect_dt <- fread(effect_path, check.names = FALSE)
effect_ids <- effect_dt[[1]]
effect_dt[[1]] <- NULL
target_genes <- toupper(sub(" \\([^()]++\\)$", "", names(effect_dt), perl = TRUE))
effect <- as.matrix(effect_dt)
storage.mode(effect) <- "double"
rownames(effect) <- effect_ids
colnames(effect) <- target_genes
rm(effect_dt)

message("Reading copy number and lineage annotations")
cnv_dt <- fread(cnv_path, check.names = FALSE)
condition_ids <- cnv_dt[[1]]
default_entry <- cnv_dt[[2]] == "Yes"
cnv_dt[[1]] <- NULL
cnv_dt[[1]] <- NULL
cnv_genes <- toupper(sub(" \\([^()]++\\)$", "", names(cnv_dt), perl = TRUE))
conditions <- fread(condition_path, select = c("ModelConditionID", "ModelID"))
model_ids <- conditions$ModelID[match(condition_ids, conditions$ModelConditionID)]
keep <- default_entry & !is.na(model_ids)
cnv <- as.matrix(cnv_dt[keep])
storage.mode(cnv) <- "double"
rownames(cnv) <- model_ids[keep]
colnames(cnv) <- cnv_genes
rm(cnv_dt)

models <- fread(model_path, select = c("ModelID", "OncotreeLineage"))
models <- models[!is.na(ModelID) & !is.na(OncotreeLineage) & nzchar(OncotreeLineage)]
common <- Reduce(intersect, list(rownames(effect), rownames(cnv), models$ModelID))
effect <- effect[common, , drop = FALSE]
cnv <- cnv[common, , drop = FALSE]
lineage <- models$OncotreeLineage[match(common, models$ModelID)]
cnv_index <- setNames(seq_along(cnv_genes), cnv_genes)

pairs <- fread(pair_path)
setorder(pairs, screen_pair_id)
shard_starts <- seq.int(1L, nrow(pairs), by = shard_size)
summaries <- vector("list", length(shard_starts))

empty_hits <- function() data.table(
  screen_pair_id = character(), source_gene = character(), partner_gene = character(),
  target_gene = character(), model_n = integer(), coamplified_n = integer(),
  source_only_n = integer(), informative_lineage_count = integer(),
  informative_coamplified_n = integer(), informative_source_only_n = integer(),
  lineage_adjusted_effect = numeric(), moderated_t = numeric(), p_value = numeric(),
  fdr_within_pair = numeric(), rank_within_pair = integer()
)

for (shard_i in seq_along(shard_starts)) {
  start <- shard_starts[[shard_i]]
  end <- min(start + shard_size - 1L, nrow(pairs))
  stem <- sprintf("shard_%06d_%06d", start, end)
  output_path <- file.path(shard_root, paste0(stem, ".csv.gz"))
  audit_path <- file.path(shard_root, paste0(stem, ".audit.csv.gz"))
  manifest_path <- file.path(shard_root, paste0(stem, ".manifest.json"))
  if (file.exists(output_path) && file.exists(audit_path) && file.exists(manifest_path)) {
    old <- fromJSON(manifest_path)
    if (identical(old$status, "complete")) {
      summaries[[shard_i]] <- data.table(start = start, end = end,
        retained_hit_count = old$retained_hit_count,
        estimable_pair_count = old$estimable_pair_count, status = "skipped_existing")
      next
    }
  }

  hit_rows <- vector("list", end - start + 1L)
  audit_rows <- vector("list", end - start + 1L)
  for (local_i in seq_len(end - start + 1L)) {
    pair_i <- start + local_i - 1L
    pair <- pairs[pair_i]
    source_amp <- is.finite(cnv[, cnv_index[[pair$source_gene]]]) &
      cnv[, cnv_index[[pair$source_gene]]] >= threshold
    partner_amp <- is.finite(cnv[, cnv_index[[pair$partner_gene]]]) &
      cnv[, cnv_index[[pair$partner_gene]]] >= threshold
    universe <- which(source_amp & is.finite(partner_amp))
    group <- partner_amp[universe]
    lin <- lineage[universe]
    composition <- data.table(lineage = lin, coamplified = group)[,
      .(coamplified_n = sum(coamplified), source_only_n = sum(!coamplified)), by = lineage]
    composition[, informative := coamplified_n > 0 & source_only_n > 0]
    informative_a <- composition[informative == TRUE, sum(coamplified_n)]
    informative_b <- composition[informative == TRUE, sum(source_only_n)]
    estimable <- informative_a >= min_informative_n && informative_b >= min_informative_n
    audit_rows[[local_i]] <- data.table(
      screen_pair_id = pair$screen_pair_id, source_gene = pair$source_gene,
      partner_gene = pair$partner_gene, model_n = length(universe),
      coamplified_n = sum(group), source_only_n = sum(!group),
      lineage_count = nrow(composition),
      informative_lineage_count = sum(composition$informative),
      informative_coamplified_n = informative_a,
      informative_source_only_n = informative_b,
      max_coamplified_lineage_fraction = if (sum(group)) max(composition$coamplified_n) / sum(group) else NA_real_,
      estimable = estimable
    )
    if (!estimable) next

    design <- model.matrix(~ factor(lin) + group)
    group_coef <- which(colnames(design) == "groupTRUE")
    if (length(group_coef) != 1L || qr(design)$rank < ncol(design)) next
    fit <- eBayes(lmFit(t(effect[universe, , drop = FALSE]), design))
    result <- topTable(fit, coef = group_coef, number = Inf, sort.by = "none",
                       adjust.method = "BH")
    hit <- which(is.finite(result$adj.P.Val) & result$adj.P.Val <= fdr_max & result$logFC < 0)
    if (!length(hit)) next
    hit <- head(hit[order(result$adj.P.Val[hit], result$logFC[hit])], top_per_pair)
    hit_rows[[local_i]] <- data.table(
      screen_pair_id = pair$screen_pair_id, source_gene = pair$source_gene,
      partner_gene = pair$partner_gene, target_gene = target_genes[hit],
      model_n = length(universe), coamplified_n = sum(group), source_only_n = sum(!group),
      informative_lineage_count = sum(composition$informative),
      informative_coamplified_n = informative_a,
      informative_source_only_n = informative_b,
      lineage_adjusted_effect = result$logFC[hit], moderated_t = result$t[hit],
      p_value = result$P.Value[hit], fdr_within_pair = result$adj.P.Val[hit],
      rank_within_pair = seq_along(hit)
    )
  }
  out <- rbindlist(hit_rows, fill = TRUE)
  if (!ncol(out)) out <- empty_hits()
  audit <- rbindlist(audit_rows, fill = TRUE)
  fwrite(out, output_path, compress = "gzip")
  fwrite(audit, audit_path, compress = "gzip")
  shard_manifest <- list(
    schema_version = 1, release = "26Q1", family = "coamplification_dependency",
    layer = "lineage_adjusted", status = "complete", pair_start = start,
    pair_end = end, pair_count = nrow(audit), estimable_pair_count = sum(audit$estimable),
    retained_hit_count = nrow(out), fdr_max = fdr_max,
    method = "limma fixed-effect model: Gene Effect ~ OncotreeLineage + coamplification status within source-amplified models"
  )
  write_json(shard_manifest, manifest_path, auto_unbox = TRUE, pretty = TRUE)
  summaries[[shard_i]] <- data.table(start = start, end = end,
    retained_hit_count = nrow(out), estimable_pair_count = sum(audit$estimable), status = "complete")
  message(sprintf("[%d/%d] pairs %d-%d estimable=%d retained=%d", shard_i,
    length(shard_starts), start, end, sum(audit$estimable), nrow(out)))
  gc(FALSE)
}

hit_files <- sort(list.files(shard_root, pattern = "^shard_[0-9_]+\\.csv\\.gz$", full.names = TRUE))
audit_files <- sort(list.files(shard_root, pattern = "\\.audit\\.csv\\.gz$", full.names = TRUE))
all_hits <- rbindlist(lapply(hit_files, fread), fill = TRUE)
all_audit <- rbindlist(lapply(audit_files, fread), fill = TRUE)
if (nrow(all_hits)) setorder(all_hits, fdr_within_pair, lineage_adjusted_effect)
fwrite(all_hits, file.path(output_root, "significant_hits.csv.gz"), compress = "gzip")
fwrite(all_audit, file.path(output_root, "pair_lineage_audit.csv.gz"), compress = "gzip")
summary_dt <- rbindlist(summaries)
fwrite(summary_dt, file.path(output_root, "shard_catalog.csv"))

manifest <- list(
  schema_version = 1, release = "26Q1", family = "coamplification_dependency",
  layer = "lineage_adjusted", status = "complete", copy_number_threshold = threshold,
  input_directional_pair_count = nrow(pairs), estimable_pair_count = sum(all_audit$estimable),
  target_gene_count = ncol(effect), retained_significant_hit_count = nrow(all_hits),
  pair_with_retained_hit_count = if (nrow(all_hits)) uniqueN(all_hits$screen_pair_id) else 0L,
  min_informative_n = min_informative_n, fdr_max = fdr_max,
  method = "limma fixed-effect model within source-amplified models: Gene Effect ~ OncotreeLineage + coamplification status",
  multiple_testing = "BH FDR across all CRISPR targets separately within each directional pair",
  interpretation = "negative adjusted effect means stronger dependency in coamplified models after OncotreeLineage adjustment",
  inputs = c(effect_path, cnv_path, condition_path, model_path, pair_path)
)
write_json(manifest, file.path(output_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
message("Completed lineage-adjusted coamplification screen: ", output_root)
