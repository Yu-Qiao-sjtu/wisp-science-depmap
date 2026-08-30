#!/usr/bin/env Rscript

# Read-only, script-audited DepMap dependency resolver. It reads the compiled
# manifest plus file metadata and release lines; it never loads matrix rows.

args <- commandArgs(trailingOnly = TRUE)

arg_value <- function(name, default = NULL) {
  index <- match(name, args)
  if (is.na(index) || index == length(args)) return(default)
  args[[index + 1L]]
}

`%||%` <- function(left, right) if (is.null(left)) right else left

if (!requireNamespace("jsonlite", quietly = TRUE)) {
  stop("The DepMap dependency resolver requires the R package 'jsonlite'.")
}

script_arg <- grep("^--file=", commandArgs(trailingOnly = FALSE), value = TRUE)
if (length(script_arg) != 1L) stop("Cannot resolve the inspector script path.")
script_path <- normalizePath(sub("^--file=", "", script_arg), winslash = "/", mustWork = TRUE)
skill_root <- dirname(dirname(script_path))
manifest_path <- file.path(skill_root, "references", "capability-manifest.json")
manifest <- jsonlite::read_json(manifest_path, simplifyVector = FALSE)

project_root <- normalizePath(arg_value("--project-root", "."), winslash = "/", mustWork = TRUE)
selected_capability <- arg_value("--capability")
selected_script <- arg_value("--script")
if (!is.null(selected_capability) && !is.null(selected_script)) {
  stop("Use only one of --capability or --script.")
}
output_arg <- arg_value("--output", "analysis/depmap-agent/project-inspection.json")
output_path <- if (grepl("^([A-Za-z]:|/)", output_arg)) output_arg else file.path(project_root, output_arg)

by_id <- function(items) setNames(items, vapply(items, function(item) item$id, character(1L)))
datasets <- by_id(manifest$datasets)
capabilities <- by_id(manifest$capabilities)

if (!is.null(selected_script)) {
  matching <- Filter(function(cap) selected_script %in% unlist(cap$scripts), capabilities)
  if (length(matching) != 1L) {
    stop(sprintf("Script '%s' does not map to exactly one capability.", selected_script))
  }
  selected_capability <- matching[[1L]]$id
}
if (!is.null(selected_capability) && is.null(capabilities[[selected_capability]])) {
  stop(sprintf("Unknown capability '%s'.", selected_capability))
}

portable_path <- function(path) gsub("\\\\", "/", path)
dataset_state <- function(dataset) {
  path <- file.path(project_root, dataset$path)
  exists <- file.exists(path)
  info <- if (exists) file.info(path) else NULL
  list(
    id = dataset$id,
    kind = dataset$kind,
    path = portable_path(dataset$path),
    exists = exists,
    bytes = if (is.null(info)) NA_real_ else unname(info$size),
    producer_scripts = unlist(dataset$producer_scripts %||% list())
  )
}

