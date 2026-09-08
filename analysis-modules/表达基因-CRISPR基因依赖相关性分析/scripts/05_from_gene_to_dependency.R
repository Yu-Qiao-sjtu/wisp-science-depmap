################################################
################################################
### 作者：果子
### 更新时间：2023-10-22
### 微信公众号:果子学生信
### 私人微信：guotosky
### 个人邮箱：hello_guozi@126.com

### 基因A的表达量调控哪些基因的Dependency
### ================================================================
### 分析思路（表达量 → 依赖性 → 通路富集）：
###
### 核心问题：基因A（如 ESR1）的表达量高低，会影响哪些基因的依赖性？
###          即：ESR1 高表达的细胞系，同时依赖哪些基因？
###
### 第一步：计算 ESR1 表达量 vs 全基因依赖性的相关性
### 第二步：将相关系数排序后做 GSEA 富集分析，看富集到哪些通路
### ================================================================

## =================== 第一部分：ESR1 表达量 → 全基因依赖性 + GSEA ===================

## 清空环境
rm(list = ls())

## 读取基因表达矩阵（行：细胞系，列：基因，值：log2(TPM+1)）
exprSet <- readRDS(file = "TM00/DepMap_TM00/output/ccle_exprSet.rds")

## 读取 CRISPR 基因效应矩阵（行：细胞系，列：基因，值：Chronos 效应分）
geneEffect <- readRDS(file = "TM00/DepMap_TM00/output/depmap_geneEffect.rds")

## 创建空 data.frame，用于存储结果
corData <- data.frame()

## 固定目标基因为 ESR1
gene1 = "ESR1"

## 提取 ESR1 在所有细胞系中的表达向量
genedata <- exprSet[,"ESR1"]

## 遍历 geneEffect 的每一列（每一个基因），计算其依赖性与 ESR1 表达的相关性
for (i in 1:ncol(geneEffect)) {
  
  ## 1. 打印进度
  print(i)
  
  ## 2. 获取当前基因名，计算其依赖性与 ESR1 表达的 Pearson 相关
  gene2 = colnames(geneEffect)[i]
  dd = cor.test(genedata, geneEffect[,gene2])
  
  ## 3. 存储结果：Gene1=ESR1(表达来源), Gene2=当前基因(依赖性来源)
  corData[i,1] = gene1
  corData[i,2] = gene2
  corData[i,3] = dd$estimate  ## 相关系数
  corData[i,4] = dd$p.value   ## p 值
}

## 给结果表添加列名
colnames(corData) <- c("Gene1","Gene2","cor","pvalue")

## =================== GSEA 富集分析 ===================

library(clusterProfiler)

## 构建 GSEA 所需的排序基因列表
## 取负号：cor 为正表示 ESR1 高表达时该基因依赖性强，取负后排在后面
##        cor 为负表示 ESR1 高表达时该基因依赖性弱，取负后排在前面
## 这样 GSEA 上富集 = ESR1 低表达时依赖的通路，下富集 = ESR1 高表达时依赖的通路
mygeneList <- -corData$cor
names(mygeneList) <- corData$Gene2
mygeneList <- sort(mygeneList, decreasing = T)
head(mygeneList)

## 读取 Hallmark 基因集（50 个经典通路）
geneSet <- read.gmt("TM00/DepMap_TM00/resource/geneSets/h.all.v2023.2.Hs.symbols.gmt")

## 运行 GSEA
mygsea <- GSEA(geneList = mygeneList, TERM2GENE = geneSet)

## 转为 data.frame 查看完整结果表
data <- as.data.frame(mygsea)

## 气泡图：按正/负富集分面展示 Top30 通路
library(ggplot2)
dotplot(mygsea, showCategory = 30,
        split = ".sign",
        font.size = 8,
        label_format = 60) + facet_grid(~.sign)

## 单个通路的 GSEA 富集曲线图（这里看 MYC 靶基因通路）
library(enrichplot)
gseaplot2(mygsea, "HALLMARK_MYC_TARGETS_V2", color = "red", pvalue_table = T)

## =================== 第二部分：IL6R 表达量 → 全基因依赖性 + GSEA ===================
## 换一个目标基因 IL6R，用 Reactome 通路库做富集
## IL6R 与染色体不稳定（CIN）相关，预期富集到 DNA 损伤修复等通路

## 清空环境
rm(list = ls())

## 读取表达矩阵
exprSet <- readRDS(file = "TM00/DepMap_TM00/output/ccle_exprSet.rds")

## 读取基因效应矩阵
geneEffect <- readRDS(file = "TM00/DepMap_TM00/output/depmap_geneEffect.rds")

## 创建空 data.frame
corData <- data.frame()

## 固定目标基因为 IL6R
gene1 = "IL6R"

## 提取 IL6R 表达向量
genedata <- exprSet[,"IL6R"]

