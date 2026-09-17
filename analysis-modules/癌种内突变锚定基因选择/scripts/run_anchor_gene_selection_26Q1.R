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
base_ids <- intersect(model$ModelID, dependency_ids)
dam_ids <- intersect(base_ids, dam$ids); hot_ids <- intersect(base_ids, hot$ids)
stopifnot(length(dam_ids)>0, length(hot_ids)>0)
dm <- dam$m[match(dam_ids,dam$ids),,drop=FALSE]>0
hm <- hot$m[match(hot_ids,hot$ids),,drop=FALSE]>0
make <- function(m, genes, ids, matrix_name) {
  lineage <- model[match(ids,ModelID),OncotreeLineage]
  L <- model.matrix(~0+factor(lineage)); colnames(L)<-levels(factor(lineage)); by <- crossprod(m*1,L)
  n <- colSums(L); ii <- which(m>=0,arr.ind=TRUE)
  rows <- lapply(seq_len(nrow(by)), function(i) data.table(gene=genes[i],lineage=colnames(by),mut_n=as.integer(by[i,]),wt_n=as.integer(n-by[i,]),matrix=matrix_name))
  z <- rbindlist(rows); z[,mut_rate:=round(mut_n/(mut_n+wt_n),3)]; z[,pass_standard:=mut_n>=3 & wt_n>=5]; z[,pass_strict:=mut_n>=10 & wt_n>=5]; z
}
cross <- rbind(make(dm,dam$genes,dam_ids,"Damaging"),make(hm,hot$genes,hot_ids,"Hotspot"))

# The release mutation table is already filtered by DepMap. This category means
# "at least one variant retained in the released somatic mutation table"; it is
# broader than Damaging and Hotspot, but is not proof that every genomic variant
# was assayed or that the retained variant is functional.
som <- fread(file.path(src,"OmicsSomaticMutations.csv"),
  select=c("ModelID","HugoSymbol","IsDefaultEntryForModel"))
som <- unique(som[IsDefaultEntryForModel=="Yes" & ModelID %chin% base_ids &
  !is.na(HugoSymbol) & HugoSymbol!="", .(ModelID,HugoSymbol)])
som[,lineage:=model[match(som$ModelID,model$ModelID),OncotreeLineage]]
lineage_n <- model[ModelID %chin% base_ids & !is.na(OncotreeLineage),
  .(cohort_n=uniqueN(ModelID)),by=.(lineage=OncotreeLineage)]
any_cross <- som[,.(mut_n=uniqueN(ModelID)),by=.(gene=HugoSymbol,lineage)]
any_cross <- merge(any_cross,lineage_n,by="lineage",all.x=TRUE)
any_cross[,`:=`(wt_n=cohort_n-mut_n,matrix="AnySelected")]
any_cross[,mut_rate:=round(mut_n/(mut_n+wt_n),3)]
any_cross[,pass_standard:=mut_n>=3 & wt_n>=5]
any_cross[,pass_strict:=mut_n>=10 & wt_n>=5]
cross <- rbind(cross,any_cross[,.(gene,lineage,mut_n,wt_n,matrix,mut_rate,pass_standard,pass_strict)],fill=TRUE)
common_essential <- clean(readLines(file.path(src,"CRISPRInferredCommonEssentials.csv"))[-1])
common_essential <- unique(common_essential[nzchar(common_essential)])
cross[,is_common_essential:=gene %chin% common_essential]
fwrite(cross,file.path(out,"gene_by_lineage_mutation_menu.csv")); saveRDS(cross,file.path(out,"gene_by_lineage_mutation_menu.rds"))
fwrite(cross[pass_standard == TRUE],file.path(out,"candidate_anchor_genes_by_lineage.csv"))
menu <- cross[pass_strict == TRUE]
fwrite(menu,file.path(out,"selectable_anchor_genes_by_lineage.csv")); saveRDS(menu,file.path(out,"selectable_anchor_genes_by_lineage.rds"))
fwrite(menu[matrix %chin% c("Damaging","Hotspot") & !is_common_essential],
  file.path(out,"sensitivity_excluding_common_essential.csv"))
summary <- menu[,.(selectable_gene_count=uniqueN(gene),gene_count_rows=.N),by=.(lineage,matrix)]
fwrite(summary,file.path(out,"selectable_gene_counts_by_lineage.csv"))
qc <- list(status="complete",release="26Q1",cohort_definition="each mutation source intersected independently with Model annotation and CRISPR Gene Dependency",dependency_model_count=length(base_ids),damaging_model_count=length(dam_ids),hotspot_model_count=length(hot_ids),lineage_count=uniqueN(model[ModelID %chin% base_ids,OncotreeLineage]),damaging_gene_count=length(dam$genes),hotspot_gene_count=length(hot$genes),any_selected_gene_count=uniqueN(som$HugoSymbol),common_essential_source="CRISPRInferredCommonEssentials.csv from DepMap Public 26Q1",common_essential_gene_count=length(common_essential),standard_rule="Mut>=3 & WT>=5",strict_rule="Mut>=10 & WT>=5",recommended="Use AnySelected for landscape only; annotate common essentials but do not remove mutation anchors automatically; use the exclusion file only for sensitivity analysis; TSG/LoF use Damaging; OG/GoF use Hotspot")
write_json(qc,file.path(out,"manifest.json"),auto_unbox=TRUE,pretty=TRUE); print(qc)
