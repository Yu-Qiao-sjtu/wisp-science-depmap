#!/usr/bin/env Rscript

# Run an R analysis without placing its script name or arguments in the process
# command line. The job specification is supplied through an environment
# variable and must be stored in a private directory.

spec_path <- Sys.getenv("WISP_PRIVATE_JOB_SPEC", unset = "")
if (!nzchar(spec_path)) {
  stop("WISP_PRIVATE_JOB_SPEC is required", call. = FALSE)
}

spec <- readLines(spec_path, warn = FALSE, encoding = "UTF-8")
if (!length(spec) || !nzchar(spec[[1]])) {
  stop("private job specification is empty", call. = FALSE)
}

target_script <- normalizePath(spec[[1]], mustWork = TRUE)
target_args <- if (length(spec) > 1L) spec[-1L] else character()
original_args <- base::commandArgs(trailingOnly = FALSE)

# Sourced analysis scripts see the private specification arguments while the
# operating-system process list only sees this neutral runner path.
commandArgs <- function(trailingOnly = FALSE) {
  if (isTRUE(trailingOnly)) {
    return(target_args)
  }
  c(original_args, "--args", target_args)
}

sys.source(target_script, envir = .GlobalEnv, chdir = TRUE)
