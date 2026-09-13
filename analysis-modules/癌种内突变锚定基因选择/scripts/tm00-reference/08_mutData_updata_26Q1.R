################################################
################################################
### 作者：果子
### 维护者：乔宇（上海交通大学，免疫药理PhD）
### 更新时间：2026-09-09
### 数据迁移：2026-09-08（23Q2 → 26Q1；突变源 CCLE_mutations.csv →
###           OmicsSomaticMutations.csv 长表，字段 isDeleterious → LikelyLoF，
###           详见下方字段演进注释）
###           脚本名同步由 08_mutData_updata_23Q2.R 更名为 _26Q1.R

### ================================================================
### 本脚本探索 DepMap 如何界定"损伤性突变"（damaging mutation）
###
### 核心问题：CCLE/DepMap 究竟如何界定突变是"损伤性"的？
###   BRAF V600E 是经典的致癌突变，但它属于 missense，
###   旧版 isDeleterious 列没有包含它——怎么解决？
###
### DepMap 突变分类系统的演进（损伤性判定字段三代更替）：
###   20Q4v2: Variant_Classification + isDeleterious（true/false）
###           分类如 Missense_Mutation, Nonsense_Mutation 等
###           定义：仅 MAF 功能缺失后果类别白名单（nonsense/frameshift/
###           splice_site/start_lost 等），missense 一律 FALSE
###           → 官方论坛已明确此字段废弃："outdated annotation which we
###             do not recommend using"（见下方链接）
###   23Q2:   VariantInfo + CCLEDeleterious
###           分类如 MISSENSE, NONSENSE 等（大写）
###   26Q1:   VariantType + VariantInfo + LikelyLoF（现行标准，第 55 列）
###           VariantType: SNV / deletion / insertion / substitution
###           VariantInfo: missense_variant / stop_gained / frameshift_variant 等
###           ★ LikelyLoF = True 的官方权威定义（26Q1 Mutation Pipeline
###             Documentation PDF，本地存档 docs/26Q1_Mutation_Pipeline_Documentation.pdf）：
###             满足以下任一条即为 True：
###             (1) OncoKB 中该变异的 mutation effect 为
###                 "Likely Loss-of-function" 或 "Loss-of-function"
###                 （注意：missense 也可通过此路径入选！）
###             (2) VEP impact == "HIGH"（frameshift/stop_gained/start_lost/
###                 stop_lost/splice_acceptor/splice_donor/transcript_ablation）
###             即：damaging = (OncoKB LoF 注释) OR (VEP impact HIGH)，
###             不是单纯的 VEP 后果类别白名单（旧 isDeleterious 的思路）
###
### 26Q1 提供两种突变数据：
###   1. OmicsSomaticMutations.csv（长格式 MAF，69 列，含 LikelyLoF/Hotspot）
###      → 完全复现官方 Damaging 标准应直接筛 LikelyLoF == True，
###        而非自行按 VariantInfo 分类（会遗漏 OncoKB LoF 路径的错义突变）
###   2. OmicsSomaticMutationsMatrixDamaging.csv（宽格式 0/1/2 矩阵）
###      → DepMap 已预先按 LikelyLoF 聚合好，直接可用（06/07 脚本使用）
###      → 值编码：0=无；GT=="1|1" 或同一基因多个 damaging 突变的 AF 加和
###        ≥0.95 → 2（近纯合）；否则 → 1（杂合）
###
### 概念辨析（勿混淆）：
###   "是否突变"（最宽，MAF 有记录即是） ⊃ "是否损伤性突变"（LikelyLoF）
###   ⊕ "是否热点致癌突变"（Hotspot，第 67 列——BRAF V600E/KRAS G12D 用这个）
###   LikelyLoF ≠ 是否突变，只是其严格子集
###
### 推荐阅读：
###   1. CCLE 如何标记突变（isDeleterious 废弃声明在此）
###      https://forum.depmap.org/t/how-is-the-isdeleterious-column-in-the-ccle-mutations-csv-file-determined/129
###   2. 26Q1 官方突变流程文档（LikelyLoF 权威定义）
###      docs/26Q1_Mutation_Pipeline_Documentation.pdf（本地存档）
### ================================================================

