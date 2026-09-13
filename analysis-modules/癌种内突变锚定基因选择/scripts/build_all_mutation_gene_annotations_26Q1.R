#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table); library(jsonlite)})

args <- commandArgs(TRUE)
stopifnot(length(args) %in% c(4L, 6L))
if (length(args) == 4L) {
  data_dir <- normalizePath(args[[1]])
  somatic_path <- file.path(data_dir, "OmicsSomaticMutations.csv")
  dependency_path <- file.path(data_dir, "CRISPRGeneDependency.csv")
  common_essential_path <- file.path(data_dir, "CRISPRInferredCommonEssentials.csv")
  annotation_dir <- normalizePath(args[[2]])
  anchor_run_dir <- normalizePath(args[[3]])
  out <- args[[4]]
} else {
  somatic_path <- normalizePath(args[[1]])
  dependency_path <- normalizePath(args[[2]])
  common_essential_path <- normalizePath(args[[3]])
  annotation_dir <- normalizePath(args[[4]])
  anchor_run_dir <- normalizePath(args[[5]])
  out <- args[[6]]
}
dir.create(out, recursive = TRUE, showWarnings = FALSE)

clean_gene <- function(x) sub(" \\(\\d+\\)$", "", as.character(x))
is_true <- function(x) !is.na(x) & tolower(as.character(x)) == "true"
collapse_values <- function(x) {
  x <- sort(unique(as.character(x[!is.na(x) & nzchar(as.character(x))])))
  if (length(x) == 0L) NA_character_ else paste(x, collapse = "|")
}
top_values <- function(x, n = 5L) {
  x <- as.character(x[!is.na(x) & nzchar(as.character(x))])
  if (length(x) == 0L) return(NA_character_)
  tab <- sort(table(x), decreasing = TRUE)
  tab <- head(tab, n)
  paste0(names(tab), ":", as.integer(tab), collapse = "|")
}

menu <- fread(file.path(anchor_run_dir, "gene_by_lineage_mutation_menu.csv"))
universe <- sort(unique(menu[matrix == "AnySelected", gene]))
stopifnot(length(universe) > 0L, !anyDuplicated(universe))

matrix_counts <- menu[, .(model_count = sum(mut_n)), by = .(gene, matrix)]
matrix_counts <- dcast(matrix_counts, gene ~ matrix, value.var = "model_count", fill = 0L)
for (name in c("AnySelected", "Damaging", "Hotspot")) {
  if (!name %chin% names(matrix_counts)) matrix_counts[, (name) := 0L]
}
setnames(
  matrix_counts,
  c("AnySelected", "Damaging", "Hotspot"),
  c("any_selected_model_count", "damaging_model_count", "hotspot_model_count")
)

lineage_summary <- menu[matrix == "AnySelected" & mut_n > 0L,
  .(
    observed_lineage_count = uniqueN(lineage),
    top_lineages = top_values(rep(lineage, mut_n), 5L)
  ),
  by = gene
]

dependency <- fread(dependency_path, select = 1L)
dependency_ids <- unique(as.character(dependency[[1L]]))

somatic_columns <- c(
  "ModelID", "HugoSymbol", "IsDefaultEntryForModel", "VariantInfo",
  "ProteinChange", "LikelyLoF", "Hotspot", "VepImpact"
)
som <- fread(somatic_path, select = somatic_columns)
som <- som[
  IsDefaultEntryForModel == "Yes" & ModelID %chin% dependency_ids &
    !is.na(HugoSymbol) & HugoSymbol %chin% universe
]
som[, `:=`(
  gene = as.character(HugoSymbol),
  is_likely_lof = is_true(LikelyLoF),
  is_hotspot = is_true(Hotspot),
  is_missense = grepl("missense_variant", VariantInfo, fixed = TRUE),
  is_truncating_or_splice = grepl(
    "frameshift_variant|stop_gained|splice_acceptor_variant|splice_donor_variant|start_lost",
    VariantInfo
  )
)]

