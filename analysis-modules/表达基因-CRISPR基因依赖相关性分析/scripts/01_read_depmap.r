################################################
################################################
### 作者：果子
### 更新时间：2023-10-22
### 数据迁移：2026-08-12（22Q2 → 26Q1）
### 微信公众号:果子学生信
### 私人微信：guotosky
### 个人邮箱：hello_guozi@126.com

### ================================================================
### DepMap 数据读取与预处理（入门版）
###
### 本脚本演示如何加载 DepMap 26Q1 的 5 类核心数据，
### 对齐细胞系和基因，保存为 RDS 供后续脚本使用。
###
### 新版数据的主要变化（22Q2 → 26Q1）：
###   - CRISPR_gene_effect.csv → CRISPRGeneEffect.csv（第一列 V1 = ACH-编号）
###   - CRISPR_gene_dependency.csv → CRISPRGeneDependency.csv
###   - CCLE_expression.csv → OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv
###     （新增 6 列元数据，需过滤 IsDefaultEntryForMC）
###   - sample_info.csv → Model.csv（字段名：DepMap_ID → ModelID）
###   - CCLE_mutations.csv → OmicsSomaticMutationsMatrixDamaging.csv
###     （格式从长表变为宽矩阵）
###
### 注意：完整的突变数据处理见脚本 07/08
### ================================================================

rm(list = ls())
library(data.table)
output_dir <- "TM00/DepMap_TM00/output"

## ---- 1. Gene Effect（CRISPR 基因效应）----
geneEffect <- data.table::fread("data/CRISPRGeneEffect.csv", data.table = F)
rownames(geneEffect) <- geneEffect[, 1]     ## V1 = ACH-编号
geneEffect <- geneEffect[, -1]
colnames(geneEffect) <- gsub("\\s+\\(\\d+\\)", "", colnames(geneEffect))
cat("Gene Effect:", nrow(geneEffect), "模型 ×", ncol(geneEffect), "基因\n")

## ---- 2. Gene Dependency（基因依赖性概率）----
geneDependency <- data.table::fread("data/CRISPRGeneDependency.csv", data.table = F)
rownames(geneDependency) <- geneDependency[, 1]
geneDependency <- geneDependency[, -1]
colnames(geneDependency) <- gsub("\\s+\\(\\d+\\)", "", colnames(geneDependency))
cat("Gene Dependency:", nrow(geneDependency), "模型 ×", ncol(geneDependency), "基因\n")

## ---- 3. CCLE 细胞系表达量（新版含元数据列，需过滤）----
exprRaw <- data.table::fread("data/OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv",
                              data.table = F)
## 新版前 6 列是元数据：V1, SequencingID, ModelConditionID, ModelID, IsDefaultEntryForMC, IsDefaultEntryForModel
exprRaw <- exprRaw[exprRaw$IsDefaultEntryForMC == "Yes", ]
exprRaw <- exprRaw[!duplicated(exprRaw$ModelID), ]
rownames(exprRaw) <- exprRaw$ModelID
## 去掉元数据列，只保留基因列
meta_cols <- c("V1", "SequencingID", "ModelConditionID", "ModelID",
                "IsDefaultEntryForMC", "IsDefaultEntryForModel")
exprSet <- exprRaw[, !(colnames(exprRaw) %in% meta_cols)]
rm(exprRaw)
colnames(exprSet) <- gsub("\\s+\\(\\d+\\)", "", colnames(exprSet))
cat("Expression:", nrow(exprSet), "模型 ×", ncol(exprSet), "基因\n")

## ---- 4. 细胞系信息（Model.csv）----
cellinfor <- data.table::fread("data/Model.csv", data.table = F)
rownames(cellinfor) <- cellinfor$ModelID      ## 旧版字段 DepMap_ID → ModelID
cat("Model info:", nrow(cellinfor), "细胞系\n")

## ---- 5. 突变数据（宽矩阵格式，旧版为长表 CCLE_mutations.csv）----
mutData <- data.table::fread("data/OmicsSomaticMutationsMatrixDamaging.csv",
                              data.table = F)
## 新版前 6 列是元数据，需过滤 IsDefaultEntryForModel == "Yes"
mutData <- mutData[mutData$IsDefaultEntryForModel == "Yes", ]
rownames(mutData) <- mutData$ModelID
mutData <- mutData[, !(colnames(mutData) %in%
                         c("V1", "ModelID", "ModelConditionID",
                           "IsDefaultEntryForMC", "IsDefaultEntryForModel",
                           "GenomeVersion"))]
colnames(mutData) <- gsub("\\s+\\(\\d+\\)", "", colnames(mutData))
cat("Mutation matrix:", nrow(mutData), "模型 ×", ncol(mutData), "基因\n")

################################
## 修剪：三套数据取交集
################################

commonindex <- intersect(rownames(geneEffect), rownames(exprSet))
commonindex <- intersect(commonindex, rownames(cellinfor))
cat("三套数据共有细胞系:", length(commonindex), "\n")

geneEffect <- geneEffect[commonindex, ]
exprSet    <- exprSet[commonindex, ]
cellinfor  <- cellinfor[commonindex, ]

## 共有基因
commonGenes <- intersect(colnames(geneEffect), colnames(exprSet))
cat("共有基因:", length(commonGenes), "\n")
geneEffect <- geneEffect[, commonGenes]
exprSet    <- exprSet[, commonGenes]

################################
## 保存 RDS（供后续脚本使用）
################################

saveRDS(geneEffect, file = file.path(output_dir, "depmap_geneEffect.rds"))
saveRDS(exprSet, file = file.path(output_dir, "ccle_exprSet.rds"))
saveRDS(cellinfor, file = file.path(output_dir, "cellinfor.rds"))
cat("已保存 3 个 RDS 到", output_dir, "\n")