library(dplyr)
library(tidyr)
library(tibble)

############################################################
## =================== 第一部分：探索 26Q1 突变分类系统 ===================
## 从长格式 MAF 文件中探索 VariantType 和 VariantInfo 的分布
############################################################
rm(list = ls())

## 读取突变数据（只加载需要的列，加快速度）
mutData_raw <- data.table::fread("data/OmicsSomaticMutations.csv", data.table = F,
                                 select = c("ModelID", "HugoSymbol", "VariantType",
                                            "VariantInfo", "ProteinChange",
                                            "IsDefaultEntryForModel"))

## ---- VariantType 分布 ----
## 26Q1 只有 4 种 VariantType（非常简洁）
cat("=== VariantType 分布 ===\n")
print(table(mutData_raw$VariantType))
## SNV:          ~102 万（单核苷酸变异，最常见）
## deletion:     ~7.9 万（缺失）
## substitution: ~3.8 万（替换，多碱基）
## insertion:    ~2.8 万（插入）


## ---- VariantInfo 分布（取首个注释项）----
## VariantInfo 是 VEP 注释，可能包含多个注释用 & 连接
## 如 "missense_variant&splice_region_variant"
## 取第一个注释作为主分类
mutData_raw$VariantInfo_main <- sub("&.*", "", mutData_raw$VariantInfo)

cat("\n=== VariantInfo 主分类 Top15 ===\n")
variant_info_table <- sort(table(mutData_raw$VariantInfo_main), decreasing = TRUE)
print(head(variant_info_table, 15))
## missense_variant:       ~93.8 万（错义突变，最多——BRAF V600E 在这里！）
## frameshift_variant:     ~9.0 万（移码突变）
## stop_gained:            ~6.1 万（终止密码子获得 = nonsense）
## splice_acceptor_variant: ~1.6 万（剪接受体位点变异）
## splice_donor_variant:    ~1.4 万（剪接供体位点变异）
## start_lost:             ~2.2 千（起始密码子丢失）
## stop_lost:              ~1.5 千（终止密码子丢失）
## inframe_deletion:       ~7.3 千（框内缺失）
## inframe_insertion:      ~2.3 千（框内插入）


## ---- BRAF 突变探索（脚本原始动机）----
## BRAF V600E 是经典致癌突变（黑色素瘤等），但旧版 isDeleterious 不包含 missense
cat("\n=== BRAF 突变分类 ===\n")
braf <- mutData_raw[mutData_raw$HugoSymbol == "BRAF", ]
print(table(braf$VariantInfo_main))
## 绝大多数 BRAF 突变是 missense_variant（包括 V600E）
cat("\nBRAF V600E（p.V600E）数量：",
    sum(grepl("V600E", braf$ProteinChange, ignore.case = TRUE)), "\n")


############################################################
## =================== 第二部分：从长格式 MAF 构建突变矩阵 ===================
## 方式一：自定义"损伤性"标准（含 missense）
## 对比方式二：直接使用 DepMap 预定义的 Damaging 矩阵
############################################################

## ---- 自定义损伤性突变分类 ----
## 参考旧版 isDeleterious 的逻辑，但我们加入 missense_variant
## 这样 BRAF V600E 等重要致癌突变就不会被漏掉
index_damaging <- c(
  "frameshift_variant",      ## 移码突变（致命，蛋白质完全改变）
  "stop_gained",             ## 终止密码子获得（= nonsense，蛋白质截短）
  "start_lost",              ## 起始密码子丢失（蛋白质无法翻译）
  "stop_lost",               ## 终止密码子丢失（蛋白质延长，可能有害）
  "splice_acceptor_variant", ## 剪接受体变异（常导致外显子跳跃）
  "splice_donor_variant",    ## 剪接供体变异（同上）
  "missense_variant",        ## 错义突变（★ 新增：BRAF V600E, KRAS G12D 等）
  "inframe_deletion",        ## 框内缺失（EGFR del19 等）
  "inframe_insertion"        ## 框内插入（EGFR ins20 等）
)

