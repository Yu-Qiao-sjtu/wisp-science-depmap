################################################
################################################
### 作者：果子
### 维护者：乔宇（上海交通大学，免疫药理PhD）
### 更新时间：2026-09-09
###
### ================================================================
### 锚定突变基因候选筛选器（07/09 脚本的前置工具：先选锚定基因，再进入依赖性分析）
###
### 解决的问题：09_batch_from_mut_to_target 系列脚本需要人工指定
###   mutGene（锚定基因），但"选哪个基因"本身需要依据。
###   本脚本按"四维选择框架"自动产出候选清单：
###
###   维度一（统计功效）：计算每个基因的突变细胞系数（泛癌 Mut_n）
###     # Damaging（LikelyLoF）口径 → 抑癌基因 TSG 适用
###     # Hotspot 口径 → 癌基因 OG 适用
###   维度二（矩阵匹配）：内置驱动基因角色表（TSG/OG），
###     # 告诉你该基因应该用哪套矩阵做 09 脚本的锚定
###   维度三（临床价值）：内置"已有靶向药"标记（SL 动机弱，排除用）
###   维度四（可验证性）：内置"已知合成致死搭档"标记（阳性对照用）
###   隐藏混杂一（组织混杂）：计算突变组织集中度 max_Lineage_Share
###     # 接近 1 = 突变几乎全在一个癌种，差异分析实为癌种间差异，慎选
###
### 输出（TM00/DepMap_TM00/output/）：
###   anchor_gene_candidates.rds / .csv  —— 全基因候选总表
###   控制台打印 TSG / OG 两个方向的 Top 候选
###
### 用法：跑完本脚本 → 在候选表中挑基因 → 把基因名填进
###   09 脚本的 mutGene（TSG 用 mutData_damaging，OG 需换 hotspot 口径）
### ================================================================

rm(list = ls())
library(data.table)
library(dplyr)

### ---------------------------------------------------------------
### 第一部分：加载数据
### ---------------------------------------------------------------
## Damaging 矩阵（07 脚本产物，行=ModelID，列=基因，值=0/1/2）
mutDam <- readRDS(file = "TM00/DepMap_TM00/output/mutData_damaging.rds")
cellinfor <- readRDS(file = "TM00/DepMap_TM00/output/cellinfor.rds")

## Hotspot 矩阵（原始 CSV，只 10MB，直接读）
## 实测结构（与 Damaging 不同）：前 6 列为元数据——
##   V1(空名) / SequencingID / ModelID / ModelConditionID /
##   IsDefaultEntryForModel / IsDefaultEntryForMC，第 7 列起才是基因（值 0.0/1.0）
##   列名格式同 "GENE (ID)"，需清理；一个模型可能出现多行（多条件）
##   → 只保留默认条件行并按 ModelID 去重（模型级口径，与 07/09 脚本一致）
hotRaw <- fread("data/OmicsSomaticMutationsMatrixHotspot.csv")
hotRaw <- hotRaw[IsDefaultEntryForModel == "Yes"]
hotRaw <- hotRaw[!duplicated(ModelID)]
mutHot <- as.data.frame(hotRaw)
rownames(mutHot) <- mutHot$ModelID
metaCols <- c(names(mutHot)[1], "SequencingID", "ModelID", "ModelConditionID",
              "IsDefaultEntryForModel", "IsDefaultEntryForMC")
mutHot <- mutHot[, setdiff(colnames(mutHot), metaCols)]
colnames(mutHot) <- gsub("\\s+\\(\\d+\\)", "", colnames(mutHot))
mutHot <- as.matrix(mutHot)
storage.mode(mutHot) <- "double"
rm(hotRaw)

## 与 cellinfor 取交集（保证组织注释完整）
coID <- intersect(rownames(cellinfor), rownames(mutDam)) %>%
  intersect(rownames(mutHot))
cat("三套数据交集细胞系:", length(coID), "株\n")

lineage <- cellinfor[coID, "OncotreeLineage"]

