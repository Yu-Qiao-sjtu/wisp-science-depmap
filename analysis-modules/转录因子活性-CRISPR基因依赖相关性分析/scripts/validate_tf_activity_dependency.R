#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table);library(jsonlite)})
a <- commandArgs(trailingOnly=TRUE)
if(length(a)!=1L) stop('用法: validate_tf_activity_dependency.R <result-root>')
root <- normalizePath(a[[1L]], winslash='/', mustWork=TRUE)
manifest <- read_json(file.path(root,'manifest.json'), simplifyVector=TRUE)
stopifnot(identical(manifest$status,'complete'))
tf <- fread(file.path(root,'tf_order.csv'))
target <- fread(file.path(root,'target_gene_order.csv'))
blocks <- list.files(file.path(root,'blocks'), '^block_[0-9]+_[0-9]+[.]rds$', full.names=TRUE)
stopifnot(nrow(tf)==manifest$tf_count, nrow(target)==manifest$target_gene_count,
          length(blocks)==manifest$block_count)
seen <- integer(); cell_count <- 0
for(p in blocks){
  x <- readRDS(p)
  stopifnot(identical(dim(x$correlation),dim(x$pair_n)), identical(dim(x$correlation),dim(x$p_value)), identical(dim(x$correlation),dim(x$fdr)))
  stopifnot(ncol(x$correlation)==nrow(target), nrow(x$correlation)==length(x$tf_index))
  seen <- c(seen,x$tf_index); cell_count <- cell_count + length(x$correlation)
  stopifnot(all(is.na(x$fdr[x$pair_n < manifest$minimum_pair_n_for_fdr_and_ranking])))
}
stopifnot(identical(sort(seen),seq_len(nrow(tf))), cell_count==nrow(tf)*nrow(target))
top <- fread(file.path(root,'top_hits.csv.gz'))
strict <- fread(file.path(root,'top_hits_fdr05_absr02.csv.gz'))
stopifnot(nrow(top)==nrow(tf)*2L*manifest$top_per_direction,
          all(top$pair_n>=manifest$minimum_pair_n_for_fdr_and_ranking),
          all(strict$fdr<=0.05), all(abs(strict$correlation)>=0.2))
report <- list(status='PASS',release=manifest$release,common_model_count=manifest$common_model_count,
               tf_count=nrow(tf),target_gene_count=nrow(target),pair_count=cell_count,
               block_count=length(blocks),top_hit_count=nrow(top),strict_hit_count=nrow(strict),
               minimum_pair_n_for_fdr_and_ranking=manifest$minimum_pair_n_for_fdr_and_ranking)
write_json(report,file.path(root,'validation.json'),auto_unbox=TRUE,pretty=TRUE)
cat(toJSON(report,auto_unbox=TRUE,pretty=TRUE),'\n')
