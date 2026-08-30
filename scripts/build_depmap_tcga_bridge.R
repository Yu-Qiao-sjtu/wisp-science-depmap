#!/usr/bin/env Rscript

# Build a read-only, gene-centric bridge between the DepMap 26Q1 CRISPR
# universe and TCGA expression plus censored survival endpoints. The output
# contains precomputed score tests only; the
# Wisp query service never opens the raw patient matrices.

suppressPackageStartupMessages({
  library(data.table)
  library(jsonlite)
})

parse_args <- function(args) {
  out <- list(projects = "all", min_n = 30L, min_events = 10L)
  i <- 1L
  while (i <= length(args)) {
    key <- sub("^--", "", args[[i]])
    if (i == length(args)) stop("missing value for --", key)
    out[[gsub("-", "_", key)]] <- args[[i + 1L]]
    i <- i + 2L
  }
  for (required in c("depmap_root", "tcga_root", "output")) {
    if (is.null(out[[required]]) || !nzchar(out[[required]])) {
      stop("--", gsub("_", "-", required), " is required")
    }
  }
  out$min_n <- as.integer(out$min_n)
  out$min_events <- as.integer(out$min_events)
  if (is.na(out$min_n) || out$min_n < 10L) stop("--min-n must be at least 10")
  if (is.na(out$min_events) || out$min_events < 3L) {
    stop("--min-events must be at least 3")
  }
  out
}

script_directory <- function() {
  file_arg <- grep("^--file=", commandArgs(trailingOnly = FALSE), value = TRUE)
  if (!length(file_arg)) return(getwd())
  dirname(normalizePath(sub("^--file=", "", file_arg[[1L]]), mustWork = TRUE))
}

PROJECT_LINEAGES <- c(
  ACC = "Adrenal Gland", BLCA = "Bladder Urinary Tract", BRCA = "Breast",
  CESC = "Cervix", CHOL = "Biliary Tract", COAD = "Bowel", DLBC = "Lymphoid",
  ESCA = "Esophagus Stomach", GBM = "CNS Brain", HNSC = "Head and Neck",
  KICH = "Kidney", KIRC = "Kidney", KIRP = "Kidney", LAML = "Myeloid",
  LGG = "CNS Brain", LIHC = "Liver", LUAD = "Lung", LUSC = "Lung",
  MESO = "Pleura", OV = "Ovary Fallopian Tube", PAAD = "Pancreas",
  PCPG = "Adrenal Gland", PRAD = "Prostate", READ = "Bowel",
  SARC = "Soft Tissue", SKCM = "Skin", STAD = "Esophagus Stomach",
  TGCT = "Testis", THCA = "Thyroid", THYM = "Other", UCEC = "Uterus",
  UCS = "Uterus", UVM = "Eye"
)

ENDPOINTS <- c("OS", "DSS", "DFI", "PFI")
patient_id <- function(x) substr(as.character(x), 1L, 12L)
sample_type <- function(x) suppressWarnings(as.integer(substr(as.character(x), 14L, 15L)))

primary_sample_types <- function(project) {
  # TCGA code 01 is Primary Solid Tumor. LAML has no solid-tumour specimens;
  # its expression matrix uses code 03, Primary Blood Derived Cancer -
  # Peripheral Blood.
  if (identical(project, "LAML")) 3L else 1L
}

first_primary_columns <- function(ids, project) {
  keep <- which(sample_type(ids) %in% primary_sample_types(project))
  keep[!duplicated(patient_id(ids[keep]))]
}

read_survival <- function(path) {
  table <- fread(cmd = paste("gzip -cd", shQuote(path)), na.strings = c("", "NA"))
  table[, patient := patient_id(sample)]
  table <- table[!duplicated(patient)]
  for (endpoint in ENDPOINTS) {
    table[, (endpoint) := suppressWarnings(as.integer(get(endpoint)))]
    table[, (paste0(endpoint, ".time")) := suppressWarnings(as.numeric(get(paste0(endpoint, ".time"))))]
  }
  table
}

