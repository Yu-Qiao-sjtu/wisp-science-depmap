################################################
################################################
### 作者：果子
### 更新时间：2023-10-22
### 微信公众号:果子学生信
### 私人微信：guotosky
### 个人邮箱：hello_guozi@126.com

### Co-Dependency（共依赖性）分析的使用
### 包括单个基因演示和全基因批量两种方式

## =================== 第一部分：直接读取原始 CSV 做快速演示 ===================
## 此时列名带 Entrez ID 括号，如 "ESR1 (2099)"、"SPDEF (25803)"

## 读取原始 CRISPR 基因效应矩阵（未处理版，列名带括号）
exprSet <- data.table::fread("data/CRISPRGeneEffect.csv",data.table = F)

## 取前 10 行、前 10 列预览数据结构
test <- exprSet[1:10,1:10]

## 打印预览
test

## 用原始列名（带 Entrez ID 括号）计算 SPDEF 与 ESR1 的 Pearson 相关
## 高度正相关 → 两个基因功能协同，属于同一通路（雌激素受体通路）
cor.test(exprSet[,"SPDEF (25803)"],exprSet[,"ESR1 (2099)"],method = "pearson")

## =================== 第二部分：用处理好的 RDS 做正式分析 ===================
## RDS 是 01 脚本处理后的版本，列名已去掉括号（"ESR1" 而非 "ESR1 (2099)"）

## 清空环境，重新开始
rm(list = ls())

## 读取 01 脚本处理过的 geneEffect 矩阵（行：细胞系 ACH- 编号，列：基因名）
exprSet <- readRDS(file = "TM00/DepMap_TM00/output/depmap_geneEffect.rds")

## FOXA1 与 ESR1 的共依赖性（默认 Pearson 相关）
## FOXA1 是雌激素受体通路的关键转录因子，预期与 ESR1 高度正相关
cor.test(exprSet[,"FOXA1"],exprSet[,"ESR1"])

## SPDEF 与 ESR1，指定 Pearson 相关（参数方法，假设正态分布）
cor.test(exprSet[,"SPDEF"],exprSet[,"ESR1"],method = "pearson")

## SPDEF 与 ESR1，指定 Spearman 相关（非参数方法，基于秩，不受异常值影响）
cor.test(exprSet[,"SPDEF"],exprSet[,"ESR1"],method = "spearman")

## 把 FOXA1 与 ESR1 的相关结果存到 dd，方便单独提取统计量
dd <- cor.test(exprSet[,"FOXA1"],exprSet[,"ESR1"])

## 提取 p 值（显著性）
dd$p.value

## 提取相关系数（cor，方向和强度）
dd$estimate

## =================== 第三部分：核心——ESR1 与全基因的批量共依赖性 ===================

## （未使用的变量，留作参考）
gene <- "ESR1"

## 创建空 data.frame，用于逐行存储所有基因与 ESR1 的相关结果
corData <- data.frame()

## 固定目标基因为 ESR1
gene1 = "ESR1"

## 提取 ESR1 在所有细胞系中的效应分向量（作为循环中的固定向量）
genedata <- exprSet[,gene1]

## 遍历表达矩阵的每一列（每一个基因，约 1.7 万个）
for (i in 1:ncol(exprSet)) {
  
  ## 1. 打印当前进度（第几个基因）
  print(i)
  
  ## 2. 获取当前列对应的基因名
  gene2 = colnames(exprSet)[i]
  ## 计算 ESR1 与该基因的 Pearson 相关检验
  dd = cor.test(genedata,exprSet[,gene2])
  
  ## 3. 将结果逐行存入 corData
  corData[i,1] = gene1        ## Gene1 = ESR1
  corData[i,2] = gene2        ## Gene2 = 当前基因
  corData[i,3] = dd$estimate  ## 相关系数
  corData[i,4] = dd$p.value   ## p 值
}

## 给结果表添加列名
colnames(corData) <- c("Gene1","Gene2","cor","pvalue")

## =================== 推荐阅读 ===================
### 单基因批量相关性分析的妙用
### https://mp.weixin.qq.com/s/TfE2koPhSkFxTWpb7TlGKA
### 单基因批量相关性分析的GSEA
### https://mp.weixin.qq.com/s/sZJPW8OWaLNBiXXrs7UYFw
### 两个功能集成到了GTBAdb中
### http://guotosky.vip:13838/GTBA/

## =================== 第四部分：全基因两两相关矩阵 ===================
## 计算所有基因两两之间的 Pearson 相关矩阵（~1.7万 × 1.7万），结果约 2.3 GB
## 分块计算 + txtProgressBar 进度条（任何 R 终端都能正常显示进度）

## 将 data.frame 转为 matrix，提高计算效率
exprMat <- as.matrix(exprSet)
ngene <- ncol(exprMat)

## 把 1.7 万列切成 100 个块，逐块计算每个列块与全部基因的相关性
## 每块约 170 个基因，每次 cor() 是一次高效的 BLAS 矩阵运算
nblocks <- 100
block_idx <- split(1:ngene, cut(1:ngene, nblocks, labels = FALSE))

## 创建进度条（style = 3 显示百分比 + 进度条）
pb <- txtProgressBar(min = 0, max = nblocks, style = 3)
cor_blocks <- vector("list", nblocks)

for (b in seq_len(nblocks)) {
  cor_blocks[[b]] <- cor(exprMat, exprMat[, block_idx[[b]]])
  setTxtProgressBar(pb, b)
}
close(pb)

## 按列拼接所有块，恢复完整矩阵
data <- do.call(cbind, cor_blocks)
dimnames(data) <- list(colnames(exprMat), colnames(exprMat))

## 保存结果，方便后续脚本快速加载
saveRDS(data, file = "TM00/DepMap_TM00/output/co_dependency_matrix.rds")

## =================== 拓展阅读：用 Co-Dependency 发表的论文 ===================
### A Ubiquitination Cascade Regulating the Integrated Stress Response and Survival in Carcinomas
### A non-canonical tricarboxylic acid cycle underlies cellular identity