## 遍历 geneEffect 每一列，计算 IL6R 表达与各基因依赖性的相关性
for (i in 1:ncol(geneEffect)) {
  
  ## 1. 打印进度
  print(i)
  
  ## 2. 计算当前基因依赖性与 IL6R 表达的 Pearson 相关
  gene2 = colnames(geneEffect)[i]
  dd = cor.test(genedata, geneEffect[,gene2])
  
  ## 3. 存储结果
  corData[i,1] = gene1
  corData[i,2] = gene2
  corData[i,3] = dd$estimate
  corData[i,4] = dd$p.value
}

## 列名与第一部分不同：exp=表达来源基因, Dependency=依赖性来源基因
colnames(corData) <- c("exp","Dependency","cor","pvalue")

## =================== GSEA 富集分析 ===================

library(clusterProfiler)

## 构建排序基因列表（取负号逻辑同上）
mygeneList <- -corData$cor
names(mygeneList) <- corData$Dependency
mygeneList <- sort(mygeneList, decreasing = T)
head(mygeneList)

## 这次用 Reactome 通路库（比 Hallmark 更详细，上千条通路）
geneSet <- read.gmt("TM00/DepMap_TM00/resource/geneSets/c2.cp.reactome.v2023.2.Hs.symbols.gmt")

## 运行 GSEA
mygsea <- GSEA(geneList = mygeneList, TERM2GENE = geneSet)

## 查看完整结果
data <- as.data.frame(mygsea)

## 气泡图
library(ggplot2)
dotplot(mygsea, showCategory = 30,
        split = ".sign",
        font.size = 8,
        label_format = 60) + facet_grid(~.sign)

## 单个通路的富集曲线（染色体维护通路，与 IL6/CIN 文献呼应）
library(enrichplot)
gseaplot2(mygsea, "REACTOME_CHROMOSOME_MAINTENANCE", color = "red", pvalue_table = T)

## =================== 第三部分：PROGENy 通路活性量化 ===================
## 升级思路：从单基因表达升级为 11 条核心信号通路的活性打分
## PROGENy 用下游响应基因表达反推上游通路活性，比单基因更稳健
## 生物学依据（Hong et al. 2022 Nature）：
##   CIN（染色体不稳定）→ cGAS-STING 激活 → IL-6 自分泌 → 癌细胞依赖 IL6R 生存
##   用 PROGENy 的 IL6_STAT3 通路活性替代单个 IL6R 基因表达，更接近生物学真相

## ---- 确保 BiocManager 可用（必须先于下面的安装循环）----
if (!requireNamespace("BiocManager", quietly = TRUE)) {
  install.packages("BiocManager")
}

## ---- 自动安装缺失依赖 ----
for (pkg in c("progeny", "decoupleR", "dorothea", "dplyr", "tibble", "tidyr")) {
  if (!requireNamespace(pkg, quietly = TRUE)) {
    if (pkg %in% c("progeny", "decoupleR", "dorothea")) {
      BiocManager::install(pkg)
    } else {
      install.packages(pkg)
    }
  }
}

library(progeny)
library(decoupleR)
library(dorothea)
library(dplyr)
library(tibble)
library(tidyr)

## 重新读取表达矩阵（前面 rm 清掉了）
exprSet <- readRDS(file = "TM00/DepMap_TM00/output/ccle_exprSet.rds")
geneEffect <- readRDS(file = "TM00/DepMap_TM00/output/depmap_geneEffect.rds")

## ---- PROGENy：计算 11 条信号通路活性分 ----
## PROGENy 需要 ExpressSet 或 matrix，且基因名需为 HGNC symbol
## 将表达矩阵转置（PROGENy 要求行=基因，列=样本）
exprMat <- t(as.matrix(exprSet))

## 计算 11 条通路活性分（Androgen, EGFR, Estrogen, Hypoxia, JAK-STAT, MAPK, NFkB, p53, PI3K, TGFb, TNFa, Trail, VEGF, WNT）
progeny_scores <- progeny(exprMat, scale = TRUE, organism = "Human")

## 转为 data.frame，行=细胞系，列=通路活性分
progeny_df <- as.data.frame(progeny_scores)

## 查看实际列名（确认 PROGENy 返回的通路名称）
colnames(progeny_df)
head(progeny_df)

## ---- 验证文献假说：JAK-STAT 通路活性 vs 全基因依赖性 ----
## 用 JAK-STAT 通路活性替代单个 IL6R 基因表达
stat3_activity <- progeny_df[, "JAK-STAT"]

## 批量计算 JAK-STAT 通路活性 vs 全基因依赖性的相关性
corData_progeny <- data.frame()
gene1 = "JAK-STAT_activity"
genedata <- stat3_activity

for (i in 1:ncol(geneEffect)) {
  print(i)
  gene2 = colnames(geneEffect)[i]
  dd = cor.test(genedata, geneEffect[,gene2])
  corData_progeny[i,1] = gene1
  corData_progeny[i,2] = gene2
  corData_progeny[i,3] = dd$estimate
  corData_progeny[i,4] = dd$p.value
}
colnames(corData_progeny) <- c("Pathway","Gene","cor","pvalue")