### ---------------------------------------------------------------
### 第二部分：维度一——突变细胞系数（两套口径）+ 组织集中度
### ---------------------------------------------------------------
## 二值化（>0 视为 Mut；damaging 值域 0/1/2，hotspot 值域 0.0/1.0）
damMat <- mutDam[coID, ] > 0
hotMat <- mutHot[coID, ] > 0

mut_n_damaging <- colSums(damMat)
mut_n_hotspot  <- colSums(hotMat)

## 组织集中度：gene × lineage 计数矩阵（一次矩阵乘法，不用循环）
## crossprod(x, L) = t(x) %*% L → 行=基因，列=组织
L <- model.matrix(~ 0 + factor(lineage))
colnames(L) <- levels(factor(lineage))

damByTissue <- crossprod(damMat * 1, L)
## max.col() 返回的向量无名，必须 setNames 后才能按基因名索引（否则全 NA）
damMaxTissue <- setNames(colnames(damByTissue)[max.col(damByTissue, ties.method = "first")],
                         rownames(damByTissue))
damMaxShare  <- apply(damByTissue, 1, max) / pmax(rowSums(damByTissue), 1)

hotByTissue <- crossprod(hotMat * 1, L)
hotMaxTissue <- setNames(colnames(hotByTissue)[max.col(hotByTissue, ties.method = "first")],
                         rownames(hotByTissue))
hotMaxShare  <- apply(hotByTissue, 1, max) / pmax(rowSums(hotByTissue), 1)

### ---------------------------------------------------------------
### 第三部分：维度二/三/四——内置驱动基因知识表
### ---------------------------------------------------------------
## 精选教科书级驱动基因（COSMIC Census / Vogelstein 2013 体系常见者），
## TSG=抑癌基因(LoF) 用 Damaging 口径；OG=癌基因(GOF) 用 Hotspot 口径
roleTab <- data.frame(
  gene = c(
    ## ---- TSG（功能丧失型，匹配 Damaging 矩阵）----
    "TP53","ARID1A","ARID1B","PTEN","KMT2D","KMT2C","CREBBP","SMARCA4",
    "STAG2","BAP1","SETD2","NSD1","CDKN2A","RB1","APC","NF1","NF2","KDM6A",
    "PBRM1","PIK3R1","TSC1","TSC2","VHL","CDH1","ATM","CHEK2","PALB2",
    "BRCA1","BRCA2","FBXW7","FAT1","KEAP1","SMARCB1","CIC","GRM3","AMER1",
    ## ---- OG（功能获得型，匹配 Hotspot 矩阵）----
    "KRAS","NRAS","HRAS","BRAF","RAF1","PIK3CA","EGFR","ERBB2","ERBB3",
    "CTNNB1","AKT1","AKT2","MTOR","FGFR1","FGFR2","FGFR3","FGFR4","JAK1",
    "JAK2","MET","ALK","RET","NTRK1","MYC","MYCN","MDM2","CCND1","CCNE1",
    "CDK4","CDK6","BCL2","IDH1","IDH2","GNAS","NFE2L2","KIT","PDGFRA",
    "FLT3","STAT3","MAP2K1","RIT1","RAC1","PPP2R5A"
  ),
  role = c(
    rep("TSG", 36),
    rep("OG", 43)
  ),
  stringsAsFactors = FALSE
)

## 维度三：已有较强靶向药（SL 筛选动机弱；注明不代表"完全不可成药"）
druggable <- c("EGFR","ERBB2","ALK","RET","NTRK1","KIT","PDGFRA","FLT3",
               "BRAF","IDH1","IDH2","CDK4","CDK6","BCL2","MET","JAK1","JAK2")

## 维度四：文献已知的合成致死搭档（阳性对照候选）
## 仅收录高置信、经典报道
slPartner <- c(
  "ARID1A"  = "ARID1B (Helming et al. Nat Med 2014)",
  "BRCA1"   = "PARP1 (Farmer et al. Nature 2005)",
  "BRCA2"   = "PARP1 (Farmer et al. Nature 2005)",
  "SMARCA4" = "SMARCA2 (Hoffmann et al. PNAS 2014)",
  "SMARCB1" = "SMARCA2/4 (RAW/DCAF5 系列)",
  "STAG2"   = "STAG1 (Benedetti et al. 2017)",
  "CDKN2A"  = "CDK4/6 (palbociclib 逻辑)",
  "KEAP1"   = "葡萄糖依赖/SLC7A11 (Romero et al. 2017)"
)

