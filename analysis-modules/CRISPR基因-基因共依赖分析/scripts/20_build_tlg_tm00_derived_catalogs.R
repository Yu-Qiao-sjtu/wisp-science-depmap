#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table); library(jsonlite)})
args <- commandArgs(trailingOnly=TRUE)
arg <- function(name, default=NULL){x<-grep(paste0('^--',name,'='),args,value=TRUE);if(length(x))sub(paste0('^--',name,'='),'',x[[1L]]) else default}
knowledge_root <- normalizePath(arg('knowledge-root'), winslash='/', mustWork=TRUE)
cor_threshold <- as.numeric(arg('cor-threshold','-0.3'))
top_k <- as.integer(arg('top-k','20'))
quality_min_n <- as.integer(arg('quality-min-n','500'))
input_root <- file.path(knowledge_root,'depmap-26q1-full','effect_correlation')
output_root <- path.expand(arg('output-root',file.path(knowledge_root,'depmap-26q1-full','true_love_gene','tm00_derived_catalogs_26Q1')))
dir.create(output_root,recursive=TRUE,showWarnings=FALSE)
order_dt <- fread(file.path(input_root,'gene_order.csv')); genes <- toupper(order_dt$symbol)
blocks <- sort(list.files(file.path(input_root,'blocks'),pattern='[.]rds$',full.names=TRUE));stopifnot(length(blocks)>0L)
negative_legacy_parts<-vector('list',length(blocks));negative_quality_parts<-vector('list',length(blocks))
pos_legacy_parts<-vector('list',length(blocks));pos_quality_parts<-vector('list',length(blocks))
p_value_for <- function(r,n){out<-rep(NA_real_,length(r));ok<-is.finite(r)&is.finite(n)&n>2;tt<-abs(r[ok])*sqrt((n[ok]-2)/pmax(1-r[ok]^2,.Machine$double.eps));out[ok]<-2*pt(tt,df=n[ok]-2,lower.tail=FALSE);out}
rank_top <- function(source_index,r,n,min_n,label){
 valid<-is.finite(r)&is.finite(n)&seq_along(r)!=source_index
 if(!is.na(min_n))valid<-valid&n>=min_n
 idx<-which(valid);if(!length(idx))return(NULL)
 ranks<-rank(-r[idx],ties.method='average',na.last='keep');keep<-idx[ranks<=top_k];if(!length(keep))return(NULL)
 rr<-ranks[match(keep,idx)];ord<-order(rr,-r[keep],genes[keep]);keep<-keep[ord];rr<-rr[ord]
 data.table(source_index=source_index,source_gene=genes[source_index],target_index=keep,target_gene=genes[keep],correlation=as.numeric(r[keep]),pair_n=as.integer(n[keep]),p_value=p_value_for(r[keep],n[keep]),rank_positive=as.numeric(rr),coverage_layer=label)
}
for(bi in seq_along(blocks)){
 x<-readRDS(blocks[[bi]]);neg1<-vector('list',nrow(x$correlation));neg2<-vector('list',nrow(x$correlation));pl<-vector('list',nrow(x$correlation));pq<-vector('list',nrow(x$correlation))
 for(ri in seq_len(nrow(x$correlation))){
  si<-x$source_start+ri-1L;r<-as.numeric(x$correlation[ri,]);n<-as.numeric(x$pair_n[ri,]);lower<-seq_along(r)<si
  k1<-which(lower&is.finite(r)&r<cor_threshold)
  if(length(k1))neg1[[ri]]<-data.table(gene_a=genes[k1],gene_b=genes[si],correlation=as.numeric(r[k1]),pair_n=as.integer(n[k1]),p_value=p_value_for(r[k1],n[k1]),coverage_pass_n500=n[k1]>=quality_min_n)
  k2<-k1[is.finite(n[k1])&n[k1]>=quality_min_n]
  if(length(k2))neg2[[ri]]<-data.table(gene_a=genes[k2],gene_b=genes[si],correlation=as.numeric(r[k2]),pair_n=as.integer(n[k2]),p_value=p_value_for(r[k2],n[k2]))
  pl[[ri]]<-rank_top(si,r,n,NA_integer_,'legacy_all_finite')
  pq[[ri]]<-rank_top(si,r,n,quality_min_n,paste0('quality_n',quality_min_n))
 }
 negative_legacy_parts[[bi]]<-rbindlist(neg1);negative_quality_parts[[bi]]<-rbindlist(neg2);pos_legacy_parts[[bi]]<-rbindlist(pl);pos_quality_parts[[bi]]<-rbindlist(pq)
 if(bi%%10L==0L||bi==length(blocks))message(sprintf('[%d/%d] derived TLG catalogs',bi,length(blocks)))
}
negative_legacy<-rbindlist(negative_legacy_parts);negative_quality<-rbindlist(negative_quality_parts);pos_legacy<-rbindlist(pos_legacy_parts);pos_quality<-rbindlist(pos_quality_parts)
setorder(negative_legacy,correlation,gene_a,gene_b);setorder(negative_quality,correlation,gene_a,gene_b)
reciprocal <- function(edges,label){
 rev<-edges[,.(source_index=target_index,target_index=source_index,reverse_correlation=correlation,reverse_pair_n=pair_n,reverse_p_value=p_value,reverse_rank_positive=rank_positive)]
 z<-merge(edges,rev,by=c('source_index','target_index'));z<-z[source_index<target_index & rank_positive<=top_k & reverse_rank_positive<=top_k]
 z[,`:=`(gene_a=genes[source_index],gene_b=genes[target_index],reciprocal_rank_sum=rank_positive+reverse_rank_positive,reciprocal_rank_max=pmax(rank_positive,reverse_rank_positive),coverage_layer=label)]
 setorder(z,reciprocal_rank_sum,-correlation,gene_a,gene_b)
 z[,.(gene_a,gene_b,correlation_a_to_b=correlation,correlation_b_to_a=reverse_correlation,pair_n_a_to_b=pair_n,pair_n_b_to_a=reverse_pair_n,p_value_a_to_b=p_value,p_value_b_to_a=reverse_p_value,rank_a_to_b=rank_positive,rank_b_to_a=reverse_rank_positive,reciprocal_rank_sum,reciprocal_rank_max,coverage_layer)]
}
recip_legacy<-reciprocal(pos_legacy,'legacy_all_finite');recip_quality<-reciprocal(pos_quality,paste0('quality_n',quality_min_n))
fwrite(negative_legacy,file.path(output_root,'negative_codependency_r_lt_minus_0.3_legacy.csv.gz'))
fwrite(negative_quality,file.path(output_root,paste0('negative_codependency_r_lt_minus_0.3_n',quality_min_n,'.csv.gz')))
fwrite(pos_legacy,file.path(output_root,'positive_top20_directed_legacy.csv.gz'));fwrite(recip_legacy,file.path(output_root,'positive_reciprocal_top20_legacy.csv.gz'))
fwrite(pos_quality,file.path(output_root,paste0('positive_top20_directed_n',quality_min_n,'.csv.gz')));fwrite(recip_quality,file.path(output_root,paste0('positive_reciprocal_top20_n',quality_min_n,'.csv.gz')))
manifest<-list(schema_version=1,status='complete',release='26Q1',family='true_love_gene_tm00_derived_catalogs',gene_count=length(genes),correlation_threshold=cor_threshold,top_k=top_k,quality_min_pair_n=quality_min_n,negative_legacy_pair_count=nrow(negative_legacy),negative_quality_pair_count=nrow(negative_quality),positive_legacy_directed_edge_count=nrow(pos_legacy),positive_legacy_reciprocal_pair_count=nrow(recip_legacy),positive_quality_directed_edge_count=nrow(pos_quality),positive_quality_reciprocal_pair_count=nrow(recip_quality),legacy_contract='finite pairwise Pearson r only; negative catalog uses r < -0.3; positive catalog requires both directional positive ranks <= 20',quality_contract=paste0('same definitions after requiring pair_n >= ',quality_min_n),terminology=list(tlg='True Love Gene / 真爱基因 is the project family name',negative='negative co-dependency candidate; correlation alone is not synthetic-lethality proof',positive='positive reciprocal co-dependency neighbor; similar dependency profiles'),tm00_references=c('05.2_synthetic_lethal.R','19_find_TLG_all(1).R'),input='depmap-26q1-full/effect_correlation')
write_json(manifest,file.path(output_root,'manifest.json'),auto_unbox=TRUE,pretty=TRUE)
cat(toJSON(manifest,auto_unbox=TRUE,pretty=TRUE),'\n')
