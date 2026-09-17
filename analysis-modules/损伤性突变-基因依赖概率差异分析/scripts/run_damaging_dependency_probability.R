#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table); library(jsonlite)})
# Two-sided Welch test, matching TM00 07. Missing mutation calls are not controls.
welch <- function(m, d) {
  positive <- !is.na(m) & m > 0
  negative <- !is.na(m) & m == 0
  available <- is.finite(d)
  offset <- colMeans(d, na.rm=TRUE); offset[!is.finite(offset)] <- 0
  y <- sweep(d,2,offset,"-"); y[!available] <- 0
  moments <- function(g) {
    n <- crossprod(g * 1, available * 1)
    s <- crossprod(g * 1, y)
    ss <- crossprod(g * 1, y^2)
    list(n=n, mean=sweep(s/n,2,offset,"+"), variance=pmax(0,(ss-s^2/n)/(n-1)))
  }
  a <- moments(positive); b <- moments(negative)
  delta <- a$mean-b$mean
  va <- a$variance/a$n; vb <- b$variance/b$n
  se <- sqrt(va+vb)
  df <- (va+vb)^2/(va^2/(a$n-1)+vb^2/(b$n-1))
  valid <- a$n>=5 & b$n>=10 & is.finite(df) & df>0 &
    se >= 10*.Machine$double.eps*pmax(abs(a$mean),abs(b$mean)) & se>0
  p <- matrix(NA_real_, nrow=ncol(m), ncol=ncol(d))
  p[valid] <- 2*pt(-abs(delta[valid]/se[valid]),df[valid])
  list(mutation_n=a$n, control_n=b$n, mutation_mean=a$mean,
       control_mean=b$mean, difference=delta, p_two_sided=p)
}
test_welch <- function() {
  set.seed(7)
  m <- matrix(c(rep(1,7),rep(0,15),NA),ncol=1)
  d <- matrix(runif(23*3),ncol=3); d[1,2] <- NA; d[23,] <- 1
  z <- welch(m,d)
  for(j in 1:3) {
    a <- d[which(m[,1]>0),j]; b <- d[which(m[,1]==0),j]
    stopifnot(isTRUE(all.equal(z$p_two_sided[1,j],t.test(a,b)$p.value,tolerance=1e-10)))
  }
  near_one <- matrix(1-runif(23)*1e-7,ncol=1)
  stopifnot(abs(welch(m,near_one)$p_two_sided[1,1] -
    t.test(near_one[which(m[,1]>0),1],near_one[which(m[,1]==0),1])$p.value)<1e-7)
  d[1:4,2] <- NA
  stopifnot(is.na(welch(m,d)$p_two_sided[1,2]))
  stopifnot(all(is.na(welch(m,matrix(1,23,1))$p_two_sided)))
  cat("Welch reference / missing calls / effective counts / constant input: PASS\n")
}
test_welch()
args <- commandArgs(TRUE)
if (identical(args,"--test")) quit(status=0)
stopifnot(length(args)==2)
source_root <- normalizePath(args[1]); out <- args[2]
if(file.exists(file.path(out,"manifest.json"))) stop("Existing run: choose a new output directory")
dir.create(out,recursive=TRUE,showWarnings=FALSE)
dir.create(file.path(out,"blocks"),showWarnings=FALSE)
paths <- file.path(source_root,c("OmicsSomaticMutationsMatrixDamaging.csv","CRISPRGeneDependency.csv","Model.csv"))
hash <- as.list(tools::md5sum(paths))
mut <- fread(paths[1]); dep <- fread(paths[2]); model <- fread(paths[3])
raw_mut_rows <- nrow(mut)
mut <- mut[IsDefaultEntryForModel=="Yes"]
stopifnot(!anyDuplicated(mut$ModelID), !anyDuplicated(dep[[1]]), !anyDuplicated(model$ModelID))
metadata <- c("V1","","SequencingID","ModelConditionID","ModelID","IsDefaultEntryForMC","IsDefaultEntryForModel")
cols <- setdiff(names(mut),metadata)
clean <- function(x) sub(" \\([0-9]+\\)$","",x)
genes <- clean(cols); targets <- clean(names(dep)[-1])
stopifnot(!anyDuplicated(genes),!anyDuplicated(targets))
ids <- Reduce(intersect,list(as.character(dep[[1]]),mut$ModelID,model$ModelID))
m <- as.matrix(mut[match(ids,mut$ModelID),..cols])
d <- as.matrix(dep[match(ids,dep[[1]]),-1,with=FALSE])
storage.mode(m)<-"double"; storage.mode(d)<-"double"
stopifnot(all(m[!is.na(m)] %in% 0:2),all(d[is.finite(d)]>=0 & d[is.finite(d)]<=1))
fwrite(data.table(ModelID=ids),file.path(out,"sample_order.csv"))
fwrite(model[match(ids,ModelID)],file.path(out,"sample_annotations.csv"))
universe <- unique(c(as.character(dep[[1]]),mut$ModelID,model$ModelID))
audit <- data.table(ModelID=universe,has_dependency=universe %in% dep[[1]],
  has_default_mutation=universe %in% mut$ModelID,has_model=universe %in% model$ModelID)
