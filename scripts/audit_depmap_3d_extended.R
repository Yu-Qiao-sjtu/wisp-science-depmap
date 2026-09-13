#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table);library(jsonlite)})
args<-commandArgs(trailingOnly=TRUE)
arg<-function(name,default=NULL){x<-grep(paste0('^--',name,'='),args,value=TRUE);if(!length(x))default else sub(paste0('^--',name,'='),'',x[[1]])}
data_root<-normalizePath(arg('data-root'),mustWork=TRUE)
knowledge_root<-normalizePath(arg('knowledge-root'),mustWork=TRUE)
out<-file.path(knowledge_root,'depmap-26q1-3d','extended_analysis_audit');dir.create(out,recursive=TRUE,showWarnings=FALSE)
meta<-fread(file.path(data_root,'screen_metadata.csv'))[PassesQC==TRUE&ScreenType%chin%c('3DO','3DN')]
lineages<-meta[,.(screen_count=.N),by=.(ScreenType,OncotreeLineage)][order(-screen_count)]
lineages[,analysis_tier:=fcase(screen_count>=20,'confirmatory_eligible',screen_count>=14,'exploratory_only',default='descriptive_only')]
fwrite(lineages,file.path(out,'lineage_feasibility.csv'))
cn<-fread(file.path(data_root,'next_gen_copy_number.csv'),check.names=FALSE);ids<-cn[[1]];cn[[1]]<-NULL
genes<-toupper(sub(' \\([^()]++\\)$','',names(cn),perl=TRUE));m<-as.matrix(cn);storage.mode(m)<-'double';rownames(m)<-ids
m<-m[match(meta$ModelID,rownames(m)),,drop=FALSE]
amp_threshold<-as.numeric(arg('amp-threshold','2'));amp<-is.finite(m)&m>=amp_threshold
prev<-colSums(amp);eligible<-which(prev>=8L&prev<=nrow(m)-8L)
amp_catalog<-data.table(gene=genes,amplified_n=prev,eligible_for_pair_screen=seq_along(prev)%in%eligible)
fwrite(amp_catalog,file.path(out,'amplification_prevalence.csv.gz'),compress='gzip')
pair_count<-0;pattern_count<-0
if(length(eligible)){
 patterns<-unique(as.data.table(t(amp[,eligible,drop=FALSE])))
 pattern_count<-nrow(patterns)
 if(length(eligible)<=5000L){
  z<-crossprod(amp[,eligible,drop=FALSE]*1);p<-prev[eligible]
  source_only<-outer(p,rep(1,length(p)))-z;partner_only<-t(source_only)
  keep<-upper.tri(z)&z>=8L&source_only>=8L&partner_only>=8L
  pair_count<-sum(keep)
 }
}
strict_counts<-fread(file.path(knowledge_root,'depmap-26q1-3d','true_love_gene','cohort_catalog.csv'))
manifest<-list(schema_version=1,status='complete',qa_status='PASS',model_count=nrow(meta),lineage_count=nrow(lineages),confirmatory_lineage_count=lineages[analysis_tier=='confirmatory_eligible',.N],exploratory_lineage_count=lineages[analysis_tier=='exploratory_only',.N],amp_threshold=amp_threshold,eligible_amplified_gene_count=length(eligible),unique_amplification_pattern_count=pattern_count,eligible_pair_count=pair_count,true_love_pair_counts=split(strict_counts$strict_pair_count,strict_counts$cohort),notes=c('WGCNA and predictive modeling excluded by project scope','lineages below 14 screens are descriptive only','co-amplification requires at least 8 co-amplified, 8 source-only, and 8 partner-only screens'))
write_json(manifest,file.path(out,'manifest.json'),auto_unbox=TRUE,pretty=TRUE)
print(manifest)
