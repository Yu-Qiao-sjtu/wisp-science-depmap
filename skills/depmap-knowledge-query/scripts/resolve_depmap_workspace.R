#!/usr/bin/env Rscript

suppressPackageStartupMessages(library(jsonlite))

args <- commandArgs(trailingOnly = TRUE)
arg_value <- function(name, default = NULL) {
  index <- match(name, args)
  if (is.na(index) || index == length(args)) return(default)
  args[[index + 1L]]
}

portable <- function(path) gsub("\\\\", "/", path)
is_absolute <- function(path) grepl("^([A-Za-z]:[/\\\\]|/)", path)
resolve_path <- function(value, base, must_work = FALSE) {
  if (is.null(value) || !nzchar(trimws(value))) return(NULL)
  candidate <- path.expand(value)
  if (!is_absolute(candidate)) candidate <- file.path(base, candidate)
  portable(normalizePath(candidate, winslash = "/", mustWork = must_work))
}
inside <- function(path, root) {
  if (is.null(path) || is.null(root)) return(FALSE)
  path <- tolower(sub("/+$", "", portable(path)))
  root <- tolower(sub("/+$", "", portable(root)))
  identical(path, root) || startsWith(path, paste0(root, "/"))
}

project_root <- resolve_path(arg_value("--project-root", "."), getwd(), TRUE)
config_arg <- arg_value("--config", file.path(project_root, ".wisp", "depmap-agent.json"))
config_path <- resolve_path(config_arg, project_root, FALSE)
script_arg <- grep("^--file=", commandArgs(trailingOnly = FALSE), value = TRUE)
script_path <- if (length(script_arg)) sub("^--file=", "", script_arg[[1L]]) else NULL
script_dir <- if (is.null(script_path)) getwd() else dirname(resolve_path(script_path, getwd(), FALSE))
coverage_arg <- arg_value(
  "--coverage-manifest",
  file.path(script_dir, "..", "references", "knowledge-coverage-manifest.json")
)
coverage_path <- resolve_path(coverage_arg, project_root, FALSE)
config <- list()
warnings <- character()
if (file.exists(config_path)) {
  config <- tryCatch(
    read_json(config_path, simplifyVector = TRUE),
    error = function(error) stop("Invalid DepMap workspace config: ", conditionMessage(error))
  )
}

pick <- function(env_name, config_name, nested_name = NULL) {
  env_value <- Sys.getenv(env_name, unset = "")
  if (nzchar(env_value)) return(list(value = env_value, source = paste0("env:", env_name)))
  if (!is.null(nested_name) && is.list(config$knowledge)) {
    nested_value <- config$knowledge[[nested_name]]
    if (!is.null(nested_value) && length(nested_value) == 1L && nzchar(nested_value)) {
      return(list(value = nested_value, source = "project_config:knowledge"))
    }
  }
  value <- config[[config_name]]
  if (!is.null(value) && length(value) == 1L && nzchar(value)) {
    return(list(value = value, source = "project_config"))
  }
  list(value = NULL, source = NULL)
}

provider_pick <- pick("DEPMAP_KNOWLEDGE_PROVIDER", "knowledge_provider", "provider")
provider <- tolower(trimws(if (is.null(provider_pick$value)) "local" else provider_pick$value))
if (!provider %in% c("local", "remote")) {
  stop("knowledge provider must be 'local' or 'remote'")
}
endpoint_pick <- pick("DEPMAP_KNOWLEDGE_ENDPOINT", "knowledge_endpoint", "endpoint")
release_pick <- pick("DEPMAP_KNOWLEDGE_RELEASE", "knowledge_release", "release")

knowledge_pick <- pick("DEPMAP_KNOWLEDGE_ROOT", "knowledge_root", "root")
if (identical(provider, "local") && is.null(knowledge_pick$value)) {
  candidates <- c(project_root, file.path(project_root, "knowledge"))
  hit <- candidates[file.exists(file.path(candidates, "depmap-26q1-qa.json"))]
  if (length(hit)) {
    knowledge_pick <- list(value = hit[[1L]], source = "auto_detected")
  }
}
knowledge_root <- resolve_path(knowledge_pick$value, project_root, FALSE)

data_pick <- pick("DEPMAP_DATA_ROOT", "data_root")
if (is.null(data_pick$value)) {
  candidate <- file.path(project_root, "data")
  if (dir.exists(candidate)) data_pick <- list(value = candidate, source = "project_default")
}
if (is.null(data_pick$value) && !is.null(knowledge_root) && identical(knowledge_root, project_root)) {
  legacy <- file.path(dirname(project_root), "data")
  if (dir.exists(legacy)) {
    data_pick <- list(value = legacy, source = "legacy_parent_inference")
    warnings <- c(warnings, paste(
      "Raw data were inferred from ../data because the project root is the knowledge directory;",
      "set data_root explicitly before recomputation."
    ))
  }
}
data_root <- resolve_path(data_pick$value, project_root, FALSE)

analysis_pick <- pick("DEPMAP_ANALYSIS_ROOT", "analysis_root")
if (is.null(analysis_pick$value)) {
  if (!is.null(knowledge_root) && identical(knowledge_root, project_root)) {
    analysis_pick <- list(
      value = file.path(dirname(project_root), "analysis", "depmap-agent"),
      source = "safe_sibling_default"
    )
  } else {
    analysis_pick <- list(
      value = file.path(project_root, "analysis", "depmap-agent"),
      source = "project_default"
    )
  }
}
analysis_root <- resolve_path(analysis_pick$value, project_root, FALSE)