load_rdata_matrix <- function(path) {
  environment <- new.env(parent = emptyenv())
  objects <- load(path, envir = environment)
  matrices <- objects[vapply(objects, function(name) {
    value <- environment[[name]]
    is.matrix(value) || is.data.frame(value)
  }, logical(1))]
  if (length(matrices) != 1L) {
    stop("expected exactly one matrix-like object in ", path)
  }
  as.matrix(environment[[matrices[[1L]]]])
}

match_expression <- function(paths, genes, project) {
  ensembl <- load_rdata_matrix(paths$expression_ensembl)
  rownames(ensembl) <- sub("\\..*$", "", rownames(ensembl))
  columns <- first_primary_columns(colnames(ensembl), project)
  if (!length(columns)) {
    stop("no eligible primary cancer samples in ", paths$expression_ensembl)
  }
  ensembl <- ensembl[, columns, drop = FALSE]
  colnames(ensembl) <- patient_id(colnames(ensembl))
  ensembl_rows <- match(genes$ensembl_gene_id, rownames(ensembl))
  ensembl_matched <- which(!is.na(ensembl_rows))
  values <- log2(pmax(ensembl[ensembl_rows[ensembl_matched], , drop = FALSE], 0) + 1)
  gene_index <- ensembl_matched
  mapping_basis <- rep("ensembl_gene_id", length(ensembl_matched))

  unmatched <- which(is.na(ensembl_rows))
  symbol <- load_rdata_matrix(paths$expression_symbol)
  symbol_columns <- first_primary_columns(colnames(symbol), project)
  symbol <- symbol[, symbol_columns, drop = FALSE]
  colnames(symbol) <- patient_id(colnames(symbol))
  patient_columns <- match(colnames(ensembl), colnames(symbol))
  if (anyNA(patient_columns)) {
    stop("Ensembl and symbol expression matrices have different primary patients for ", project)
  }
  symbol_rows <- match(genes$symbol[unmatched], rownames(symbol))
  fallback <- which(!is.na(symbol_rows))
  if (length(fallback)) {
    fallback_values <- log2(pmax(
      symbol[symbol_rows[fallback], patient_columns, drop = FALSE], 0
    ) + 1)
    values <- rbind(values, fallback_values)
    gene_index <- c(gene_index, unmatched[fallback])
    mapping_basis <- c(mapping_basis, rep("symbol_fallback", length(fallback)))
  }
  storage.mode(values) <- "double"
  list(
    gene_index = gene_index, values = values, patients = colnames(ensembl),
    mapping_basis = mapping_basis
  )
}

cox_score_test <- function(values, patients, survival, endpoint, min_n, min_events) {
  time_name <- paste0(endpoint, ".time")
  clinical <- survival[match(patients, survival$patient)]
  valid <- which(
    !is.na(clinical[[endpoint]]) & !is.na(clinical[[time_name]]) &
      clinical[[time_name]] >= 0
  )
  result <- list(
    n = length(valid), events = if (length(valid)) sum(clinical[[endpoint]][valid] == 1L) else 0L,
    z = rep(NA_real_, nrow(values)), p = rep(NA_real_, nrow(values)),
    fdr = rep(NA_real_, nrow(values))
  )
  if (result$n < min_n || result$events < min_events) return(result)

  x <- values[, valid, drop = FALSE]
  times <- clinical[[time_name]][valid]
  events <- clinical[[endpoint]][valid] == 1L
  order_index <- order(times, decreasing = TRUE)
  x <- x[, order_index, drop = FALSE]
  times <- times[order_index]
  events <- events[order_index]

  risk_sum <- numeric(nrow(x))
  risk_sum_sq <- numeric(nrow(x))
  score <- numeric(nrow(x))
  information <- numeric(nrow(x))
  risk_n <- 0L
  groups <- split(seq_along(times), cumsum(c(TRUE, diff(times) != 0)))
  for (group in groups) {
    block <- x[, group, drop = FALSE]
    risk_sum <- risk_sum + rowSums(block)
    risk_sum_sq <- risk_sum_sq + rowSums(block * block)
    risk_n <- risk_n + length(group)
    event_columns <- which(events[group])
    event_n <- length(event_columns)
    if (!event_n) next
    event_sum <- rowSums(block[, event_columns, drop = FALSE])
    risk_mean <- risk_sum / risk_n
    score <- score + event_sum - event_n * risk_mean
    information <- information + event_n * pmax(risk_sum_sq / risk_n - risk_mean^2, 0)
  }
  eligible <- is.finite(score) & is.finite(information) & information > 1e-12
  result$z[eligible] <- score[eligible] / sqrt(information[eligible])
  result$p[eligible] <- 2 * pnorm(-abs(result$z[eligible]))
  result$fdr[eligible] <- p.adjust(result$p[eligible], method = "BH")
  result
}

