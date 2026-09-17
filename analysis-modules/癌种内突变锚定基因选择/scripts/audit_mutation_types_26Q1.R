#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table); library(jsonlite)})
args<-commandArgs(TRUE);stopifnot(length(args)==2L)
src<-args[[1]];out<-args[[2]];dir.create(out,recursive=TRUE,showWarnings=FALSE)
dep<-fread(file.path(src,"CRISPRGeneDependency.csv"),select=1);ids<-as.character(dep[[1]])
x<-fread(file.path(src,"OmicsSomaticMutations.csv"),select=c("ModelID","IsDefaultEntryForModel","HugoSymbol","VariantType","VariantInfo","VepImpact","LikelyLoF","Hotspot"))
x<-x[IsDefaultEntryForModel=="Yes" & ModelID %chin% ids & !is.na(HugoSymbol) & HugoSymbol!=""]
x[,consequence:=sub("&.*","",VariantInfo)]
x[,likely_lof:=LikelyLoF %in% c(TRUE,"True","TRUE",1)]
x[,hotspot:=Hotspot %in% c(TRUE,"True","TRUE",1)]
overview<-x[,.(variant_rows=.N,model_gene_events=uniqueN(paste(ModelID,HugoSymbol))),by=.(consequence,VariantType,VepImpact)][order(-variant_rows)]
fwrite(overview,file.path(out,"mutation_type_overview.csv"))
overlap<-x[,.(variant_rows=.N,model_gene_events=uniqueN(paste(ModelID,HugoSymbol))),by=.(likely_lof,hotspot)][order(-variant_rows)]
fwrite(overlap,file.path(out,"damaging_hotspot_overlap.csv"))
manifest<-list(status="complete",dependency_models=uniqueN(ids),retained_variant_rows=nrow(x),genes=uniqueN(x$HugoSymbol),models=uniqueN(x$ModelID),consequence_groups=uniqueN(x$consequence),note="Counts use default model entries within the CRISPR Gene Dependency cohort. OmicsSomaticMutations.csv is already a filtered/rescued release table, not every raw genomic call.")
write_json(manifest,file.path(out,"manifest.json"),auto_unbox=TRUE,pretty=TRUE);print(manifest);print(head(overview,15));print(overlap)
