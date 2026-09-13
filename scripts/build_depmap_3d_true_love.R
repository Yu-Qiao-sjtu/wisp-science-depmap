#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table);library(jsonlite)})
args<-commandArgs(trailingOnly=TRUE)
arg<-function(name,default=NULL){x<-grep(paste0("^--",name,"="),args,value=TRUE);if(!length(x))default else sub(paste0("^--",name,"="),"",x[[1]])}
knowledge_root<-normalizePath(arg("knowledge-root"),mustWork=TRUE)
cor_max<-as.numeric(arg("cor-max","-0.2"));fdr_max<-as.numeric(arg("fdr-max","0.05"))
input_root<-file.path(knowledge_root,"depmap-26q1-3d","codependency")
output_root<-file.path(knowledge_root,"depmap-26q1-3d","true_love_gene");dir.create(output_root,recursive=TRUE,showWarnings=FALSE)
cohorts<-c("all_3d_lineage_adjusted","organoid_3do_lineage_adjusted")
catalog<-list()
for(key in cohorts){
 root<-file.path(input_root,key); order<-fread(file.path(root,"gene_order.csv")); genes<-order$symbol
 blocks<-sort(list.files(file.path(root,"blocks"),pattern="\\.rds$",full.names=TRUE)); directed_parts<-list()
 for(bi in seq_along(blocks)){
  b<-readRDS(blocks[[bi]]);rows<-vector("list",nrow(b$correlation))
  for(i in seq_len(nrow(b$correlation))){
   si<-b$source_start+i-1L;r<-as.numeric(b$correlation[i,]);valid<-is.finite(r)&seq_along(r)!=si
   ti<-which(valid)[which.min(r[valid])];t<-abs(r[valid])*sqrt(b$residual_df/pmax(1-r[valid]^2,.Machine$double.eps));pall<-2*pt(t,df=b$residual_df,lower.tail=FALSE);pos<-match(ti,which(valid))
   rows[[i]]<-data.table(source_index=si,source_gene=genes[[si]],target_index=ti,target_gene=genes[[ti]],correlation=r[[ti]],p_value=pall[[pos]],fdr_within_source=p.adjust(pall,"BH")[[pos]])
  }
  directed_parts[[bi]]<-rbindlist(rows)
 }
 directed<-rbindlist(directed_parts);lookup<-setNames(seq_len(nrow(directed)),directed$source_index)
 reciprocal<-vapply(seq_len(nrow(directed)),function(i){j<-unname(lookup[as.character(directed$target_index[[i]])]);length(j)==1L&&!is.na(j)&&directed$target_index[[j]]==directed$source_index[[i]]},logical(1))
 f<-directed[reciprocal&source_index<target_index];rev<-directed[match(f$target_index,source_index)]
 pairs<-data.table(gene_a=f$source_gene,gene_b=f$target_gene,correlation=f$correlation,fdr_a=f$fdr_within_source,fdr_b=rev$fdr_within_source)
 pairs[,high_confidence:=correlation<=cor_max&fdr_a<=fdr_max&fdr_b<=fdr_max];setorder(pairs,-high_confidence,fdr_a,fdr_b,correlation)
 cohort_out<-file.path(output_root,key);dir.create(cohort_out,recursive=TRUE,showWarnings=FALSE)
 fwrite(directed,file.path(cohort_out,"rank1_negative_partner_by_gene.csv.gz"),compress="gzip");fwrite(pairs,file.path(cohort_out,"strict_mutual_rank1_pairs.csv.gz"),compress="gzip");fwrite(pairs[high_confidence==TRUE],file.path(cohort_out,"high_confidence_pairs.csv.gz"),compress="gzip")
 m<-fromJSON(file.path(root,"manifest.json"));manifest<-list(schema_version=1,release="NextGen Model Manuscript 2026",family="3d_true_love_gene",cohort=key,status="complete",screen_count=m$screen_count,gene_count=nrow(directed),strict_pair_count=nrow(pairs),high_confidence_pair_count=pairs[high_confidence==TRUE,.N],correlation_max=cor_max,fdr_max=fdr_max,definition="mutual rank-1 negative lineage-adjusted Gene Effect correlation")
 write_json(manifest,file.path(cohort_out,"manifest.json"),auto_unbox=TRUE,pretty=TRUE);catalog[[key]]<-as.data.table(manifest)[,.(cohort,screen_count,gene_count,strict_pair_count,high_confidence_pair_count,status)]
}
catalog<-rbindlist(catalog);fwrite(catalog,file.path(output_root,"cohort_catalog.csv"));write_json(list(schema_version=1,release="NextGen Model Manuscript 2026",family="3d_true_love_gene",status="complete",cohort_count=nrow(catalog)),file.path(output_root,"manifest.json"),auto_unbox=TRUE,pretty=TRUE)
