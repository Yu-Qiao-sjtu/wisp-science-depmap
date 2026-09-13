#!/usr/bin/env Rscript
suppressPackageStartupMessages({library(data.table);library(jsonlite)})
parse<-function(a){z<-list(limit=20L);i<-1L;while(i<=length(a)){k<-sub("^--","",a[[i]]);if(i==length(a))stop("missing value for ",a[[i]]);z[[gsub("-","_",k)]]<-a[[i+1L]];i<-i+2L};z$limit<-as.integer(z$limit);z}
clean<-function(x)toupper(trimws(x));emit<-function(x)cat(toJSON(x,auto_unbox=TRUE,pretty=TRUE,na="null"),"\n");a<-parse(commandArgs(trailingOnly=TRUE));if(is.null(a$kb_root)||is.null(a$mode))stop("--kb-root and --mode are required")
decode_lineage_codepoints<-function(x){
  parts<-strsplit(trimws(x),"-",fixed=TRUE)[[1L]]
  values<-strtoi(parts,base=16L)
  if(!length(values)||anyNA(values)||any(values<0L|values>0x10FFFFL))stop("invalid --lineage-codepoints value")
  intToUtf8(values)
}
canonical_lineage_codepoints<-function(x){
  aliases<-c(
    "80BE-4E0A-817A"="Adrenal Gland","56-61-74-65-72-58F6-8179"="Ampulla of Vater",
    "80C6-9053"="Biliary Tract","8180-80F1"="Bladder Urinary Tract","9AA8"="Bone",
    "80A0-9053"="Bowel","4E73-817A"="Breast","5BAB-9888"="Cervix",
    "4E2D-67A2-795E-7ECF-7CFB-7EDF"="CNS Brain","80DA-80CE-6027"="Embryonal",
    "98DF-7BA1"="Esophagus Stomach","773C"="Eye","6210-7EA4-7EF4-7EC6-80DE"="Fibroblast",
    "6BDB-53D1"="Hair","5934-9888"="Head and Neck","80BE"="Kidney","809D"="Liver",
    "80BA"="Lung","6DCB-5DF4"="Lymphoid","808C-8089"="Muscle","9AD3-7CFB"="Myeloid",
    "6B63-5E38"="Normal","5176-4ED6"="Other","5375-5DE2"="Ovary Fallopian Tube",
    "80F0-817A"="Pancreas","5916-5468-795E-7ECF-7CFB-7EDF"="Peripheral Nervous System",
    "80F8-819C"="Pleura","524D-5217-817A"="Prostate","76AE-80A4"="Skin",
    "8F6F-7EC4-7EC7"="Soft Tissue","777E-4E38"="Testis","7532-72B6-817A"="Thyroid",
    "5B50-5BAB"="Uterus","5916-9634"="Vulva Vagina")
  hit<-unname(aliases[toupper(trimws(x))])
  if(length(hit)&&!is.na(hit))hit else decode_lineage_codepoints(x)
}
if(!is.null(a$lineage_codepoints))a$lineage<-canonical_lineage_codepoints(a$lineage_codepoints)
lineage_match_key<-function(x)tolower(gsub("[^A-Za-z0-9]+","",trimws(x)))
unicode_alias_key<-function(x)tolower(gsub("[[:space:]_/,()\\-]+","",trimws(x),perl=TRUE))
canonical_lineage<-function(x){
  requested<-trimws(x);base<-trimws(gsub("[[:space:]]+(cancer|carcinoma|tumors?|lineage)$","",requested,ignore.case=TRUE))
  chinese_aliases<-c(
    "肾上腺"="Adrenal Gland","肾上腺癌"="Adrenal Gland","肾上腺肿瘤"="Adrenal Gland",
    "Vater壶腹"="Ampulla of Vater","壶腹部"="Ampulla of Vater","壶腹癌"="Ampulla of Vater","壶腹部癌"="Ampulla of Vater",
    "胆道"="Biliary Tract","胆道癌"="Biliary Tract","胆管癌"="Biliary Tract","胆囊癌"="Biliary Tract",
    "膀胱"="Bladder Urinary Tract","膀胱癌"="Bladder Urinary Tract","尿路"="Bladder Urinary Tract","尿路癌"="Bladder Urinary Tract","尿路上皮癌"="Bladder Urinary Tract",
    "骨"="Bone","骨癌"="Bone","骨肿瘤"="Bone","骨肉瘤"="Bone",
    "肠道"="Bowel","肠癌"="Bowel","大肠癌"="Bowel","结肠癌"="Bowel","直肠癌"="Bowel","结直肠癌"="Bowel",
    "乳腺"="Breast","乳腺癌"="Breast","乳癌"="Breast",
    "宫颈"="Cervix","宫颈癌"="Cervix","子宫颈癌"="Cervix",
    "中枢神经系统"="CNS Brain","脑"="CNS Brain","脑癌"="CNS Brain","脑肿瘤"="CNS Brain","胶质瘤"="CNS Brain",
    "胚胎性"="Embryonal","胚胎性肿瘤"="Embryonal","胚胎肿瘤"="Embryonal",
    "食管"="Esophagus Stomach","食管癌"="Esophagus Stomach","胃"="Esophagus Stomach","胃癌"="Esophagus Stomach","食管胃"="Esophagus Stomach","食管胃癌"="Esophagus Stomach","胃食管癌"="Esophagus Stomach",
    "眼"="Eye","眼部"="Eye","眼部肿瘤"="Eye","眼癌"="Eye","葡萄膜黑色素瘤"="Eye",
    "成纤维细胞"="Fibroblast","成纤维细胞系"="Fibroblast","毛发"="Hair","毛囊"="Hair",
    "头颈"="Head and Neck","头颈癌"="Head and Neck","口腔癌"="Head and Neck","咽癌"="Head and Neck","喉癌"="Head and Neck",
    "肾"="Kidney","肾癌"="Kidney","肾脏癌"="Kidney","肾细胞癌"="Kidney",
    "肝"="Liver","肝癌"="Liver","肝脏癌"="Liver","肝脏肿瘤"="Liver",
    "肺"="Lung","肺癌"="Lung","肺部肿瘤"="Lung",
    "淋巴"="Lymphoid","淋巴系统"="Lymphoid","淋巴系统肿瘤"="Lymphoid","淋巴瘤"="Lymphoid","淋巴细胞白血病"="Lymphoid",
    "肌肉"="Muscle","肌肉肿瘤"="Muscle","髓系"="Myeloid","髓系肿瘤"="Myeloid","髓系白血病"="Myeloid","急性髓系白血病"="Myeloid",
    "正常"="Normal","正常组织"="Normal","正常细胞"="Normal","其他"="Other","其他肿瘤"="Other",
    "卵巢"="Ovary Fallopian Tube","卵巢癌"="Ovary Fallopian Tube","输卵管"="Ovary Fallopian Tube","输卵管癌"="Ovary Fallopian Tube","卵巢输卵管"="Ovary Fallopian Tube",
    "胰腺"="Pancreas","胰腺癌"="Pancreas","胰癌"="Pancreas",
    "外周神经系统"="Peripheral Nervous System","周围神经系统"="Peripheral Nervous System","外周神经系统肿瘤"="Peripheral Nervous System","神经母细胞瘤"="Peripheral Nervous System",
    "胸膜"="Pleura","胸膜肿瘤"="Pleura","胸膜间皮瘤"="Pleura","间皮瘤"="Pleura",
    "前列腺"="Prostate","前列腺癌"="Prostate",
    "皮肤"="Skin","皮肤癌"="Skin","皮肤肿瘤"="Skin","黑色素瘤"="Skin",
    "软组织"="Soft Tissue","软组织肿瘤"="Soft Tissue","软组织肉瘤"="Soft Tissue",
    "睾丸"="Testis","睾丸癌"="Testis","睾丸肿瘤"="Testis",
    "甲状腺"="Thyroid","甲状腺癌"="Thyroid","甲状腺肿瘤"="Thyroid",
    "子宫"="Uterus","子宫癌"="Uterus","子宫体癌"="Uterus","子宫内膜癌"="Uterus",
    "外阴"="Vulva Vagina","外阴癌"="Vulva Vagina","阴道"="Vulva Vagina","阴道癌"="Vulva Vagina","外阴阴道"="Vulva Vagina")
  chinese<-unname(chinese_aliases[unicode_alias_key(requested)])
  if(length(chinese)&&!is.na(chinese))return(chinese)
  aliases<-c(colorectal="Bowel",colon="Bowel",rectal="Bowel",brain="CNS Brain",cns="CNS Brain",centralnervoussystem="CNS Brain",ovarian="Ovary Fallopian Tube",ovary="Ovary Fallopian Tube",fallopiantube="Ovary Fallopian Tube",gastric="Esophagus Stomach",stomach="Esophagus Stomach",esophageal="Esophagus Stomach",bladder="Bladder Urinary Tract",urinarytract="Bladder Urinary Tract")
  canonical<-c("Adrenal Gland","Ampulla of Vater","Biliary Tract","Bladder Urinary Tract","Bone","Bowel","Breast","Cervix","CNS Brain","Embryonal","Esophagus Stomach","Eye","Fibroblast","Hair","Head and Neck","Kidney","Liver","Lung","Lymphoid","Muscle","Myeloid","Normal","Other","Ovary Fallopian Tube","Pancreas","Peripheral Nervous System","Pleura","Prostate","Skin","Soft Tissue","Testis","Thyroid","Uterus","Vulva Vagina")
  hit<-canonical[lineage_match_key(canonical)==lineage_match_key(base)]
  if(length(hit))return(hit[[1L]])
  alias<-unname(aliases[lineage_match_key(base)])
  if(length(alias)&&!is.na(alias))alias else requested
}
if(!is.null(a$lineage))a$lineage<-canonical_lineage(a$lineage)
if(a$mode=="normalize_lineage"){emit(list(mode=a$mode,lineage=a$lineage));quit(save="no")}
kb<-normalizePath(a$kb_root,winslash="/",mustWork=TRUE);full<-file.path(kb,"depmap-26q1-full");core<-file.path(kb,"depmap-26q1-core")
mutation_module_root<-file.path(kb,"analysis-modules","癌种内突变锚定基因选择","cancer_anchor_catalog_v2","downstream_dependency","05_precomputed_gene_effect_matrices")
mutation_modules<-c("damaging_mutation_dependency","custom_missense_mutation_dependency","hotspot_mutation_dependency","lineage_damaging_mutation_dependency","lineage_custom_missense_mutation_dependency","lineage_hotspot_mutation_dependency","observational_synthetic_lethal_candidates")
resolve_module_root<-function(module){
  relocated<-file.path(mutation_module_root,module)
  if(module%in%mutation_modules&&dir.exists(relocated))relocated else file.path(full,module)
}
if(a$mode=="catalog"){
  installed<-if(dir.exists(full))basename(list.dirs(full,recursive=FALSE,full.names=TRUE))else character()
  relocated<-if(dir.exists(mutation_module_root))basename(list.dirs(mutation_module_root,recursive=FALSE,full.names=TRUE))else character()
  emit(list(
    qa=fromJSON(file.path(kb,"depmap-26q1-qa.json")),
    modules=fread(file.path(kb,"depmap-26q1-module-catalog.csv")),
    installed_full_modules=sort(unique(c(installed,relocated)))
  ))
  quit(save="no")
}
if(a$mode=="core"){if(!requireNamespace("arrow",quietly=TRUE))stop("arrow package required");g<-clean(a$gene);s<-as.data.table(arrow::read_parquet(file.path(core,"gene_core_summary.parquet")));hit<-s[toupper(symbol)==g];blocks<-list.files(file.path(core,"lineage_blocks"),full.names=TRUE);lin<-rbindlist(lapply(blocks,function(p){x<-as.data.table(arrow::read_parquet(p));x[toupper(symbol)==g]}),fill=TRUE);emit(list(mode="core",gene=g,summary=hit,lineages=lin,provenance=c(file.path(core,"gene_core_summary.parquet"),file.path(core,"lineage_blocks"))));quit(save="no")}
if(a$mode=="tcga_expression_survival"){
  if(!requireNamespace("arrow",quietly=TRUE))stop("arrow package required")
  root<-file.path(kb,"depmap-26q1-tcga");qa_path<-file.path(root,"qa.json")
  gene<-clean(a$gene);endpoint<-toupper(if(is.null(a$endpoint))"OS" else a$endpoint)
  if(!endpoint%in%c("OS","DSS","DFI","PFI"))stop("endpoint must be OS, DSS, DFI, or PFI")
  if(!file.exists(qa_path)){emit(list(mode=a$mode,status="MODULE_UNAVAILABLE",reason="the TCGA expression-survival bridge is not installed",gene=gene,endpoint=endpoint,provenance=root));quit(save="no")}
  qa<-fromJSON(qa_path,simplifyVector=FALSE)
  if(!identical(qa$status,"PASS")){emit(list(mode=a$mode,status="INELIGIBLE",reason="the installed TCGA expression-survival bridge has not passed QA",gene=gene,endpoint=endpoint,manifest=qa,provenance=qa_path));quit(save="no")}
  catalog_path<-file.path(root,"project_catalog.csv");catalog<-fread(catalog_path)
  project<-if(is.null(a$project))NA_character_ else toupper(trimws(a$project));if(!is.na(project)&&!startsWith(project,"TCGA-"))project<-paste0("TCGA-",project)
  selected<-catalog[status=="complete"]
  if(!is.na(project))selected<-selected[tcga_project==project]
  if(!is.null(a$lineage))selected<-selected[depmap_lineage==a$lineage]
  if(!nrow(selected)){emit(list(mode=a$mode,status="NOT_COMPUTED",reason="no completed TCGA project matches the request",gene=gene,project=project,lineage=a$lineage,endpoint=endpoint,manifest=qa,provenance=c(catalog_path,qa_path)));quit(save="no")}
  prefix<-paste0("expression_",tolower(endpoint));columns<-c("symbol","hgnc_id","entrez_id","ensembl_gene_id","tcga_project","depmap_lineage","expression_available","expression_mapping_basis","expression_n","expression_median_log2_tpm",paste0(prefix,c("_n","_events","_score_z","_p_value","_fdr")))
  rows<-rbindlist(lapply(selected$tcga_project,function(project_name){
    project_root<-file.path(root,"projects",project_name);path<-file.path(project_root,"gene_associations.parquet")
    if(!file.exists(path))return(NULL)
    manifest_path<-file.path(project_root,"manifest.json");manifest<-if(file.exists(manifest_path))fromJSON(manifest_path,simplifyVector=FALSE) else list()
    x<-as.data.table(arrow::read_parquet(path,col_select=columns));hit<-x[toupper(symbol)==gene&expression_available==TRUE]
    if(nrow(hit)){hit[,min_n:=as.integer(if(is.null(manifest$min_n))30L else manifest$min_n)];hit[,min_events:=as.integer(if(is.null(manifest$min_events))10L else manifest$min_events)]}
    hit
  }),fill=TRUE)
  limit<-if(is.null(a$limit))100L else as.integer(a$limit);if(nrow(rows)>limit)rows<-rows[seq_len(limit)]
  provenance<-c(qa_path,catalog_path,unlist(lapply(selected$tcga_project,function(project_name)c(file.path(root,"projects",project_name,"manifest.json"),file.path(root,"projects",project_name,"gene_associations.parquet")))))
  if(!nrow(rows)){emit(list(mode=a$mode,status="NOT_COMPUTED",reason="the gene is absent from the matched TCGA expression universe",gene=gene,project=project,lineage=a$lineage,endpoint=endpoint,manifest=qa,provenance=provenance));quit(save="no")}
  n_col<-paste0(prefix,"_n");event_col<-paste0(prefix,"_events");score_col<-paste0(prefix,"_score_z")
  rows[,association_status:=fifelse(is.na(get(n_col))|is.na(get(event_col)),"NOT_COMPUTED",fifelse(get(n_col)<min_n|get(event_col)<min_events|is.na(get(score_col)),"INELIGIBLE","FOUND"))]
  response_status<-if(any(rows$association_status=="FOUND"))"FOUND" else "INELIGIBLE"
  emit(list(mode=a$mode,status=response_status,reason=if(response_status=="FOUND")"bounded precomputed TCGA expression-survival rows found" else "expression is available, but no selected project has a testable survival association",gene=gene,project=project,lineage=a$lineage,endpoint=endpoint,rows=rows,summary=list(matched_project_count=nrow(rows),selected_project_count=nrow(selected),target_gene_count=qa$target_gene_count,association_status_counts=as.list(table(rows$association_status))),manifest=list(release=qa$release,method="Breslow univariate Cox score test at beta=0",multiple_testing="BH FDR within TCGA project and survival endpoint",endpoint=endpoint,expression_scale="log2(TPM+1)"),provenance=provenance));quit(save="no")
}
matrix_specs<-list(
 effect_correlation=list(source="gene_order.csv",target="gene_order.csv",value="correlation",n="pair_n",p=NULL,fdr=NULL),
 expression_correlation=list(source="gene_order.csv",target="gene_order.csv",value="correlation",n=NULL,p=NULL,fdr=NULL),
 expression_dependency=list(source="expression_gene_order.csv",target="dependency_gene_order.csv",value="correlation",n="pair_n",p=NULL,fdr=NULL),
 damaging_mutation_dependency=list(source="mutation_gene_order.csv",target="target_gene_order.csv",value="mean_difference",n="mutation_n",p="p_mutation_more_dependent",fdr="fdr_mutation_more_dependent"),
 custom_missense_mutation_dependency=list(source="mutation_gene_order.csv",target="target_gene_order.csv",value="mean_difference",n="mutation_n",p="p_mutation_more_dependent",fdr="fdr_mutation_more_dependent"),
 hotspot_mutation_dependency=list(source="mutation_gene_order.csv",target="target_gene_order.csv",value="mean_difference",n="mutation_n",p="p_mutation_more_dependent",fdr="fdr_mutation_more_dependent"),
 cnv_amplification_dependency=list(source="cnv_gene_order.csv",target="target_gene_order.csv",value="mean_difference",n="amplified_n",p="p_amplified_more_dependent",fdr="fdr_amplified_more_dependent"))