qa_path <- if (is.null(knowledge_root)) NULL else file.path(knowledge_root, "depmap-26q1-qa.json")
qa <- if (!is.null(qa_path) && file.exists(qa_path)) {
  tryCatch(read_json(qa_path, simplifyVector = TRUE), error = function(error) NULL)
} else NULL
qa_pass <- !is.null(qa) && identical(toupper(qa$qa_status), "PASS")
coverage <- if (!is.null(coverage_path) && file.exists(coverage_path)) {
  tryCatch(read_json(coverage_path, simplifyVector = FALSE), error = function(error) NULL)
} else NULL
coverage_checks <- list()
available_families <- character()
if (identical(provider, "local") && !is.null(coverage) && !is.null(knowledge_root)) {
  for (family in coverage$query_families) {
    required <- unlist(family$required_paths, use.names = FALSE)
    modules <- unlist(family$modules, use.names = FALSE)
    template <- family$required_path_template
    if (length(modules) && !is.null(template)) {
      required <- c(required, vapply(
        modules,
        function(module) sub("\\{module\\}", module, template),
        character(1L)
      ))
    }
    required <- required[nzchar(required)]
    missing <- required[!file.exists(file.path(knowledge_root, required))]
    available <- length(required) > 0L && !length(missing)
    coverage_checks[[length(coverage_checks) + 1L]] <- list(
      id = family$id,
      mode = family$mode,
      available = available,
      missing_paths = as.list(unname(missing))
    )
    if (available) available_families <- c(available_families, family$id)
  }
}
write_boundary_pass <- !is.null(analysis_root) &&
  (is.null(knowledge_root) || !inside(analysis_root, knowledge_root)) &&
  (is.null(data_root) || !inside(analysis_root, data_root))

blocking <- character()
if (identical(provider, "local")) {
  if (is.null(knowledge_root) || !dir.exists(knowledge_root)) {
    blocking <- c(blocking, "knowledge_root_missing")
  } else if (!qa_pass) {
    blocking <- c(blocking, "knowledge_qa_not_pass")
  }
} else if (is.null(endpoint_pick$value) || !nzchar(endpoint_pick$value)) {
  blocking <- c(blocking, "knowledge_endpoint_missing")
}
if (!write_boundary_pass) blocking <- c(blocking, "analysis_root_overlaps_read_only_source")

remote_needs_probe <- identical(provider, "remote") && !length(blocking)
tunnel <- if (is.list(config$knowledge) && is.list(config$knowledge$tunnel)) {
  config$knowledge$tunnel
} else {
  NULL
}
tunnel_enabled <- !is.null(tunnel) && !identical(tunnel$enabled, FALSE)
tunnel_access_authorized <- tunnel_enabled && identical(tunnel$access_authorized, TRUE)

result <- list(
  schema_version = 2,
  status = if (length(blocking)) {
    "blocked"
  } else if (tunnel_enabled && !tunnel_access_authorized) {
    "awaiting_access"
  } else if (remote_needs_probe) {
    "needs_probe"
  } else {
    "ready"
  },
  project_root = project_root,
  config_path = config_path,
  config_exists = file.exists(config_path),
  knowledge = list(
    provider = provider,
    root = if (is.null(knowledge_root)) NA_character_ else knowledge_root,
    source = if (is.null(knowledge_pick$source)) NA_character_ else knowledge_pick$source,
    endpoint = if (is.null(endpoint_pick$value)) NA_character_ else endpoint_pick$value,
    endpoint_source = if (is.null(endpoint_pick$source)) NA_character_ else endpoint_pick$source,
    transport = if (tunnel_enabled) "managed_ssh_tunnel" else "https",
    tunnel = if (!tunnel_enabled) NULL else list(
      configured = TRUE,
      context_id = tunnel$context_id,
      local_port = tunnel$local_port,
      remote_port = tunnel$remote_port,
      access_authorized = tunnel_access_authorized,
      connection_attempted = FALSE
    ),
    qa_path = if (is.null(qa_path)) NULL else portable(qa_path),
    qa_status = if (is.null(qa)) NULL else qa$qa_status,
    release = if (!is.null(qa)) qa$release else release_pick$value,
    query_ready = identical(provider, "local") && qa_pass,
    health = if (identical(provider, "local")) {
      if (qa_pass) "qa_pass" else "unavailable"
    } else {
      "unverified"
    },
    coverage_manifest_path = coverage_path,
    coverage_manifest_loaded = !is.null(coverage),
    available_query_families = as.list(available_families),
    coverage_checks = coverage_checks
  ),
  data = list(
    root = data_root,
    source = data_pick$source,
    available = !is.null(data_root) && dir.exists(data_root),
    recompute_requires_explicit_transition = TRUE
  ),
  analysis = list(
    root = analysis_root,
    source = analysis_pick$source,
    write_boundary_pass = write_boundary_pass
  ),
  blocking_failures = blocking,
  warnings = warnings
)

output <- arg_value("--output")
json <- toJSON(result, auto_unbox = TRUE, pretty = TRUE, na = "null")
if (!is.null(output)) {
  output_path <- resolve_path(output, project_root, FALSE)
  dir.create(dirname(output_path), recursive = TRUE, showWarnings = FALSE)
  writeLines(json, output_path, useBytes = TRUE)
}
cat(json, "\n")