### ---------------------------------------------------------------
### 第四部分：汇总候选总表
### ---------------------------------------------------------------
allGenes <- union(names(mut_n_damaging), names(mut_n_hotspot))
cand <- data.frame(
  gene            = allGenes,
  mut_n_damaging  = as.integer(mut_n_damaging[allGenes]),
  mut_n_hotspot   = as.integer(mut_n_hotspot[allGenes]),
  stringsAsFactors = FALSE
)
cand$mut_rate_damaging <- round(cand$mut_n_damaging / length(coID), 3)

## 组织集中度（取该基因适用口径下的值）
cand$max_tissue_damaging <- damMaxTissue[allGenes]
cand$max_share_damaging  <- round(damMaxShare[allGenes], 2)
cand$max_tissue_hotspot  <- hotMaxTissue[allGenes]
cand$max_share_hotspot   <- round(hotMaxShare[allGenes], 2)

## 合并内置知识表（维度二/三/四）
cand <- cand %>%
  left_join(roleTab, by = "gene") %>%
  mutate(
    role         = ifelse(is.na(role), "NA(非经典驱动基因)", role),
    druggable    = ifelse(gene %in% druggable, "Yes", ""),
    known_SL     = ifelse(gene %in% names(slPartner), slPartner[gene], "")
  )

## 推荐口径：TSG 看 Damaging 列，OG 看 Hotspot 列
cand <- cand %>%
  mutate(
    mut_n_recommended = ifelse(role == "OG", mut_n_hotspot, mut_n_damaging),
    recommended_matrix = ifelse(role == "OG", "Hotspot",
                         ifelse(role == "TSG", "Damaging", NA)),
    ## 组织集中度预警：>0.8 = 突变几乎全在单一癌种
    tissue_warning = ifelse(
      !is.na(mut_n_recommended) & mut_n_recommended >= 30 &
        ifelse(role == "OG", max_share_hotspot, max_share_damaging) > 0.8,
      "组织集中，建议单癌种分层", "")
  )

## 排序：先按推荐口径 Mut_n 降序
cand <- cand %>% arrange(desc(mut_n_recommended))

### ---------------------------------------------------------------
### 第四点五部分：基因 × 癌种可分析性交叉验证
### ---------------------------------------------------------------
## 目的：泛癌 Mut_n 高 ≠ 每个癌种都能做分析。09 脚本对每个癌种有
##   准入门槛（Mut >= 3 且 WT >= 5），不满足的癌种会被自动跳过；
##   单癌种课题另有强门槛（Mut >= 10）才值得做分层验证。
##   本部分逐癌种验证每个基因的分组可行性，并回填两个便利列：
##   n_lineage_pass_09   = 过 09 准入门槛的癌种数（能进散点图的气泡数）
##   n_lineage_pass_10   = 过强门槛 Mut>=10 的癌种数（可做单癌种课题）

## 各癌种总细胞数（WT = 总数 - Mut，复用已有的基因×癌种矩阵，零循环）
nByLineage <- colSums(L)

## 构造函数：基因×癌种互表 → 长表 + 双门槛标记
makeCrossTab <- function(mutByTissue, nByLineage, matrixName) {
  genes <- rownames(mutByTissue)
  lineages <- colnames(mutByTissue)
  df <- data.frame(
    gene    = rep(genes, times = length(lineages)),
    lineage = rep(lineages, each = length(genes)),
    mut_n   = as.integer(as.vector(mutByTissue)),
    stringsAsFactors = FALSE
  )
  df$wt_n  <- as.integer(nByLineage[df$lineage] - df$mut_n)
  df$matrix <- matrixName
  df %>% mutate(
    pass_09   = mut_n >= 3 & wt_n >= 5,   ## 09 脚本准入门槛
    pass_10   = mut_n >= 10 & wt_n >= 5   ## 单癌种课题强门槛
  )
}

