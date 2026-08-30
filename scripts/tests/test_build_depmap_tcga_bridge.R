#!/usr/bin/env Rscript

suppressPackageStartupMessages({
  library(data.table)
  library(survival)
})

args <- commandArgs(trailingOnly = FALSE)
file_arg <- sub("^--file=", "", grep("^--file=", args, value = TRUE)[[1L]])
source(file.path(dirname(normalizePath(file_arg)), "..", "build_depmap_tcga_bridge.R"))

ids <- c(
  "TCGA-AA-0001-01A", "TCGA-AA-0001-01B", "TCGA-AA-0002-11A",
  "TCGA-BB-0001-03A", "TCGA-BB-0001-03B"
)
stopifnot(identical(first_primary_columns(ids, "BRCA"), 1L))
stopifnot(identical(first_primary_columns(ids, "LAML"), 4L))

patients <- sprintf("TCGA-AA-%04d", seq_len(12L))
value <- c(-1.2, -0.8, -0.4, 0.1, 0.3, 0.5, 0.9, 1.2, 1.5, 1.8, 2.1, 2.4)
time <- c(3, 5, 5, 6, 8, 8, 9, 11, 12, 14, 15, 18)
event <- c(1L, 1L, 0L, 1L, 0L, 1L, 1L, 0L, 1L, 0L, 1L, 0L)
clinical <- data.table(patient = patients, OS = event, OS.time = time)
observed <- cox_score_test(
  matrix(value, nrow = 1L), patients, clinical, "OS", min_n = 10L,
  min_events = 3L
)
fit <- coxph(Surv(time, event) ~ value, ties = "breslow")
expected <- sign(unname(coef(fit))) * sqrt(unname(summary(fit)$sctest[["test"]]))
stopifnot(observed$n == 12L, observed$events == sum(event))
stopifnot(isTRUE(all.equal(observed$z[[1L]], expected, tolerance = 1e-10)))

cat("TCGA bridge tests passed\n")