add_survival_columns <- function(output, prefix, matched, survival, min_n, min_events) {
  for (endpoint in ENDPOINTS) {
    stats <- cox_score_test(
      matched$values, matched$patients, survival, endpoint, min_n, min_events
    )
    key <- tolower(endpoint)
    output[[paste0(prefix, "_", key, "_n")]] <- NA_integer_
    output[[paste0(prefix, "_", key, "_events")]] <- NA_integer_
    output[[paste0(prefix, "_", key, "_score_z")]] <- NA_real_
    output[[paste0(prefix, "_", key, "_p_value")]] <- NA_real_
    output[[paste0(prefix, "_", key, "_fdr")]] <- NA_real_
    index <- matched$gene_index
    output[[paste0(prefix, "_", key, "_n")]][index] <- stats$n
    output[[paste0(prefix, "_", key, "_events")]][index] <- stats$events
    output[[paste0(prefix, "_", key, "_score_z")]][index] <- stats$z
    output[[paste0(prefix, "_", key, "_p_value")]][index] <- stats$p
    output[[paste0(prefix, "_", key, "_fdr")]][index] <- stats$fdr
  }
  output
}

project_paths <- function(tcga_root, project) {
  directory <- file.path(tcga_root, "tcga_counts_fpkm_tpm")
  list(
    expression_ensembl = file.path(
      directory, paste0("TCGA-", project, "_tpm_Ensembl_IDl.Rdata")
    ),
    expression_symbol = file.path(
      directory, paste0("TCGA-", project, "_tpm_gene_symbol.Rdata")
    )
  )
}

build_project <- function(project, genes, survival, args, staging_root) {
  paths <- project_paths(args$tcga_root, project)
  missing <- names(paths)[!vapply(paths, function(path) length(path) == 1L && !is.na(path) && file.exists(path), logical(1))]
  if (length(missing)) stop(project, " is missing raw modalities: ", paste(missing, collapse = ", "))

  message("[", project, "] expression")
  expression <- match_expression(paths, genes, project)

  output <- copy(genes)
  output[, `:=`(
    tcga_project = paste0("TCGA-", project),
    depmap_lineage = unname(PROJECT_LINEAGES[[project]]),
    expression_available = FALSE,
    expression_mapping_basis = NA_character_,
    expression_n = NA_integer_, expression_median_log2_tpm = NA_real_
  )]

  expression_index <- expression$gene_index
  output$expression_available[expression_index] <- TRUE
  output$expression_mapping_basis[expression_index] <- expression$mapping_basis
  output$expression_n[expression_index] <- ncol(expression$values)
  output$expression_median_log2_tpm[expression_index] <- apply(expression$values, 1L, median, na.rm = TRUE)

  message("[", project, "] censored survival score tests")
  output <- add_survival_columns(output, "expression", expression, survival, args$min_n, args$min_events)

  project_root <- file.path(staging_root, "projects", paste0("TCGA-", project))
  dir.create(project_root, recursive = TRUE, showWarnings = FALSE)
  parquet_path <- file.path(project_root, "gene_associations.parquet")
  csv_path <- file.path(project_root, "gene_associations.building.csv")
  fwrite(output, csv_path, na = "")
  converter <- if (!is.null(args$converter)) args$converter else {
    file.path(script_directory(), "convert_tcga_csv_to_parquet.py")
  }
  status <- system2(
    if (!is.null(args$python)) args$python else "python3",
    c(shQuote(converter), shQuote(csv_path), shQuote(parquet_path), "--expected-rows", nrow(genes))
  )
  if (!identical(status, 0L) || !file.exists(parquet_path)) {
    stop("Parquet conversion failed for ", project)
  }
  if (!file.remove(csv_path)) stop("failed to remove temporary CSV: ", csv_path)
  manifest <- list(
    schema_version = 1L,
    status = "complete",
    release = "26Q1-TCGA-v1",
    tcga_project = paste0("TCGA-", project),
    depmap_lineage = unname(PROJECT_LINEAGES[[project]]),
    target_gene_count = nrow(genes),
    expression_matched_gene_count = length(expression$gene_index),
    expression_ensembl_matched_gene_count = sum(expression$mapping_basis == "ensembl_gene_id"),
    expression_symbol_fallback_gene_count = sum(expression$mapping_basis == "symbol_fallback"),
    expression_primary_tumour_n = length(expression$patients),
    selected_sample_type_codes = primary_sample_types(project),
    endpoints = ENDPOINTS,
    min_n = args$min_n,
    min_events = args$min_events,
    method = "Breslow univariate Cox score test at beta=0 with BH FDR within project/modality/endpoint",
    inputs = unname(paths),
    output = parquet_path
  )
  write_json(manifest, file.path(project_root, "manifest.json"), auto_unbox = TRUE, pretty = TRUE)
  data.table(
    tcga_project = manifest$tcga_project,
    depmap_lineage = manifest$depmap_lineage,
    target_gene_count = manifest$target_gene_count,
    expression_matched_gene_count = manifest$expression_matched_gene_count,
    expression_primary_tumour_n = manifest$expression_primary_tumour_n,
    status = manifest$status,
    manifest = file.path(project_root, "manifest.json")
  )
}

