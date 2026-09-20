#!/usr/bin/env Rscript

suppressPackageStartupMessages(library(jsonlite))

args <- commandArgs(trailingOnly = FALSE)
file_arg <- grep("^--file=", args, value = TRUE)
this_file <- normalizePath(sub("^--file=", "", file_arg[[1L]]), winslash = "/", mustWork = TRUE)
skill_root <- normalizePath(file.path(dirname(this_file), ".."), winslash = "/", mustWork = TRUE)
resolver <- file.path(skill_root, "scripts", "resolve_depmap_workspace.R")
coverage <- file.path(skill_root, "references", "knowledge-coverage-manifest.json")

run_resolver <- function(project_root, config_path = NULL) {
  command_args <- c(
    resolver,
    "--project-root", project_root,
    "--coverage-manifest", coverage
  )
  if (!is.null(config_path)) command_args <- c(command_args, "--config", config_path)
  output <- system2("Rscript", command_args, stdout = TRUE, stderr = FALSE)
  fromJSON(paste(output, collapse = "\n"), simplifyVector = FALSE)
}

root <- tempfile("depmap-resolver-")
dir.create(file.path(root, ".wisp"), recursive = TRUE)

write_json(
  list(
    schema_version = 2,
    knowledge = list(
      provider = "remote",
      endpoint = "http://127.0.0.1:18876/api/v1",
      release = "26Q1"
    ),
    analysis_root = "analysis/depmap-agent"
  ),
  file.path(root, ".wisp", "depmap-agent.json"),
  auto_unbox = TRUE,
  pretty = TRUE
)

remote <- run_resolver(root)
stopifnot(identical(remote$schema_version, 2L))
stopifnot(identical(remote$status, "needs_probe"))
stopifnot(identical(remote$knowledge$provider, "remote"))
stopifnot(identical(remote$knowledge$health, "unverified"))
stopifnot(identical(remote$knowledge$query_ready, FALSE))
stopifnot(identical(remote$knowledge$transport, "configured_endpoint"))
stopifnot(identical(remote$blocking_failures, list()))

config_path <- file.path(root, ".wisp", "depmap-agent.json")
tunnel_config <- fromJSON(config_path, simplifyVector = FALSE)
tunnel_config$knowledge$tunnel <- list(
  enabled = TRUE,
  context_id = "ssh:lab-server",
  local_port = 18876L,
  remote_port = 8876L
)
write_json(tunnel_config, config_path, auto_unbox = TRUE, pretty = TRUE)
blocked_tunnel <- run_resolver(root)
stopifnot(identical(blocked_tunnel$status, "blocked"))
stopifnot("managed_tunnel_not_supported" %in% unlist(blocked_tunnel$blocking_failures))

write_json(
  list(schema_version = 2, knowledge = list(provider = "remote")),
  file.path(root, ".wisp", "depmap-agent.json"),
  auto_unbox = TRUE,
  pretty = TRUE
)
missing_endpoint <- run_resolver(root)
stopifnot(identical(missing_endpoint$status, "blocked"))
stopifnot("knowledge_endpoint_missing" %in% unlist(missing_endpoint$blocking_failures))
stopifnot(identical(missing_endpoint$fallback$release, "26Q1"))
stopifnot(identical(missing_endpoint$fallback$knowledge_query_status, "MODULE_UNAVAILABLE"))
stopifnot(identical(missing_endpoint$fallback$state, "new_analysis_proposed"))
stopifnot(identical(missing_endpoint$fallback$authorized_skills$acquisition, "public-data-access"))
stopifnot(identical(missing_endpoint$fallback$authorized_skills$analysis, "depmap-coding-agent"))
stopifnot(identical(missing_endpoint$fallback$provider_policy$live_portal_api_is_query_provider, FALSE))
stopifnot(any(vapply(missing_endpoint$fallback$files, function(x) identical(x$path, "data/Model.csv"), logical(1L))))

write_json(
  list(schema_version = 2, knowledge = list(provider = "local")),
  file.path(root, ".wisp", "depmap-agent.json"),
  auto_unbox = TRUE,
  pretty = TRUE
)
missing_knowledge <- run_resolver(root)
stopifnot(identical(missing_knowledge$status, "blocked"))
stopifnot("knowledge_root_missing" %in% unlist(missing_knowledge$blocking_failures))
stopifnot(identical(missing_knowledge$fallback$knowledge_query_status, "MODULE_UNAVAILABLE"))
stopifnot(identical(missing_knowledge$fallback$state, "new_analysis_proposed"))

unlink(root, recursive = TRUE)
cat("test_resolve_depmap_workspace: ok\n")
