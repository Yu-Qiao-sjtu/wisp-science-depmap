################################################
################################################
### 作者：果子
### 维护者：乔宇（上海交通大学，免疫药理PhD）
### 更新时间：2026-09-09
### 数据迁移：2026-08-12（23Q2 → 26Q1）
###           2026-09-09 脚本名由 09_batch_from_mut_to_target_23Q2.R 更名为 _26Q1.R

### ================================================================
### 批量找到 ARID1A 突变后哪个基因受到的影响最大
###
### 分析思路：
###   1. 单基因示例：ARID1A 突变 vs HMGCR 依赖性（散点图）
###      横轴 = 突变细胞系对该基因的依赖性均值（Sensitivity）
###      纵轴 = 突变 vs 野生型依赖性差值（Difference）
###   2. 批量遍历：对所有基因计算 Score = Sensitivity × Difference
###      Score 越高 = ARID1A 突变后越依赖该基因 = 潜在合成致死靶点
### ================================================================

### ================================================================
### 如何选择锚定突变基因（mutGene）——四维选择框架
###
### 方法论是通用的：把下方 mutGene 换成任何基因即可批量筛选
### "该基因突变后最依赖什么"。但锚定基因不能随便选，依据如下：
###
### 维度一：统计功效——突变频率必须够高（硬性门槛）
###   # 泛癌层面：突变细胞系至少 30~50 个（ARID1A 约 121 个），
###   #   否则批量遍历 2 万个基因时噪声会淹没信号
###   # 癌种层面：单癌种课题要求该癌种内 Mut >= 10 才有分析价值
###   #   （本脚本组织准入门槛 Mut >= 3 只是画散点图的最低要求）
###   # 反面例子：BRCA1/BRCA2 故事好但突变细胞系少，
###   #   适合按癌种分层或纳入甲基化失活来扩样
###
### 维度二：基因功能性质——决定用哪套突变矩阵
###   # 抑癌基因（TSG，功能丧失型）→ 用 Damaging/LikelyLoF 矩阵
###   #   例：ARID1A、PTEN、KMT2D、CDKN2A（本脚本即此口径）
###   # 癌基因（功能获得型）→ 必须换 Hotspot 矩阵
###   #   例：KRAS、BRAF、PIK3CA、EGFR（只有关键位点才激活）
###   # 陷阱：拿 Damaging 矩阵筛 KRAS，大量无害错义突变会把
###   #   Mut 组稀释成假 WT，差异直接被抹平
###
### 维度三：临床价值——优先选"不可成药"的驱动基因
###   # 合成致死的逻辑：驱动基因本身治不了，就打它的代偿依赖
###   # 首选：染色质调控因子（ARID1A、KMT2C/D、CREBBP、SMARCA4）
###   #   ——突变高频 + 不可成药，是 SL 筛选的黄金靶
###   # 慎选：TP53（突变太泛约 50%，与基因组不稳定性强混杂，
###   #   Mut/WT 组系统性不同，差异分析假阳性多）
###   # 排除：已有很好靶向药的基因（EGFR、BRAF V600E），
###   #   直接用药即可，SL 筛选动机弱
###
### 维度四：可验证性——有没有已知合成致死答案（阳性对照）
###   # 选有已知文献搭档的基因 = 给流水线装阳性对照
###   # ARID1A → ARID1B（Nat Med 2014，FDR=7.8e-09 排第 2）、
###   #   WRN、DCAF7 均被捞回 Top，证明方法正确
###   # 若选完全未知的基因，方法对错无从校验，首个课题不建议
###
### 两个隐藏混杂因素（进阶注意）：
###   # 组织混杂：突变集中在单一癌种的基因（如 APC 集中在结直肠癌），
###   #   "突变 vs 依赖"差异其实是"癌种 vs 癌种"差异；
###   #   本脚本已按 OncotreeLineage 分层，解读时仍需确认
###   #   信号不是被一两个组织驱动
###   # 拷贝数混杂：近纯合缺失区域基因有"假依赖"，锚定基因
###   #   若在常缺失位点（如 9p21 CDKN2A 区域）需用 CN 数据校正
###
### 实操筛选流程：
###   # COSMIC Cancer Gene Census / OncoKB 驱动基因池（约 700 个）
###   #   → 泛癌 Mut 数 >= 30（单癌种则该癌种 Mut >= 10）
###   #   → TSG 用 Damaging / 癌基因用 Hotspot
###   #   → 排除已有靶向药、超高频混杂（TP53）、单一癌种富集
###   #   → 查文献确认有已知 SL 搭档做阳性对照
###   #   → 锁定基因跑批量，取 Difference 最正端 + FDR < 0.05
### ====================================================