main <- function() {
  args <- parse_args(commandArgs(trailingOnly = TRUE))
  args$depmap_root <- normalizePath(args$depmap_root, mustWork = TRUE)
  args$tcga_root <- normalizePath(args$tcga_root, mustWork = TRUE)
  output <- normalizePath(args$output, mustWork = FALSE)
  if (file.exists(output)) stop("output already exists: ", output)

  gene_path <- file.path(args$depmap_root, "depmap-26q1-core", "analysis_gene_list.csv")
  survival_path <- file.path(args$tcga_root, "tcga_target_gtex", "TCGA_survival_data.gz")
  if (!file.exists(gene_path)) stop("missing DepMap gene universe: ", gene_path)
  if (!file.exists(survival_path)) stop("missing TCGA survival table: ", survival_path)

  genes <- fread(gene_path, encoding = "UTF-8")
  genes[, symbol := toupper(trimws(symbol))]
  if (nrow(genes) != 18531L || anyDuplicated(genes$symbol)) {
    stop("DepMap analysis gene universe must contain 18,531 unique symbols")
  }
  survival <- read_survival(survival_path)
  projects <- if (identical(tolower(args$projects), "all")) {
    names(PROJECT_LINEAGES)
  } else {
    unique(toupper(trimws(strsplit(args$projects, ",", fixed = TRUE)[[1L]])))
  }
  unknown <- setdiff(projects, names(PROJECT_LINEAGES))
  if (length(unknown)) stop("unsupported TCGA projects: ", paste(unknown, collapse = ", "))

  staging <- paste0(output, ".building-", Sys.getpid())
  if (file.exists(staging)) stop("staging path already exists: ", staging)
  dir.create(file.path(staging, "projects"), recursive = TRUE, showWarnings = FALSE)
  catalog <- rbindlist(lapply(projects, build_project, genes = genes, survival = survival,
                              args = args, staging_root = staging), fill = TRUE)
  fwrite(catalog, file.path(staging, "project_catalog.csv"))
  file.copy(gene_path, file.path(staging, "analysis_gene_list.csv"), overwrite = FALSE)
  qa <- list(
    schema_version = 1L,
    release = "26Q1-TCGA-v1",
    generated_at = format(Sys.time(), "%Y-%m-%dT%H:%M:%S%z"),
    status = "PASS",
    target_gene_count = nrow(genes),
    project_count = nrow(catalog),
    endpoints = ENDPOINTS,
    modality_count = 1L,
    raw_data_mutated = FALSE
  )
  write_json(qa, file.path(staging, "qa.json"), auto_unbox = TRUE, pretty = TRUE)
  if (!file.rename(staging, output)) stop("failed to atomically publish ", output)
  message("published ", output)
}

if (sys.nframe() == 0L) main()
