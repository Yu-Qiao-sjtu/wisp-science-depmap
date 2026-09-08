################################################
################################################
### 作者：果子
### 更新时间：2023-10-22
### 数据迁移：2026-08-12（22Q2 → 26Q1）
### 微信公众号:果子学生信
### 私人微信：guotosky
### 个人邮箱：hello_guozi@126.com

### 复习批量操作
### 基因和基因的相关性

rm(list = ls())
library(ggplot2)

## 读取 01 脚本生成的表达量 RDS
exprSet <- readRDS(file = "TM00/DepMap_TM00/output/ccle_exprSet.rds")

## 单个基因对的相关性验证：FOXA1 vs ESR1
cor.test(exprSet[, "FOXA1"], exprSet[, "ESR1"])

dd <- cor.test(exprSet[, "FOXA1"], exprSet[, "ESR1"])
dd$p.value
dd$estimate

## 批量计算 ESR1 与所有基因的相关性
gene1 <- "ESR1"
genedata <- exprSet[, gene1]
corData <- data.frame()

for (i in seq_len(ncol(exprSet))) {
  if (i %% 2000 == 0) cat("进度:", i, "/", ncol(exprSet), "\n")

  gene2 <- colnames(exprSet)[i]
  dd <- cor.test(genedata, exprSet[, gene2])

  corData[i, 1] <- gene1
  corData[i, 2] <- gene2
  corData[i, 3] <- dd$estimate
  corData[i, 4] <- dd$p.value
}

colnames(corData) <- c("Gene1", "Gene2", "cor", "pvalue")

## 保存结果
saveRDS(corData, file = "TM00/DepMap_TM00/output/02_ESR1_gene_correlation.rds")

### 推荐阅读: GZ07,批量技能