rm(list = ls())
### 加载数据分析三剑客
library(dplyr)
library(tidyr)
library(tibble)

## 读取预处理好的三份数据（由 07 脚本生成 RDS）
mutData <- readRDS(file = "TM00/DepMap_TM00/output/mutData_damaging.rds")
cellinfor <- readRDS(file = "TM00/DepMap_TM00/output/cellinfor.rds")
geneDependency <- readRDS(file = "TM00/DepMap_TM00/output/geneDependency.rds")

### 三份数据取交集
coID <- intersect(rownames(geneDependency),rownames(cellinfor)) %>% 
  intersect(rownames(mutData))

## 新版 Model.csv 中 lineage 字段改名为 OncotreeLineage
Tissue = "OncotreeLineage"
## 锚定基因选择依据见脚本头部"四维选择框架"注释块；
## 也可直接跑 06_mut_anchor_gene_selection_26Q1.R 自动产出候选清单
## （含突变频率、TSG/OG角色、可药性、已知SL搭档、组织集中度五列参考）。
## ARID1A 四条全占：高频突变(Mut_n约121) + 抑癌基因LoF(匹配Damaging矩阵)
## + 不可成药(SL有临床价值) + 有已知答案(ARID1B可做阳性对照)
mutGene = "ARID1A"
targetGene = "HMGCR"

# targetGene= "ARID1B"
# targetGene= "SCAP"
# targetGene= "YPEL5"

data <- data.frame(
  DepmapID = coID,
  Tissue = cellinfor[coID,Tissue],
  Mutation = ifelse(mutData[coID,mutGene]==0,"WT","Mut"),
  Dependency = geneDependency[coID,targetGene]
)

## 数据筛选，Mut >=2,WT>=5(跟原文不同)
TissueStatus <- data %>% 
  select(Tissue,Mutation) %>%
  group_by(Tissue,Mutation) %>% 
  summarise(n =n(),.groups = "drop") %>% 
  ungroup() %>% 
  pivot_wider(names_from = "Mutation",
              values_from = n,
              values_fill = 0) %>% 
  filter(Mut >=3,WT>=5) %>%
  mutate(Sum=Mut+WT) %>% 
  mutate(Mut_Number = sum(Mut),WT_Number=sum(WT)) 

mydata <- data %>% 
  filter(Tissue %in% TissueStatus$Tissue)

## 作图数据整理
Mean_Dep <- mydata %>% 
  filter(Mutation=="Mut") %>% 
  group_by(Tissue) %>% 
  summarise(Sensitivity = mean(Dependency))

Diff_Dep <- mydata %>%
  group_by(Tissue) %>%
  summarize(Difference = mean(Dependency[Mutation == "Mut"]) - mean(Dependency[Mutation == "WT"]))

## 数据合并
PlotData <- merge(Mean_Dep,Diff_Dep,by="Tissue") %>% 
  merge(TissueStatus,by="Tissue")

## 作图展示
library(ggplot2)
library(ggrepel)
library(stringr)
library(ggfun)

p1 <- ggplot(PlotData, aes(x = Sensitivity, y = Difference)) +
  geom_point(aes(size = Sum), shape = 21, color = "black", fill = "green", alpha = 0.6, stroke = 1.5) + 
  geom_text_repel(aes(label = Tissue), vjust = 1.5, hjust = 1.5) + 
  annotate("rect", xmin = -Inf, xmax = Inf, ymin = 0, ymax = Inf,
           fill = "yellow", alpha = 0.1, colour = "green", linetype = "dashed", linewidth = 1) +
  labs(x = "Sensitivity Score for Cell Lines with Mutations", 
       y = "Mutant vs. Wildtype Sensitivity", 
       title = paste0("Mutant (n=", unique(PlotData$Mut_Number),
                      " cell lines) vs. Wildtype (n=",
                      unique(PlotData$WT_Number), " cell lines)")) +
  theme_bw() + 
  guides(size = guide_legend(title = str_wrap("Number of cells", width = 8))) +
  theme(panel.grid.major = element_line(linetype = "dashed"),
        panel.grid.minor = element_line(linetype = "dashed", linewidth = 0.5),
        panel.background = element_blank(),
        legend.position = c(.95, .05),
        legend.justification = c("right", "bottom"),
        legend.box.just = "right",
        legend.margin = margin(4, 4, 4, 4),
        legend.background = element_roundrect(color = "#808080", linetype = 1)
  )

