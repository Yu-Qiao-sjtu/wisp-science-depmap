################################################
################################################
### 作者：果子
### 维护者：乔宇（上海交通大学，免疫药理PhD）
### 更新时间：2026-09-09
### 数据迁移：2026-08-12（23Q2 → 26Q1）
###           2026-09-09 脚本名由 11_batch_from_gene_to_mut_23Q2_add_celltype.R 更名为 _26Q1_add_celltype.R

### ================================================================
### 反向合成致死筛选：哪种基因突变的病人最能从靶向某基因的治疗中获益？
###
### 与 09/10 的区别（方向相反）：
###   09/10：固定突变基因（ARID1A），遍历所有靶基因 → 找合成致死靶点
###   11：  固定靶基因（PARP1），遍历所有突变基因 → 找预测性生物标志物
###
### 经典案例：BRCA1/2 突变 → 对 PARP 抑制剂敏感（已获 FDA 批准）
###   本脚本批量回答：除了 BRCA1/2，还有哪些基因突变也能预测 PARP1 敏感性？
###
### 注意事项：
###   1. 不是每个基因都能把细胞系分成突变和非突变两组
###   2. 能区分突变的基因每次都在变，因此留下来的组织也在变
###      → 用 Tissue_keep 机制维持固定的组织列结构
###
### 新版疾病名称变化（OncotreeLineage 字段）：
###   "breast" → "Breast"（首字母大写）
###   "ovary"  → "Ovary/Fallopian Tube"（合并命名）
### ================================================================

############################################################
## =================== 第一部分：批量遍历突变基因 ===================
## 固定靶基因 targetGene = "PARP1"
## 遍历所有突变基因，计算 Score = Sensitivity × Difference
############################################################
rm(list = ls())

## 加载数据分析三剑客
library(dplyr)     ## 数据操作
library(tidyr)     ## 宽长格式转换
library(tibble)    ## 行名/列名操作

## 读取预处理好的三份数据（由 07 脚本生成 RDS）
mutData <- readRDS(file = "TM00/DepMap_TM00/output/mutData_damaging.rds")
cellinfor <- readRDS(file = "TM00/DepMap_TM00/output/cellinfor.rds")
geneDependency <- readRDS(file = "TM00/DepMap_TM00/output/geneDependency.rds")

## 三份数据取交集
coID <- intersect(rownames(geneDependency), rownames(cellinfor)) %>%
  intersect(rownames(mutData))

## 新版 Model.csv 中 lineage 字段改名为 OncotreeLineage
Tissue = "OncotreeLineage"

## 筛选细胞系：只保留组织内细胞系 >= 5 的组织（小组织统计效力不够）
length(table(cellinfor[coID, Tissue]))
expexted_tissues = names(table(cellinfor[coID, Tissue])[table(cellinfor[coID, Tissue]) >= 5])

## 重新取交集——只保留属于合格组织的细胞系
coID <- intersect(rownames(geneDependency),
                  rownames(cellinfor[cellinfor[, Tissue] %in% expexted_tissues, ])) %>%
  intersect(rownames(mutData))

## 保存合格组织列表（Tissue_keep）
## 关键作用：batch loop 中用 merge(all.y=TRUE) 维持固定的列结构
## 即使某个基因突变后某些组织不满足筛选条件，对应列也保留为 NA
Tissue_keep = data.frame(Tissue = expexted_tissues)
nTissue <- nrow(Tissue_keep)

## 靶基因固定为 PARP1（可以改成 ESR1 等其他靶基因）
targetGene = "PARP1"

## 预计算循环内不变的量
tissueVec <- cellinfor[coID, Tissue]

## 结果存储
results = data.frame()
genelist = colnames(mutData)
cat("共", length(genelist), "个突变基因需要遍历，靶基因:", targetGene, "\n")