read_cell<-function(root,spec,source,target,top=FALSE,limit=20L){sp<-file.path(root,spec$source);tp<-file.path(root,spec$target);if(!file.exists(tp))tp<-file.path(dirname(root),spec$target);s<-fread(sp);t<-fread(tp);i<-which(clean(s$symbol)==clean(source));if(!length(i))stop("source not found or ineligible: ",source);paths<-list.files(file.path(root,"blocks"),pattern="^block_[0-9]+_[0-9]+\\.rds$",full.names=TRUE);bounds<-t(vapply(basename(paths),function(q){q<-sub("\\.rds$","",sub("^block_","",q));as.integer(strsplit(q,"_",fixed=TRUE)[[1L]])},integer(2)));hit<-which(bounds[,1L]<=i&bounds[,2L]>=i);if(!length(hit))stop("source block not found");p<-paths[hit[1L]];st<-bounds[hit[1L],1L];x<-readRDS(p);k<-i-st+1L;v<-as.numeric(x[[spec$value]][k,]);n<-if(!is.null(spec$n)&&!is.null(x[[spec$n]]))as.numeric(x[[spec$n]][k,]) else rep(NA_real_,length(v));pv<-if(!is.null(spec$p))as.numeric(x[[spec$p]][k,]) else rep(NA_real_,length(v));fd<-if(!is.null(spec$fdr))as.numeric(x[[spec$fdr]][k,]) else rep(NA_real_,length(v));z<-data.table(target=t$symbol,value=v,n=n,p_value=pv,fdr=fd);if(top){z<-z[is.finite(value)&(is.na(n)|n>=10)][order(-abs(value))][seq_len(min(limit,.N))]}else{j<-which(clean(t$symbol)==clean(target));if(!length(j))stop("target not found: ",target);z<-z[j,]};list(result=z,provenance=p,source_index=i)}
if(a$mode%in%c("pair","top")){spec<-matrix_specs[[a$module]];if(is.null(spec))stop("unsupported module");root<-resolve_module_root(a$module);q<-read_cell(root,spec,a$source,a$target,top=a$mode=="top",limit=a$limit);emit(c(list(mode=a$mode,module=a$module,source=clean(a$source)),q));quit(save="no")}
if(a$mode=="lineage"){map<-c(damaging="lineage_damaging_mutation_dependency",custom_missense="lineage_custom_missense_mutation_dependency",hotspot="lineage_hotspot_mutation_dependency");module<-unname(map[a$event]);if(is.na(module))stop("event");root<-file.path(resolve_module_root(module),gsub("[^A-Za-z0-9]+","_",a$lineage));if(!dir.exists(root)){emit(list(mode="lineage",status="not_testable",event=a$event,lineage=a$lineage));quit(save="no")};spec<-matrix_specs[[sub("^lineage_","",module)]];spec$source<-"mutation_gene_order.csv";q<-tryCatch(read_cell(root,spec,a$source,a$target),error=function(e)NULL);if(is.null(q)){emit(list(mode="lineage",status="not_testable",reason="source event did not satisfy Mut>=3 and WT>=5",event=a$event,lineage=a$lineage,source=a$source,target=a$target))}else emit(c(list(mode="lineage",status="tested",event=a$event,lineage=a$lineage,source=clean(a$source)),q));quit(save="no")}
if(a$mode=="pathway"){p<-file.path(full,"progeny_dependency","progeny_pathway_dependency_associations.csv");x<-fread(p);hit<-x[toupper(pathway)==toupper(a$pathway)&clean(target_gene)==clean(a$target)];emit(list(mode="pathway",result=hit,provenance=p));quit(save="no")}
if(a$mode=="drug"){module<-paste0("prism_auc_",a$omic,"_correlation");root<-file.path(full,module);d<-fread(file.path(root,"drug_order.csv"));g<-fread(file.path(root,"feature_gene_order.csv"));i<-which(toupper(d$CompoundID)==toupper(a$drug)|toupper(d$ConditionCompoundName)==toupper(a$drug));j<-which(clean(g$symbol)==clean(a$target));if(!length(i)||!length(j))stop("drug or target not found");st<-floor((i-1L)/16L)*16L+1L;en<-min(st+15L,nrow(d));p<-file.path(root,"blocks",sprintf("block_%05d_%05d.rds",st,en));x<-readRDS(p);k<-i-st+1L;emit(list(mode="drug",drug=d[i],target=g$symbol[j],omic=a$omic,n=x$n[k,j],pearson_r=x$pearson_r[k,j],p_value=x$p_value[k,j],fdr=x$fdr[k,j],provenance=p));quit(save="no")}

