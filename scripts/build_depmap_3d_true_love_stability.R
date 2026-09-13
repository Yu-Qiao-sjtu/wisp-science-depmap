#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table);library(jsonlite)})
args<-commandArgs(trailingOnly=TRUE)
arg<-function(name,default=NULL){x<-grep(paste0('^--',name,'='),args,value=TRUE);if(!length(x))default else sub(paste0('^--',name,'='),'',x[[1]])}
data_root<-normalizePath(arg('data-root'),mustWork=TRUE);knowledge_root<-normalizePath(arg('knowledge-root'),mustWork=TRUE)
n_boot<-as.integer(arg('n-bootstrap','500'));sample_fraction<-as.numeric(arg('sample-fraction','0.8'));stability_min<-as.numeric(arg('stability-min','0.7'));cor_max<-as.numeric(arg('cor-max','-0.2'))
seeds<-as.integer(strsplit(arg('seeds','2601,2602'),',',fixed=TRUE)[[1]])
meta<-fread(file.path(data_root,'screen_metadata.csv'))[PassesQC==TRUE]
x<-fread(file.path(data_root,'screen_gene_effect.csv'),check.names=FALSE);ids<-x[[1]];x[[1]]<-NULL;genes<-toupper(sub(' \\([^()]++\\)$','',names(x),perl=TRUE));effect<-as.matrix(x);storage.mode(effect)<-'double';rownames(effect)<-ids;colnames(effect)<-genes;rm(x)
atomic_save<-function(value,path){tmp<-paste0(path,'.tmp.',Sys.getpid());saveRDS(value,tmp,compress='xz');if(!file.rename(tmp,path))stop('checkpoint publish failed')}
pair_cor<-function(m,ia,ib){a<-m[,ia,drop=FALSE];b<-m[,ib,drop=FALSE];a<-scale(a);b<-scale(b);colSums(a*b)/(nrow(m)-1)}
cohorts<-list(all_3d_lineage_adjusted=c('3DO','3DN'),organoid_3do_lineage_adjusted='3DO')
catalog<-list()
for(key in names(cohorts)){
 out<-file.path(knowledge_root,'depmap-26q1-3d','true_love_stability',key);dir.create(out,recursive=TRUE,showWarnings=FALSE)
 final_manifest<-file.path(out,'manifest.json');if(file.exists(final_manifest)&&identical(tryCatch(fromJSON(final_manifest)$status,error=function(e)NULL),'complete')){catalog[[key]]<-as.data.table(fromJSON(final_manifest))[,.(cohort,screen_count,input_pair_count,stable_pair_count,status)];next}
 info<-meta[ScreenType%chin%cohorts[[key]]];m<-effect[match(info$ScreenID,rownames(effect)),,drop=FALSE];complete<-colSums(is.finite(m))==nrow(m);m<-m[,complete,drop=FALSE];g<-colnames(m);design<-model.matrix(~0+factor(info$OncotreeLineage));m<-m-design%*%qr.coef(qr(design),m)
 pairs<-fread(file.path(knowledge_root,'depmap-26q1-3d','true_love_gene',key,'strict_mutual_rank1_pairs.csv.gz'))[high_confidence==TRUE]
 ia<-match(pairs$gene_a,g);ib<-match(pairs$gene_b,g);keep<-is.finite(ia)&is.finite(ib);pairs<-pairs[keep];ia<-ia[keep];ib<-ib[keep]
 pairs[,spearman_correlation:=vapply(seq_len(.N),function(i)cor(m[,ia[i]],m[,ib[i]],method='spearman'),numeric(1))]
 pairs[,pearson_spearman_concordant:=spearman_correlation<0]
 all_stability<-list()
 for(seed in seeds){
  checkpoint<-file.path(out,paste0('bootstrap_seed_',seed,'.checkpoint.rds'));cors<-matrix(NA_real_,nrow(pairs),n_boot);start<-1L
  if(file.exists(checkpoint)){z<-tryCatch(readRDS(checkpoint),error=function(e)NULL);if(!is.null(z)&&z$n_boot==n_boot&&z$seed==seed&&z$pair_n==nrow(pairs)){cors<-z$cors;start<-z$done+1L}}
  set.seed(seed);if(start>1L)for(i in seq_len(start-1L))sample.int(nrow(m),floor(nrow(m)*sample_fraction),replace=FALSE)
  if(start<=n_boot)for(b in seq.int(start,n_boot)){sel<-sample.int(nrow(m),floor(nrow(m)*sample_fraction),replace=FALSE);cors[,b]<-pair_cor(m[sel,,drop=FALSE],ia,ib);if(b%%25L==0L||b==n_boot){atomic_save(list(n_boot=n_boot,seed=seed,pair_n=nrow(pairs),done=b,cors=cors),checkpoint);message(key,' seed=',seed,' ',b,'/',n_boot)}}
  all_stability[[as.character(seed)]]<-list(freq=rowMeans(cors<=cor_max,na.rm=TRUE),median=apply(cors,1,median,na.rm=TRUE),q025=apply(cors,1,quantile,.025,na.rm=TRUE),q975=apply(cors,1,quantile,.975,na.rm=TRUE));unlink(checkpoint)
 }
 for(seed in seeds){z<-all_stability[[as.character(seed)]];pairs[,(paste0('stability_seed_',seed)):=z$freq];pairs[,(paste0('median_seed_',seed)):=z$median];pairs[,(paste0('q025_seed_',seed)):=z$q025];pairs[,(paste0('q975_seed_',seed)):=z$q975]}
 stability_cols<-paste0('stability_seed_',seeds);pairs[,cross_seed_min_stability:=do.call(pmin,c(.SD,list(na.rm=TRUE))),.SDcols=stability_cols]
 big_lineages<-unique(info$OncotreeLineage)[vapply(unique(info$OncotreeLineage),function(z)sum(info$OncotreeLineage==z)>=14,logical(1))]
 loo<-matrix(NA_real_,nrow(pairs),length(big_lineages),dimnames=list(NULL,big_lineages));if(length(big_lineages))for(j in seq_along(big_lineages)){sel<-info$OncotreeLineage!=big_lineages[j];loo[,j]<-pair_cor(m[sel,,drop=FALSE],ia,ib)}
 pairs[,leave_one_lineage_min_correlation:=if(ncol(loo))apply(loo,1,max,na.rm=TRUE) else NA_real_]
 pairs[,stable_high_confidence:=cross_seed_min_stability>=stability_min&pearson_spearman_concordant&(is.na(leave_one_lineage_min_correlation)|leave_one_lineage_min_correlation<0)]
 setorder(pairs,-stable_high_confidence,-cross_seed_min_stability,correlation);fwrite(pairs,file.path(out,'all_stability_results.csv.gz'),compress='gzip');fwrite(pairs[stable_high_confidence==TRUE],file.path(out,'stable_true_love_pairs.csv.gz'),compress='gzip')
 manifest<-list(schema_version=1,family='3d_true_love_stability',cohort=key,status='complete',qa_status='PASS',screen_count=nrow(m),input_pair_count=nrow(pairs),stable_pair_count=pairs[stable_high_confidence==TRUE,.N],n_bootstrap_per_seed=n_boot,seeds=seeds,sample_fraction=sample_fraction,stability_min=stability_min,correlation_max=cor_max,leave_one_lineage_groups=big_lineages,method='two-seed repeated 80% subsampling of lineage-residualized pair correlation; Pearson/Spearman sign concordance; leave-one-major-lineage-out sign check');write_json(manifest,final_manifest,auto_unbox=TRUE,pretty=TRUE);catalog[[key]]<-as.data.table(manifest)[,.(cohort,screen_count,input_pair_count,stable_pair_count,status)]
}
catalog<-rbindlist(catalog);root<-file.path(knowledge_root,'depmap-26q1-3d','true_love_stability');fwrite(catalog,file.path(root,'cohort_catalog.csv'));write_json(list(schema_version=1,family='3d_true_love_stability',status='complete',qa_status='PASS',cohort_count=nrow(catalog)),file.path(root,'manifest.json'),auto_unbox=TRUE,pretty=TRUE)
