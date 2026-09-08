#!/usr/bin/env Rscript
suppressPackageStartupMessages(library(data.table))
args <- commandArgs(TRUE)
stopifnot(length(args) == 1L)
x <- fread(file.path(args[[1]], "gene_by_lineage_mutation_menu.csv"))
summary <- x[, .(
  models = max(mut_n + wt_n),
  damaging_standard = sum(matrix == "Damaging" & pass_standard),
  damaging_strict = sum(matrix == "Damaging" & pass_strict),
  hotspot_standard = sum(matrix == "Hotspot" & pass_standard),
  hotspot_strict = sum(matrix == "Hotspot" & pass_strict)
), by = lineage][order(-models, lineage)]
print(summary, nrows = Inf)
