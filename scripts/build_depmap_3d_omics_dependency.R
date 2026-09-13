#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table);library(jsonlite);library(Matrix)})
args<-commandArgs(trailingOnly=TRUE)
arg<-function(name,default=NULL){x<-grep(paste0("^--",name,"="),args,value=TRUE);if(!length(x))default else sub(paste0("^--",name,"="),"",x[[1]])}
data_root<-normalizePath(arg("data-root"),mustWork=TRUE);knowledge_root<-normalizePath(arg("knowledge-root"),mustWork=TRUE)
cor_min<-as.numeric(arg("cor-min","0.3"));fdr_max<-as.numeric(arg("fdr-max","0.05"));top_k<-as.integer(arg("top-k","20"));min_event_n<-as.integer(arg("min-event-n","5"))
out_root<-file.path(knowledge_root,"depmap-26q1-3d","omics_dependency");dir.create(out_root,recursive=TRUE,showWarnings=FALSE)
meta<-fread(file.path(data_root,"screen_metadata.csv"))[PassesQC==TRUE&ScreenType%chin%c("3DO","3DN")]
read_mat<-function(path){x<-fread(path,check.names=FALSE);ids<-x[[1]];x[[1]]<-NULL;g<-toupper(sub(" \\([^()]++\\)$","",names(x),perl=TRUE));m<-as.matrix(x);storage.mode(m)<-"double";rownames(m)<-ids;colnames(m)<-g;list(matrix=m,genes=g)}
e<-read_mat(file.path(data_root,"screen_gene_effect.csv"));idx<-match(meta$ScreenID,rownames(e$matrix));effect<-e$matrix[idx,,drop=FALSE];complete_target<-colSums(is.finite(effect))==nrow(effect);effect<-effect[,complete_target,drop=FALSE];targets<-e$genes[complete_target]
design<-model.matrix(~0+factor(meta$OncotreeLineage));q<-qr(design);effect<-effect-design%*%qr.coef(q,effect);df<-nrow(effect)-q$rank-2L
completed_modality<-function(name){
 path<-file.path(out_root,name,"manifest.json")
 if(!file.exists(path))return(NULL)
 manifest<-tryCatch(fromJSON(path),error=function(...)NULL)
 if(is.null(manifest)||!identical(manifest$status,"complete"))return(NULL)
 message(name," already complete; skipping")
 as.data.table(manifest)[,.(modality,feature_count,target_count,retained_count,status)]
}

run_continuous<-function(name,file){done<-completed_modality(name);if(!is.null(done))return(done);d<-read_mat(file.path(data_root,file));ri<-match(meta$ModelID,rownames(d$matrix));m<-d$matrix[ri,,drop=FALSE];complete<-colSums(is.finite(m))==nrow(m);m<-m[,complete,drop=FALSE];features<-d$genes[complete];m<-m-design%*%qr.coef(q,m);m<-scale(m);y<-scale(effect);rows<-list();starts<-seq.int(1L,ncol(m),by=128L)
 for(b in seq_along(starts)){s<-starts[[b]];en<-min(s+127L,ncol(m));r<-crossprod(m[,s:en,drop=FALSE],y)/(nrow(m)-1L);t<-abs(r)*sqrt(df/pmax(1-r^2,.Machine$double.eps));p<-2*pt(t,df=df,lower.tail=FALSE)
  rr<-vector("list",nrow(r));for(i in seq_len(nrow(r))){f<-p.adjust(p[i,],"BH");hit<-which(abs(r[i,])>=cor_min&f<=fdr_max);if(length(hit)){hit<-head(hit[order(f[hit],-abs(r[i,hit]))],top_k);rr[[i]]<-data.table(feature_gene=features[[s+i-1L]],target_gene=targets[hit],correlation=r[i,hit],p_value=p[i,hit],fdr=f[hit],rank_within_feature=seq_along(hit))}}
  rows[[b]]<-rbindlist(rr,fill=TRUE);if(b%%20L==0L||b==length(starts))message(name," ",b,"/",length(starts))}
 out<-rbindlist(rows,fill=TRUE);root<-file.path(out_root,name);dir.create(root,recursive=TRUE,showWarnings=FALSE);fwrite(out,file.path(root,"significant_associations.csv.gz"),compress="gzip");manifest<-list(schema_version=1,release="NextGen Model Manuscript 2026",family="3d_omics_dependency",modality=name,status="complete",model_count=nrow(m),feature_count=ncol(m),target_count=ncol(effect),retained_count=nrow(out),correlation_min=cor_min,fdr_max=fdr_max,method="lineage-residualized Pearson correlation; BH FDR per molecular feature");write_json(manifest,file.path(root,"manifest.json"),auto_unbox=TRUE,pretty=TRUE);as.data.table(manifest)[,.(modality,feature_count,target_count,retained_count,status)]}