#######################################################
## 核心循环：遍历每个突变基因
for(i in 1:length(genelist)){

  if(i %% 2000 == 0) cat("进度:", i, "/", length(genelist), "\n")

  #######################
  ## 1. 每次提取一个突变基因
  mutGene = genelist[i]

  ## 构建突变状态数据
  data <- data.frame(
    Tissue = tissueVec,
    Mutation = ifelse(mutData[coID, mutGene] == 0, "WT", "Mut")
  )

  ## 如果所有细胞系都是 WT（该基因在所有细胞系中都没有突变），跳过
  if(all(data$Mutation == "WT")) next

  ## 根据突变信息筛选组织：要求 Mut >= 3 且 WT >= 5
  ## 注意：每个突变基因分出来的组织可能不同
  TissueStatus <- data %>%
    select(Tissue, Mutation) %>%
    group_by(Tissue, Mutation) %>%
    summarise(n = n(), .groups = "drop") %>%
    ungroup() %>%
    pivot_wider(names_from = "Mutation",
                values_from = n,
                values_fill = 0) %>%
    filter(Mut >= 3, WT >= 5) %>%
    mutate(Sum = Mut + WT) %>%
    mutate(Mut_Number = sum(Mut), WT_Number = sum(WT))

  ## 如果没有组织满足条件，也跳过
  if(nrow(TissueStatus) == 0) next

  ########################
  ## 2. 构建分析数据
  data <- data.frame(
    DepmapID = coID,
    Tissue = tissueVec,
    Mutation = ifelse(mutData[coID, mutGene] == 0, "WT", "Mut"),
    Dependency = geneDependency[coID, targetGene]
  )

  ## 只保留合格组织的细胞系
  mydata <- data %>%
    filter(Tissue %in% TissueStatus$Tissue)

  #########################
  ## 3. 数据分析
  ## Sensitivity：突变细胞系对靶基因的平均依赖性
  Mean_Dep <- mydata %>%
    filter(Mutation == "Mut") %>%
    group_by(Tissue) %>%
    summarise(Sensitivity = mean(Dependency))

  ## Difference：突变组 - 野生型组的依赖性差值
  Diff_Dep <- mydata %>%
    group_by(Tissue) %>%
    summarize(Difference = mean(Dependency[Mutation == "Mut"]) - mean(Dependency[Mutation == "WT"]))

  ## 数据合并，引入 Tissue_keep 维持固定列结构
  ## merge(all.y=TRUE) 确保所有合格组织都出现，不满足条件的填 NA
  PlotData <- merge(Mean_Dep, Diff_Dep, by = "Tissue") %>%
    merge(TissueStatus, by = "Tissue") %>%
    merge(Tissue_keep, by = "Tissue", all.y = TRUE)

  ##########################
  ## 4. 计算筛选分数（考虑 NA）
  ## score_all：所有组织的综合分数
  ## score_celltype：每个组织各自的分数
  score_all = mean(PlotData$Sensitivity, na.rm = TRUE) * mean(PlotData$Difference, na.rm = TRUE) * 100

  ##########################
  ## 5. 填入结果表格
  ## 列结构：mutGene, targetGene, mean_Sensitivity, mean_Diff, [各组织分数...], Score
  results[i, 1] = mutGene
  results[i, 2] = targetGene
  results[i, 3] = mean(PlotData$Sensitivity, na.rm = TRUE)
  results[i, 4] = mean(PlotData$Difference, na.rm = TRUE)
  ## Tissue_keep 保证了 PlotData 行数固定 == nTissue，列对齐不会错位
  results[i, 5:(4 + nTissue)] = PlotData$Sensitivity * PlotData$Difference * 100
  results[i, 5 + nTissue] = score_all
}

## 设置列名
colnames(results) = c("mutGene", "targetGene", "mean_Sensitivity", "mean_Diff",
                       Tissue_keep$Tissue, "Score")

## 保存含 NA 行的完整结果（跳过的基因行全为 NA）
saveRDS(results, file = "TM00/DepMap_TM00/output/target2mut_results_celltype_withNA.rds")

## 去掉全是 NA 的行（被跳过的基因）
results <- results[!apply(is.na(results), 1, all), ]

## 按 Score 降序排列
results <- results %>% arrange(desc(Score))

## 保存清理后的结果
saveRDS(results, file = "TM00/DepMap_TM00/output/target2mut_results_celltype.rds")

cat("\n=== 第一部分批量分析完成 ===\n")
cat("靶基因:", targetGene, "\n")
cat("有效突变基因数:", nrow(results), "\n")
cat("\nTop 20 突变基因（预测", targetGene, "敏感性最强的基因）:\n")
print(head(results[, c("mutGene", "targetGene", "mean_Sensitivity", "mean_Diff", "Score")], 20))


############################################################
## =================== 第二部分：单基因验证 + 散点图 ===================
## 从批量结果中挑选典型突变基因，画散点图验证
############################################################
rm(list = ls())

## 重新加载数据
library(dplyr)
library(tidyr)
library(tibble)

## 读取批量分析结果
results <- readRDS(file = "TM00/DepMap_TM00/output/target2mut_results_celltype.rds")

## 读取原始数据
mutData <- readRDS(file = "TM00/DepMap_TM00/output/mutData_damaging.rds")
cellinfor <- readRDS(file = "TM00/DepMap_TM00/output/cellinfor.rds")
geneDependency <- readRDS(file = "TM00/DepMap_TM00/output/geneDependency.rds")

## 三份数据取交集
coID <- intersect(rownames(geneDependency), rownames(cellinfor)) %>%
  intersect(rownames(mutData))

## 新版字段名
Tissue = "OncotreeLineage"

## 选择要验证的突变基因（取消注释选择其中一个）
mutGene = "ARID1A"
#mutGene = "BRCA1"
#mutGene = "BRCA2"
#mutGene = "TMEM185B"