crossDam <- makeCrossTab(damByTissue, nByLineage, "Damaging")
crossHot <- makeCrossTab(hotByTissue, nByLineage, "Hotspot")

## 落盘完整交叉表（两口径合并，便于 Excel 透视）
crossAll <- rbind(crossDam, crossHot)
saveRDS(crossAll, file = "TM00/DepMap_TM00/output/anchor_gene_by_lineage.rds")
fwrite(crossAll, file = "TM00/DepMap_TM00/output/anchor_gene_by_lineage.csv")

## 按基因汇总：过门槛的癌种数 + 最佳癌种名单（推荐口径）
summarizePass <- function(crossTab) {
  crossTab %>%
    filter(pass_09) %>%
    group_by(gene) %>%
    summarise(
      n_lineage_pass_09 = n(),
      n_lineage_pass_10 = sum(pass_10),
      ## 最佳癌种 = Mut 数最多的前三个（超出用逗号连接）
      top_lineages = paste0(
        paste0(lineage[order(-mut_n)][1:3], "(", mut_n[order(-mut_n)][1:3], ")"),
        collapse = ", "
      ),
      .groups = "drop"
    )
}
passDam <- summarizePass(crossDam) %>% rename_with(~ paste0(.x, "_damaging"), -gene)
passHot <- summarizePass(crossHot) %>% rename_with(~ paste0(.x, "_hotspot"), -gene)

cand <- cand %>%
  left_join(passDam, by = "gene") %>%
  left_join(passHot, by = "gene") %>%
  mutate(
    n_lineage_pass_09   = ifelse(role == "OG", n_lineage_pass_09_hotspot,
                           n_lineage_pass_09_damaging),
    n_lineage_pass_10   = ifelse(role == "OG", n_lineage_pass_10_hotspot,
                           n_lineage_pass_10_damaging),
    best_lineages       = ifelse(role == "OG", top_lineages_hotspot,
                           top_lineages_damaging)
  ) %>%
  select(-n_lineage_pass_09_damaging, -n_lineage_pass_09_hotspot,
         -n_lineage_pass_10_damaging, -n_lineage_pass_10_hotspot,
         -top_lineages_damaging, -top_lineages_hotspot)

## 过门槛癌种数为 0 = 泛癌能算但每个癌种都分不开组，单癌种不可做
## （保留在候选表中但提示需慎选）

### ---------------------------------------------------------------
### 第四点六部分：逐癌种锚定基因菜单（全基因，非仅驱动基因）
### ---------------------------------------------------------------
## 目的：提供"菜单"而非"排名"——锁定某个癌种后，列出该癌种内
##   【所有】能过强门槛（Mut>=10 & WT>=5）的突变基因，供自由选择。
##   不替用户预设选择：驱动基因身份、可药性、已知 SL 搭档仅作参考列。
## 取数口径：经典驱动基因按角色取推荐口径（TSG→Damaging，OG→Hotspot）；
##   非驱动基因默认取 Damaging（09 脚本现用口径），Hotspot 突变数可查交叉表。

lineageAnchors <- crossAll %>%
  left_join(cand %>% select(gene, role, druggable, known_SL), by = "gene") %>%
  filter(pass_10) %>%
  mutate(is_driver = !is.na(role) & role %in% c("TSG", "OG")) %>%
  filter(ifelse(is_driver,
                (role == "TSG" & matrix == "Damaging") |
                (role == "OG"  & matrix == "Hotspot"),
                matrix == "Damaging")) %>%
  mutate(mut_rate_in_lineage = round(mut_n / (mut_n + wt_n), 3)) %>%
  select(lineage, gene, is_driver, role, matrix, mut_n, wt_n,
         mut_rate_in_lineage, druggable, known_SL) %>%
  arrange(lineage, desc(mut_n))