## 构建排序基因列表，做 GSEA
library(clusterProfiler)
mygeneList_progeny <- -corData_progeny$cor
names(mygeneList_progeny) <- corData_progeny$Gene
mygeneList_progeny <- sort(mygeneList_progeny, decreasing = T)

## GSEA（用 Reactome 通路库）
geneSet_reactome <- read.gmt("TM00/DepMap_TM00/resource/geneSets/c2.cp.reactome.v2023.2.Hs.symbols.gmt")
mygsea_progeny <- GSEA(geneList = mygeneList_progeny, TERM2GENE = geneSet_reactome)

## 气泡图
library(ggplot2)
dotplot(mygsea_progeny, showCategory = 30,
        split = ".sign",
        font.size = 8,
        label_format = 60) + facet_grid(~.sign)

## =================== 第四部分：decoupleR 通路活性推断 ===================
## 升级思路：用 decoupleR 推断每个细胞系的下游通路活性
## 与 PROGENy 互补：decoupleR 支持多种基因集库，覆盖面更广

## 用 decoupleR 的 enrich基因集库（基于 Footprint）
## 这里使用 decoupleR 内置的 Dorothea 转录因子活性推断作为示例

## 获取 Dorothea 转录因子调控网络（置信度 A-C，高质量 TF-Target 关系）
## 直接用 dorothea 包内置数据，无需 OmnipathR
library(dorothea)
data("dorothea_hs", package = "dorothea")
net <- dorothea_hs
## 仅保留置信度 A、B、C 的调控关系
net <- net[net$confidence %in% c("A", "B", "C"), ]
cat("转录因子调控网络：", nrow(net), "条 TF-Target 关系，",
    length(unique(net$tf)), "个转录因子\n")

## 计算每个细胞系的转录因子活性分（用 ulm 方法：无权重最小二乘）
## decoupleR 2.x API：参数 net→network，.mor 通过 args 传递
net <- net[, c("tf", "target", "mor")]  ## 确保只保留需要的列
tf_scores <- decouple(mat = exprMat,
                      network = net,
                      .source = "tf",
                      .target = "target",
                      statistics = "ulm",
                      args = list(.mor = "mor"),
                      minsize = 5)

## 提取 ulm 分数，转为矩阵形式（行=细胞系，列=转录因子）
tf_mat <- tf_scores |>
  filter(statistic == "ulm") |>
  pivot_wider(id_cols = condition, names_from = source, values_from = score) |>
  as.data.frame()
rownames(tf_mat) <- tf_mat[,1]
tf_mat <- tf_mat[,-1]
tf_mat <- as.matrix(tf_mat)

## 查看结果：每个细胞系在各转录因子上的活性分
dim(tf_mat)
head(tf_mat[, 1:6])

## ---- 验证假说：STAT3 转录因子活性 vs 全基因依赖性 ----
## 用 STAT3 转录因子活性分替代单个 IL6R 表达
tf_target <- "STAT3"
if (tf_target %in% colnames(tf_mat)) {
  tf_target_data <- tf_mat[, tf_target]
  corData_tf <- data.frame()
  genedata <- tf_target_data
  gene1 <- tf_target

  for (i in 1:ncol(geneEffect)) {
    print(i)
    gene2 = colnames(geneEffect)[i]
    dd = cor.test(genedata, geneEffect[,gene2])
    corData_tf[i,1] = gene1
    corData_tf[i,2] = gene2
    corData_tf[i,3] = dd$estimate
    corData_tf[i,4] = dd$p.value
  }
  colnames(corData_tf) <- c("TF","Gene","cor","pvalue")

  ## GSEA
  mygeneList_tf <- -corData_tf$cor
  names(mygeneList_tf) <- corData_tf$Gene
  mygeneList_tf <- sort(mygeneList_tf, decreasing = T)

  mygsea_tf <- GSEA(geneList = mygeneList_tf, TERM2GENE = geneSet_reactome)

  dotplot(mygsea_tf, showCategory = 30,
          split = ".sign",
          font.size = 8,
          label_format = 60) + facet_grid(~.sign)
}

## =================== 保存结果 ===================
saveRDS(progeny_scores, file = "TM00/DepMap_TM00/output/progeny_pathway_scores.rds")
saveRDS(tf_scores, file = "TM00/DepMap_TM00/output/decoupleR_tf_scores.rds")

## =================== 推荐阅读 ===================
### cGAS–STING drives the IL-6-dependent survival of chromosomally instable cancers
### Hong et al. 2022 Nature
### https://www.bioconductor.org/packages/release/bioc/vignettes/decoupleR/inst/doc/pw_bk.html
### https://saezlab.github.io/progeny/articles/progeny.html
###
### 升级逻辑：
### 单基因表达（IL6R）→ PROGENy 通路活性（JAK-STAT3）→ decoupleR 转录因子活性（STAT3）
### 越往上越接近生物学机制，越不受单个基因波动影响
###
### 拓展方向
### 机器学习（基于通路活性的预测模型）
### 协同致死（通路活性相关的依赖性基因对）
### 药物敏感性（通路活性 vs PRISM/GDSC 药物筛选）
### 基因功能模块（WGCNA 共表达网络）