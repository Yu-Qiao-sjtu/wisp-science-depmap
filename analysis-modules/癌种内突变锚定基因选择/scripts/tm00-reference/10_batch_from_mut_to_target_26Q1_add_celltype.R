################################################
################################################
### 作者：果子
### 维护者：乔宇（上海交通大学，免疫药理PhD）
### 更新时间：2026-09-09
### 数据迁移：2026-08-12（23Q2 → 26Q1）
###           2026-09-09 脚本名由 10_batch_from_mut_to_target_23Q2_add_celltype.R 更名为 _26Q1_add_celltype.R

### ================================================================
### 批量找到 ARID1A 突变后哪个基因受到的影响最大
### 在 09 基础上增加：每个细胞类型各自的分数（score_celltype）
###
### 与 09 的区别：
###   09：只输出总 Score（所有组织平均）
###   10：额外输出每个组织（如 Skin、Brain、Lung）各自的分数
###       → 可以看到哪个组织的合成致死效应最强
### ================================================================

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
mutGene = "ARID1A"


### 优化流程
## 数据筛选，Mut >=2,WT>=5(跟原文不同)
TissueStatus <- data.frame(
  Tissue = cellinfor[coID,Tissue],
  Mutation = ifelse(mutData[coID,mutGene]==0,"WT","Mut")
) %>% 
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


targetGene = "HMGCR"
# targetGene= "ARID1B"
# targetGene= "PIK3CA"
targetGene= "GATA3"

data <- data.frame(
  DepmapID = coID,
  Tissue = cellinfor[coID,Tissue],
  Mutation = ifelse(mutData[coID,mutGene]==0,"WT","Mut"),
  Dependency = geneDependency[coID,targetGene]
)

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
ggsave("TM00/DepMap_TM00/output/10_ARID1A_GATA3_scatter.png", p1, width = 8, height = 6, dpi = 300)
cat("散点图已保存\n")

## 为了批量,定义总分数
## 以及每个类型的分数
score_all = mean(PlotData$Sensitivity)*mean(PlotData$Difference)*100
score_celltype = PlotData$Sensitivity*PlotData$Difference*100

#######################################################
### 批量遍历所有基因，计算总分数 + 每个组织各自的分数
genelist = colnames(geneDependency)
Tissue = "OncotreeLineage"
mutGene = "ARID1A"

## 预先计算 TissueStatus（所有基因共用，只算一次）
TissueStatus <- data.frame(
  Tissue = cellinfor[coID,Tissue],
  Mutation = ifelse(mutData[coID,mutGene]==0,"WT","Mut")
) %>% 
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

## 预计算循环内不变的量
mutStatus <- ifelse(mutData[coID, mutGene] == 0, "WT", "Mut")
tissueVec <- cellinfor[coID, Tissue]
nTissue <- nrow(TissueStatus)

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
  
  ## 跳过空结果
  if(nrow(PlotData) == 0 || all(is.na(PlotData$Sensitivity))) next
  
  score_all = mean(PlotData$Sensitivity)*mean(PlotData$Difference)*100
  
  ## 每个组织各自的分数，按 TissueStatus 顺序对齐（防止列错位）
  score_celltype <- PlotData$Sensitivity * PlotData$Difference * 100
  names(score_celltype) <- PlotData$Tissue
  score_aligned <- as.numeric(score_celltype[TissueStatus$Tissue])
  
  results[i,1] = mutGene
  results[i,2] = targetGene
  results[i,3] = mean(PlotData$Sensitivity)
  results[i,4] = mean(PlotData$Difference)
  results[i, 5:(4+nTissue)] = score_aligned
  results[i, 5+nTissue] = score_all
}

colnames(results) = c("mutGene","targetGene","mean_Sensitivity","mean_Diff",TissueStatus$Tissue,"Score")

## 去除跳过的基因（NA 行）
results <- results[!is.na(results$Score), ]

## 按 Score 降序排列
results <- results %>% arrange(desc(Score))

saveRDS(results, file = "TM00/DepMap_TM00/output/mut2target_results_celltype.rds")

cat("\n=== 批量分析完成 ===\n")
cat("有效基因数:", nrow(results), "\n")
cat("\nTop 20 目标基因（按总 Score 排序）:\n")
print(head(results[, c("mutGene","targetGene","mean_Sensitivity","mean_Diff","Score")], 20))

### 未来迭代版本
### 考虑突变基因是否适配流程
### 采用并行化提速, 50

### 新的话题
### mut target_gene：
### target_gene mut 

### https://www.ncbi.nlm.nih.gov/pmc/articles/PMC3954704/