saveRDS(lineageAnchors, file = "TM00/DepMap_TM00/output/anchor_gene_per_lineage.rds")
fwrite(lineageAnchors, file = "TM00/DepMap_TM00/output/anchor_gene_per_lineage.csv")

cat("\n=== 逐癌种锚定基因菜单已生成 ===\n")
cat("覆盖癌种数:", n_distinct(lineageAnchors$lineage),
    "；基因×癌种组合数:", nrow(lineageAnchors),
    "；其中经典驱动基因:", sum(lineageAnchors$is_driver), "个组合\n")
cat("输出: TM00/DepMap_TM00/output/anchor_gene_per_lineage.csv\n")
cat("（驱动基因为参考标注，非过滤器；选哪个基因由课题设计决定）\n")

### ---------------------------------------------------------------
### 第五部分：输出
### ---------------------------------------------------------------
saveRDS(cand, file = "TM00/DepMap_TM00/output/anchor_gene_candidates.rds")
fwrite(cand, file = "TM00/DepMap_TM00/output/anchor_gene_candidates.csv")

cat("\n=== 锚定基因候选表已生成 ===\n")
cat("总基因数:", nrow(cand), "\n")
cat("输出: TM00/DepMap_TM00/output/anchor_gene_candidates.csv\n")

## ---- 筛选逻辑演示：四维框架落地 ----
## 门槛：推荐口径 Mut_n >= 30（维度一）
## 排除：已有靶向药（维度三）、组织集中 > 0.8（隐藏混杂一）
shortlist <- cand %>%
  filter(!is.na(recommended_matrix),
         mut_n_recommended >= 30,
         druggable != "Yes",
         tissue_warning == "")

cat("\n=== TSG 方向 Top 15（用 Damaging 口径跑 09 脚本）===\n")
shortlist %>% filter(role == "TSG") %>%
  select(gene, mut_n_damaging, mut_rate_damaging,
         max_tissue_damaging, max_share_damaging, known_SL) %>%
  head(15) %>% print()

cat("\n=== OG 方向 Top 15（需换 Hotspot 矩阵口径）===\n")
shortlist %>% filter(role == "OG") %>%
  select(gene, mut_n_hotspot, max_tissue_hotspot, max_share_hotspot) %>%
  head(15) %>% print()

## ---- 交叉验证演示：看候选基因逐癌种可分析性 ----
cat("\n=== 交叉验证演示：Top TSG 候选逐癌种门槛核验 ===\n")
shortlist %>% filter(role == "TSG") %>%
  select(gene, mut_n_damaging, n_lineage_pass_09, n_lineage_pass_10,
         best_lineages, known_SL) %>%
  head(10) %>% print()

cat("\n=== 示例：ARID1A 与 KEAP1 的逐癌种 Mut/WT 分布 ===\n")
crossDam %>% filter(gene %in% c("ARID1A", "KEAP1"), mut_n >= 3, wt_n >= 5) %>%
  arrange(gene, desc(mut_n)) %>% head(30) %>% print()

## ---- 逐癌种菜单演示：Lung 全量菜单 + 各癌种可选基因数 ----
cat("\n=== 各癌种可选锚定基因数（Mut>=10 & WT>=5，含非驱动基因）===\n")
lineageAnchors %>%
  count(lineage, name = "n_selectable") %>%
  arrange(desc(n_selectable)) %>% as.data.frame() %>% print(row.names = FALSE)

cat("\n=== 示例：Lung 癌种全量菜单（前 40 行，完整见 CSV）===\n")
lineageAnchors %>% filter(lineage == "Lung") %>%
  select(gene, is_driver, role, mut_n, wt_n,
         mut_rate_in_lineage, druggable, known_SL) %>%
  head(40) %>% print()

cat("\n提示：known_SL 非空的基因 = 有阳性对照，首个课题优先选它们；\n")
cat("max_share > 0.8 的基因建议锁定单一癌种分层分析（见 tissue_warning 列）；\n")
cat("n_lineage_pass_10 >= 1 才值得做单癌种课题；0 = 只能做泛癌池化\n")
