#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table);library(jsonlite)})
args<-commandArgs(trailingOnly=TRUE);arg<-function(name,default=NULL){x<-grep(paste0('^--',name,'='),args,value=TRUE);if(!length(x))default else sub(paste0('^--',name,'='),'',x[[1]])}
source_root<-normalizePath(arg('source-root'),mustWork=TRUE);out<-path.expand(arg('output-root'));dir.create(out,recursive=TRUE,showWarnings=FALSE)
meta<-fread(file.path(source_root,'screen_metadata.csv'))[PassesQC==TRUE&ScreenType%chin%c('3DO','3DN')];stopifnot(nrow(meta)==108L,uniqueN(meta$ScreenID)==108L,uniqueN(meta$ModelID)==108L)
fwrite(meta,file.path(out,'screen_metadata.csv'));fwrite(meta[,.(ModelConditionID,ModelID)],file.path(out,'ModelCondition.csv'))
models<-fread(file.path(source_root,'model_metadata.csv'));models<-models[ModelID%chin%meta$ModelID];fwrite(models,file.path(out,'model_metadata.csv'));fwrite(models,file.path(out,'Model.csv'))
subset_matrix<-function(input,output,row_ids,id_kind=c('screen','model')){id_kind<-match.arg(id_kind);x<-fread(file.path(source_root,input),check.names=FALSE);id<-x[[1]];keep<-id%chin%row_ids;x<-x[keep];ord<-match(row_ids,x[[1]]);stopifnot(!anyNA(ord));x<-x[ord];fwrite(x,file.path(out,output));list(file=output,rows=nrow(x),columns=ncol(x),id_kind=id_kind)}
catalog<-list()
catalog[['screen_gene_effect.csv']]<-subset_matrix('screen_gene_effect.csv','screen_gene_effect.csv',meta$ScreenID,'screen')
catalog[['CRISPRGeneEffect.csv']]<-subset_matrix('screen_gene_effect.csv','CRISPRGeneEffect.csv',meta$ScreenID,'screen')
catalog[['screen_gene_dependency.csv']]<-subset_matrix('screen_gene_dependency.csv','screen_gene_dependency.csv',meta$ScreenID,'screen')
catalog[['CRISPRGeneDependency.csv']]<-subset_matrix('screen_gene_dependency.csv','CRISPRGeneDependency.csv',meta$ScreenID,'screen')
catalog[['crispr_naive_gene_score.csv']]<-subset_matrix('crispr_naive_gene_score.csv','crispr_naive_gene_score.csv',meta$ScreenID,'screen')
for(alias in c('CRISPRGeneEffect.csv','CRISPRGeneDependency.csv')){x<-fread(file.path(out,alias),check.names=FALSE);x[[1]]<-meta$ModelID;setnames(x,1L,'ModelID');fwrite(x,file.path(out,alias));catalog[[alias]]$id_kind<-'model'}
for(spec in list(c('next_gen_expression.csv','next_gen_expression.csv'),c('next_gen_copy_number.csv','next_gen_copy_number.csv'),c('next_gen_damaging.csv','next_gen_damaging.csv'),c('next_gen_hotspot.csv','next_gen_hotspot.csv'))){catalog[[spec[2]]]<-subset_matrix(spec[1],spec[2],meta$ModelID,'model')}
file.copy(file.path(out,'next_gen_expression.csv'),file.path(out,'OmicsExpressionProteinCodingGenesTPMLogp1.csv'),overwrite=TRUE)
file.copy(file.path(out,'next_gen_copy_number.csv'),file.path(out,'OmicsCNGene.csv'),overwrite=TRUE)
file.copy(file.path(out,'next_gen_damaging.csv'),file.path(out,'OmicsSomaticMutationsDamaging.csv'),overwrite=TRUE)
file.copy(file.path(out,'next_gen_hotspot.csv'),file.path(out,'OmicsSomaticMutationsHotspot.csv'),overwrite=TRUE)
file.copy(file.path(source_root,'geneset_table_updated.csv'),file.path(out,'geneset_table_updated.csv'),overwrite=TRUE)
file.copy(file.path(source_root,'crispr_control_genes.csv'),file.path(out,'crispr_control_genes.csv'),overwrite=TRUE)
rows<-rbindlist(lapply(names(catalog),function(n)data.table(file=n,rows=catalog[[n]]$rows,columns=catalog[[n]]$columns,id_kind=catalog[[n]]$id_kind)));fwrite(rows,file.path(out,'matrix_catalog.csv'))
manifest<-list(schema_version=1,family='tm00_3d_standardized_inputs',status='complete',qa_status='PASS',screen_count=nrow(meta),model_count=uniqueN(meta$ModelID),screen_types=as.list(meta[,.N,by=ScreenType]),lineage_counts=as.list(meta[,.N,by=OncotreeLineage]),matrix_count=nrow(rows),source_root=source_root,output_root=normalizePath(out),notes='TM00-compatible aliases plus 3D ScreenID/ModelID mappings; only QC-passing 3DO and 3DN samples retained')
write_json(manifest,file.path(out,'manifest.json'),auto_unbox=TRUE,pretty=TRUE);message('Prepared ',nrow(meta),' 3D models at ',out)