run_binary<-function(name,file){done<-completed_modality(name);if(!is.null(done))return(done);d<-read_mat(file.path(data_root,file));m<-d$matrix[match(meta$ModelID,rownames(d$matrix)),,drop=FALSE];binary<-is.finite(m)&m>0;na<-colSums(binary);eligible<-na>=min_event_n&(nrow(binary)-na)>=min_event_n;binary<-binary[,eligible,drop=FALSE];features<-d$genes[eligible];rows<-list();starts<-seq.int(1L,ncol(binary),by=128L);ysq<-effect^2
 for(b in seq_along(starts)){s<-starts[[b]];en<-min(s+127L,ncol(binary));a<-Matrix(t(binary[,s:en,drop=FALSE])*1,sparse=TRUE);z<-Matrix(1-t(binary[,s:en,drop=FALSE]),sparse=TRUE);n1<-rowSums(a);n0<-rowSums(z);sum1<-as.matrix(a%*%effect);sum0<-as.matrix(z%*%effect);sq1<-as.matrix(a%*%ysq);sq0<-as.matrix(z%*%ysq);mean1<-sum1/n1;mean0<-sum0/n0;v1<-pmax((sq1-sum1^2/n1)/(n1-1),0);v0<-pmax((sq0-sum0^2/n0)/(n0-1),0);se2<-v1/n1+v0/n0;diff<-mean1-mean0;tstat<-diff/sqrt(se2);welchdf<-se2^2/((v1/n1)^2/(n1-1)+(v0/n0)^2/(n0-1));p<-pt(tstat,df=welchdf,lower.tail=TRUE)
  rr<-vector("list",nrow(p));for(i in seq_len(nrow(p))){f<-p.adjust(p[i,],"BH");hit<-which(diff[i,]<0&f<=fdr_max);if(length(hit)){hit<-head(hit[order(f[hit],diff[i,hit])],top_k);rr[[i]]<-data.table(feature_gene=features[[s+i-1L]],target_gene=targets[hit],event_n=n1[[i]],wildtype_n=n0[[i]],mean_difference=diff[i,hit],p_value=p[i,hit],fdr=f[hit],rank_within_feature=seq_along(hit))}}
  rows[[b]]<-rbindlist(rr,fill=TRUE);if(b%%10L==0L||b==length(starts))message(name," ",b,"/",length(starts))}
 out<-rbindlist(rows,fill=TRUE);root<-file.path(out_root,name);dir.create(root,recursive=TRUE,showWarnings=FALSE);fwrite(out,file.path(root,"significant_associations.csv.gz"),compress="gzip");manifest<-list(schema_version=1,release="NextGen Model Manuscript 2026",family="3d_omics_dependency",modality=name,status="complete",model_count=nrow(binary),feature_count=ncol(binary),target_count=ncol(effect),retained_count=nrow(out),min_event_n=min_event_n,fdr_max=fdr_max,method="Welch comparison on lineage-residualized Gene Effect; altered versus unaltered; BH FDR per event");write_json(manifest,file.path(root,"manifest.json"),auto_unbox=TRUE,pretty=TRUE);as.data.table(manifest)[,.(modality,feature_count,target_count,retained_count,status)]}

catalog<-rbindlist(list(run_continuous("expression_dependency","next_gen_expression.csv"),run_continuous("cnv_dependency","next_gen_copy_number.csv"),run_binary("damaging_dependency","next_gen_damaging.csv"),run_binary("hotspot_dependency","next_gen_hotspot.csv")))
fwrite(catalog,file.path(out_root,"modality_catalog.csv"));write_json(list(schema_version=1,release="NextGen Model Manuscript 2026",family="3d_omics_dependency",status="complete",modality_count=nrow(catalog)),file.path(out_root,"manifest.json"),auto_unbox=TRUE,pretty=TRUE)