mutation_summary <- som[, .(
  variant_record_count = .N,
  mutated_model_count = uniqueN(ModelID),
  likely_lof_record_count = sum(is_likely_lof),
  likely_lof_model_count = uniqueN(ModelID[is_likely_lof]),
  hotspot_record_count = sum(is_hotspot),
  hotspot_flag_model_count = uniqueN(ModelID[is_hotspot]),
  missense_record_count = sum(is_missense),
  missense_model_count = uniqueN(ModelID[is_missense]),
  truncating_or_splice_record_count = sum(is_truncating_or_splice),
  truncating_or_splice_model_count = uniqueN(ModelID[is_truncating_or_splice]),
  protein_change_count = uniqueN(ProteinChange[!is.na(ProteinChange) & ProteinChange != ""]),
  top_protein_changes = top_values(ProteinChange, 5L),
  top_variant_annotations = top_values(VariantInfo, 5L),
  top_vep_impacts = top_values(VepImpact, 4L)
), by = gene]

hgnc_columns <- c(
  "hgnc_id", "symbol", "name", "locus_group", "locus_type", "status",
  "location", "prev_symbol", "alias_symbol", "gene_group", "entrez_id",
  "ensembl_gene_id", "uniprot_ids", "cosmic", "omim_id"
)
hgnc <- fread(file.path(annotation_dir, "hgnc_table.csv"), select = hgnc_columns)
hgnc <- hgnc[!duplicated(symbol)]
hgnc[, hgnc_row := .I]

annotation_columns <- setdiff(hgnc_columns, c("prev_symbol", "alias_symbol"))
exact <- copy(hgnc[, c(annotation_columns, "hgnc_row"), with = FALSE])
exact[, `:=`(gene = symbol, hgnc_match_type = "exact", match_priority = 1L)]

build_symbol_map <- function(column, match_type, priority) {
  mapped <- hgnc[!is.na(get(column)) & nzchar(get(column)),
    .(gene = trimws(unlist(strsplit(get(column), "[|,;]")))),
    by = c(annotation_columns, "hgnc_row")
  ]
  mapped <- mapped[nzchar(gene)]
  mapped[, candidate_count := uniqueN(hgnc_row), by = gene]
  mapped <- mapped[candidate_count == 1L]
  mapped[, `:=`(hgnc_match_type = match_type, match_priority = priority, candidate_count = NULL)]
  mapped
}

previous <- build_symbol_map("prev_symbol", "previous_symbol", 2L)
alias <- build_symbol_map("alias_symbol", "alias_symbol", 3L)
hgnc_map <- rbindlist(list(exact, previous, alias), fill = TRUE)
setorder(hgnc_map, gene, match_priority)
hgnc_map <- hgnc_map[!duplicated(gene)]
hgnc_map[, c("match_priority", "hgnc_row") := NULL]
setnames(hgnc_map, "symbol", "canonical_symbol")

role <- fread(file.path(annotation_dir, "OncoKB_mutant_class.csv"))
stopifnot(ncol(role) == 4L)
setnames(role, c("row_id", "Mut", "oncoKB", "class"))
role[, event := fcase(
  grepl("_dam$", Mut), "Damaging",
  grepl("_HS$", Mut), "Hotspot",
  default = NA_character_
)]
role[, gene := sub("_(dam|HS)$", "", Mut)]
role_summary <- role[!is.na(event), .(
  oncokb_damaging_role = collapse_values(oncoKB[event == "Damaging"]),
  oncokb_damaging_class = collapse_values(class[event == "Damaging"]),
  oncokb_hotspot_role = collapse_values(oncoKB[event == "Hotspot"]),
  oncokb_hotspot_class = collapse_values(class[event == "Hotspot"])
), by = gene]

common_essential <- fread(common_essential_path, header = TRUE)[[1L]]
common_essential <- unique(clean_gene(common_essential))

result <- data.table(gene = universe)
result <- merge(result, hgnc_map, by = "gene", all.x = TRUE)
result <- merge(result, matrix_counts, by = "gene", all.x = TRUE)
result <- merge(result, lineage_summary, by = "gene", all.x = TRUE)
result <- merge(result, mutation_summary, by = "gene", all.x = TRUE)
result <- merge(result, role_summary, by = "gene", all.x = TRUE)

