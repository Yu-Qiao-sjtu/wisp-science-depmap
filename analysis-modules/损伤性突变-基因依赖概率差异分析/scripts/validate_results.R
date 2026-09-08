suppressPackageStartupMessages({library(data.table);library(jsonlite)})
out<-commandArgs(TRUE)[1]
manifest<-read_json(file.path(out,"manifest.json"),simplifyVector=TRUE)
stopifnot(manifest$status=="complete")
paths<-list.files(file.path(out,"blocks"),"^block_.*rds$",full.names=TRUE)
reverse<-list.files(file.path(out,"blocks"),"^reverse_fdr_.*rds$",full.names=TRUE)
stopifnot(length(paths)==manifest$block_count,length(reverse)==length(paths))
targets<-fread(file.path(out,"target_gene_order.csv"))$symbol
sources<-fread(file.path(out,"mutation_gene_order.csv"))$symbol
h<-match("HMGCR",targets); ps<-fs<-c(); rows<-0; valid<-0
for(i in seq_along(paths)) {
 z<-readRDS(paths[i]); r<-readRDS(reverse[i])
 stopifnot(identical(z$source_symbols,r$source_symbols),
   identical(z$target_symbols,targets),identical(r$target_symbols,targets))
 stopifnot(all(z$mutation_n[is.finite(z$p_two_sided)]>=5),
   all(z$control_n[is.finite(z$p_two_sided)]>=10))
 stopifnot(isTRUE(all.equal(as.numeric(z$fdr_forward[1,]),
   p.adjust(z$p_two_sided[1,],"BH"))))
 ps<-c(ps,z$p_two_sided[,h]);fs<-c(fs,r$fdr_reverse[,h])
 rows<-rows+length(z$source_symbols);valid<-valid+sum(is.finite(z$p_two_sided))
 if("ARID1A" %in% z$source_symbols) {
   k<-match("ARID1A",z$source_symbols)
   tab<-data.table(TargetGene=targets,Mut_n=z$mutation_n[k,],Control_n=z$control_n[k,],
     Difference=z$difference[k,],pvalue=z$p_two_sided[k,],FDR=z$fdr_forward[k,])
   pilot<-fread(file.path(out,"ARID1A_batch_target_screening.csv"))
   stopifnot(max(abs(tab$pvalue-pilot$pvalue),na.rm=TRUE)<1e-7)
   fwrite(tab[order(FDR)][1:20],file.path(out,"ARID1A_top20_by_FDR.csv"))
 }
}
stopifnot(rows==length(sources),isTRUE(all.equal(fs,p.adjust(ps,"BH"))))
report<-list(status="PASS",block_count=length(paths),source_count=rows,
 target_count=length(targets),valid_tests=valid,
 checks=c("all blocks and axes","effective group thresholds","forward BH sampled per block",
 "HMGCR reverse BH across all sources","ARID1A full vs pilot"))
write_json(report,file.path(out,"validation.json"),auto_unbox=TRUE,pretty=TRUE)
print(report)