## 靶基因
targetGene = "PARP1"

## 数据筛选：Mut >= 3, WT >= 5
TissueStatus <- data.frame(
  Tissue = cellinfor[coID, Tissue],
  Mutation = ifelse(mutData[coID, mutGene] == 0, "WT", "Mut")
) %>%
  select(Tissue, Mutation) %>%
  group_by(Tissue, Mutation) %>%
  summarise(n = n(), .groups = "drop") %>%
  ungroup() %>%
  pivot_wider(names_from = "Mutation",
              values_from = n,
              values_fill = 0) %>%
  filter(Mut >= 3, WT >= 5) %>%
  mutate(Sum = Mut + WT) %>%
  mutate(Mut_Number = sum(Mut), WT_Number = sum(WT))

## 构建分析数据
data <- data.frame(
  DepmapID = coID,
  Tissue = cellinfor[coID, Tissue],
  Mutation = ifelse(mutData[coID, mutGene] == 0, "WT", "Mut"),
  Dependency = geneDependency[coID, targetGene]
)

## 只保留合格组织
mydata <- data %>%
  filter(Tissue %in% TissueStatus$Tissue)

## 作图数据整理
## Sensitivity：突变细胞系对靶基因的平均依赖性
Mean_Dep <- mydata %>%
  filter(Mutation == "Mut") %>%
  group_by(Tissue) %>%
  summarise(Sensitivity = mean(Dependency))

## Difference：突变组 - 野生型组的依赖性差值
Diff_Dep <- mydata %>%
  group_by(Tissue) %>%
  summarize(Difference = mean(Dependency[Mutation == "Mut"]) - mean(Dependency[Mutation == "WT"]))

## 数据合并
PlotData <- merge(Mean_Dep, Diff_Dep, by = "Tissue") %>%
  merge(TissueStatus, by = "Tissue")

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
       title = paste0(mutGene, " Mutant (n=", unique(PlotData$Mut_Number),
                      ") vs Wildtype (n=", unique(PlotData$WT_Number),
                      ") → ", targetGene, " Dependency")) +
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
ggsave(paste0("TM00/DepMap_TM00/output/11_", mutGene, "_", targetGene, "_scatter.png"),
       p1, width = 8, height = 6, dpi = 300)
cat("散点图已保存:", mutGene, "×", targetGene, "\n")


############################################################
## =================== 第三部分：组织特异性分析 ===================
## 找出哪些突变基因具有特定组织（如 Breast、Ovary）的特异性
############################################################

## 新版 OncotreeLineage 中组织名称已改为首字母大写
## "breast" → "Breast"，"ovary" → "Ovary/Fallopian Tube"

## 动态获取组织列范围（不再硬编码 5:29）
tissue_cols <- which(colnames(results) %in% TissueStatus$Tissue)
## 更宽泛：获取所有组织名列（第5列到倒数第2列）
tissue_cols <- 5:(ncol(results) - 1)
cat("\n组织列范围:", min(tissue_cols), "-", max(tissue_cols), "（共", length(tissue_cols), "个组织）\n")

## ---- Breast 特异性：哪些突变基因对 PARP1 的敏感性具有 Breast 特异性 ----
## 条件：该基因在 Breast 列的分数 == 所有组织列中的最大值
if("Breast" %in% colnames(results)) {
  index_breast <- apply(results[, tissue_cols], 1, max, na.rm = TRUE) == results[, "Breast"]
  results_breast <- results[which(index_breast == 1), ]
  results_breast <- results_breast %>% arrange(desc(Breast))

  cat("\n=== Breast 特异性突变基因（预测", targetGene, "敏感性）===\n")
  cat("基因数:", nrow(results_breast), "\n")
  print(head(results_breast[, c("mutGene", "Breast", "Score")], 20))
} else {
  cat("\n警告：结果中没有 'Breast' 列，请检查 OncotreeLineage 值\n")
}

## ---- Ovary 特异性 ----
ovary_col <- "Ovary/Fallopian Tube"
if(ovary_col %in% colnames(results)) {
  index_ovary <- apply(results[, tissue_cols], 1, max, na.rm = TRUE) == results[, ovary_col]
  results_ovary <- results[which(index_ovary == 1), ]
  results_ovary <- results_ovary %>% arrange(desc(!!sym(ovary_col)))

  cat("\n=== Ovary 特异性突变基因（预测", targetGene, "敏感性）===\n")
  cat("基因数:", nrow(results_ovary), "\n")
  print(head(results_ovary[, c("mutGene", ovary_col, "Score")], 20))
} else {
  cat("\n警告：结果中没有 '", ovary_col, "' 列，请检查 OncotreeLineage 值\n")
}

cat("\n=== 脚本全部完成 ===\n")

### 参考链接
### https://www.ncbi.nlm.nih.gov/pmc/articles/PMC3954704/
### GZ07 批量课程
