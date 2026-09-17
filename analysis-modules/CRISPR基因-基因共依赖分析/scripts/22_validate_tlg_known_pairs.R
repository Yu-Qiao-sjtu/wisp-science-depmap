#!/usr/bin/env Rscript
suppressPackageStartupMessages(library(data.table))
a<-commandArgs(trailingOnly=TRUE);if(length(a)!=2L)stop('用法: 22_validate_tlg_known_pairs.R <knowledge-root> <CRISPRGeneEffect.csv>')
root<-normalizePath(a[[1L]],winslash='/',mustWork=TRUE);raw_path<-normalizePath(a[[2L]],winslash='/',mustWork=TRUE);full<-file.path(root,'depmap-26q1-full')
pairs<-data.table(gene_a=c('TSC1','CAD','ATIC','TP53','ASB7'),gene_b=c('TSC2','UMPS','PAICS','MDM2','SUV39H1'),expected=c('positive','positive','positive','negative','negative'))
header<-names(fread(raw_path,nrows=0));symbols<-sub(' \\(.*$','',header);wanted<-header[header=='ModelID'|symbols%in%c(pairs$gene_a,pairs$gene_b)];raw<-fread(raw_path,select=wanted);setnames(raw,names(raw),sub(' \\(.*$','',names(raw)))
ord<-fread(file.path(full,'effect_correlation','gene_order.csv'));genes<-toupper(ord$symbol);blocks<-list.files(file.path(full,'effect_correlation','blocks'),pattern='[.]rds$',full.names=TRUE)
read_pair<-function(a,b){i<-match(a,genes);j<-match(b,genes);for(q in blocks){x<-readRDS(q);if(i>=x$source_start&&i<=x$source_end)return(c(r=x$correlation[i-x$source_start+1L,j],n=x$pair_n[i-x$source_start+1L,j]))};stop('block missing')}
derived<-file.path(full,'true_love_gene','tm00_derived_catalogs_26Q1');neg<-fread(file.path(derived,'negative_codependency_r_lt_minus_0.3_n500.csv.gz'));pos<-fread(file.path(derived,'positive_reciprocal_top20_n500.csv.gz'))
out<-rbindlist(lapply(seq_len(nrow(pairs)),function(i){aa<-pairs$gene_a[i];bb<-pairs$gene_b[i];x<-raw[[aa]];y<-raw[[bb]];ok<-is.finite(x)&is.finite(y);direct<-cor(x[ok],y[ok]);mat<-read_pair(aa,bb);ph<-pos[(gene_a==aa&gene_b==bb)|(gene_a==bb&gene_b==aa)];nh<-neg[(gene_a==aa&gene_b==bb)|(gene_a==bb&gene_b==aa)];data.table(gene_a=aa,gene_b=bb,expected=pairs$expected[i],direct_r=direct,direct_n=sum(ok),matrix_r=mat[['r']],matrix_n=mat[['n']],derived_r=if(nrow(ph))ph$correlation_a_to_b[1] else if(nrow(nh))nh$correlation[1] else NA_real_,catalog=if(nrow(ph))'positive_reciprocal_n500' else if(nrow(nh))'negative_r_lt_minus_0.3_n500' else 'not_retained',absolute_difference=abs(direct-mat[['r']]))}))
stopifnot(all(out$absolute_difference<1e-12),all(out$direct_n==out$matrix_n),all(sign(out$direct_r)==ifelse(out$expected=='positive',1,-1)))
fwrite(out,file.path(derived,'known_pair_validation.csv'));print(out);cat('KNOWN_PAIR_VALIDATION=PASS\n')