fwrite(audit,file.path(out,"sample_availability.csv"))
counts <- data.table(symbol=genes,mutation_n=colSums(m>0,na.rm=TRUE),
  control_n=colSums(m==0,na.rm=TRUE),unknown_n=colSums(is.na(m)))
counts[,eligible:=mutation_n>=5 & control_n>=10]
fwrite(counts,file.path(out,"all_mutation_gene_counts.csv"))
eligible <- which(counts$eligible)
fwrite(data.table(index=seq_along(eligible),symbol=genes[eligible]),file.path(out,"mutation_gene_order.csv"))
fwrite(data.table(index=seq_along(targets),symbol=targets),file.path(out,"target_gene_order.csv"))
qc <- list(common_sample_count=length(ids),raw_mutation_rows=raw_mut_rows,
 default_mutation_rows=nrow(mut),model_rows=nrow(model),dependency_rows=nrow(dep),
 mutation_missing=sum(is.na(m)),dependency_missing=sum(!is.finite(d)),
 eligible_mutation_genes=length(eligible),target_genes=length(targets))
write_json(qc,file.path(out,"input_qc.json"),auto_unbox=TRUE,pretty=TRUE)
print(qc)
# Real-data pilot: compare all valid targets for ARID1A and all eligible sources for HMGCR with base R.
pilot <- function(source_indices,target_indices,name) {
  z <- welch(m[,source_indices,drop=FALSE],d[,target_indices,drop=FALSE])
  rows <- list(); k<-0L
  for(i in seq_along(source_indices)) for(j in seq_along(target_indices)) {
    a <- d[which(m[,source_indices[i]]>0),target_indices[j]]
    b <- d[which(m[,source_indices[i]]==0),target_indices[j]]
    ref <- if(sum(is.finite(a))>=5 && sum(is.finite(b))>=10)
      tryCatch(t.test(a,b)$p.value,error=function(e) NA_real_) else NA_real_
    got <- z$p_two_sided[i,j]
    stopifnot(identical(is.na(ref),is.na(got)))
    if(is.finite(ref)) stopifnot(abs(ref-got)<1e-7)
    k<-k+1L
    rows[[k]]<-data.table(MutGene=genes[source_indices[i]],TargetGene=targets[target_indices[j]],
      Mut_n=z$mutation_n[i,j],Control_n=z$control_n[i,j],Difference=z$difference[i,j],pvalue=got)
  }
  tab<-rbindlist(rows); tab[,FDR:=p.adjust(pvalue,"BH")]
  fwrite(tab,file.path(out,paste0(name,".csv")))
}
pilot(match("ARID1A",genes),seq_along(targets),"ARID1A_batch_target_screening")
pilot(eligible,match("HMGCR",targets),"HMGCR_batch_mutation_screening")
cat("Real-data forward and reverse pilot vs t.test: PASS\n")
p_all <- matrix(NA_real_,length(eligible),length(targets))
blocks <- split(seq_along(eligible),ceiling(seq_along(eligible)/32))
for(b in seq_along(blocks)) {
  idx<-blocks[[b]]
  z<-welch(m[,eligible[idx],drop=FALSE],d)
  z$source_symbols<-genes[eligible[idx]]; z$target_symbols<-targets
  z$fdr_forward<-t(apply(z$p_two_sided,1,p.adjust,method="BH"))
  p_all[idx,]<-z$p_two_sided
  saveRDS(z,file.path(out,"blocks",sprintf("block_%04d.rds",b)),compress=FALSE)
  cat(sprintf("Block %d/%d completed\n",b,length(blocks))); flush.console()
}
# Separate column-wise BH family for reverse queries; do not reuse forward FDR.
for(j in seq_len(ncol(p_all))) p_all[,j]<-p.adjust(p_all[,j],"BH")
for(b in seq_along(blocks)) {
  saveRDS(list(source_symbols=genes[eligible[blocks[[b]]]],target_symbols=targets,
    fdr_reverse=p_all[blocks[[b]],,drop=FALSE]),
    file.path(out,"blocks",sprintf("reverse_fdr_%04d.rds",b)),compress=FALSE)
}
manifest<-c(qc,list(status="complete",release="26Q1",input_md5=hash,
 completed_at=as.character(Sys.time()),method="Two-sided Welch t-test; per-pair Mut>=5 control>=10",
 mutation_definition="Damaging >0 vs ==0; NA excluded; default model entry",
 dependency_metric="CRISPRGeneDependency probability; positive Mut-control difference means more dependent",
 fdr_forward="BH across targets within mutation source",
 fdr_reverse="BH across eligible mutation sources within target",
 validation="synthetic edge cases and full ARID1A/HMGCR pilot against R t.test passed",
 reference="07_mutant_dependency_26Q1.R",block_count=length(blocks)))
write_json(manifest,file.path(out,"manifest.json"),auto_unbox=TRUE,pretty=TRUE)
cat("COMPLETE\n")