## 标记损伤性突变
mutData_raw$isDamaging <- mutData_raw$VariantInfo_main %in% index_damaging

cat("\n=== 自定义损伤性突变统计 ===\n")
cat("总突变记录数：", nrow(mutData_raw), "\n")
cat("其中损伤性：", sum(mutData_raw$isDamaging), "\n")
cat("比例：", round(mean(mutData_raw$isDamaging) * 100, 1), "%\n")

## ---- 构建宽格式突变矩阵（行=细胞系，列=基因，值=突变次数）----
## 仅保留默认条目
mutData_default <- mutData_raw[mutData_raw$IsDefaultEntryForModel == "Yes", ]

mutData_custom <- mutData_default %>%
  filter(isDamaging) %>%                                ## 只保留损伤性突变
  select(ModelID, HugoSymbol) %>%                       ## 只需要这两列
  mutate(count = 1) %>%                                 ## 每行计1次
  group_by(ModelID, HugoSymbol) %>%                     ## 按细胞系×基因分组
  summarise(isDamaging = sum(count)) %>%                ## 同一基因多次突变累加
  ungroup() %>%
  pivot_wider(names_from = "HugoSymbol",                ## 转为宽格式
              values_from = "isDamaging",
              values_fill = 0) %>%
  as.data.frame() %>%
  column_to_rownames("ModelID")

cat("\n自定义突变矩阵维度：", nrow(mutData_custom), "细胞系 ×",
    ncol(mutData_custom), "基因\n")

saveRDS(mutData_custom,
        file = "TM00/DepMap_TM00/output/mutData_custom_with_missense.rds")


############################################################
## =================== 第三部分：对比两种突变矩阵 ===================
## 方式一（自定义含 missense）vs 方式二（DepMap 预定义 Damaging）
############################################################
rm(list = ls())

## ---- 加载自定义矩阵（第二部分已保存为 RDS）----
mutData_custom <- readRDS(file = "TM00/DepMap_TM00/output/mutData_custom_with_missense.rds")

## ---- 加载 DepMap 预定义的 Damaging 矩阵 ----
## 这个矩阵由官方 LikelyLoF 字段聚合（OncoKB LoF 注释 OR VEP impact HIGH），
## 绝大多数 missense 不在内（仅 OncoKB 标为 LoF 的错义例外）
## 06/07 脚本直接使用这个矩阵
mutData_damaging <- data.table::fread("data/OmicsSomaticMutationsMatrixDamaging.csv",
                                       data.table = F)
mutData_damaging <- mutData_damaging[mutData_damaging$IsDefaultEntryForModel == "Yes", ]
mutData_damaging <- mutData_damaging[!duplicated(mutData_damaging$ModelID), ]
rownames(mutData_damaging) <- mutData_damaging$ModelID
mutData_damaging <- mutData_damaging[, -(1:6)]
mutData_damaging[is.na(mutData_damaging)] <- 0
colnames(mutData_damaging) <- gsub(" \\((\\d+)\\)", "", colnames(mutData_damaging))

cat("\nDepMap Damaging 矩阵维度：", nrow(mutData_damaging), "细胞系 ×",
    ncol(mutData_damaging), "基因\n")

## ---- 加载 DepMap 预定义的 Hotspot 矩阵（官方三套口径的第三套）----
## 官方定义（26Q1 README.txt 第 726 行，逐字引用）：
##   "A variant is considered a hot spot if it's present in one of the following:
##    Hess et al. 2019 paper, OncoKB hotspot, COSMIC mutation significance
##    tier 1, TERT promoter mutations (C228T or C250T), mutations in the
##    polypyrimidine track in intron 13 of MET."
##   即四个来源的并集：① Hess et al. 2019 论文热点 ② OncoKB hotspot
##   ③ COSMIC 显著性 tier 1 ④ TERT 启动子 C228T/C250T 及 MET 内含子 13
##   多聚嘫啶轨道（polypyrimidine tract，原始 README 写作 track）突变
## 值编码与 Damaging 矩阵相同（README 第 728 行）：0=无；同一基因多个
##   hotspot 的 AF 加和 >0.95 → 2，否则 → 1
## 结构与 Damaging 宽矩阵完全同构（前 6 列元数据相同顺序），
##   但仅含 ~554 个热点基因列（只收录有官方热点收录的基因）
## 适用：癌基因的激活突变（BRAF V600E、KRAS G12D、EGFR L858R 等）——
##   这些 missense 既不在 Damaging（除非 OncoKB 标 LoF），
##   也远比"全部 missense"（自定义矩阵）严格得多
mutData_hotspot <- data.table::fread("data/OmicsSomaticMutationsMatrixHotspot.csv",
                                     data.table = F)