count_columns <- names(result)[grepl("(_count|_model_count)$", names(result))]
for (column in count_columns) set(result, which(is.na(result[[column]])), column, 0L)
result[, `:=`(
  annotation_status = fifelse(is.na(hgnc_id), "unresolved", "resolved"),
  is_common_essential = gene %chin% common_essential |
    (!is.na(canonical_symbol) & canonical_symbol %chin% common_essential),
  has_damaging_event = damaging_model_count > 0L,
  has_hotspot_event = hotspot_model_count > 0L,
  has_oncokb_role = !is.na(oncokb_damaging_role) | !is.na(oncokb_hotspot_role)
)]
result[, available_event_definitions := paste0(
  "AnySelected",
  fifelse(has_damaging_event, "|Damaging", ""),
  fifelse(has_hotspot_event, "|Hotspot", ""),
  fifelse(protein_change_count > 0L, "|ProteinChange", "")
)]

setcolorder(result, c(
  "gene", "canonical_symbol", "hgnc_match_type", "annotation_status", "hgnc_id",
  "name", "locus_group", "locus_type", "status", "location", "gene_group",
  "entrez_id", "ensembl_gene_id", "uniprot_ids", "cosmic", "omim_id",
  "available_event_definitions", "any_selected_model_count", "damaging_model_count",
  "hotspot_model_count", "variant_record_count", "mutated_model_count",
  "observed_lineage_count", "top_lineages", "likely_lof_record_count",
  "likely_lof_model_count", "hotspot_record_count", "hotspot_flag_model_count",
  "missense_record_count", "missense_model_count", "truncating_or_splice_record_count",
  "truncating_or_splice_model_count", "protein_change_count", "top_protein_changes",
  "top_variant_annotations", "top_vep_impacts", "oncokb_damaging_role",
  "oncokb_damaging_class", "oncokb_hotspot_role", "oncokb_hotspot_class",
  "has_oncokb_role", "is_common_essential"
))
setorder(result, gene)

stopifnot(nrow(result) == length(universe), !anyDuplicated(result$gene))
fwrite(result, file.path(out, "all_mutation_gene_annotations.csv"))
fwrite(
  result[annotation_status == "unresolved"],
  file.path(out, "unresolved_hgnc_symbols.csv")
)
writeLines(
  vapply(seq_len(nrow(result)), function(i) {
    toJSON(as.list(result[i]), auto_unbox = TRUE, na = "null")
  }, character(1)),
  file.path(out, "all_mutation_gene_annotations.jsonl")
)

manifest <- list(
  status = "complete",
  release = "26Q1",
  gene_universe = "genes with at least one retained somatic mutation record among CRISPR Gene Dependency models",
  gene_count = nrow(result),
  hgnc_resolved_count = result[annotation_status == "resolved", .N],
  hgnc_exact_count = result[hgnc_match_type == "exact", .N],
  hgnc_previous_symbol_count = result[hgnc_match_type == "previous_symbol", .N],
  hgnc_alias_symbol_count = result[hgnc_match_type == "alias_symbol", .N],
  hgnc_unresolved_count = result[annotation_status == "unresolved", .N],
  hgnc_unresolved_genes = result[annotation_status == "unresolved", gene],
  oncokb_role_gene_count = result[has_oncokb_role == TRUE, .N],
  common_essential_gene_count = result[is_common_essential == TRUE, .N],
  damaging_positive_gene_count = result[has_damaging_event == TRUE, .N],
  hotspot_positive_gene_count = result[has_hotspot_event == TRUE, .N],
  sources = c(
    "DepMap Public 26Q1 OmicsSomaticMutations.csv",
    "DepMap Public 26Q1 mutation matrices and CRISPR Gene Dependency model universe",
    "DepMap Public 26Q1 CRISPRInferredCommonEssentials.csv",
    "local HGNC table",
    "local OncoKB-derived mutation class table"
  ),
  limitations = c(
    "OncoKB-derived roles are a local snapshot and cover cancer-role genes only",
    "an absent OncoKB role is unclassified in this snapshot, not evidence that a gene has no cancer role",
    "top protein changes and annotations summarize retained DepMap records and are not functional validation",
    "unresolved HGNC symbols are retained with their DepMap symbol and explicit unresolved status"
  )
)
write_json(manifest, file.path(out, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
print(manifest)