sparse_modes<-c("lineage_catalog","lineage_dependency","lineage_network","lineage_cnv","lineage_drug","enrichment")
if(a$mode%in%sparse_modes){
  if(!requireNamespace("arrow",quietly=TRUE))stop("arrow package required for sparse lineage queries")
  lineage_key<-function(x)gsub("^_+|_+$","",gsub("[^A-Za-z0-9]+","_",trimws(x)))
  manifest_at<-function(root){
    p<-file.path(root,"manifest.json")
    if(!file.exists(p))return(NULL)
    fromJSON(p,simplifyVector=FALSE)
  }
  evidence<-function(status,mode,reason,...,manifest=NULL,provenance=character()){
    c(list(mode=mode,status=status,reason=reason),list(...),list(manifest=manifest,provenance=provenance))
  }
  resolve_lineage<-function(module,lineage,mode){
    module_root<-file.path(full,module)
    if(!dir.exists(module_root))return(list(gap=evidence("MODULE_UNAVAILABLE",mode,"the requested precomputed module is not installed",module=module,lineage=lineage)))
    root<-file.path(module_root,lineage_key(lineage))
    if(!dir.exists(root))return(list(gap=evidence("NOT_COMPUTED",mode,"no lineage output directory exists for this request",module=module,lineage=lineage,provenance=module_root)))
    manifest<-manifest_at(root)
    if(is.null(manifest))return(list(gap=evidence("NOT_COMPUTED",mode,"the lineage output has no manifest",module=module,lineage=lineage,provenance=root)))
    if(!identical(manifest$status,"complete"))return(list(gap=evidence("INELIGIBLE",mode,paste("lineage manifest status is",manifest$status),module=module,lineage=lineage,manifest=manifest,provenance=file.path(root,"manifest.json"))))
    list(root=root,manifest=manifest,gap=NULL)
  }
  if(a$mode=="lineage_catalog"){
    specs<-list(
      "network:effect_correlation"=file.path(full,"lineage_sparse_networks","effect_correlation"),
      "network:expression_correlation"=file.path(full,"lineage_sparse_networks","expression_correlation"),
      "network:expression_dependency"=file.path(full,"lineage_sparse_networks","expression_dependency"),
      "cnv:amplification_dependency"=file.path(full,"lineage_cnv_amplification_dependency"),
      "drug:effect"=file.path(full,"lineage_prism_associations","effect"),
      "drug:expression"=file.path(full,"lineage_prism_associations","expression"),
      "drug:cnv"=file.path(full,"lineage_prism_associations","cnv"),
      "enrichment:pathway_and_tf"=file.path(full,"lineage_gene_enrichment"))
    modules<-Map(function(label,module_root){
      root<-file.path(module_root,lineage_key(a$lineage));manifest<-manifest_at(root)
      if(!dir.exists(module_root))return(list(label=label,status="MODULE_UNAVAILABLE",reason="the requested precomputed module is not installed",manifest=NULL,provenance=module_root))
      if(is.null(manifest))return(list(label=label,status="NOT_COMPUTED",reason="no lineage output manifest exists for this module",manifest=NULL,provenance=module_root))
      complete<-identical(manifest$status,"complete")
      list(label=label,status=if(complete)"FOUND" else "INELIGIBLE",reason=if(complete)"complete lineage module is available" else paste("lineage manifest status is",manifest$status),manifest=manifest,provenance=file.path(root,"manifest.json"))
    },names(specs),unname(specs))
    tcga_catalog_path<-file.path(kb,"depmap-26q1-tcga","project_catalog.csv")
    tcga_projects<-if(file.exists(tcga_catalog_path))fread(tcga_catalog_path)[status=="complete"&depmap_lineage==a$lineage,tcga_project] else character()
    modules[[length(modules)+1L]]<-list(label="tcga:expression_survival",status=if(length(tcga_projects))"FOUND" else "NOT_COMPUTED",reason=if(length(tcga_projects))"completed TCGA expression-survival projects are available" else "no completed TCGA project maps to this DepMap lineage",projects=tcga_projects,manifest=list(project_count=length(tcga_projects)),provenance=tcga_catalog_path)
    available<-sum(vapply(modules,function(x)identical(x$status,"FOUND"),logical(1)))
    emit(list(mode="lineage_catalog",state=if(available)"precomputed_query" else "coverage_gap",lineage=a$lineage,modules=modules,summary=list(module_count=length(modules),available_module_count=available,coverage_gap_count=length(modules)-available),scope="lineage_availability_only",new_analysis_started=FALSE));quit(save="no")
  }
  if(a$mode=="lineage_dependency"){
    root<-file.path(core,"lineage_dependency_tests");manifest_path<-file.path(root,"manifest.json");manifest<-manifest_at(root)
    ranking<-tolower(if(is.null(a$ranking))"selective" else trimws(a$ranking));limit<-if(is.null(a$limit))20L else as.integer(a$limit)
    if(!ranking%in%c("selective","mean_dependency"))stop("ranking must be selective or mean_dependency")
    if(!dir.exists(root)){emit(evidence("MODULE_UNAVAILABLE",a$mode,"the precomputed lineage dependency-test module is not installed",lineage=a$lineage,ranking=ranking,provenance=root));quit(save="no")}
    lineage_file_key<-lineage_key(a$lineage);paths<-list.files(root,pattern=paste0("^[0-9]+_",lineage_file_key,"\\.parquet$"),full.names=TRUE)
    if(!length(paths)){emit(evidence("NOT_COMPUTED",a$mode,"no completed lineage-vs-rest dependency table matches this lineage",lineage=a$lineage,ranking=ranking,manifest=manifest,provenance=manifest_path));quit(save="no")}
    compact_manifest<-if(is.null(manifest))NULL else list(schema_version=manifest$schema_version,release=manifest$release,method=manifest$method,effect_interpretation=manifest$effect_interpretation,lineage_count=manifest$lineage_count,gene_count=manifest$gene_count)
    p<-paths[[1L]];x<-as.data.table(arrow::read_parquet(p));tested<-x[test_status=="tested"&is.finite(effect_mean_lineage)]
    if(ranking=="selective"){
      candidates<-tested[is.finite(fdr_lineage_more_dependent)&fdr_lineage_more_dependent<=0.05&is.finite(effect_mean_difference)&effect_mean_difference<0]
      setorder(candidates,rank_more_dependent,effect_mean_difference,na.last=TRUE)
      ranking_rule<-"tested genes with one-sided within-lineage BH FDR <= 0.05 and negative lineage-minus-rest Gene Effect difference, ordered by precomputed rank_more_dependent"
    }else{
      candidates<-copy(tested);setorder(candidates,effect_mean_lineage,effect_mean_difference,na.last=TRUE)
      ranking_rule<-"tested genes ordered by ascending descriptive lineage mean Gene Effect; no lineage-vs-rest significance filter"
    }
    rows<-candidates[seq_len(min(limit,.N)),.(symbol,lineage_n,rest_n,effect_mean_lineage,effect_mean_rest,effect_mean_difference,effect_median_lineage,effect_median_rest,effect_median_difference,welch_t,p_lineage_more_dependent,fdr_lineage_more_dependent,dependency_probability_mean_lineage,dependency_probability_mean_rest,dependency_probability_mean_difference,effect_direction,rank_more_dependent)]
    status<-if(nrow(rows))"FOUND" else "NOT_RETAINED";reason<-if(nrow(rows))"bounded rows selected from the completed precomputed lineage dependency test" else "the completed table contains no rows satisfying the requested fixed ranking rule"
    emit(evidence(status,a$mode,reason,lineage=a$lineage,ranking=ranking,ranking_rule=ranking_rule,housekeeping_filter_applied=FALSE,housekeeping_filter_note="the precomputed lineage test has no validated housekeeping/common-essential exclusion field; selective means statistically stronger dependency versus the rest, not non-housekeeping",rows=rows,summary=list(total_gene_rows=nrow(x),tested_gene_count=nrow(tested),eligible_ranked_gene_count=nrow(candidates),returned_count=nrow(rows),lineage_n_modal=if(nrow(tested))as.integer(names(sort(table(tested$lineage_n),decreasing=TRUE))[1L]) else NA_integer_),manifest=compact_manifest,provenance=c(manifest_path,p)));quit(save="no")
  }
  source_index<-function(order,source){
    if(!file.exists(order))return(NA_integer_)
    x<-fread(order)
    hit<-which(clean(x$symbol)==clean(source))
    if(!length(hit))NA_integer_ else as.integer(x$source_index[hit[1L]])
  }
  source_block<-function(root,index){
    paths<-list.files(file.path(root,"blocks"),pattern="^block_[0-9]+_[0-9]+\\.parquet$",full.names=TRUE)
    if(!length(paths)||is.na(index))return(NA_character_)
    bounds<-t(vapply(basename(paths),function(q){as.integer(strsplit(sub("\\.parquet$","",sub("^block_","",q)),"_",fixed=TRUE)[[1L]])},integer(2)))
    hit<-which(bounds[,1L]<=index&bounds[,2L]>=index)
    if(!length(hit))NA_character_ else paths[hit[1L]]
  }
  read_parquet_dt<-function(path)as.data.table(arrow::read_parquet(path))
  bounded<-function(rows,value,limit){
    if(!nrow(rows))return(rows)
    rows[order(-abs(get(value)))][seq_len(min(as.integer(limit),.N))]
  }
  limit<-if(is.null(a$limit))20L else as.integer(a$limit)

  if(a$mode=="lineage_network"){
    module_root<-file.path(full,"lineage_sparse_networks",a$family)
    if(!dir.exists(module_root)){emit(evidence("MODULE_UNAVAILABLE",a$mode,"the requested lineage network family is not installed",family=a$family,lineage=a$lineage));quit(save="no")}
    root<-file.path(module_root,lineage_key(a$lineage));manifest<-manifest_at(root)
    if(is.null(manifest)){emit(evidence("NOT_COMPUTED",a$mode,"this lineage/family combination was not computed",family=a$family,lineage=a$lineage,provenance=module_root));quit(save="no")}
    if(!identical(manifest$status,"complete")){emit(evidence("INELIGIBLE",a$mode,paste("lineage manifest status is",manifest$status),family=a$family,lineage=a$lineage,manifest=manifest,provenance=file.path(root,"manifest.json")));quit(save="no")}
    source<-clean(a$source);target<-if(is.null(a$target))NA_character_ else clean(a$target);reciprocal<-tolower(if(is.null(a$reciprocal))"false" else a$reciprocal)%in%c("true","1")
    if(reciprocal){
      p<-file.path(root,"reciprocal_pairs.parquet")
      if(!file.exists(p)){emit(evidence("NOT_COMPUTED",a$mode,"reciprocal-pair output is absent",family=a$family,lineage=a$lineage,source=source,target=target,reciprocal=TRUE,manifest=manifest,provenance=file.path(root,"manifest.json")));quit(save="no")}
      rows<-read_parquet_dt(p)[clean(source_gene)==source|clean(target_gene)==source]
      if(!is.na(target))rows<-rows[(clean(source_gene)==target|clean(target_gene)==target)]
      rows<-bounded(rows,"reciprocal_score",limit)
    }else{
      index<-source_index(file.path(root,"source_gene_order.csv"),source);p<-source_block(root,index)
      if(is.na(index)){emit(evidence("NOT_COMPUTED",a$mode,"source gene is absent from the computed source universe",family=a$family,lineage=a$lineage,source=source,target=target,reciprocal=FALSE,manifest=manifest,provenance=file.path(root,"source_gene_order.csv")));quit(save="no")}
      if(is.na(p)){emit(evidence("NOT_COMPUTED",a$mode,"the source gene block is absent",family=a$family,lineage=a$lineage,source=source,target=target,reciprocal=FALSE,manifest=manifest,provenance=file.path(root,"blocks")));quit(save="no")}
      rows<-read_parquet_dt(p)[clean(source_gene)==source]
      if(!is.na(target))rows<-rows[clean(target_gene)==target]
      rows<-bounded(rows,"correlation",limit)
    }
    status<-if(nrow(rows))"FOUND" else "NOT_RETAINED";reason<-if(nrow(rows))"bounded precomputed rows found" else "the eligible pair was tested but is absent from the retained sparse top-K output"
    emit(evidence(status,a$mode,reason,family=a$family,lineage=a$lineage,source=source,target=target,reciprocal=reciprocal,rows=rows,manifest=manifest,provenance=c(file.path(root,"manifest.json"),p)));quit(save="no")
  }

  if(a$mode=="lineage_cnv"){
    resolved<-resolve_lineage("lineage_cnv_amplification_dependency",a$lineage,a$mode)
    if(!is.null(resolved$gap)){resolved$gap$source<-clean(a$source);resolved$gap$target<-if(is.null(a$target))NA_character_ else clean(a$target);emit(resolved$gap);quit(save="no")}
    root<-resolved$root;manifest<-resolved$manifest;source<-clean(a$source);target<-if(is.null(a$target))NA_character_ else clean(a$target)
    index<-source_index(file.path(root,"source_gene_order.csv"),source);p<-source_block(root,index)
    if(is.na(index)){emit(evidence("INELIGIBLE",a$mode,"source amplification did not satisfy amplified/control thresholds",lineage=a$lineage,source=source,target=target,manifest=manifest,provenance=file.path(root,"source_gene_order.csv")));quit(save="no")}
    if(is.na(p)){emit(evidence("NOT_COMPUTED",a$mode,"the source event block is absent",lineage=a$lineage,source=source,target=target,manifest=manifest,provenance=file.path(root,"blocks")));quit(save="no")}
    rows<-read_parquet_dt(p)[clean(source_gene)==source];if(!is.na(target))rows<-rows[clean(target_gene)==target];rows<-bounded(rows,"mean_difference",limit)
    status<-if(nrow(rows))"FOUND" else "NOT_RETAINED";reason<-if(nrow(rows))"bounded precomputed rows found" else "the eligible pair was tested but is absent from the retained sparse top-K output"
    emit(evidence(status,a$mode,reason,lineage=a$lineage,source=source,target=target,rows=rows,manifest=manifest,provenance=c(file.path(root,"manifest.json"),p)));quit(save="no")
  }

  if(a$mode=="lineage_drug"){
    module_root<-file.path(full,"lineage_prism_associations")
    if(!dir.exists(module_root)){emit(evidence("MODULE_UNAVAILABLE",a$mode,"lineage PRISM module is not installed",feature=a$omic,lineage=a$lineage));quit(save="no")}
    root<-file.path(module_root,a$omic,lineage_key(a$lineage));manifest<-manifest_at(root)
    if(is.null(manifest)){emit(evidence("NOT_COMPUTED",a$mode,"this lineage/omic combination was not computed",feature=a$omic,lineage=a$lineage,provenance=file.path(module_root,a$omic)));quit(save="no")}
    if(!identical(manifest$status,"complete")){emit(evidence("INELIGIBLE",a$mode,paste("lineage manifest status is",manifest$status),feature=a$omic,lineage=a$lineage,manifest=manifest,provenance=file.path(root,"manifest.json")));quit(save="no")}
    p<-file.path(root,"associations.parquet");meta_path<-file.path(root,"drug_metadata.parquet");rows<-read_parquet_dt(p);meta<-read_parquet_dt(meta_path)
    target<-if(is.null(a$target))NA_character_ else clean(a$target);drug<-if(is.null(a$drug))NA_character_ else clean(a$drug)
    if(!is.na(target))rows<-rows[clean(gene)==target]
    if(!is.na(drug)){
      ids<-meta[clean(CompoundID)==drug|clean(CompoundName)==drug,as.character(CompoundID)]
      if(!length(ids)){emit(evidence("NOT_COMPUTED",a$mode,"drug is absent from this lineage PRISM index",feature=a$omic,lineage=a$lineage,drug=drug,target=target,manifest=manifest,provenance=meta_path));quit(save="no")}
      rows<-rows[as.character(drug_id)%in%ids]
    }
    rows<-bounded(rows,"pearson_r",limit);if(nrow(rows))rows<-merge(rows,meta[,.(drug_id=as.character(CompoundID),drug_name=CompoundName)],by="drug_id",all.x=TRUE,sort=FALSE)
    status<-if(nrow(rows))"FOUND" else "NOT_RETAINED";reason<-if(nrow(rows))"bounded precomputed rows found" else "the eligible association is absent from the retained sparse top-K output"
    emit(evidence(status,a$mode,reason,feature=a$omic,lineage=a$lineage,drug=drug,target=target,rows=rows,manifest=manifest,provenance=c(file.path(root,"manifest.json"),p)));quit(save="no")
  }

  if(a$mode=="enrichment"){
    resolved<-resolve_lineage("lineage_gene_enrichment",a$lineage,a$mode)
    collection<-if(is.null(a$collection))NA_character_ else a$collection;term<-if(is.null(a$term))NA_character_ else a$term
    if(!is.null(resolved$gap)){resolved$gap$source<-clean(a$source);resolved$gap$collection<-collection;resolved$gap$term<-term;emit(resolved$gap);quit(save="no")}
    root<-resolved$root;manifest<-resolved$manifest;source<-clean(a$source);rows<-data.table();p<-NA_character_
    for(path in list.files(file.path(root,"blocks"),pattern="^block_[0-9]+_[0-9]+\\.parquet$",full.names=TRUE)){
      hit<-read_parquet_dt(path)[clean(source_gene)==source]
      if(!is.na(collection))hit<-hit[get("collection")==collection]
      if(!is.na(term))hit<-hit[get("term")==term]
      if(nrow(hit)){rows<-hit;p<-path;break}
    }
    rows<-bounded(rows,"enrichment_z",limit);status<-if(nrow(rows))"FOUND" else "NOT_RETAINED";reason<-if(nrow(rows))"bounded precomputed rows found" else "no retained enrichment row matched this eligible gene/lineage request"
    emit(evidence(status,a$mode,reason,lineage=a$lineage,source=source,collection=collection,term=term,rows=rows,manifest=manifest,provenance=c(file.path(root,"manifest.json"),p[!is.na(p)])));quit(save="no")
  }
}
stop("unsupported mode")
