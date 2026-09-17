#!/usr/bin/env Rscript
suppressPackageStartupMessages({ library(data.table); library(jsonlite) })
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 3L) stop("用法: query_tf_dependency.R <result-root> <TF> <target|TOP> [limit]")
root <- normalizePath(args[[1L]], winslash = "/", mustWork = TRUE)
tf <- toupper(args[[2L]])
target <- toupper(args[[3L]])
limit <- if (length(args) >= 4L) as.integer(args[[4L]]) else 20L
manifest <- read_json(file.path(root, "manifest.json"), simplifyVector = TRUE)
if (!identical(manifest$status, "complete")) stop("结果manifest不是complete")
order <- fread(file.path(root, "tf_order.csv"))
idx <- match(tf, toupper(order$TF))
if (is.na(idx)) stop("TF不存在: ", tf)
files <- list.files(file.path(root, "blocks"), pattern = "[.]rds$", full.names = TRUE)
hit <- files[vapply(files, function(p) { x <- readRDS(p); idx %in% x$tf_index }, logical(1))]
if (length(hit) != 1L) stop("无法唯一定位TF block")
x <- readRDS(hit)
i <- match(idx, x$tf_index)
if (target == "TOP") {
  out <- fread(file.path(root, "top_hits.csv.gz"))[toupper(TF) == tf]
  setorder(out, direction, rank)
  print(out[rank <= limit])
} else {
  j <- match(target, toupper(x$target_genes))
  if (is.na(j)) stop("靶基因不存在: ", target)
  print(data.table(TF = x$tf_names[i], target_gene = x$target_genes[j], correlation = x$correlation[i,j],
                   pair_n = x$pair_n[i,j], eligible_for_fdr = x$pair_n[i,j] >= manifest$minimum_pair_n_for_fdr_and_ranking,
                   p_value = x$p_value[i,j], fdr = x$fdr[i,j]))
}
