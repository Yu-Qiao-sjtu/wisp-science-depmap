#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table); library(jsonlite)})
args <- commandArgs(TRUE); stopifnot(length(args)==3L)
run_dir <- args[[1]]; annotation_dir <- args[[2]]; out <- args[[3]]
dir.create(out,recursive=TRUE,showWarnings=FALSE)

x <- fread(file.path(run_dir,"candidate_anchor_genes_by_lineage.csv"))
hgnc <- fread(file.path(annotation_dir,"hgnc_table.csv"),select=c("symbol","name","locus_type","location","gene_group"))
hgnc <- hgnc[!duplicated(symbol)]
setnames(hgnc,"symbol","gene")
role <- fread(file.path(annotation_dir,"OncoKB_mutant_class.csv"),header=TRUE)
stopifnot(ncol(role)==4L)
setnames(role,c("row_id","Mut","oncoKB","class"))
role[,matrix:=fifelse(grepl("_dam$",Mut),"Damaging",fifelse(grepl("_HS$",Mut),"Hotspot",NA_character_))]
role[,gene:=sub("_(dam|HS)$","",Mut)]
role <- role[!is.na(matrix),.(gene,matrix,oncokb_role=oncoKB,oncokb_class=class)]
role <- role[!duplicated(role,by=c("gene","matrix"))]

cards <- merge(x,hgnc,by="gene",all.x=TRUE)
cards <- merge(cards,role,by=c("gene","matrix"),all.x=TRUE)
cards[,role_match:=oncokb_class %chin% c("TSG_dam","Onc_HS")]
cards[,selection_tier:=fcase(
  matrix=="AnySelected","landscape_only",
  pass_strict & role_match,"A_role_matched_strict",
  pass_strict,"B_strict_unclassified",
  default="C_exploratory"
)]
cards[,interpretation:=fcase(
  matrix=="Damaging","LikelyLoF/damaging-positive versus damaging-negative; suited to loss-of-function hypotheses",
  matrix=="Hotspot","hotspot-positive versus hotspot-negative; suited to recurrent driver/gain-of-function hypotheses",
  default="at least one variant retained in the released somatic mutation table; use for landscape review"
)]
cards[,warning:=fcase(
  matrix=="AnySelected","Broad mutation category; do not infer functional consequence",
  is_common_essential,"Common-essential annotation concerns knockout-target interpretation; do not automatically remove this mutation anchor",
  !role_match,"No role-matched OncoKB-derived classification in the local annotation table; review driver evidence",
  default=""
)]
setcolorder(cards,c("lineage","gene","name","matrix","selection_tier","mut_n","wt_n","mut_rate","pass_standard","pass_strict","oncokb_role","oncokb_class","role_match","is_common_essential","locus_type","location","gene_group","interpretation","warning"))
setorder(cards,lineage,selection_tier,-mut_n,gene)
fwrite(cards,file.path(out,"anchor_gene_cards_all.csv"))
fwrite(cards[matrix %chin% c("Damaging","Hotspot") & pass_strict],file.path(out,"anchor_gene_cards_strict.csv"))
fwrite(cards[selection_tier=="A_role_matched_strict"],file.path(out,"anchor_gene_cards_priority.csv"))
writeLines(vapply(seq_len(nrow(cards)),function(i) toJSON(as.list(cards[i]),auto_unbox=TRUE,na="null"),character(1)),file.path(out,"anchor_gene_cards_all.jsonl"))
manifest <- list(status="complete",card_count=nrow(cards),strict_functional_card_count=nrow(cards[matrix %chin% c("Damaging","Hotspot") & pass_strict]),priority_card_count=nrow(cards[selection_tier=="A_role_matched_strict"]),sources=c("DepMap 26Q1 mutation matrices and Model/Gene Dependency overlap","DepMap 26Q1 CRISPRInferredCommonEssentials.csv","local HGNC table","local OncoKB-derived mutation class table"),limitations=c("OncoKB-derived table is a local snapshot, not a live API response","cards describe analyzability and annotations, not causal validation","AnySelected is a released-variant landscape category"))
write_json(manifest,file.path(out,"manifest.json"),auto_unbox=TRUE,pretty=TRUE)
print(manifest)