resolve_capability <- function(capability) {
  required <- character()
  missing_derived <- character()
  missing_source <- character()
  derived_depths <- list()

  visit <- function(dataset_id, depth = 0L) {
    if (dataset_id %in% required) return(invisible(NULL))
    dataset <- datasets[[dataset_id]]
    if (is.null(dataset)) stop(sprintf("Manifest references unknown dataset '%s'.", dataset_id))
    required <<- c(required, dataset_id)
    state <- dataset_state(dataset)
    if (isTRUE(state$exists)) return(invisible(NULL))
    if (identical(dataset$kind, "derived")) {
      missing_derived <<- c(missing_derived, dataset_id)
      derived_depths[[dataset_id]] <<- depth
      for (source_id in unlist(dataset$source_inputs %||% list())) visit(source_id, depth + 1L)
    } else {
      missing_source <<- c(missing_source, dataset_id)
    }
    invisible(NULL)
  }

  for (dataset_id in unlist(capability$inputs)) visit(dataset_id)
  required <- unique(required)
  missing_derived <- unique(missing_derived)
  missing_source <- unique(missing_source)
  # Pick the smallest set of audited producer scripts that can materialize the
  # missing derived inputs. A producer that creates several required caches
  # (07 creates mutation, dependency and cell-info objects together) wins over
  # unrelated one-output producers. Then order upstream producers first.
  uncovered <- missing_derived
  selected_producers <- character()
  while (length(uncovered) > 0L) {
    candidates <- unique(unlist(lapply(uncovered, function(id) datasets[[id]]$producer_scripts %||% list())))
    if (length(candidates) == 0L) break
    coverage <- vapply(candidates, function(producer) {
      sum(vapply(uncovered, function(id) producer %in% unlist(datasets[[id]]$producer_scripts %||% list()), logical(1L)))
    }, integer(1L))
    chosen <- candidates[[which.max(coverage)]]
    selected_producers <- c(selected_producers, chosen)
    uncovered <- Filter(function(id) !(chosen %in% unlist(datasets[[id]]$producer_scripts %||% list())), uncovered)
  }
  producer_depth <- function(producer) {
    covered <- Filter(function(id) producer %in% unlist(datasets[[id]]$producer_scripts %||% list()), missing_derived)
    if (length(covered) == 0L) return(0L)
    max(vapply(covered, function(id) as.integer(derived_depths[[id]] %||% 0L), integer(1L)))
  }
  # Larger recursion depth means closer to raw source and must execute first.
  preparation_scripts <- selected_producers[order(
    -vapply(selected_producers, producer_depth, integer(1L)),
    match(selected_producers, selected_producers)
  )]
  analysis_scripts <- unlist(capability$scripts)
  states <- lapply(required, function(id) dataset_state(datasets[[id]]))
  raw_bytes <- sum(vapply(states, function(state) {
    if (state$kind %in% c("raw", "user_input") && isTRUE(state$exists)) state$bytes else 0
  }, numeric(1L)), na.rm = TRUE)
  status <- if (length(missing_source) > 0L) {
    "missing_inputs"
  } else if (length(missing_derived) > 0L) {
    "preprocessing_required"
  } else {
    "ready"
  }
  list(
    id = capability$id,
    title = capability$title,
    status = status,
    reference_scripts = analysis_scripts,
    execution_plan = unique(c(preparation_scripts, analysis_scripts)),
    preparation_scripts = preparation_scripts,
    required_datasets = states,
    missing_derived = missing_derived,
    missing_inputs = missing_source,
    referenced_source_bytes = raw_bytes
  )
}

reference_script_root <- file.path(project_root, manifest$audit_basis$script_root)
available_scripts <- if (dir.exists(reference_script_root)) {
  sort(list.files(reference_script_root, pattern = "\\.[Rr]$", full.names = FALSE))
} else character()
expected_scripts <- sort(unique(unlist(lapply(manifest$capabilities, function(cap) cap$scripts))))

readme_path <- file.path(project_root, "data", "README.txt")
release_line <- if (file.exists(readme_path)) {
  lines <- readLines(readme_path, n = 12L, warn = FALSE)
  matches <- grep("DepMap Public|DepMap Release|26Q1", lines, value = TRUE)
  if (length(matches) > 0L) trimws(matches[[1L]]) else NA_character_
} else NA_character_

plans <- if (is.null(selected_capability)) {
  lapply(manifest$capabilities, resolve_capability)
} else {
  list(resolve_capability(capabilities[[selected_capability]]))
}

all_source_ids <- unique(unlist(lapply(manifest$datasets, function(dataset) {
  if (dataset$kind %in% c("raw", "user_input")) dataset$id else NULL
})))
source_states <- lapply(all_source_ids, function(id) dataset_state(datasets[[id]]))

inspection <- list(
  schema_version = 2L,
  audit_release = manifest$audit_basis$release,
  observed_release = release_line,
  project_root = project_root,
  manifest_path = normalizePath(manifest_path, winslash = "/", mustWork = TRUE),
  reference_script_root = portable_path(manifest$audit_basis$script_root),
  script_audit = list(
    expected = expected_scripts,
    available = available_scripts,
    missing = setdiff(expected_scripts, available_scripts),
    unexpected = setdiff(available_scripts, expected_scripts)
  ),
  selected_capability = selected_capability,
  capabilities = plans,
  source_inventory = source_states,
  context_policy = list(
    reads = c("manifest", "file metadata", "README release lines"),
    does_not_read = c("matrix rows", "RDS contents", "complete logs"),
    model_receives = "compact JSON plan and artifact references only"
  )
)

dir.create(dirname(output_path), recursive = TRUE, showWarnings = FALSE)
jsonlite::write_json(inspection, output_path, pretty = TRUE, auto_unbox = TRUE, na = "null")
cat(normalizePath(output_path, winslash = "/", mustWork = TRUE), "\n")
