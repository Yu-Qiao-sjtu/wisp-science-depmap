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
      release = "26Q1",
      tunnel = list(
        enabled = TRUE,
        context_id = "ssh:lab-server",
        local_port = 18876L,
        remote_port = 8876L,
        access_authorized = TRUE
      )
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
stopifnot(identical(remote$knowledge$transport, "managed_ssh_tunnel"))
stopifnot(identical(remote$knowledge$tunnel$context_id, "ssh:lab-server"))
stopifnot(identical(remote$knowledge$tunnel$access_authorized, TRUE))
stopifnot(identical(remote$knowledge$tunnel$connection_attempted, FALSE))
stopifnot(identical(remote$blocking_failures, list()))

config_path <- file.path(root, ".wisp", "depmap-agent.json")
pending_config <- fromJSON(config_path, simplifyVector = FALSE)
pending_config$knowledge$tunnel$access_authorized <- FALSE
write_json(pending_config, config_path, auto_unbox = TRUE, pretty = TRUE)
pending <- run_resolver(root)
stopifnot(identical(pending$status, "awaiting_access"))
stopifnot(identical(pending$knowledge$tunnel$connection_attempted, FALSE))

write_json(
  list(schema_version = 2, knowledge = list(provider = "remote")),
  file.path(root, ".wisp", "depmap-agent.json"),
  auto_unbox = TRUE,
  pretty = TRUE
)
missing_endpoint <- run_resolver(root)
stopifnot(identical(missing_endpoint$status, "blocked"))
stopifnot("knowledge_endpoint_missing" %in% unlist(missing_endpoint$blocking_failures))

unlink(root, recursive = TRUE)
cat("test_resolve_depmap_workspace: ok\n")
