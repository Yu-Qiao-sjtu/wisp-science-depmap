#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table); library(jsonlite)})
args <- commandArgs(TRUE); stopifnot(length(args)==2)
src <- normalizePath(args[1]); out <- args[2]; dir.create(out,recursive=TRUE,showWarnings=FALSE)
clean <- function(x) sub(" \\(\\d+\\)$", "", x)
read_matrix <- function(path) {
  x <- fread(path); id <- as.character(x$ModelID)
  x <- x[IsDefaultEntryForModel=="Yes"]; x <- x[!duplicated(ModelID)]
  meta <- intersect(c("V1","SequencingID","ModelID","ModelConditionID","IsDefaultEntryForModel","IsDefaultEntryForMC"),names(x))
  genes <- clean(setdiff(names(x),meta)); m <- as.matrix(x[,setdiff(names(x),meta),with=FALSE]); storage.mode(m)<-"double"
  list(ids=as.character(x$ModelID), genes=genes, m=m)
}
dam <- read_matrix(file.path(src,"OmicsSomaticMutationsMatrixDamaging.csv"))
hot <- read_matrix(file.path(src,"OmicsSomaticMutationsMatrixHotspot.csv"))
model <- fread(file.path(src,"Model.csv")); model <- model[!duplicated(ModelID)]
dependency <- fread(file.path(src,"CRISPRGeneDependency.csv"), select=1)
dependency_ids <- as.character(dependency[[1]])
stopifnot(!anyDuplicated(dependency_ids))
ids <- Reduce(intersect,list(dam$ids,hot$ids,model$ModelID,dependency_ids)); lineage <- model[match(ids,ModelID),OncotreeLineage]
stopifnot(length(ids)>0)
dm <- dam$m[match(ids,dam$ids),,drop=FALSE]>0; hm <- hot$m[match(ids,hot$ids),,drop=FALSE]>0
make <- function(m, genes, matrix_name) {
  L <- model.matrix(~0+factor(lineage)); colnames(L)<-levels(factor(lineage)); by <- crossprod(m*1,L)
  n <- colSums(L); ii <- which(m>=0,arr.ind=TRUE)
  rows <- lapply(seq_len(nrow(by)), function(i) data.table(gene=genes[i],lineage=colnames(by),mut_n=as.integer(by[i,]),wt_n=as.integer(n-by[i,]),matrix=matrix_name))
  z <- rbindlist(rows); z[,mut_rate:=round(mut_n/(mut_n+wt_n),3)]; z[,pass_standard:=mut_n>=3 & wt_n>=5]; z[,pass_strict:=mut_n>=10 & wt_n>=5]; z
}
cross <- rbind(make(dm,dam$genes,"Damaging"),make(hm,hot$genes,"Hotspot"))
fwrite(cross,file.path(out,"gene_by_lineage_mutation_menu.csv")); saveRDS(cross,file.path(out,"gene_by_lineage_mutation_menu.rds"))
fwrite(cross[pass_standard == TRUE],file.path(out,"candidate_anchor_genes_by_lineage.csv"))
menu <- cross[pass_strict == TRUE]
fwrite(menu,file.path(out,"selectable_anchor_genes_by_lineage.csv")); saveRDS(menu,file.path(out,"selectable_anchor_genes_by_lineage.rds"))
summary <- menu[,.(selectable_gene_count=uniqueN(gene),gene_count_rows=.N),by=.(lineage,matrix)]
fwrite(summary,file.path(out,"selectable_gene_counts_by_lineage.csv"))
qc <- list(status="complete",release="26Q1",cohort_definition="intersection of default Damaging, default Hotspot, Model annotation, and CRISPR Gene Dependency",common_model_count=length(ids),lineage_count=uniqueN(lineage),damaging_gene_count=length(dam$genes),hotspot_gene_count=length(hot$genes),standard_rule="Mut>=3 & WT>=5",strict_rule="Mut>=10 & WT>=5",recommended="TSG/LoF use Damaging; OG/GoF use Hotspot; unclassified genes remain available with Damaging and are not assigned a driver role")
write_json(qc,file.path(out,"manifest.json"),auto_unbox=TRUE,pretty=TRUE); print(qc)
