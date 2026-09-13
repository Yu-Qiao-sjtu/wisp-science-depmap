#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table); library(jsonlite)})
atomic_save_rds <- function(value, path) {
  temp <- paste0(path, ".tmp.", Sys.getpid())
  on.exit(unlink(temp), add = TRUE)
  saveRDS(value, temp, compress = "xz")
  if (!file.rename(temp, path)) stop("failed to publish checkpoint: ", path)
}
valid_block <- function(path, source_start, source_end) {
  if (!file.exists(path)) return(FALSE)
  tryCatch({
    value <- readRDS(path)
    identical(value$source_start, source_start) && identical(value$source_end, source_end) &&
      is.matrix(value$correlation) && nrow(value$correlation) == source_end-source_start+1L
  }, error = function(...) FALSE)
}
args <- commandArgs(trailingOnly = TRUE)
arg <- function(name, default = NULL) {x <- grep(paste0("^--", name, "="), args, value=TRUE); if(!length(x)) default else sub(paste0("^--",name,"="),"",x[[1]])}
data_root <- normalizePath(arg("data-root"), mustWork=TRUE)
knowledge_root <- normalizePath(arg("knowledge-root"), mustWork=TRUE)
block_size <- as.integer(arg("block-size", "128"))
lineage_adjust <- tolower(arg("lineage-adjust", "true")) %chin% c("true", "1", "yes")
out_root <- file.path(knowledge_root, "depmap-26q1-3d", "codependency")
dir.create(out_root, recursive=TRUE, showWarnings=FALSE)
meta <- fread(file.path(data_root,"screen_metadata.csv"))[PassesQC==TRUE]
x <- fread(file.path(data_root,"screen_gene_effect.csv"), check.names=FALSE)
ids <- x[[1]]; x[[1]] <- NULL
genes <- toupper(sub(" \\([^()]++\\)$","",names(x),perl=TRUE))
effect <- as.matrix(x); storage.mode(effect)<-"double"; rownames(effect)<-ids; colnames(effect)<-genes; rm(x)
meta <- meta[match(rownames(effect),ScreenID)]
cohorts <- list(all_3d_lineage_adjusted=c("3DO","3DN"), organoid_3do_lineage_adjusted="3DO", cns_3dn_exploratory="3DN")
catalog <- list()
for (key in names(cohorts)) {
 idx <- which(meta$ScreenType %chin% cohorts[[key]])
 mat <- effect[idx,,drop=FALSE]; info <- meta[idx]
 complete <- colSums(is.finite(mat))==nrow(mat); mat <- mat[,complete,drop=FALSE]; g <- genes[complete]
 adjusted <- lineage_adjust && grepl("lineage_adjusted",key) && uniqueN(info$OncotreeLineage)>1L
 if (adjusted) {
   design <- model.matrix(~0+factor(info$OncotreeLineage))
   mat <- mat - design %*% qr.coef(qr(design),mat)
   residual_df <- nrow(mat)-qr(design)$rank-2L
 } else residual_df <- nrow(mat)-2L
 mat <- scale(mat); mat[,!is.finite(colSums(mat))] <- 0
 cohort_root <- file.path(out_root,key); block_root <- file.path(cohort_root,"blocks")
 dir.create(block_root,recursive=TRUE,showWarnings=FALSE)
 fwrite(data.table(gene_index=seq_along(g),symbol=g),file.path(cohort_root,"gene_order.csv"))
 starts <- seq.int(1L,ncol(mat),by=block_size)
 for (bi in seq_along(starts)) {
   s<-starts[[bi]]; e<-min(s+block_size-1L,ncol(mat)); path<-file.path(block_root,sprintf("block_%05d_%05d.rds",s,e))
   if(valid_block(path,s,e)) next
   if(file.exists(path)) unlink(path)
   cor_block <- crossprod(mat[,s:e,drop=FALSE],mat)/pmax(nrow(mat)-1L,1L)
   atomic_save_rds(list(source_start=s,source_end=e,correlation=cor_block,pair_n=nrow(mat),residual_df=residual_df),path)
   if(bi%%10L==0L||bi==length(starts)) message(sprintf("%s [%d/%d]",key,bi,length(starts)))
 }
 manifest<-list(schema_version=1,release="NextGen Model Manuscript 2026",family="3d_codependency",cohort=key,status="complete",
   screen_types=cohorts[[key]],screen_count=nrow(mat),gene_count=ncol(mat),lineage_count=uniqueN(info$OncotreeLineage),
   lineage_adjusted=adjusted,residual_df=residual_df,method="Pearson correlation of complete-coverage Screen Gene Effect; optional fixed-effect residualization by OncotreeLineage",
   interpretation="co-dependency association, not causality")
 write_json(manifest,file.path(cohort_root,"manifest.json"),auto_unbox=TRUE,pretty=TRUE)
 catalog[[key]]<-data.table(cohort=key,screen_count=nrow(mat),gene_count=ncol(mat),lineage_count=uniqueN(info$OncotreeLineage),lineage_adjusted=adjusted,status="complete")
}
catalog<-rbindlist(catalog); fwrite(catalog,file.path(out_root,"cohort_catalog.csv"))
write_json(list(schema_version=1,release="NextGen Model Manuscript 2026",family="3d_codependency",status="complete",cohort_count=nrow(catalog)),file.path(out_root,"manifest.json"),auto_unbox=TRUE,pretty=TRUE)