mutData_hotspot <- mutData_hotspot[mutData_hotspot$IsDefaultEntryForModel == "Yes", ]
mutData_hotspot <- mutData_hotspot[!duplicated(mutData_hotspot$ModelID), ]
rownames(mutData_hotspot) <- mutData_hotspot$ModelID
mutData_hotspot <- mutData_hotspot[, -(1:6)]
mutData_hotspot[is.na(mutData_hotspot)] <- 0
colnames(mutData_hotspot) <- gsub(" \\((\\d+)\\)", "", colnames(mutData_hotspot))

cat("DepMap Hotspot 矩阵维度：", nrow(mutData_hotspot), "细胞系 ×",
    ncol(mutData_hotspot), "热点基因\n")


## ---- 关键基因对比（三套口径）----
## ARID1A：肿瘤抑制基因，loss-of-function 突变（nonsense/frameshift 为主）
## BRAF/KRAS：致癌基因，V600E/G12D 等 missense 热点激活突变为主
cat("\n=== 关键基因突变细胞系数对比（三套口径）===\n")
cat(sprintf("%-8s  %-18s  %-16s  %-14s\n",
            "基因", "自定义(含missense)", "官方Damaging", "官方Hotspot"))
test_genes <- c("ARID1A", "BRAF", "KRAS", "TP53", "EGFR", "PIK3CA")
for (gene in test_genes) {
  custom_n <- if (gene %in% colnames(mutData_custom))
    sum(mutData_custom[, gene] > 0) else NA
  damaging_n <- if (gene %in% colnames(mutData_damaging))
    sum(mutData_damaging[, gene] > 0) else NA
  hotspot_n <- if (gene %in% colnames(mutData_hotspot))
    sum(mutData_hotspot[, gene] > 0) else NA
  cat(sprintf("%-8s  %-18s  %-16s  %-14s\n",
              gene, custom_n, damaging_n, hotspot_n))
}

cat("\n三套口径的官方定位与选用原则：\n")
cat("  ① 自定义(含missense)：最宽——全部 9 类后果自名单，仅作对照实验；\n")
cat("     大量中性 missense 混入，噪声大，不建议正式分析使用\n")
cat("  ② 官方 Damaging（LikelyLoF）：功能丧失——抑癌基因分析首选\n")
cat("     （ARID1A/TP53 在此列计数接近自定义，因它们本来就以 LoF 突变为主）\n")
cat("  ③ 官方 Hotspot：热点激活——癌基因分析首选\n")
cat("     （BRAF/KRAS 在此列有代表性计数，比自定义严格得多，只收复发热点）\n")
cat("  ⚠ 批量分析同时含抑癌基因与癌基因时，应按基因类型分别选 ②或③，\n")
cat("     统一用 ② 会对癌基因系统性假阴性（如 KRAS 在 Damaging 中几乎为 0）\n")


############################################################
## =================== 第四部分：加载基因依赖性和细胞系信息 ===================
############################################################
rm(list = ls())

## 基因依赖性
geneDependency <- data.table::fread("data/CRISPRGeneDependency.csv", data.table = F)
colnames(geneDependency) <- gsub("\\s+\\(\\d+\\)", "", colnames(geneDependency))
rownames(geneDependency) <- geneDependency[, 1]
geneDependency <- geneDependency[, -1]
saveRDS(geneDependency, file = "TM00/DepMap_TM00/output/geneDependency.rds")

