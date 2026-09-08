#!/usr/bin/env Rscript

suppressPackageStartupMessages(library(data.table))

args <- commandArgs(trailingOnly = TRUE)
if (!length(args) %in% c(2L, 3L)) {
  stop("usage: Rscript compare_tm00_current_esr1_foxa1.R CRISPRGeneEffect.csv EXPRESSION_SAMPLE_ORDER.csv [Model.csv]")
}

effect <- fread(args[[1L]], select = c("V1", "ESR1 (2099)", "FOXA1 (3169)"))
setnames(effect, c("ModelID", "ESR1", "FOXA1"))
expression_samples <- fread(args[[2L]])
if (!"sample_id" %in% names(expression_samples)) stop("expression sample order must contain sample_id")
common <- intersect(effect$ModelID, expression_samples$sample_id)
if (length(args) == 3L) {
  model <- fread(args[[3L]], select = "ModelID")
  common <- intersect(common, model$ModelID)
}
tm00 <- effect[match(common, ModelID)]
ok <- complete.cases(tm00[, .(ESR1, FOXA1)])
test <- cor.test(tm00$ESR1[ok], tm00$FOXA1[ok], method = "pearson")

cat(sprintf(
  "TM00-01 intersection: common_models=%d complete_pairs=%d r=%.16f df=%d p=%.16e CI=[%.7f, %.7f]\n",
  length(common), sum(ok), unname(test$estimate), unname(test$parameter),
  test$p.value, test$conf.int[[1L]], test$conf.int[[2L]]
))
