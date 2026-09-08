#!/usr/bin/env Rscript

suppressPackageStartupMessages(library(data.table))

args <- commandArgs(trailingOnly = TRUE)
if (length(args) != 1L) stop("usage: Rscript validate_esr1_foxa1.R CRISPRGeneEffect.csv")

d <- fread(args[[1L]], select = c("ESR1 (2099)", "FOXA1 (3169)"))
ok <- complete.cases(d)
test <- cor.test(d[[1L]][ok], d[[2L]][ok], method = "pearson")
cat(sprintf(
  "ESR1-FOXA1 raw recomputation: r=%.16f n=%d p=%.16e\n",
  unname(test$estimate), sum(ok), test$p.value
))