## 细胞系信息
cellinfor <- data.table::fread("data/Model.csv", data.table = F)
rownames(cellinfor) <- cellinfor$ModelID
saveRDS(cellinfor, file = "TM00/DepMap_TM00/output/cellinfor.rds")

cat("数据预处理完成\n")


############################################################
## =================== 第五部分：验证——ARID1A × HMGCR 散点图 ===================
## 使用自定义含 missense 的突变矩阵验证
############################################################

## 加载数据
mutData <- readRDS(file = "TM00/DepMap_TM00/output/mutData_custom_with_missense.rds")
geneDependency <- readRDS(file = "TM00/DepMap_TM00/output/geneDependency.rds")
cellinfor <- readRDS(file = "TM00/DepMap_TM00/output/cellinfor.rds")

## 细胞系取交集
coID <- intersect(rownames(geneDependency), rownames(cellinfor)) %>%
  intersect(rownames(mutData))

## 提取 ARID1A 突变 × HMGCR 依赖性
data <- data.frame(
  DepmapID = coID,
  Tissue = cellinfor[coID, "OncotreePrimaryDisease"],
  Mutation = ifelse(mutData[coID, "ARID1A"] == 0, "WT", "Mut"),
  Dependency = geneDependency[coID, "HMGCR"]
)

## 组织筛选（Mut>=3, WT>=5）
TissueStatus <- data %>%
  select(Tissue, Mutation) %>%
  group_by(Tissue, Mutation) %>%
  summarise(n = n()) %>%
  ungroup() %>%
  pivot_wider(names_from = "Mutation",
              values_from = n, values_fill = 0) %>%
  filter(Mut >= 3, WT >= 5) %>%
  mutate(Sum = Mut + WT) %>%
  mutate(Mut_Number = sum(Mut), WT_Number = sum(WT))

mydata <- data %>%
  filter(Tissue %in% TissueStatus$Tissue)

## 计算指标
Mean_Dep <- mydata %>%
  filter(Mutation == "Mut") %>%
  group_by(Tissue) %>%
  summarise(Sensitivity = mean(Dependency))

Diff_Dep <- mydata %>%
  group_by(Tissue) %>%
  summarize(Difference = mean(Dependency[Mutation == "Mut"]) -
                          mean(Dependency[Mutation == "WT"]))

PlotData <- merge(Mean_Dep, Diff_Dep, by = "Tissue") %>%
  merge(TissueStatus, by = "Tissue")

## 作图
library(ggplot2)
library(ggrepel)
library(stringr)
library(ggfun)

p1 <- ggplot(PlotData, aes(x = Sensitivity, y = Difference)) +
  geom_point(aes(size = Sum), shape = 21, color = "black", fill = "green",
             alpha = 0.6, stroke = 1.5) +
  geom_text_repel(aes(label = Tissue), vjust = 1.5, hjust = 1.5) +
  annotate("rect", xmin = -Inf, xmax = Inf, ymin = 0, ymax = Inf,
           fill = "yellow", alpha = 0.1, colour = "green",
           linetype = "dashed", linewidth = 1) +
  labs(x = "Sensitivity Score for Cell Lines with Mutations",
       y = "Mutant vs. Wildtype Sensitivity",
       title = paste0("Mutant (n=", unique(PlotData$Mut_Number),
                      ") vs. Wildtype (n=", unique(PlotData$WT_Number), ")")) +
  theme_bw() +
  guides(size = guide_legend(title = str_wrap("Number of cells", width = 8))) +
  theme(panel.grid.major = element_line(linetype = "dashed"),
        panel.grid.minor = element_line(linetype = "dashed", size = 0.5),
        panel.background = element_blank(),
        legend.position = c(.95, .05),
        legend.justification = c("right", "bottom"),
        legend.box.just = "right",
        legend.margin = margin(4, 4, 4, 4),
        legend.background = element_roundrect(color = "#808080", linetype = 1)
  )

print(p1)
ggsave("TM00/DepMap_TM00/output/08_ARID1A_HMGCR_custom.png", p1,
       width = 8, height = 6, dpi = 300)
cat("散点图已保存（使用自定义含 missense 的突变矩阵）\n")