print(p1)
ggsave("TM00/DepMap_TM00/output/09_ARID1A_HMGCR_scatter.png", p1, width = 8, height = 6, dpi = 300)
cat("散点图已保存\n")

## 为了批量,定义一个分数
score = mean(PlotData$Sensitivity)*mean(PlotData$Difference)*100

# score = as.numeric(PlotData$Sensitivity %*% PlotData$Sum)*
#   as.numeric(PlotData$Difference %*% PlotData$Sum)/1000
#######################################################
### 批量遍历所有基因，计算 ARID1A 突变对各基因依赖性的影响分数
genelist = colnames(geneDependency)
Tissue = "OncotreeLineage"
mutGene = "ARID1A"

## 预先计算突变状态和组织分类（循环内不变，提取到外面避免重复计算）
mutStatus <- ifelse(mutData[coID, mutGene] == 0, "WT", "Mut")
tissueVec <- cellinfor[coID, Tissue]

## 结果存储
results = data.frame()
cat("共", length(genelist), "个基因需要遍历\n")

for(i in 1:length(genelist)){
  if(i %% 1000 == 0) cat("进度:", i, "/", length(genelist), "\n")
  
  targetGene = genelist[i]
  data <- data.frame(
    DepmapID = coID,
    Tissue = tissueVec,
    Mutation = mutStatus,
    Dependency = geneDependency[coID, targetGene]
  )
  
  ## 数据筛选，Mut >=3,WT>=5(跟原文不同)
  TissueStatus <- data %>% 
    select(Tissue,Mutation) %>%
    group_by(Tissue,Mutation) %>% 
    summarise(n =n(),.groups = "drop") %>% 
    ungroup() %>% 
    pivot_wider(names_from = "Mutation",
                values_from = n,
                values_fill = 0) %>% 
    filter(Mut >=3,WT>=5) %>%
    mutate(Sum=Mut+WT) %>% 
    mutate(Mut_Number = sum(Mut),WT_Number=sum(WT)) 
  
  ## 如果没有组织满足条件，跳过
  if(nrow(TissueStatus) == 0) next
  
  mydata <- data %>% 
    filter(Tissue %in% TissueStatus$Tissue)
  
  ## 作图数据整理
  Mean_Dep <- mydata %>% 
    filter(Mutation=="Mut") %>% 
    group_by(Tissue) %>% 
    summarise(Sensitivity = mean(Dependency))
  
  Diff_Dep <- mydata %>%
    group_by(Tissue) %>%
    summarize(Difference = mean(Dependency[Mutation == "Mut"]) - mean(Dependency[Mutation == "WT"]))
  
  ## 数据合并
  PlotData <- merge(Mean_Dep,Diff_Dep,by="Tissue") %>% 
    merge(TissueStatus,by="Tissue")
  
  ## 如果 Dependency 全是 NA，跳过
  if(nrow(PlotData) == 0 || all(is.na(PlotData$Sensitivity))) next
  
  score = mean(PlotData$Sensitivity)*mean(PlotData$Difference)*100
  
  results[i,1] = mutGene
  results[i,2] = targetGene
  results[i,3] = mean(PlotData$Sensitivity)
  results[i,4] = mean(PlotData$Difference)
  results[i,5] = score
}

colnames(results) = c("mutGene","targetGene","mean_Sensitivity","mean_Diff","Score")

## 去除跳过的基因（NA 行）
results <- results[!is.na(results$Score), ]

## 按 Score 降序排列
results <- results %>% arrange(desc(Score))

saveRDS(results, file = "TM00/DepMap_TM00/output/mut2target_results.rds")

cat("\n=== 批量分析完成 ===\n")
cat("有效基因数:", nrow(results), "\n")
cat("\nTop 20 目标基因（ARID1A 突变后依赖性最高的基因）:\n")
print(head(results, 20))

### 参考文献链接
### https://www.ncbi.nlm.nih.gov/pmc/articles/PMC3954704/
### GZ07 批量课程
